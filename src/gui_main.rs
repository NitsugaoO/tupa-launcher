#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! tupa-gui —— GPUI 面板。
//!
//! 独立于 `tupa-launcher.exe`：
//! - 自带托盘图标与全局热键（默认 `alt+0`，见 `[gui] hotkey`）
//! - 不依赖守护进程，`Esc` 只是隐藏，点托盘 / 按热键再回来
//! - `App` 状态机、provider、`exec` 与 TUI 共用（同一个 lib）

use anyhow::Result;

use tupa_launcher::{config, daemon, gui, i18n, theme, tray, util};

/// ratatui 的颜色 → 图标用的 `(r,g,b)`
fn rgb(color: ratatui::style::Color) -> (u8, u8, u8) {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        _ => (0x27, 0x2e, 0x33),
    }
}

fn main() -> Result<()> {
    // 不带黑控制台窗口：释放控制台（`#![windows_subsystem = "windows"]` 会让 GPUI 静默崩掉）
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Console::FreeConsole()
    };

    // panic 时写到日志（无控制台时 stderr 看不见）
    std::panic::set_hook(Box::new(|info| {
        tray::log(&format!("panic: {info}"));
    }));

    let cfg = config::Config::load()?;
    i18n::apply(&cfg);

    // 已经有一个面板进程：把它叫到前面就返回（单实例）
    if !util::find_panel(util::PanelKind::Gui).is_null() {
        util::show_panel(util::PanelKind::Gui);
        return Ok(());
    }

    let theme = theme::Theme::load(&cfg);

    if cfg.gui.tray {
        let hotkey = daemon::parse_hotkey(&cfg.gui.hotkey)?;
        let icon = gui::icon::draw(rgb(theme.background), rgb(theme.accent));
        tray::start(hotkey, icon as isize);
    }

    gui::run(cfg, theme)
}
