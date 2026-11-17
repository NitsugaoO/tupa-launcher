//! GPUI 版自绘面板。
//!
//! 分工（与 TUI 的关系）：
//! - `App`、provider、`exec`、`history` —— **原样复用**，一行不改
//! - `ui.rs` —— TUI 专用（ratatui Frame），这里不碰
//! - 本模块只负责把 `App` 的状态画成原生窗口：输入行 / 分隔线 / 结果列表 / 状态栏
//!
//! 窗口形态（2026-11-17 实测决定）：
//! - `WindowKind::Normal` + `titlebar.appears_transparent` —— 有 `WS_THICKFRAME`
//!   可拖边缩放、无可见标题栏；`PopUp` 虽然天然不进任务栏，但 `dwstyle = 0`
//!   根本不可缩放
//! - 创建后再把 `WS_EX_APPWINDOW` 换成 `WS_EX_TOOLWINDOW`（[`make_overlay_window`]），
//!   否则会占任务栏 / 出现在 Alt-Tab
//! - GPUI 的窗口类名是全框架共用的 `Zed::Window`，所以 `find_panel` 只能靠
//!   **进程名 + 窗口标题**认人

mod keys;
mod style;

use std::time::Duration;

use anyhow::Result;
use gpui::{
    AnyElement, AppContext as _, Bounds, Context, FocusHandle, KeyDownEvent, Render, TitlebarOptions,
    Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, prelude::*,
    px, size,
};

use crate::app::App;
use crate::config::Config;
use crate::providers::Item;
use crate::theme::Theme;
use crate::util::PanelKind;

use style::Palette;

/// 窗口标题：`find_panel` / `is_panel_title` 都认它，别随便改。
const WINDOW_TITLE: &str = "tupa-launcher";

/// Step 1 的列表只画前这么多条（Step 2 换成虚拟列表 + 滚动）
const MAX_ROWS: usize = 12;

pub fn run(cfg: Config, theme: Theme) -> Result<()> {
    // 已经开着就叫到前面，别开第二个
    #[cfg(windows)]
    if focus_existing() {
        return Ok(());
    }

    let app = App::new(cfg, theme);

    gpui::Application::new().run(move |cx| {
        let bounds = Bounds::centered(
            None,
            size(px(style::DEFAULT_WIDTH), px(style::DEFAULT_HEIGHT)),
            cx,
        );
        let result = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(WINDOW_TITLE.into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                kind: WindowKind::Normal,
                is_resizable: true,
                is_minimizable: true,
                focus: true,
                window_background: WindowBackgroundAppearance::Opaque,
                ..Default::default()
            },
            move |window, cx| cx.new(|cx| Panel::new(app, window, cx)),
        );
        if let Err(error) = result {
            eprintln!("[tupa-launcher] 创建 GPUI 窗口失败：{error:#}");
            cx.quit();
            return;
        }
        cx.activate(true);
    });

    Ok(())
}

/// 面板根 view
struct Panel {
    app: App,
    palette: Palette,
    focus: FocusHandle,
    /// 调试日志只写一次（`TUPA_GUI_LOG=<路径>` 时生效）
    logged: bool,
}

impl Panel {
    fn new(mut app: App, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 开屏（收藏 + 最近命令）要一次查询，provider 在工作线程里跑
        app.refresh();

        let palette = Palette::new(&app.theme);
        let focus = cx.focus_handle();
        window.focus(&focus);

        // 后台结果走 mpsc（`App::drain_events`），GPUI 没有 channel 唤醒，
        // 只能轻量轮询：查询中 16ms，空闲 120ms。拿到结果就 notify 一次。
        cx.spawn(async move |this, cx| {
            let mut pending = false;
            let mut styled = false;
            loop {
                let delay = if pending { 16 } else { 120 };
                cx.background_executor()
                    .timer(Duration::from_millis(delay))
                    .await;

                // 窗口已经真正上屏了，这时改 EXSTYLE 才生效（APPWINDOW → TOOLWINDOW）
                if !styled {
                    styled = true;
                    #[cfg(windows)]
                    make_overlay_window();
                }

                match this.update(cx, |panel, cx| {
                    let was_pending = panel.app.pending;
                    panel.app.drain_events();
                    let now_pending = panel.app.pending;
                    if was_pending != now_pending {
                        cx.notify();
                    }
                    now_pending
                }) {
                    Ok(next) => pending = next,
                    Err(_) => break, // 窗口和 view 都没了
                }
            }
        })
        .detach();

        Self {
            app,
            palette,
            focus,
            logged: false,
        }
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(key) = keys::to_key(&event.keystroke) {
            self.app.on_key(key);
        }

        if self.app.should_hide {
            self.app.should_hide = false;
            #[cfg(windows)]
            crate::util::hide_panel(PanelKind::Gui);
            return;
        }
        if self.app.should_quit {
            cx.quit();
            return;
        }

        // 焦点别丢（Esc 清空输入、点列表之后都要能继续打字）
        window.focus(&self.focus);
        cx.notify();
    }

    /// 输入行：`❯ ` + 输入内容 + 光标（Step 3 换成真光标 + 水平滚动）
    fn input_row(&self, p: Palette) -> AnyElement {
        let input = self.app.input.clone();
        div()
            .h(px(style::INPUT_HEIGHT))
            .flex()
            .flex_row()
            .items_center()
            .overflow_hidden()
            .child(
                div()
                    .text_size(px(style::INPUT_SIZE))
                    .text_color(p.accent)
                    .child(style::PROMPT),
            )
            .child(
                div()
                    .text_size(px(style::INPUT_SIZE))
                    .text_color(p.fg)
                    .child(input),
            )
            .child(
                div()
                    .text_size(px(style::INPUT_SIZE))
                    .text_color(p.accent)
                    .child(style::CURSOR),
            )
            .into_any_element()
    }

    fn separator(&self, p: Palette) -> AnyElement {
        div().h(px(1.)).w_full().bg(p.border).into_any_element()
    }

    /// 结果列表（Step 1：纯文本行；Step 2 换虚拟列表，Step 3 上卡片样式）
    fn list_area(&self, p: Palette) -> AnyElement {
        let selected = self.app.list_state.selected();
        let rows: Vec<AnyElement> = self
            .app
            .items
            .iter()
            .take(MAX_ROWS)
            .enumerate()
            .map(|(index, item)| self.row(index, item, selected == Some(index), p))
            .collect();

        if rows.is_empty() {
            let hint = if self.app.loaded {
                self.app.empty_hint()
            } else {
                String::new()
            };
            return div()
                .flex_1()
                .px(px(8.))
                .py(px(6.))
                .text_color(p.dim)
                .child(hint)
                .into_any_element();
        }

        div()
            .flex_1()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(style::ROW_GAP))
            .py(px(6.))
            .overflow_hidden()
            .children(rows)
            .into_any_element()
    }

    fn row(&self, index: usize, item: &Item, selected: bool, p: Palette) -> AnyElement {
        let title = item.columns.first().cloned().unwrap_or_default();
        let note = if item.columns.len() > 1 {
            item.columns[item.columns.len() - 1].clone()
        } else {
            String::new()
        };

        let mut row = div()
            .id(("row", index))
            .h(px(style::ROW_HEIGHT))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(8.))
            .rounded(px(style::ROW_RADIUS));

        if selected {
            row = row.bg(p.overlay);
        }

        row.child(
            // 选中行左侧的强调条；未选中时留同样宽度占位，避免文字左右跳
            div()
                .w(px(style::ACCENT_BAR))
                .h_full()
                .bg(if selected { p.accent } else { p.background }),
        )
        .child(
            div()
                .w(px(style::BADGE_WIDTH))
                .h(px(style::BADGE_HEIGHT))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(style::BADGE_RADIUS))
                .bg(p.background)
                .text_size(px(style::BADGE_SIZE))
                .text_color(style::badge_color(item.kind))
                .child(item.kind.badge()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .text_size(px(style::ROW_SIZE))
                .text_color(p.fg)
                .child(style::fit(&title, style::TITLE_COLS)),
        )
        .child(
            div()
                .whitespace_nowrap()
                .text_size(px(style::NOTE_SIZE))
                .text_color(p.dim)
                .child(style::fit(&note, style::NOTE_COLS)),
        )
        .into_any_element()
    }

    /// 状态栏：左 = 状态消息 / provider 说明，右 = 选中位置
    fn status_bar(&self, p: Palette) -> AnyElement {
        let (left, color) = match &self.app.status {
            Some((message, true)) => (message.clone(), p.bad),
            Some((message, false)) => (message.clone(), p.good),
            None => (self.app.active_description().to_owned(), p.muted),
        };
        let right = match self.app.list_state.selected() {
            Some(index) => format!("{}/{}", index + 1, self.app.items.len()),
            None => String::new(),
        };

        div()
            .h(px(style::STATUS_HEIGHT))
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(style::STATUS_SIZE))
            .child(div().text_color(color).child(left))
            .child(div().text_color(p.faint).child(right))
            .into_any_element()
    }
}

impl Render for Panel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 后台查询结果（provider 在工作线程里跑，走 mpsc）
        self.app.drain_events();

        if !self.logged && !self.app.items.is_empty() {
            self.logged = true;
            let first = &self.app.items[0];
            let raw = first.columns.first().map(String::as_str).unwrap_or("");
            debug_log(&format!(
                "rows={} cols={:?} first_raw={:?} first_fit={:?}",
                self.app.items.len(),
                first.columns,
                raw,
                style::fit(raw, style::TITLE_COLS),
            ));
        }

        let p = self.palette;
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(p.background)
            .rounded(px(style::RADIUS))
            .p(px(style::PADDING))
            .font_family(style::FONT)
            .font_weight(style::FONT_WEIGHT)
            .text_color(p.fg)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::handle_key))
            .child(self.input_row(p))
            .child(self.separator(p))
            .child(self.list_area(p))
            .child(self.status_bar(p))
    }
}

/// 把一行写进 `TUPA_GUI_LOG` 指向的文件（不设这个环境变量就什么都不做）
fn debug_log(message: &str) {
    use std::io::Write;

    let Ok(path) = std::env::var("TUPA_GUI_LOG") else {
        return;
    };
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{message}");
    }
}

/// 已经有面板窗口了吗？有就叫到前面。
#[cfg(windows)]
fn focus_existing() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsWindowVisible, SW_SHOW, SetForegroundWindow, ShowWindow,
    };

    let hwnd = crate::util::find_panel(PanelKind::Gui);
    if hwnd.is_null() {
        return false;
    }
    unsafe {
        if IsWindowVisible(hwnd) == 0 {
            ShowWindow(hwnd, SW_SHOW);
        }
        SetForegroundWindow(hwnd);
    }
    true
}

/// 把 GUI 面板窗口变成「浮层」：去掉 `WS_EX_APPWINDOW`、换上 `WS_EX_TOOLWINDOW`。
///
/// GPUI 只有 `WindowKind::Normal` 才保留 `WS_THICKFRAME`（可拖边缩放），代价是
/// 会带 `WS_EX_APPWINDOW`（占任务栏 + 进 Alt-Tab）。这一步把两边要的都拿回来。
/// `PopUp` 虽然天然是 TOOLWINDOW，但 `dwstyle = 0`，压根不可缩放。
#[cfg(windows)]
fn make_overlay_window() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };

    let hwnd = crate::util::find_panel(PanelKind::Gui);
    if hwnd.is_null() {
        return;
    }
    unsafe {
        let extended = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let wanted = (extended & !WS_EX_APPWINDOW) | WS_EX_TOOLWINDOW;
        if wanted == extended {
            return;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted as isize);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}
