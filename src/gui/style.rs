//! 视觉常量与主题转换。
//!
//! TUI 用 [`crate::theme::Theme`]（ratatui 的 `Color`）上色；GPUI 要 `Rgba`，
//! 这里做一次转换，不再让 `theme.rs` 依赖 GUI 框架。

use gpui::{FontWeight, Rgba, rgb};
use ratatui::style::Color;

use crate::theme::Theme;

/// 面板默认尺寸（逻辑像素）：拿不到主屏信息时的兜底
pub const DEFAULT_WIDTH: f32 = 760.0;
pub const DEFAULT_HEIGHT: f32 = 520.0;
/// 面板占主屏的比例（和 TUI 面板的 GlazeWM 规则保持一致：62% × 55%）
pub const WIDTH_RATIO: f32 = 0.62;
pub const HEIGHT_RATIO: f32 = 0.55;

pub const RADIUS: f32 = 12.0;
pub const PADDING: f32 = 12.0;
/// 超宽行的左右滚动：像素速度、两端停顿、单程的最短时长
pub const SCROLL_SPEED: f32 = 240.0; // px/s
pub const SCROLL_PAUSE: f32 = 0.6; // 两端各停多久（秒）
/// 列表上下滚动：每帧向目标偏移逼近的比例（0..1，越大越快）
pub const SCROLL_EASE: f32 = 0.28;
/// 选框 / 光标滑动的缓动系数（比滚动稍快，跟手一点）
pub const MOVE_EASE: f32 = 0.55;
/// 位移小于这个像素数就吸附到目标
pub const SCROLL_SNAP: f32 = 0.4;

/// chip 灰 ↔ 彩 的渐变时长（秒）
pub const CHIP_FADE_SECS: f32 = 0.14;

/// 列表展开时的淡入时长（秒）
pub const REVEAL_SECS: f32 = 0.12;

/// 面板自己画的边框（草绿 accent 色，1px）
pub const BORDER: f32 = 1.0;
/// 展开后最多显示多少行结果（超出滚动）
pub const MAX_VISIBLE_ROWS: usize = 12;

/// 等宽字体：输入行与列表都用它，列对齐靠字符宽度而不是 flex 估算
pub const FONT: &str = "JetBrains Maple Mono";
pub const FONT_WEIGHT: FontWeight = FontWeight::NORMAL;

/// 输入行：高 40、字号 15
pub const INPUT_HEIGHT: f32 = 40.0;
pub const INPUT_SIZE: f32 = 15.0;
/// 光标块的尺寸（`app.cursor` 处那根竖线）。
/// 高度跟字号一致（15px），比 18 更贴合字形上下边界——之前高出字头一截，看着像错位。
/// 宽度 2px：1px 在 200% DPI 下就是半像素，看着像缺了一块。
pub const CURSOR_WIDTH: f32 = 2.0;
pub const CURSOR_HEIGHT: f32 = 15.0;
/// 结果行：卡片 30、间距 4、圆角 7、字号 13（YASB 那种紧凑感）
pub const ROW_HEIGHT: f32 = 28.0;
pub const ROW_GAP: f32 = 3.0;
pub const ROW_RADIUS: f32 = 7.0;
pub const ROW_SIZE: f32 = 13.0;
/// 徽标 chip：宽 50、高 18、圆角 9（胶囊）、字号 10
pub const BADGE_WIDTH: f32 = 50.0;
pub const BADGE_HEIGHT: f32 = 18.0;
pub const BADGE_RADIUS: f32 = 9.0;
pub const BADGE_SIZE: f32 = 10.0;
/// 行内横向间距 / 左右内边距（小一点，列表整体更靠左）
pub const ROW_GAP_X: f32 = 6.0;
pub const ROW_PAD_X: f32 = 6.0;
/// 选择模式下的序号列 / 收藏星标列
pub const NUMBER_WIDTH: f32 = 18.0;
pub const STAR_WIDTH: f32 = 14.0;
/// 控件行的状态列（`开` / `关` / 数值 / 选项），TUI 里是 `ui.rs` 追加的
pub const STATE_WIDTH: f32 = 72.0;
/// detail 模式每一行的行高 / 每条条目的上下内边距
pub const DETAIL_LINE: f32 = 20.0;
pub const DETAIL_PAD_Y: f32 = 4.0;
/// 附注（副标题 / 末列）
pub const NOTE_SIZE: f32 = 11.0;
/// 状态栏：高 24、字号 11
pub const STATUS_HEIGHT: f32 = 24.0;
pub const STATUS_SIZE: f32 = 11.0;
/// 选中行左侧的强调条宽度
pub const ACCENT_BAR: f32 = 3.0;
/// 列表区顶部留白（别让第一行贴着分隔线）
pub const LIST_TOP: f32 = 4.0;
/// 列表滚动条：宽 3px，贴在右侧内缩 1px
pub const SCROLLBAR_WIDTH: f32 = 3.0;
/// 内容再长，滑块也不低于轨道的这个比例（否则小到看不见）
pub const SCROLLBAR_MIN_RATIO: f32 = 0.06;
/// 滚动条滑块的亮度（用 `with_alpha(fg, ..)` 套出来，别写死颜色）
pub const SCROLLBAR_ALPHA: f32 = 0.25;

/// 两色插值：`t` = 0 取 `a`，1 取 `b`（chip 灰 ↔ 彩的渐变就用它）
pub fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// 给颜色套一个不透明度（1.0 = 原样）
pub fn with_alpha(color: Rgba, alpha: f32) -> Rgba {
    Rgba {
        a: alpha.clamp(0.0, 1.0),
        ..color
    }
}

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
    /// 比背景略亮一档：hover 底色、未选中 chip 的底色
    pub panel: Rgba,
    pub overlay: Rgba,
    pub border: Rgba,
    pub fg: Rgba,
    pub muted: Rgba,
    pub dim: Rgba,
    pub faint: Rgba,
    pub accent: Rgba,
    pub good: Rgba,
    pub warn: Rgba,
    pub bad: Rgba,
}

impl Palette {
    pub fn new(theme: &Theme) -> Self {
        let background = rgba(theme.background, theme.background);
        Self {
            background,
            panel: rgba(theme.panel, theme.background),
            overlay: rgba(theme.overlay, theme.background),
            border: rgba(theme.border, theme.background),
            fg: rgba(theme.fg, theme.background),
            muted: rgba(theme.muted, theme.background),
            dim: rgba(theme.dim, theme.background),
            faint: rgba(theme.faint, theme.background),
            accent: rgba(theme.accent, theme.background),
            good: rgba(theme.good, theme.background),
            warn: rgba(theme.warn, theme.background),
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

/// 输入行左侧的提示符
pub const PROMPT: &str = "❯ ";
/// 提示符 `❯ ` 占几个「显示格」——实测 JetBrains Maple Mono 下是 18px = 2 格 @ 9px。
///
/// **只给 IME 候选窗定位兜底用**；输入行光标的横坐标已经改用 `shape_line` 实测宽度。
/// 换字体后这个值要重量（临时设个明显偏的值看候选窗落点反推）。
pub const PROMPT_CHARS: f32 = 2.0;
