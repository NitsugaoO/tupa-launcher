//! tupa-launcher 的库部分。
//!
//! 两个可执行文件共用这里的模块：
//! - `src/main.rs` → `tupa-launcher.exe`（TUI + 守护进程，不含 GPUI）
//! - `src/gui_main.rs` → `tupa-gui.exe`（GPUI 面板 + 托盘 + 热键，需 feature `gui`）

pub mod actions;
pub mod app;
pub mod clipboard;
pub mod config;
pub mod destlist;
pub mod explorer_watch;
#[cfg(windows)]
pub mod daemon;
pub mod exec;
pub mod favorites;
pub mod glazewm;
#[cfg(all(windows, feature = "gui"))]
pub mod tray;
#[cfg(feature = "gui")]
pub mod gui;
pub mod history;
pub mod i18n;
pub mod providers;
pub mod serve;
pub mod theme;
pub mod ui;
pub mod util;
