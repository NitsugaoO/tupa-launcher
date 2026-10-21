//! 把 ratatui 的 `Buffer` 画成像素。
//!
//! 思路：仍然让 `ui::draw` 排版到 cell 网格上，这里只负责
//! 「cell → 背景矩形 + 字形位图」。字形用 cosmic-text 排版（自带字体回退，
//! 中文会落到系统字体），swash 光栅化后的位图按字符缓存起来。

use std::collections::HashMap;

use cosmic_text::{
    Attrs, Buffer as TextBuffer, CacheKey, Fallback, Family, FontSystem, Metrics, Shaping,
    SwashCache, Weight, fontdb,
};
use unicode_script::Script;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme::Theme;

/// 逻辑字号（对齐 Windows Terminal 的默认 12pt ≈ 16px）
const FONT_SIZE: f32 = 16.0;
/// 最小行高系数：和 WT 的默认行距（1.2 em）保持一致
const LINE_HEIGHT_RATIO: f32 = 1.2;
/// 等宽字体（英文 / 符号）。中文没有字形，由 cosmic-text 回退到系统 CJK 字体
const FONT_FAMILY: &str = "FiraCode Nerd Font Mono";
/// 中文兜底的字体（Windows 的 script fallback 默认就是它，这里显式钉住）
const CJK_FAMILY: &str = "Microsoft YaHei UI";

/// 一个光栅化好的字形，坐标相对 cell 左上角。
struct Glyph {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    /// `data` 是 RGBA（彩色 emoji）还是单通道 alpha 蒙版
    rgba: bool,
    data: Vec<u8>,
}

type GlyphRun = Vec<Glyph>;

/// CPU 上的像素画布（`softbuffer` 用的是 `0x00RRGGBB`）
pub struct Canvas<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: &'a mut [u32],
}

impl Canvas<'_> {
    /// 整块铺底色：网格铺不满窗口时，右/下不会露出未绘制的黑边
    pub fn fill_all(&mut self, color: u32) {
        self.pixels.fill(color);
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: u32) {
        let (x0, y0) = (x.max(0), y.max(0));
        let x1 = (x + w as i32).min(self.width as i32);
        let y1 = (y + h as i32).min(self.height as i32);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for py in y0..y1 {
            let start = py as usize * self.width as usize;
            let row = &mut self.pixels[start..start + self.width as usize];
            for px in x0..x1 {
                row[px as usize] = color;
            }
        }
    }

    /// 把一个字形混合到画布上
    fn blend(&mut self, x: i32, y: i32, glyph: &Glyph, fg: u32) {
        let (w, h) = (glyph.width, glyph.height);
        let (data, rgba) = (glyph.data.as_slice(), glyph.rgba);
        let (fr, fg_, fb) = (fg >> 16 & 0xff, fg >> 8 & 0xff, fg & 0xff);
        for row in 0..h {
            let py = y + row as i32;
            if py < 0 || py >= self.height as i32 {
                continue;
            }
            let base = py as usize * self.width as usize;
            for col in 0..w {
                let px = x + col as i32;
                if px < 0 || px >= self.width as i32 {
                    continue;
                }
                let (alpha, color) = if rgba {
                    let i = (row * w + col) as usize * 4;
                    let Some(p) = data.get(i..i + 4) else { continue };
                    (p[3], Some((p[0], p[1], p[2])))
                } else {
                    let i = (row * w + col) as usize;
                    match data.get(i) {
                        Some(&a) => (a, None),
                        None => continue,
                    }
                };
                if alpha == 0 {
                    continue;
                }
                let dst = self.pixels[base + px as usize];
                let (dr, dg, db) = (dst >> 16 & 0xff, dst >> 8 & 0xff, dst & 0xff);
                let (sr, sg, sb) = match color {
                    Some((r, g, b)) => (r as u32, g as u32, b as u32),
                    None => (fr, fg_, fb),
                };
                let a = u32::from(GAMMA[alpha as usize]);
                let blend = |src: u32, dst: u32| (src * a + dst * (255 - a)) / 255;
                let out = (blend(sr, dr) << 16) | (blend(sg, dg) << 8) | blend(sb, db);
                self.pixels[base + px as usize] = out;
            }
        }
    }
}

/// 抗锯齿的 gamma 校正表。
///
/// 直接按 alpha 线性混合时，细笔画会偏灰、发虚；终端（DirectWrite）合成前
/// 会做 gamma 校正，我们要看起来一样就得补上。
static GAMMA: std::sync::LazyLock<[u8; 256]> = std::sync::LazyLock::new(|| {
    let mut table = [0u8; 256];
    for (alpha, slot) in table.iter_mut().enumerate() {
        let value = (alpha as f64 / 255.0).powf(1.0 / 1.2) * 255.0;
        *slot = value.round().clamp(0.0, 255.0) as u8;
    }
    table
});

/// cell 网格 → 像素
pub struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    scratch: TextBuffer,
    scale: f32,
    /// cell 尺寸（物理像素）
    pub cell_w: f32,
    pub cell_h: f32,
    /// 统一基线在 cell 内的位置（物理像素）
    baseline: f32,
    /// `symbol → [普通, 粗体]`
    glyphs: HashMap<String, [Option<GlyphRun>; 2]>,
}

impl TextRenderer {
    pub fn new(scale: f32) -> Self {
        let mut font_system = new_font_system();
        let mut swash_cache = SwashCache::new();
        let cell = measure(&mut font_system, &mut swash_cache, scale);
        let scratch = TextBuffer::new(
            &mut font_system,
            Metrics::new(FONT_SIZE * scale, cell.cell_h),
        );
        TextRenderer {
            font_system,
            swash_cache,
            scratch,
            scale,
            cell_w: cell.advance,
            cell_h: cell.cell_h,
            baseline: cell.baseline,
            glyphs: HashMap::new(),
        }
    }

    /// DPI 变了：重排度量并丢掉字形缓存
    pub fn set_scale(&mut self, scale: f32) {
        if (scale - self.scale).abs() < f32::EPSILON {
            return;
        }
        self.scale = scale;
        let cell = measure(&mut self.font_system, &mut self.swash_cache, scale);
        self.scratch.set_metrics_and_size(
            Metrics::new(FONT_SIZE * scale, cell.cell_h),
            None,
            None,
        );
        self.cell_w = cell.advance;
        self.cell_h = cell.cell_h;
        self.baseline = cell.baseline;
        self.glyphs.clear();
    }

    /// 窗口内某个 cell 的左上角（物理像素）
    pub fn cell_origin(&self, col: u16, row: u16) -> (i32, i32) {
        (
            (col as f32 * self.cell_w).round() as i32,
            (row as f32 * self.cell_h).round() as i32,
        )
    }

    /// 把一帧 ratatui 的 Buffer 画到画布上
    /// `origin` 是网格左上角在窗口里的物理像素位置（给圆角留边距）
    pub fn render(
        &mut self,
        buffer: &Buffer,
        theme: Theme,
        canvas: &mut Canvas,
        origin: (i32, i32),
    ) {
        let area = buffer.area;
        let (cw, ch) = (self.cell_w.ceil() as u32, self.cell_h.ceil() as u32);

        // 1) 背景：先铺满，字形溢出到相邻 cell 时才不会被后面盖掉
        for row in 0..area.height {
            for col in 0..area.width {
                let cell = &buffer[(area.left() + col, area.top() + row)];
                // 宽字符的第二列（符号是空白）样式会被 reset 掉：
                // 沿用左邻的背景，否则选中行的高亮会中间断开
                let left = col
                    .checked_sub(1)
                    .map(|left| &buffer[(area.left() + left, area.top() + row)]);
                let tail = left.is_some_and(|left| {
                    left.symbol().width() > 1 && cell.symbol().chars().all(|c| c == ' ')
                });
                let bg = if tail {
                    left.map(|left| left.style().bg)
                } else {
                    None
                }
                .unwrap_or(cell.style().bg);
                let (x, y) = self.cell_origin(col, row);
                canvas.fill_rect(
                    x + origin.0,
                    y + origin.1,
                    cw,
                    ch,
                    resolve(bg, theme.background),
                );
            }
        }

        // 2) 缺字形先补缓存（借不到就分两步）
        let mut missing: Vec<(String, usize)> = Vec::new();
        for row in 0..area.height {
            for col in 0..area.width {
                let symbol = buffer[(area.left() + col, area.top() + row)].symbol();
                if symbol.is_empty() {
                    continue;
                }
                let bold = usize::from(
                    buffer[(area.left() + col, area.top() + row)]
                        .style()
                        .add_modifier
                        .contains(Modifier::BOLD),
                );
                let slot = self.glyphs.get(symbol);
                let has = slot.is_some_and(|runs| runs[bold].is_some());
                if !has && !missing.iter().any(|(s, b)| s == symbol && *b == bold) {
                    missing.push((symbol.to_string(), bold));
                }
            }
        }
        for (symbol, bold) in missing {
            let run = self.shape(&symbol, bold == 1);
            self.glyphs.entry(symbol).or_insert([None, None])[bold] = Some(run);
        }

        // 3) 画字
        for row in 0..area.height {
            for col in 0..area.width {
                let cell = &buffer[(area.left() + col, area.top() + row)];
                let symbol = cell.symbol();
                if symbol.is_empty() {
                    continue;
                }
                let bold = usize::from(cell.style().add_modifier.contains(Modifier::BOLD));
                let Some(run) = self.glyphs.get(symbol).and_then(|runs| runs[bold].as_ref())
                else {
                    continue;
                };
                let fg = resolve(cell.style().fg, theme.fg);
                let (x, y) = self.cell_origin(col, row);
                let (x, y) = (x + origin.0, y + origin.1);
                // 宽字符占两格，但字形只画一次
                let _w = UnicodeWidthStr::width(symbol).max(1);
                for glyph in run {
                    canvas.blend(x + glyph.left, y + glyph.top, glyph, fg);
                }
            }
        }
    }

    /// 用 cosmic-text 排一个 symbol，拿到它的所有字形位图
    fn shape(&mut self, symbol: &str, bold: bool) -> GlyphRun {
        let attrs = Attrs::new()
            .family(family_for(symbol))
            .weight(if bold { Weight::BOLD } else { Weight::NORMAL });

        // 排版 → 只留 (cache_key, x)：后面要单独借 font_system 光栅化
        let mut placed: Vec<(CacheKey, i32)> = Vec::new();
        {
            let mut buffer = self.scratch.borrow_with(&mut self.font_system);
            buffer.set_size(None, None);
            buffer.set_text(symbol, &attrs, Shaping::Advanced, None);
            for run in buffer.layout_runs() {
                for glyph in run.glyphs {
                    let physical = glyph.physical((0.0, 0.0), 1.0);
                    placed.push((physical.cache_key, physical.x));
                }
            }
        }

        let mut run = GlyphRun::new();
        for (key, x) in placed {
            let Some(image) = self.swash_cache.get_image(&mut self.font_system, key) else {
                continue;
            };
            let placement = image.placement;
            if placement.width == 0 || placement.height == 0 {
                continue;
            }
            run.push(Glyph {
                left: x + placement.left,
                // placement.top 是「基线到字形顶部」的距离；所有字形共用一条基线
                top: (self.baseline - placement.top as f32).round() as i32,
                width: placement.width,
                height: placement.height,
                rgba: matches!(image.content, cosmic_text::SwashContent::Color),
                data: image.data.clone(),
            });
        }
        run
    }
}

/// 字体回退：除了等宽字体，其余（汉字、全角符号）都优先用微软雅黑。
///
/// 默认的 `PlatformFallback` 在中文系统上不一定会挑到雅黑
/// （实测会落到 `MS Gothic`），这里直接钉死。
struct UiFallback;

impl Fallback for UiFallback {
    fn common_fallback(&self) -> &'static [&'static str] {
        &[CJK_FAMILY, "Segoe UI Emoji", "Segoe UI Symbol"]
    }

    fn forbidden_fallback(&self) -> &'static [&'static str] {
        &[]
    }

    fn script_fallback(&self, _script: Script, _locale: &str) -> &'static [&'static str] {
        &[CJK_FAMILY]
    }
}

/// 这个字该用哪套字体：全角（汉字 / 中文标点）走雅黑，其余走等宽字体。
///
/// 不依赖 fallback 猜测：逐字符排版，直接按宽度选字体更可控，
/// 也避免汉字被系统随机挑成 `MS Gothic` 之类的日文字体。
fn family_for(symbol: &str) -> Family<'static> {
    if symbol
        .chars()
        .any(|c| UnicodeWidthChar::width(c).unwrap_or(0) > 1)
    {
        Family::Name(CJK_FAMILY)
    } else {
        Family::Name(FONT_FAMILY)
    }
}

fn new_font_system() -> FontSystem {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    db.set_monospace_family(FONT_FAMILY);
    db.set_sans_serif_family(CJK_FAMILY);

    FontSystem::new_with_locale_and_db_and_fallback("zh-CN".to_string(), db, UiFallback)
}

/// 量出 cell 尺寸：半角字的 advance 和「中英文混排时的实际行高」。
///
/// 行高必须按字体度量（ascent + descent）来，不能硬编码：
/// 中文 fallback 字体比等宽字体高，行高给小了字形会溢出到下一行。
struct CellMetrics {
    advance: f32,
    baseline: f32,
    cell_h: f32,
}

fn measure(font_system: &mut FontSystem, swash_cache: &mut SwashCache, scale: f32) -> CellMetrics {
    let font_size = FONT_SIZE * scale;
    // 先给个宽松的行高，免得测量时被裁
    let mut scratch = TextBuffer::new(font_system, Metrics::new(font_size, font_size * 1.8));
    let attrs = Attrs::new().family(Family::Monospace);
    let mut placed: Vec<(CacheKey, i32)> = Vec::new();

    let advance = {
        let mut buffer = scratch.borrow_with(font_system);
        buffer.set_size(None, None);

        buffer.set_text("0000000000", &attrs, Shaping::Advanced, None);
        let width = buffer.layout_runs().next().map_or(0.0, |run| run.line_w);
        let advance = if width > 0.0 {
            width / 10.0
        } else {
            font_size * 0.6
        };

        // 中英混排取样（中文会回退到别的字体，通常比英文高）
        buffer.set_text("Mg常国0gy", &attrs, Shaping::Advanced, None);
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let physical = glyph.physical((0.0, 0.0), 1.0);
                placed.push((physical.cache_key, physical.y));
            }
        }
        advance
    };

    // 真的光栅化一遍：用「最大上伸 / 最大下伸」定基线和行高
    let mut ascent: f32 = 0.0;
    let mut descent: f32 = 0.0;
    for (key, _) in placed {
        let Some(image) = swash_cache.get_image(font_system, key) else {
            continue;
        };
        let placement = image.placement;
        ascent = ascent.max(placement.top as f32);
        descent = descent.max(placement.height as f32 - placement.top as f32);
    }
    if ascent <= 0.0 {
        ascent = font_size;
        descent = font_size * 0.25;
    }

    // 行高取「字形实际高度」和「1.2 em」的较大值：字挤在一起不好看，
    // 也保证和 Windows Terminal 版本的行距一致
    let text_height = ascent + descent;
    let line_height = text_height.max(font_size * LINE_HEIGHT_RATIO);

    CellMetrics {
        // 保留字体真实的 advance（不取整）：ratatui 的列宽是按字符数算的，
        // 这里一旦取整，列位置就会相对终端版本慢慢漂移
        advance,
        baseline: ascent.ceil() + (line_height - text_height) / 2.0,
        cell_h: line_height.ceil() + 1.0,
    }
}

/// `ratatui::Color` → 帧缓冲用的 `0x00RRGGBB`
pub fn to_pixel(color: Color) -> u32 {
    let (r, g, b) = rgb(color);
    (r as u32) << 16 | (g as u32) << 8 | b as u32
}

/// `ratatui::Color` → `0x00RRGGBB`。`Reset` 表示「用主题色」，
/// 不能当成具体颜色（宽字符的第二列就是 Reset，硬编码会画出整列色块）
fn resolve(color: Option<Color>, fallback: Color) -> u32 {
    match color {
        None | Some(Color::Reset) => to_pixel(fallback),
        Some(color) => to_pixel(color),
    }
}

fn rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Reset => (0xd3, 0xc6, 0xaa),
        Color::Black => (0x27, 0x2e, 0x33),
        Color::Red => (0xe6, 0x7e, 0x80),
        Color::Green => (0xa7, 0xc0, 0x80),
        Color::Yellow => (0xdb, 0xbc, 0x7f),
        Color::Blue => (0x7f, 0xbb, 0xb3),
        Color::Magenta => (0xd6, 0x99, 0xb8),
        Color::Cyan => (0x83, 0xc0, 0x92),
        Color::Gray => (0x85, 0x92, 0x89),
        Color::DarkGray => (0x47, 0x52, 0x58),
        Color::LightRed => (0xe6, 0x7e, 0x80),
        Color::LightGreen => (0xa7, 0xc0, 0x80),
        Color::LightYellow => (0xdb, 0xbc, 0x7f),
        Color::LightBlue => (0x7f, 0xbb, 0xb3),
        Color::LightMagenta => (0xd6, 0x99, 0xb8),
        Color::LightCyan => (0x83, 0xc0, 0x92),
        Color::White => (0xd3, 0xc6, 0xaa),
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Indexed(i) => {
            // 只用得到几个基础色，其余按灰度近似
            let v = i.saturating_mul(255) / 15;
            (v, v, v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    fn render(cols: u16, rows: u16, setup: impl Fn(&mut Buffer)) -> (Vec<u32>, u32, u32) {
        let mut renderer = TextRenderer::new(2.0);
        let mut buffer = Buffer::empty(Rect::new(0, 0, cols, rows));
        setup(&mut buffer);

        let width = (renderer.cell_w * f32::from(cols)).ceil() as u32;
        let height = (renderer.cell_h * f32::from(rows)).ceil() as u32;
        let mut pixels = vec![0u32; (width * height) as usize];
        let mut canvas = Canvas {
            width,
            height,
            pixels: &mut pixels,
        };
        renderer.render(&buffer, Theme::default(), &mut canvas, (0, 0));
        (pixels, width, height)
    }

    /// 默认样式的 cell（背景是 `Color::Reset`）必须铺成主题背景，而不是被当成某个颜色
    #[test]
    fn reset_background_falls_back_to_theme() {
        let (pixels, _, _) = render(8, 2, |buffer| {
            buffer.set_style(Rect::new(0, 0, 8, 2), Style::new());
        });
        let background = super::to_pixel(Theme::default().background);
        assert!(pixels.iter().all(|pixel| *pixel == background));
    }

    /// 宽字符占两列，第二列的 cell 会被 reset，背景要跟左邻一致
    #[test]
    fn wide_char_tail_inherits_background() {
        let highlight = Color::Rgb(0x37, 0x41, 0x45);
        let (pixels, width, _) = render(4, 1, |buffer| {
            buffer.set_string(0, 0, "中", Style::new().bg(highlight));
        });

        // 第二列（宽字符的尾巴）也应该是高亮色
        let tail_x = (width / 4) + 1;
        assert_eq!(pixels[tail_x as usize], super::to_pixel(highlight));
    }

    /// 英文走 FiraCode，中文回退到微软雅黑
    #[test]
    fn font_fallbacks() {
        fn families(text: &str) -> Vec<String> {
            let mut renderer = TextRenderer::new(2.0);
            let attrs = Attrs::new().family(family_for(text));
            let mut scratch = TextBuffer::new(
                &mut renderer.font_system,
                Metrics::new(FONT_SIZE * 2.0, 40.0),
            );
            let ids: Vec<_> = {
                let mut buffer = scratch.borrow_with(&mut renderer.font_system);
                buffer.set_size(None, None);
                buffer.set_text(text, &attrs, Shaping::Advanced, None);
                buffer
                    .layout_runs()
                    .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.font_id))
                    .collect()
            };
            ids.iter()
                .filter_map(|id| renderer.font_system.db().face(*id))
                .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
                .collect()
        }

        let latin = families("Menu");
        assert!(
            latin.iter().any(|name| name.to_lowercase().contains("fira")),
            "英文实际用的是 {latin:?}"
        );

        let chinese = families("启动");
        assert!(
            chinese.iter().any(|name| name.to_lowercase().contains("yahei")),
            "中文实际用的是 {chinese:?}"
        );
    }

    /// 字形不能溢出 cell：行高必须够，否则每行文字会压到下一行
    #[test]
    fn glyphs_fit_inside_cell() {
        let mut renderer = TextRenderer::new(2.0);
        let cell_height = renderer.cell_h;
        for symbol in ["M", "g", "常", "国", "❯", "→", "@"] {
            for glyph in renderer.shape(symbol, false) {
                let bottom = glyph.top + glyph.height as i32;
                assert!(
                    bottom as f32 <= cell_height,
                    "{symbol:?} 底边 {bottom} 超出 cell 高度 {cell_height}"
                );
            }
        }
    }

    /// 中文必须画出字形（字体回退成功），不能只剩背景
    #[test]
    fn chinese_renders_pixels() {
        let (pixels, _, _) = render(4, 1, |buffer| {
            buffer.set_string(0, 0, "常", Style::new());
        });
        let background = super::to_pixel(Theme::default().background);
        let painted = pixels.iter().filter(|pixel| **pixel != background).count();
        assert!(painted > 50, "只画出了 {painted} 个像素");
    }
}
