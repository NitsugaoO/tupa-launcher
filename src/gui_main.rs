//! tupa-gui —— GPUI 面板。
//!
//! 独立于 `tupa-launcher.exe`：
//! - 自带托盘图标与全局热键（默认 `alt+0`，见 `[gui] hotkey`）
//! - 不依赖守护进程，`Esc` 只是隐藏，点托盘 / 按热键再回来
//! - `App` 状态机、provider、`exec` 与 TUI 共用（同一个 lib）

use anyhow::Result;

use tupa_launcher::{config, daemon, gui, i18n, theme, tray, util};

fn main() -> Result<()> {
    let cfg = config::Config::load()?;
    i18n::apply(&cfg);

    // 已经有一个面板进程：把它叫到前面就返回（单实例）
    if !util::find_panel(util::PanelKind::Gui).is_null() {
        util::show_panel(util::PanelKind::Gui);
        return Ok(());
    }

    if cfg.gui.tray {
        let hotkey = daemon::parse_hotkey(&cfg.gui.hotkey)?;
        tray::start(hotkey);
    }

    let theme = theme::Theme::load(&cfg);
    gui::run(cfg, theme)
}
