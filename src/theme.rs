//! 主题：内置一套 Everforest(hard) 默认色，可被覆盖。
//!
//! 覆盖顺序（后者优先）：
//!   1. 内置默认
//!   2. `~/.config/term-palette/theme.toml`
//!   3. `config.toml` 里的 `[theme]` 段
//!
//! 角色名刻意与 `~/Themes/themes/everforest/tokens.yaml` 对齐，
//! 后续由生成器直接产出 `theme.toml` 即可并入既有配色流水线。

use ratatui::style::Color;
use serde::Deserialize;

use crate::config::{self, Config};

/// 一个 RGB 颜色
#[derive(Debug, Clone, Copy)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn parse(s: &str) -> Option<Rgb> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 {
            return None;
        }
        Some(Rgb(
            u8::from_str_radix(&s[0..2], 16).ok()?,
            u8::from_str_radix(&s[2..4], 16).ok()?,
            u8::from_str_radix(&s[4..6], 16).ok()?,
        ))
    }

    pub fn color(self) -> Color {
        Color::Rgb(self.0, self.1, self.2)
    }
}

/// 可在配置里覆盖的角色（全部可选）
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ThemeOverrides {
    pub background: Option<String>,
    pub panel: Option<String>,
    pub overlay: Option<String>,
    pub border: Option<String>,
    pub fg: Option<String>,
    pub muted: Option<String>,
    pub dim: Option<String>,
    pub faint: Option<String>,
    pub accent: Option<String>,
    pub good: Option<String>,
    pub warn: Option<String>,
    pub bad: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub background: Color,
    pub panel: Color,
    pub overlay: Color,
    pub border: Color,
    pub fg: Color,
    pub muted: Color,
    pub dim: Color,
    pub faint: Color,
    pub accent: Color,
    pub good: Color,
    pub warn: Color,
    pub bad: Color,
}

impl Default for Theme {
    fn default() -> Self {
        // Everforest hard 档（与 tokens.yaml 暗色一致）
        Theme {
            background: Color::Rgb(0x27, 0x2e, 0x33),
            panel: Color::Rgb(0x2e, 0x38, 0x3c),
            overlay: Color::Rgb(0x37, 0x41, 0x45),
            border: Color::Rgb(0x41, 0x4b, 0x50),
            fg: Color::Rgb(0xd3, 0xc6, 0xaa),
            muted: Color::Rgb(0x9d, 0xa9, 0xa0),
            dim: Color::Rgb(0x85, 0x92, 0x89),
            faint: Color::Rgb(0x7a, 0x84, 0x78),
            accent: Color::Rgb(0xa7, 0xc0, 0x80),
            good: Color::Rgb(0x83, 0xc0, 0x92),
            warn: Color::Rgb(0xdb, 0xbc, 0x7f),
            bad: Color::Rgb(0xe6, 0x7e, 0x80),
        }
    }
}

impl Theme {
    pub fn load(cfg: &Config) -> Theme {
        let mut theme = Theme::default();

        // 2) 独立的 theme.toml
        if let Some(path) = config::config_dir().map(|d| d.join("theme.toml"))
            && let Ok(text) = std::fs::read_to_string(&path) {
                match toml::from_str::<ThemeOverrides>(&text) {
                    Ok(ov) => theme.apply(&ov),
                    Err(e) => eprintln!("[term-palette] theme.toml 解析失败：{e}"),
                }
            }

        // 3) config.toml 的 [theme] 段
        theme.apply(&cfg.theme);
        theme
    }

    fn apply(&mut self, ov: &ThemeOverrides) {
        fn set(target: &mut Color, src: &Option<String>) {
            if let Some(v) = src.as_deref()
                && let Some(rgb) = Rgb::parse(v) {
                    *target = rgb.color();
                }
        }
        set(&mut self.background, &ov.background);
        set(&mut self.panel, &ov.panel);
        set(&mut self.overlay, &ov.overlay);
        set(&mut self.border, &ov.border);
        set(&mut self.fg, &ov.fg);
        set(&mut self.muted, &ov.muted);
        set(&mut self.dim, &ov.dim);
        set(&mut self.faint, &ov.faint);
        set(&mut self.accent, &ov.accent);
        set(&mut self.good, &ov.good);
        set(&mut self.warn, &ov.warn);
        set(&mut self.bad, &ov.bad);
    }
}
