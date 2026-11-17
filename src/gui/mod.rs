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

mod input;
mod keys;
mod style;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use gpui::{
    AnyElement, AppContext as _, Bounds, Context, FocusHandle, KeyDownEvent, Render, Rgba,
    ScrollHandle, ScrollStrategy, TitlebarOptions, UniformListScrollHandle, WeakEntity, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, prelude::*, px, size,
    uniform_list,
};

use crate::app::App;
use crate::config::Config;
use crate::i18n;
use crate::theme::Theme;
use crate::util::PanelKind;

use style::Palette;

/// 窗口标题：`find_panel` / `is_panel_title` 都认它，别随便改。
const WINDOW_TITLE: &str = "tupa-launcher";

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
    /// 结果列表的滚动状态（`uniform_list` 虚拟化）
    scroll: UniformListScrollHandle,
    /// detail 模式用的是非等高行，走普通滚动
    detail_scroll: ScrollHandle,
    /// 上一帧的选中项，用来判断要不要跟随滚动
    last_selected: Option<usize>,
    /// `?` 打开的快捷键帮助浮层
    help: bool,
    /// 光标是否显示（500ms 翻转一次，模拟闪烁）
    cursor_on: bool,
    /// 输入法预编辑：`(起始字符下标, 文本)`，还没上屏的拼音
    preedit: Option<(usize, String)>,
    /// 上一次写进调试日志的结果条数（`TUPA_GUI_LOG=<路径>` 时生效）
    logged_rows: usize,
    /// 上一次写进调试日志的输入框内容
    logged_input: String,
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
            let mut last_blink = std::time::Instant::now();
            loop {
                let delay = if pending { 16 } else { 120 };
                cx.background_executor()
                    .timer(Duration::from_millis(delay))
                    .await;

                // 光标闪烁：500ms 一次，顺便让状态栏的「加载中」有机会刷新
                if last_blink.elapsed() >= Duration::from_millis(500) {
                    last_blink = std::time::Instant::now();
                    let _ = this.update(cx, |panel, cx| {
                        panel.cursor_on = !panel.cursor_on;
                        cx.notify();
                    });
                }

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
            scroll: UniformListScrollHandle::new(),
            detail_scroll: ScrollHandle::new(),
            last_selected: None,
            help: false,
            cursor_on: true,
            preedit: None,
            logged_rows: usize::MAX,
            logged_input: String::new(),
        }
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = keys::to_key(&event.keystroke);
        let question = matches!(
            key,
            Some(crossterm::event::KeyEvent {
                code: crossterm::event::KeyCode::Char('?'),
                ..
            })
        );

        // 帮助浮层：`?` 开关，其它任意键关掉（Esc 也关）
        if self.help {
            if !question {
                self.help = false;
            }
            window.focus(&self.focus);
            cx.notify();
            return;
        }
        if question {
            self.help = true;
            window.focus(&self.focus);
            cx.notify();
            return;
        }

        if let Some(key) = key {
            // 普通字符由 IME / input handler 通过 `replace_text_in_range` 送进来，
            // 这里再 insert 一次就成双份了。只有选择模式的 1–9 要自己处理。
            let select_digit = self.app.select_mode
                && matches!(
                    key.code,
                    crossterm::event::KeyCode::Char('1'..='9')
                );
            let plain_text = matches!(key.code, crossterm::event::KeyCode::Char(_))
                && !key
                    .modifiers
                    .intersects(crossterm::event::KeyModifiers::CONTROL | crossterm::event::KeyModifiers::ALT);
            if !plain_text || select_digit {
                self.app.on_key(key);
            }
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
    fn input_row(&self, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        // 光标按 `app.cursor` 拆开，前后各画一段，中间是竖线
        let at = self
            .app
            .input
            .char_indices()
            .nth(self.app.cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.app.input.len());
        let (before, after) = self.app.input.split_at(at);

        let mut row = div()
            .h(px(style::INPUT_HEIGHT))
            .flex()
            .flex_row()
            .items_center()
            .relative()
            .overflow_hidden()
            .child(
                // 文本输入 / IME 的注册点：`handle_input` 只能在 paint 阶段调用。
                // 用 1×1 的绝对定位块贴在行首——0×0 的元素不会 paint，
                // handler 就永远注册不上。
                gpui::canvas(
                    |bounds, _window, _cx| bounds,
                    {
                        let focus = self.focus.clone();
                        let entity = cx.entity();
                        move |bounds, _prepainted, window, cx| {
                            register_input_handler(bounds, &focus, entity.clone(), window, cx);
                        }
                    },
                )
                .absolute()
                .top(px(0.))
                .left(px(0.))
                .w(px(1.))
                .h(px(1.)),
            )
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
                    .child(before.to_owned()),
            );

        if self.cursor_on {
            row = row.child(
                div()
                    .w(px(style::CURSOR_WIDTH))
                    .h(px(style::CURSOR_HEIGHT))
                    .flex_shrink_0()
                    .bg(p.accent),
            );
        }

        // 输入法预编辑（拼音还没上屏）：插在光标处，画成带底色的临时文本
        if let Some((_, text)) = &self.preedit {
            row = row.child(
                div()
                    .px(px(1.))
                    .rounded(px(2.))
                    .bg(p.panel)
                    .text_size(px(style::INPUT_SIZE))
                    .text_color(p.fg)
                    .child(text.clone()),
            );
        }

        row.child(
            div()
                .text_size(px(style::INPUT_SIZE))
                .text_color(p.fg)
                .child(after.to_owned()),
        )
        .into_any_element()
    }

    fn separator(&self, p: Palette) -> AnyElement {
        div().h(px(1.)).w_full().bg(p.border).into_any_element()
    }

    /// 结果列表：`uniform_list` 虚拟化（`@hist` 200 条 / `@web` 上千条也不会全渲染）
    fn list_area(&self, window: &Window, cx: &mut Context<Self>, p: Palette) -> AnyElement {
        if self.app.items.is_empty() {
            let hint = if self.app.pending {
                i18n::loading().to_string()
            } else if self.app.loaded {
                self.app.empty_hint()
            } else {
                String::new()
            };
            return div()
                .flex_1()
                .w_full()
                .px(px(8.))
                .py(px(6.))
                .text_color(p.dim)
                .child(hint)
                .into_any_element();
        }

        // 列宽按真实窗口宽度算（等宽字体 + truncate 由 GPUI 按像素裁）
        let total = f32::from(window.bounds().size.width) - style::PADDING * 2.0;
        let note_width = (total * 0.32).clamp(80.0, 240.0);
        let fixed = style::ACCENT_BAR
            + 8.0 * 3.0
            + style::NUMBER_WIDTH
            + style::STAR_WIDTH
            + style::BADGE_WIDTH;
        let metrics = RowMetrics {
            title: (total - fixed - note_width).max(80.0),
            note: note_width,
        };

        let rows = Arc::new(self.snapshot());
        let count = rows.len();
        let panel = cx.entity().downgrade();

        // detail 模式行高不等，退化成普通滚动容器（条目数一般不多）
        if self.app.detail {
            let mut block = div()
                .id("detail-list")
                .flex_1()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(2.))
                .py(px(6.))
                .overflow_y_scroll()
                .track_scroll(&self.detail_scroll);
            for row in rows.iter() {
                block = block.child(render_detail_row(row, p, metrics, &panel));
            }
            return block.into_any_element();
        }

        div()
            .flex_1()
            .w_full()
            .flex()
            .flex_col()
            .child(div().h(px(style::LIST_TOP)).flex_shrink_0())
            .child(
                uniform_list("results", count, move |range, _window, _cx| {
                    range
                        .map(|index| render_row(&rows[index], p, metrics, &panel))
                        .collect()
                })
                .flex_1()
                .w_full()
                .track_scroll(self.scroll.clone()),
            )
            .into_any_element()
    }

    /// 把当前结果快照成「一行一个结构体」，供 `uniform_list` 的闭包使用
    /// （闭包拿到的是 `&mut App`，碰不到 `self`）
    fn snapshot(&self) -> Vec<RowSnapshot> {
        let selected = self.app.list_state.selected();
        let select_mode = self.app.select_mode;
        self.app
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| RowSnapshot {
                index,
                columns: item.columns.clone(),
                badge: item.kind.badge(),
                color: style::badge_color(item.kind),
                selected: selected == Some(index),
                favorite: self.app.is_favorite(item),
                unavailable: matches!(item.control, crate::providers::Control::Unavailable),
                number: (select_mode && index < 9).then_some(index + 1),
            })
            .collect()
    }

    /// `?` 打开的键位说明（替换列表区）
    fn help_panel(&self, p: Palette) -> AnyElement {
        let mut list = div().w_full().flex().flex_col().gap(px(7.)).py(px(8.));
        for (keys, description) in i18n::gui_help() {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(
                        div()
                            .w(px(150.))
                            .flex_shrink_0()
                            .text_color(p.accent)
                            .child(keys),
                    )
                    .child(div().text_color(p.fg).child(description)),
            );
        }

        div()
            .flex_1()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .py(px(4.))
                    .text_size(px(style::STATUS_SIZE))
                    .text_color(p.dim)
                    .child(i18n::t(
                        "快捷键（按任意键关闭）",
                        "Keyboard shortcuts (any key closes)",
                    )),
            )
            .child(list)
            .into_any_element()
    }

    /// 状态栏：左 = 状态消息 / provider 说明，右 = 选中位置
    fn status_bar(&self, p: Palette) -> AnyElement {
        let (left, color) = match &self.app.status {
            Some((message, true)) => (message.clone(), p.bad),
            Some((message, false)) => (message.clone(), p.good),
            None => (self.app.active_description().to_owned(), p.muted),
        };
        let position = match self.app.list_state.selected() {
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().text_color(p.faint).child(position))
                    // 键位说明进二级菜单：按 `?` 打开
                    .child(
                        div()
                            .text_color(p.dim)
                            .child(i18n::t("? 键位", "? keys")),
                    ),
            )
            .into_any_element()
    }
}

struct RowSnapshot {
    index: usize,
    columns: Vec<String>,
    badge: &'static str,
    color: Rgba,
    selected: bool,
    favorite: bool,
    /// 开关类条目不可用（灰掉，提示是死项）
    unavailable: bool,
    /// 选择模式下的跳转序号（1–9）
    number: Option<usize>,
}

/// 行内固定列的宽度（`number` 只在选择模式下占位）
#[derive(Clone, Copy)]
struct RowMetrics {
    title: f32,
    note: f32,
}

/// 画一行结果卡片（普通模式）。
///
/// 宽度必须显式给（`w(px(n))`）——GPUI 的 `truncate()` 只对确定宽度的元素生效，
/// `flex_1` 的容器会被内容撑开、文字画到窗口外。
fn render_row(
    row: &RowSnapshot,
    p: Palette,
    metrics: RowMetrics,
    panel: &WeakEntity<Panel>,
) -> AnyElement {
    let title = row.columns.first().cloned().unwrap_or_default();
    let note = if row.columns.len() > 1 {
        row.columns[row.columns.len() - 1].clone()
    } else {
        String::new()
    };
    let fg = if row.unavailable { p.faint } else { p.fg };

    let mut card = div()
        .id(("row", row.index))
        .h(px(style::ROW_HEIGHT))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .px(px(8.))
        .rounded(px(style::ROW_RADIUS))
        .whitespace_nowrap()
        .hover(|style| style.bg(p.panel));

    if row.selected {
        card = card.bg(p.overlay);
    }

    let card = card
        .child(
            // 选中行左侧的强调条；未选中时留同样宽度占位，避免文字左右跳
            div()
                .w(px(style::ACCENT_BAR))
                .h_full()
                .bg(if row.selected { p.accent } else { p.background }),
        )
        .child(
            // 选择模式（Shift+Tab）下的 1-9 跳转序号
            div()
                .w(px(style::NUMBER_WIDTH))
                .flex_shrink_0()
                .text_size(px(style::NOTE_SIZE))
                .text_color(p.dim)
                .child(row.number.map(|n| n.to_string()).unwrap_or_default()),
        )
        .child(
            // 收藏星标（未收藏也占位，保证列对齐）
            div()
                .w(px(style::STAR_WIDTH))
                .flex_shrink_0()
                .text_color(p.warn)
                .child(if row.favorite { "\u{2605}" } else { "" }),
        )
        .child(
            // chip：胶囊底；未选中灰底 + 类型色字，选中行反白成类型色底 + 深色字
            div()
                .w(px(style::BADGE_WIDTH))
                .h(px(style::BADGE_HEIGHT))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(style::BADGE_RADIUS))
                .bg(if row.selected { row.color } else { p.panel })
                .text_size(px(style::BADGE_SIZE))
                .text_color(if row.selected { p.background } else { row.color })
                .child(row.badge),
        )
        .child(
            div()
                .w(px(metrics.title))
                .flex_shrink_0()
                .truncate()
                .text_size(px(style::ROW_SIZE))
                .text_color(fg)
                .child(title),
        )
        .child(
            div()
                .w(px(metrics.note))
                .flex_shrink_0()
                .truncate()
                .text_size(px(style::NOTE_SIZE))
                .text_color(p.dim)
                .child(note),
        )
        .on_click({
            let panel = panel.clone();
            let index = row.index;
            move |_event, _window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    panel.app.list_state.select(Some(index));
                    cx.notify();
                });
            }
        });

    // `uniform_list` 的行高 = 第一项高度，间距只能塞进行高里
    div()
        .w_full()
        .h(px(style::ROW_HEIGHT + style::ROW_GAP))
        .flex()
        .items_center()
        .child(card)
        .into_any_element()
}

/// detail 模式（`Ctrl+Tab`）：第一列作标题，其余列竖排
fn render_detail_row(
    row: &RowSnapshot,
    p: Palette,
    metrics: RowMetrics,
    panel: &WeakEntity<Panel>,
) -> AnyElement {
    let head = row.columns.first().cloned().unwrap_or_default();
    let fg = if row.unavailable { p.faint } else { p.fg };
    let indent = style::ACCENT_BAR
        + 8.0 * 2.0
        + style::STAR_WIDTH
        + style::BADGE_WIDTH
        + 8.0;

    let mut block = div()
        .id(("detail", row.index))
        .w_full()
        .flex()
        .flex_col()
        .gap(px(2.))
        .py(px(4.))
        .px(px(8.))
        .rounded(px(style::ROW_RADIUS))
        .whitespace_nowrap()
        .hover(|style| style.bg(p.panel));

    if row.selected {
        block = block.bg(p.overlay);
    }

    let mut block = block.child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .w(px(style::ACCENT_BAR))
                    .h(px(style::ROW_HEIGHT - 6.0))
                    .bg(if row.selected { p.accent } else { p.background }),
            )
            .child(
                div()
                    .w(px(style::STAR_WIDTH))
                    .flex_shrink_0()
                    .text_color(p.warn)
                    .child(if row.favorite { "\u{2605}" } else { "" }),
            )
            .child(
                div()
                    .w(px(style::BADGE_WIDTH))
                    .h(px(style::BADGE_HEIGHT))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(style::BADGE_RADIUS))
                    .bg(if row.selected { row.color } else { p.panel })
                    .text_size(px(style::BADGE_SIZE))
                    .text_color(if row.selected { p.background } else { row.color })
                    .child(row.badge),
            )
            .child(
                div()
                    .w(px(metrics.title))
                    .flex_shrink_0()
                    .truncate()
                    .text_size(px(style::ROW_SIZE))
                    .text_color(fg)
                    .child(head),
            ),
    );

    for value in row.columns.iter().skip(1) {
        block = block.child(
            div()
                .pl(px(indent))
                .w_full()
                .truncate()
                .h(px(style::DETAIL_LINE))
                .text_size(px(style::NOTE_SIZE))
                .text_color(p.dim)
                .child(value.clone()),
        );
    }

    block
        .on_click({
            let panel = panel.clone();
            let index = row.index;
            move |_event, _window, cx| {
                let _ = panel.update(cx, |panel, cx| {
                    panel.app.list_state.select(Some(index));
                    cx.notify();
                });
            }
        })
        .into_any_element()
}

impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 后台查询结果（provider 在工作线程里跑，走 mpsc）
        self.app.drain_events();

        // 选中项跟手滚动（键盘 ↑↓ / 鼠标点击都走这里）
        let selected = self.app.list_state.selected();
        if selected != self.last_selected {
            self.last_selected = selected;
            if let Some(index) = selected {
                if self.app.detail {
                    self.detail_scroll.scroll_to_item(index);
                } else {
                    self.scroll.scroll_to_item(index, ScrollStrategy::Center);
                }
            }
        }

        // 输入框内容变了就记一行（排查 IME 提交 / 双插入）
        if self.app.input != self.logged_input {
            self.logged_input = self.app.input.clone();
            debug_log(&format!(
                "input={:?} cursor={} preedit={:?}",
                self.app.input, self.app.cursor, self.preedit
            ));
        }

        // 结果条数变了就记一行（`TUPA_GUI_LOG` 只用于开发期排查）
        if self.app.items.len() != self.logged_rows {
            self.logged_rows = self.app.items.len();
            let head = self
                .app
                .items
                .first()
                .and_then(|item| item.columns.first())
                .cloned()
                .unwrap_or_default();
            debug_log(&format!(
                "rows={} sel={:?} first={head:?}",
                self.app.items.len(),
                self.app.list_state.selected(),
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
            .child(self.input_row(p, cx))
            .child(self.separator(p))
            .child(if self.help {
                self.help_panel(p)
            } else {
                self.list_area(window, cx, p)
            })
            .child(self.status_bar(p))
    }
}

/// 把 Panel 注册成窗口的输入处理器（IME 与普通字符都走它）
fn register_input_handler(
    bounds: Bounds<gpui::Pixels>,
    focus: &FocusHandle,
    entity: gpui::Entity<Panel>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    window.handle_input(
        focus,
        gpui::ElementInputHandler::new(bounds, entity),
        cx,
    );
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
