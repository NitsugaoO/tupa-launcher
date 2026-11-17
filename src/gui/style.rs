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

/// 输入行：高 40、字号 15
pub const INPUT_HEIGHT: f32 = 40.0;
pub const INPUT_SIZE: f32 = 15.0;
/// 光标块的尺寸（`app.cursor` 处那根竖线）
pub const CURSOR_WIDTH: f32 = 2.0;
pub const CURSOR_HEIGHT: f32 = 18.0;
/// 结果行：卡片 30、间距 4、圆角 7、字号 13（YASB 那种紧凑感）
pub const ROW_HEIGHT: f32 = 30.0;
pub const ROW_GAP: f32 = 4.0;
pub const ROW_RADIUS: f32 = 7.0;
pub const ROW_SIZE: f32 = 13.0;
/// 徽标 chip：宽 50、高 18、圆角 9（胶囊）、字号 10
pub const BADGE_WIDTH: f32 = 50.0;
pub const BADGE_HEIGHT: f32 = 18.0;
pub const BADGE_RADIUS: f32 = 9.0;
pub const BADGE_SIZE: f32 = 10.0;
/// 附注（副标题 / 末列）
pub const NOTE_SIZE: f32 = 11.0;
/// 状态栏：高 24、字号 11
pub const STATUS_HEIGHT: f32 = 24.0;
pub const STATUS_SIZE: f32 = 11.0;
/// 选中行左侧的强调条宽度
pub const ACCENT_BAR: f32 = 3.0;
/// 列表区顶部留白（别让第一行贴着分隔线）
pub const LIST_TOP: f32 = 4.0;

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
