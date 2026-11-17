//! 视觉常量与主题转换。
//!
//! TUI 用 [`crate::theme::Theme`]（ratatui 的 `Color`）上色；GPUI 要 `Rgba`，
//! 这里做一次转换，不再让 `theme.rs` 依赖 GUI 框架。

use gpui::{FontWeight, Rgba, rgb};
use ratatui::style::Color;

use crate::theme::Theme;

/// 面板默认尺寸（逻辑像素）—— 用户拖边缩放后写入 `gui.json`
pub const DEFAULT_WIDTH: f32 = 760.0;
pub const DEFAULT_HEIGHT: f32 = 520.0;

pub const RADIUS: f32 = 12.0;
pub const PADDING: f32 = 12.0;

/// 等宽字体：输入行与列表都用它，列对齐靠字符宽度而不是 flex 估算
pub const FONT: &str = "JetBrains Maple Mono";
pub const FONT_WEIGHT: FontWeight = FontWeight::NORMAL;

/// 输入行：高 44、字号 16
pub const INPUT_HEIGHT: f32 = 44.0;
pub const INPUT_SIZE: f32 = 16.0;
/// 结果行：高 34、字号 14、卡片圆角 8
pub const ROW_HEIGHT: f32 = 34.0;
pub const ROW_GAP: f32 = 2.0;
pub const ROW_RADIUS: f32 = 8.0;
pub const ROW_SIZE: f32 = 14.0;
/// 徽标 chip：宽 52、高 20、圆角 4、字号 11
pub const BADGE_WIDTH: f32 = 52.0;
pub const BADGE_HEIGHT: f32 = 20.0;
pub const BADGE_RADIUS: f32 = 4.0;
pub const BADGE_SIZE: f32 = 11.0;
/// 附注（副标题 / 末列）
pub const NOTE_SIZE: f32 = 12.0;
/// 状态栏：高 26、字号 11
pub const STATUS_HEIGHT: f32 = 26.0;
pub const STATUS_SIZE: f32 = 11.0;
/// 选中行左侧的强调条宽度
pub const ACCENT_BAR: f32 = 3.0;

/// ratatui 的 `Color` → GPUI `Rgba`（只处理 `Rgb`，其余回退到背景色）
pub fn rgba(color: Color, fallback: Color) -> Rgba {
    match color {
        Color::Rgb(r, g, b) => rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32),
        _ => rgba(fallback, Color::Rgb(0x27, 0x2e, 0x33)),
    }
}

/// 主题的常用色，避免每处都写 `rgba(t.xxx, t.background)`
#[derive(Clone, Copy)]
pub struct Palette {
    pub background: Rgba,
    pub overlay: Rgba,
    pub border: Rgba,
    pub fg: Rgba,
    pub muted: Rgba,
    pub dim: Rgba,
    pub faint: Rgba,
    pub accent: Rgba,
    pub good: Rgba,
    pub bad: Rgba,
}

impl Palette {
    pub fn new(theme: &Theme) -> Self {
        let background = rgba(theme.background, theme.background);
        Self {
            background,
            overlay: rgba(theme.overlay, theme.background),
            border: rgba(theme.border, theme.background),
            fg: rgba(theme.fg, theme.background),
            muted: rgba(theme.muted, theme.background),
            dim: rgba(theme.dim, theme.background),
            faint: rgba(theme.faint, theme.background),
            accent: rgba(theme.accent, theme.background),
            good: rgba(theme.good, theme.background),
            bad: rgba(theme.bad, theme.background),
        }
    }
}

/// 徽标配色（Everforest 调色板，按 `Kind` 区分；`theme.toml` 以后可覆盖）
pub fn badge_color(kind: crate::providers::Kind) -> Rgba {
    use crate::providers::Kind;
    match kind {
        Kind::App => rgb(0xa7c080),
        Kind::Web => rgb(0x7fbbb3),
        Kind::File | Kind::Dir => rgb(0x83c092),
        Kind::Setting | Kind::Switch => rgb(0xe69875),
        Kind::Command | Kind::Help => rgb(0xd699b6),
        Kind::Pi => rgb(0xd3c6aa),
        Kind::Clean => rgb(0xe67e80),
        Kind::History => rgb(0x859289),
        Kind::Clip => rgb(0xdbbc7f),
    }
}

/// 列表标题列 / 附注列大约能显示多少个「显示格」（等宽字体，1 中文 = 2 格）。
///
/// Step 1 先按固定字符数截断——GPUI 0.2.2 的 `truncate()` 对文本元素实测不裁剪
/// （文字照样画到父元素外面），等宽字体下自己截反而可控。Step 3 再按真实窗口宽度算。
pub const TITLE_COLS: usize = 56;
pub const NOTE_COLS: usize = 22;

/// 按「显示宽度」截断到 `max_cols` 格，超出补 `…`。
pub fn fit(text: &str, max_cols: usize) -> String {
    use unicode_width::UnicodeWidthChar;

    let mut width = 0;
    let mut out = String::new();
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if width + w > max_cols {
            // 给省略号留一格
            while width + 1 > max_cols {
                let Some(prev) = out.pop() else { break };
                width -= prev.width().unwrap_or(0);
            }
            out.push('…');
            return out;
        }
        width += w;
        out.push(ch);
    }
    out
}

/// 输入行左侧的提示符
pub const PROMPT: &str = "❯ ";

/// 光标：等宽字的一格宽（Step 1 先用静态竖线，Step 3 换成动画块）
pub const CURSOR: &str = "▏";

#[cfg(test)]
mod tests {
    use super::fit;

    #[test]
    fn short_text_is_untouched() {
        assert_eq!(fit("hello", 10), "hello");
    }

    #[test]
    fn ascii_is_cut_with_ellipsis() {
        let out = fit("abcdefghij", 5);
        assert_eq!(out, "abcd…");
        assert_eq!(out.chars().count(), 5);
    }

    #[test]
    fn wide_chars_count_as_two() {
        // 4 个汉字 = 8 格；第 5 个放不下（要 10 格 > 9）
        assert_eq!(fit("中文测试中文", 9), "中文测试…");
    }

    #[test]
    fn exact_fit_has_no_ellipsis() {
        assert_eq!(fit("abcdef", 6), "abcdef");
    }
}
