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

pub mod icon;
mod input;
mod keys;
mod style;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use gpui::{
    AnyElement, AppContext as _, Bounds, Context, FocusHandle, KeyDownEvent, Render, Rgba,
    ScrollDelta, ScrollWheelEvent, TitlebarOptions, WeakEntity, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, prelude::*, px, size,
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

    let opacity = cfg.gui.opacity.clamp(0.3, 1.0);
    let animations = cfg.gui.animations;
    let app = App::new(cfg, theme);

    gpui::Application::new().run(move |cx| {
        // 尺寸与 TUI 面板对齐（屏幕的 62%×55%），居中放在主屏上。
        // GlazeWM 那边这条规则是 `ignore`（面板是浮层，不该进 tiling / 不归 workspace 管），
        // 所以位置和大小由这里说了算。
        let display = cx.primary_display();
        let bounds = match &display {
            Some(display) => {
                let area = display.bounds();
                let width = px(f32::from(area.size.width) * style::WIDTH_RATIO);
                let height = px(f32::from(area.size.height) * style::HEIGHT_RATIO);
                Bounds::centered(Some(display.id()), size(width, height), cx)
            }
            None => Bounds::centered(
                None,
                size(px(style::DEFAULT_WIDTH), px(style::DEFAULT_HEIGHT)),
                cx,
            ),
        };
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
                // 只用 Transparent（纯 alpha）。`Blurred` 走的是
                // `SetWindowCompositionAttribute(ACCENT_ENABLE_ACRYLICBLURBEHIND)`，
                // 但 GPUI 的窗口是 DirectComposition 合成，DWM 的 accent 会把整个窗口
                // 涂成黑玻璃（实测四角发黑、底色全黑），看不到模糊。
                window_background: if opacity < 1.0 {
                    WindowBackgroundAppearance::Transparent
                } else {
                    WindowBackgroundAppearance::Opaque
                },
                ..Default::default()
            },
            move |window, cx| cx.new(|cx| Panel::new(app, opacity, animations, window, cx)),
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
    /// 垂直滚动：当前位置 / 目标位置（逻辑像素，自己插值才算平滑）
    scroll_y: f32,
    scroll_target: f32,
    /// 列表可视区高度与最大滚动量（每帧由 list_area 算好）
    scroll_viewport: f32,
    scroll_max: f32,
    /// 是否开动画（`[gui] animations`）
    animations: bool,
    /// 选框滑动的当前 y / 目标 y（像素）
    marker_y: f32,
    marker_target: f32,
    /// 选框是否正在滑动（滑动期间 chip 不点亮，落地后再渐变成彩色）
    marker_moving: bool,
    /// 上一次选中项（它需要在滑动开始时从彩色渐隐回灰）
    chip_from: Option<usize>,
    /// 开始滑动的时刻 / 选框落地的时刻（chip 渐变的两端）
    chip_fade_start: std::time::Instant,
    chip_settled_at: std::time::Instant,
    /// 光标滑动的当前 x / 目标 x（像素）
    caret_x: f32,
    caret_target: f32,
    /// 光标块的高度与上边界（按字体 ascent/descent 算，和文本行严格对齐）
    caret_height: f32,
    caret_top: f32,
    /// 上一帧的选中项，用来判断要不要跟随滚动
    last_selected: Option<usize>,
    /// `?` 打开的快捷键帮助浮层
    help: bool,
    /// 光标是否显示（500ms 翻转一次，模拟闪烁）
    cursor_on: bool,
    /// 下一次翻转光标的时间点（移动光标后重置，让它先亮着）
    blink_at: std::time::Instant,
    /// 输入法预编辑：`(起始字符下标, 文本)`，还没上屏的拼音
    preedit: Option<(usize, String)>,
    /// 紧凑态下按 ↓ 手动展开的开屏列表（收藏 + 最近命令）
    hint_open: bool,
    /// 上一帧算出来的行宽（左右滚动要用）
    row_metrics: RowMetrics,
    /// 窗口 DPI 缩放（改高度时要把逻辑像素换成物理像素）
    scale: f32,
    /// 面板背景不透明度（来自 `[gui] opacity`）
    opacity: f32,
    /// 选中行变化的时间点（驱动左右滚动）
    scroll_started: std::time::Instant,
    /// 列表展开的时间点（驱动淡入）
    reveal_started: std::time::Instant,
    /// 上一帧是不是紧凑形态（用来检测「刚展开」）
    was_compact: bool,
    /// 上一次写进调试日志的结果条数（`TUPA_GUI_LOG=<路径>` 时生效）
    logged_rows: usize,
    /// 上一次写进调试日志的输入框内容
    logged_input: String,
    /// 上一次的「是否需要左右滚动」（只在变化时记日志）
    logged_scroll: bool,
}

impl Panel {
    fn new(mut app: App, opacity: f32, animations: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 开屏（收藏 + 最近命令）要一次查询，provider 在工作线程里跑
        app.refresh();

        let palette = Palette::new(&app.theme);
        let focus = cx.focus_handle();
        window.focus(&focus);

        // 后台结果走 mpsc（`App::drain_events`），GPUI 没有 channel 唤醒，
        // 只能轻量轮询：查询中 16ms，空闲 120ms。拿到结果就 notify 一次。
        cx.spawn_in(window, async move |this, cx| {
            let mut pending = false;
            let mut styled = false;
            // 有滚动或展开动画在进行 → 按 60fps 重绘
            let mut animating = false;
            loop {
                let delay = if pending || animating { style::ANIM_FRAME_MS } else { 120 };
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
                    // 光标闪烁：到点就翻一次（移动光标后 `blink_at` 会被推后）
                    let now = std::time::Instant::now();
                    if now >= panel.blink_at {
                        panel.cursor_on = !panel.cursor_on;
                        panel.blink_at = now + Duration::from_millis(500);
                        cx.notify();
                    }

                    // 推进垂直滚动 / 选框 / 光标动画；再叠加左右滚动与展开淡入，
                    // 任一还在动就按 60fps 重绘
                    let moving = panel.animate();
                    let scrolling = panel.needs_scroll();
                    let revealing =
                        panel.reveal_started.elapsed().as_secs_f32() < style::REVEAL_SECS;
                    let animating = moving || scrolling || revealing;
                    if animating {
                        cx.notify();
                    }
                    panel.app.tick += 1;

                    let was_pending = panel.app.pending;
                    panel.app.drain_events();
                    let now_pending = panel.app.pending;
                    if was_pending != now_pending {
                        cx.notify();
                    }
                    (now_pending, animating)
                }) {
                    Ok((next_pending, next_anim)) => {
                        pending = next_pending;
                        animating = next_anim;
                    }
                    Err(_) => break, // 窗口和 view 都没了
                }
            }
        })
        .detach();

        Self {
            app,
            palette,
            focus,
            scroll_y: 0.0,
            scroll_target: 0.0,
            scroll_viewport: 0.0,
            scroll_max: 0.0,
            animations,
            marker_y: 0.0,
            marker_target: 0.0,
            marker_moving: false,
            chip_from: None,
            chip_fade_start: std::time::Instant::now(),
            chip_settled_at: std::time::Instant::now(),
            caret_x: 0.0,
            caret_target: 0.0,
            caret_height: style::INPUT_SIZE,
            caret_top: (style::INPUT_HEIGHT - style::INPUT_SIZE) / 2.0,
            last_selected: None,
            help: false,
            cursor_on: true,
            blink_at: std::time::Instant::now() + Duration::from_millis(500),
            preedit: None,
            hint_open: false,
            row_metrics: RowMetrics { title: 0.0, note: 0.0 },
            scale: 1.0,
            opacity,
            scroll_started: std::time::Instant::now(),
            reveal_started: std::time::Instant::now(),
            was_compact: true,
            logged_rows: usize::MAX,
            logged_input: String::new(),
            logged_scroll: false,
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

        // 紧凑态（刚呼出）：按 ↓ 才把开屏列表（收藏 + 最近命令）拉下来
        let plain_down = matches!(
            &key,
            Some(k) if k.code == crossterm::event::KeyCode::Down && k.modifiers.is_empty()
        );
        if self.compact() && plain_down {
            self.hint_open = true;
            window.focus(&self.focus);
            cx.notify();
            return;
        }

        // 展开着的时候，Esc 先收回紧凑形态，而不是直接隐藏面板
        let is_escape = matches!(
            &key,
            Some(k) if k.code == crossterm::event::KeyCode::Esc
        );
        if self.hint_open && is_escape {
            self.hint_open = false;
            window.focus(&self.focus);
            cx.notify();
            return;
        }

        let before = (self.app.input.len(), self.app.cursor);
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

        // 光标动了（←→ / Home / End / 打字）就让它先亮着，再重新开始闪
        if (self.app.input.len(), self.app.cursor) != before {
            self.cursor_on = true;
            self.blink_at = std::time::Instant::now() + Duration::from_millis(500);
            // 清空输入 = 回到刚呼出的紧凑形态
            if self.app.input.is_empty() {
                self.hint_open = false;
            }
        }

        // 焦点别丢（Esc 清空输入、点列表之后都要能继续打字）
        window.focus(&self.focus);
        cx.notify();
    }

    /// 双击列表项 = 按回车（执行当前项）
    fn run_selected(&mut self, cx: &mut Context<Self>) {
        self.app.on_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        if self.app.should_hide {
            self.app.should_hide = false;
            #[cfg(windows)]
            crate::util::hide_panel(PanelKind::Gui);
        }
        if self.app.should_quit {
            cx.quit();
        }
    }

    /// 输入行：`❯ ` + 输入内容 + 光标（Step 3 换成真光标 + 水平滚动）
    fn input_row(&mut self, window: &Window, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        // 光标按 `app.cursor` 拆开，前后各画一段
        let at = self
            .app
            .input
            .char_indices()
            .nth(self.app.cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.app.input.len());
        let (before, after) = self.app.input.split_at(at);

        // 光标的目标位置：提示符 + 已输入 + 预编辑（全部按真实字形宽度量，不猜格数）
        let preedit = self.preedit.as_ref().map(|(_, text)| text.clone());
        let prompt_width = text_width(window, style::PROMPT, style::INPUT_SIZE);
        let before_width = text_width(window, before, style::INPUT_SIZE);
        let preedit_width = preedit
            .as_ref()
            .map(|text| text_width(window, text, style::INPUT_SIZE))
            .unwrap_or(0.0);
        self.caret_target = prompt_width + before_width + preedit_width;

        // 光标高度 = 文本行高（ascent + descent），上下边界与字形一致
        {
            let text_system = window.text_system();
            let font_id = text_system.resolve_font(&gpui::font(style::FONT));
            let size = px(style::INPUT_SIZE);
            let ascent = f32::from(text_system.ascent(font_id, size));
            // 注意：GPUI 的 `descent()` 返回**负值**（形如 -4.5），行高是 ascent - descent
            let descent = f32::from(text_system.descent(font_id, size));
            let line_height = ascent - descent;
            if line_height > 0.0 {
                self.caret_height = line_height;
                self.caret_top = (style::INPUT_HEIGHT - self.caret_height) / 2.0;
                {
                    // 只在几何变化时记一行（对比紧凑态 / 展开态）
                    if (self.caret_height - line_height).abs() > 0.05
                        || (self.caret_top - (style::INPUT_HEIGHT - line_height) / 2.0).abs() > 0.05
                    {
                        debug_log(&format!(
                            "caret 变化: h {:.1} -> {:.1}, top {:.1} -> {:.1} (line={:.1})",
                            self.caret_height,
                            line_height,
                            self.caret_top,
                            (style::INPUT_HEIGHT - line_height) / 2.0,
                            line_height
                        ));
                    }
                }
            }
        }

        let mut row = div()
            .relative()
            .h(px(style::INPUT_HEIGHT))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .overflow_hidden()
            .child(
                // 文本输入 / IME 的注册点：`handle_input` 只能在 paint 阶段调用。
                // 用 1×1 的绝对定位块贴在行首——0×0 的元素不会 paint，handler 就注册不上。
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

        // 输入法预编辑（拼音还没上屏）：画在光标处、带底色
        if let Some(text) = preedit {
            row = row.child(
                div()
                    .px(px(1.))
                    .rounded(px(2.))
                    .bg(p.panel)
                    .text_size(px(style::INPUT_SIZE))
                    .text_color(p.fg)
                    .child(text),
            );
        }

        row.child(
            div()
                .text_size(px(style::INPUT_SIZE))
                .text_color(p.fg)
                .child(after.to_owned()),
        )
        // 光标：绝对定位（位置由动画插值驱动，所以不能占布局）。
        // 高度与位置按字体的 ascent/descent 算 —— GPUI 的文本在容器里是 `items_center`，
        // 字形上边界就是 (行高 - (ascent+descent)) / 2，和这里完全一致。
        .child(
            div()
                .absolute()
                .left(px(self.caret_x))
                .top(px(self.caret_top))
                .w(px(style::CURSOR_WIDTH))
                .h(px(self.caret_height))
                .bg(if self.cursor_on { p.accent } else { p.background }),
        )
        .into_any_element()
    }

    fn separator(&self, p: Palette) -> AnyElement {
        div().h(px(1.)).w_full().bg(p.border).into_any_element()
    }

    /// 输入框下面那块：紧凑模式下空输入时整块隐藏（只留一条输入行）
    fn body(&mut self, window: &Window, cx: &mut Context<Self>, p: Palette) -> AnyElement {
        if !self.help && self.compact() {
            return div().flex_1().into_any_element();
        }
        self.list_area(window, cx, p)
    }

    /// 鼠标滚轮 = 上下移动选中项（和 ↑↓ 等价）
    fn on_scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let lines = match event.delta {
            // 精确像素：按一行 30px 折算
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / 30.0,
            ScrollDelta::Lines(delta) => delta.y,
        };
        let steps = lines.round() as i32;
        debug_log(&format!("wheel: delta={:?} lines={lines:.2} steps={steps}", event.delta));
        if steps == 0 {
            return;
        }
        // 向下滚 = 往下选（Windows 的滚动方向在 GPUI 里已经是「内容滚动量」，
        // 正值表示内容向上走，所以这里取反）
        let code = if steps > 0 {
            crossterm::event::KeyCode::Up
        } else {
            crossterm::event::KeyCode::Down
        };
        for _ in 0..steps.unsigned_abs() {
            self.app.on_key(crossterm::event::KeyEvent::new(
                code,
                crossterm::event::KeyModifiers::NONE,
            ));
        }
        window.focus(&self.focus);
        cx.notify();
    }

    /// 每帧推进动画：垂直滚动、选框滑动、光标滑动。
    ///
    /// 返回「是否还在动」——调用方据此决定要不要按 60fps 继续重绘。
    /// `[gui] animations = false` 时全部瞬移（不做插值）。
    fn animate(&mut self) -> bool {
        if !self.animations {
            self.scroll_y = self.scroll_target;
            self.marker_y = self.marker_target;
            self.caret_x = self.caret_target;
            self.marker_moving = false;
            self.chip_settled_at = std::time::Instant::now() - Duration::from_secs(5);
            return false;
        }

        let mut moving = false;

        let diff = self.scroll_target - self.scroll_y;
        if diff.abs() > style::SCROLL_SNAP {
            self.scroll_y += diff * style::SCROLL_EASE;
            moving = true;
        } else if self.scroll_y != self.scroll_target {
            self.scroll_y = self.scroll_target;
        }

        let diff = self.marker_target - self.marker_y;
        let was_moving = self.marker_moving;
        if diff.abs() > style::SCROLL_SNAP {
            self.marker_y += diff * style::MOVE_EASE;
            self.marker_moving = true;
            moving = true;
        } else {
            if self.marker_y != self.marker_target {
                self.marker_y = self.marker_target;
            }
            self.marker_moving = false;
        }
        // 选框刚落地：记时刻（新 chip 从这里开始渐变成彩色），并多画一帧推进渐变
        if was_moving != self.marker_moving {
            if !self.marker_moving {
                self.chip_settled_at = std::time::Instant::now();
            }
            moving = true;
        }

        // chip 渐变（新行渐入 / 旧行渐隐）还在进行中 → 继续按 60fps 重绘
        if self.marker_moving
            || self.chip_settled_at.elapsed().as_secs_f32() < style::CHIP_FADE_SECS
            || (self.chip_from.is_some()
                && self.chip_fade_start.elapsed().as_secs_f32() < style::CHIP_FADE_SECS)
        {
            moving = true;
        }

        let diff = self.caret_target - self.caret_x;
        if diff.abs() > style::SCROLL_SNAP {
            self.caret_x += diff * style::MOVE_EASE;
            moving = true;
        } else if self.caret_x != self.caret_target {
            self.caret_x = self.caret_target;
        }

        moving
    }

    /// chip 的点亮进度：新选中行落地后 0 → 1 渐入，旧选中行在滑动开始时 1 → 0 渐出。
    fn chip_progress(&self, index: usize, selected: Option<usize>) -> f32 {
        {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static N: AtomicUsize = AtomicUsize::new(0);
            if N.fetch_add(1, Ordering::Relaxed) < 40
                && (selected == Some(index) || self.chip_from == Some(index))
            {
                debug_log(&format!(
                    "chip: idx={index} sel={selected:?} from={:?} moving={} y={:.1} -> {:.2}",
                    self.chip_from,
                    self.marker_moving,
                    self.marker_y,
                    self.chip_progress_raw(index, selected)
                ));
            }
        }
        self.chip_progress_raw(index, selected)
    }

    fn chip_progress_raw(&self, index: usize, selected: Option<usize>) -> f32 {
        if !self.animations {
            return f32::from(selected == Some(index));
        }
        let fade = style::CHIP_FADE_SECS;
        if selected == Some(index) {
            if self.app.detail {
                // detail 模式没有选框滑动，直接按时间 0 → 1 渐入
                return (self.chip_settled_at.elapsed().as_secs_f32() / fade).clamp(0.0, 1.0);
            }
            if !self.marker_moving {
                // 落地后从 85% 续到 100%
                let rest = (self.chip_settled_at.elapsed().as_secs_f32() / fade).clamp(0.0, 1.0);
                return 0.85 + rest * 0.15;
            }
            // 滑动中：**按时间**渐入（不用「离目标多远」——滑动太快，距离曲线两三帧就冲完，
            // 看起来像闪一下）。延迟 40ms 起头，140ms 内到 85%，剩下 15% 落地后补满。
            let elapsed = self.chip_fade_start.elapsed().as_secs_f32();
            ((elapsed - 0.04).max(0.0) / fade).clamp(0.0, 1.0) * 0.85
        } else if self.chip_from == Some(index) {
            // 旧行：从 1 渐隐到 0
            1.0 - (self.chip_fade_start.elapsed().as_secs_f32() / fade).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// 选中的那行是不是宽到需要左右滚动（等宽字体估算）
    fn needs_scroll(&mut self) -> bool {
        let need = self.needs_scroll_raw();
        if need != self.logged_scroll {
            self.logged_scroll = need;
            debug_log(&format!(
                "needs_scroll={need} title_w={:.1} sel={:?}",
                self.row_metrics.title,
                self.app.list_state.selected()
            ));
        }
        need
    }

    fn needs_scroll_raw(&self) -> bool {
        if self.row_metrics.title <= 0.0 {
            return false;
        }
        let Some(index) = self.app.list_state.selected() else {
            return false;
        };
        let Some(item) = self.app.items.get(index) else {
            return false;
        };
        let title = item.columns.first().map(String::as_str).unwrap_or("");
        let advance = style::ROW_SIZE * 0.6;
        let total = display_width(title) as f32 * advance;
        total > self.row_metrics.title
    }

    /// 是否按「呼出只有输入框」的紧凑形态显示。
    ///
    /// 空输入 + 没有补全候选 + 没有手动按 ↓ 展开开屏列表。
    fn compact(&self) -> bool {
        self.app.input.is_empty() && self.app.completion.is_none() && !self.hint_open
    }

    /// 窗口应该多高：紧凑时只有输入行，展开时按结果行数算（最多 MAX_VISIBLE_ROWS 行）
    fn desired_height(&self) -> f32 {
        let border = style::BORDER * 2.0;
        let base = style::PADDING * 2.0 + border + style::INPUT_HEIGHT + 1.0;

        if self.compact() && !self.help {
            return base - 1.0; // 不画分隔线
        }

        if self.help {
            let rows = i18n::gui_help().len() as f32;
            return base + 24.0 + rows * 26.0 + style::STATUS_HEIGHT;
        }

        let visible = self.app.items.len().min(style::MAX_VISIBLE_ROWS) as f32;
        let rows = visible.max(1.0);

        if self.app.detail {
            // detail 模式下每条高度随列数变化：头行 + (列数-1) 行明细 + 上下内边距 + 行间距。
            // 之前按「固定两行」算，列数多的条目会被裁掉底部（4 条都铺不满就是这个原因）。
            let max_columns = self
                .app
                .items
                .iter()
                .map(|item| item.columns.len())
                .max()
                .unwrap_or(1)
                .max(1);
            let extra = (max_columns - 1) as f32;
            // 头部实际高度是 `ROW_HEIGHT - 6`（和渲染里 accent bar 一致），不是整行 28
            let per_row = style::ROW_HEIGHT - 6.0
                + extra * style::DETAIL_LINE
                + style::DETAIL_PAD_Y * 2.0
                + extra * 2.0;
            return base + style::LIST_TOP + rows * per_row + style::STATUS_HEIGHT;
        }

        base + style::LIST_TOP + rows * (style::ROW_HEIGHT + style::ROW_GAP) + style::STATUS_HEIGHT
    }

    /// 结果列表：`uniform_list` 虚拟化（`@hist` 200 条 / `@web` 上千条也不会全渲染）
    fn list_area(&mut self, window: &Window, cx: &mut Context<Self>, p: Palette) -> AnyElement {
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
        let number = if self.app.select_mode {
            style::NUMBER_WIDTH
        } else {
            0.0
        };
        let fixed = style::ACCENT_BAR
            + style::ROW_PAD_X * 2.0
            + style::ROW_GAP_X * 2.0
            + number
            + style::STAR_WIDTH
            + style::BADGE_WIDTH;
        let metrics = RowMetrics {
            title: (total - fixed - note_width).max(80.0),
            note: note_width,
        };
        self.row_metrics = metrics;

        let rows = Arc::new(self.snapshot(metrics));
        let panel = cx.entity().downgrade();

        // 行高：普通模式固定；detail 模式按「最大列数」统一（所有行等高才能复用
        // 虚拟窗口 + 平滑滚动 + 选框滑动这套，多算的几像素对少列条目只是留白）
        let detail = self.app.detail;
        let row_h = if detail {
            let max_columns = rows
                .iter()
                .map(|row| row.columns.len())
                .max()
                .unwrap_or(1)
                .max(1);
            let extra = (max_columns - 1) as f32;
            (style::ROW_HEIGHT - 6.0
                + extra * style::DETAIL_LINE
                + style::DETAIL_PAD_Y * 2.0
                + extra * 2.0)
                .max(style::ROW_HEIGHT)
                + style::ROW_GAP
        } else {
            style::ROW_HEIGHT + style::ROW_GAP
        };

        // 自己算「可视窗口 + 垂直偏移」，这样才能做平滑滚动
        // （GPUI 的 `uniform_list` 只能瞬间 scroll_to_item，没法插值）
        let visible_rows = rows.len().min(style::MAX_VISIBLE_ROWS);
        self.scroll_viewport = visible_rows as f32 * row_h;
        self.scroll_max = (rows.len() as f32 * row_h - self.scroll_viewport).max(0.0);

        // 选中项保持在可视区内（上下各留一行余量）
        if let Some(index) = self.app.list_state.selected() {
            let top = index as f32 * row_h;
            let bottom = top + row_h;
            let margin = row_h;
            if top < self.scroll_target {
                self.scroll_target = (top - margin).max(0.0);
            } else if bottom > self.scroll_target + self.scroll_viewport {
                self.scroll_target =
                    (bottom - self.scroll_viewport + margin).min(self.scroll_max);
            }
        }
        self.scroll_target = self.scroll_target.clamp(0.0, self.scroll_max);
        self.scroll_y = self.scroll_y.clamp(0.0, self.scroll_max);

        // 选框滑动的目标位置
        {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static N: AtomicUsize = AtomicUsize::new(0);
            if N.fetch_add(1, Ordering::Relaxed) < 5 {
                debug_log(&format!(
                    "marker: sel={:?} row_h={} scroll_y={:.1} target={:.1} cur={:.1} rows={} viewport={:.1}",
                    self.app.list_state.selected(),
                    row_h,
                    self.scroll_y,
                    self.marker_target,
                    self.marker_y,
                    rows.len(),
                    self.scroll_viewport
                ));
            }
        }
        self.marker_target = self
            .app
            .list_state
            .selected()
            .map(|index| index as f32 * row_h)
            .unwrap_or(0.0);

        // 只渲染可视区附近的行
        let first = ((self.scroll_y / row_h).floor() as usize).saturating_sub(1);
        let count = (self.scroll_viewport / row_h).ceil() as usize + 3;
        let last = (first + count).min(rows.len()).max(first);
        let mut inner = div()
            .absolute()
            .top(px(-self.scroll_y))
            .left(px(0.))
            .w_full()
            .flex()
            .flex_col();

        // 选框：一个独立的绝对定位块，位置由 `marker_y` 插值 —— 这样换行时是滑过去的
        if self.app.list_state.selected().is_some() {
            inner = inner.child(
                div()
                    .absolute()
                    // 行高 31 而卡片 28，卡片是居中的（上下各 ROW_GAP/2），marker 要跟着偏
                    .top(px(self.marker_y + style::ROW_GAP / 2.0))
                    .left(px(0.))
                    .w_full()
                    .h(px(row_h - style::ROW_GAP))
                    .rounded(px(style::ROW_RADIUS))
                    // 左侧绿条用边框画：不占内容宽度，列表能整体左移
                    .border_l(px(style::ACCENT_BAR))
                    .border_color(p.accent)
                    .bg(p.overlay),
            );
        }

        for row in &rows[first..last] {
            inner = inner.child(if detail {
                render_detail_row(row, p, metrics, &panel, row_h - style::ROW_GAP)
            } else {
                render_row(row, p, metrics, &panel)
            });
        }

        div()
            .id("list")
            .relative()
            .flex_1()
            .w_full()
            .overflow_hidden()
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(inner)
            .into_any_element()
    }

    /// 把当前结果快照成「一行一个结构体」，供 `uniform_list` 的闭包使用
    /// （闭包拿到的是 `&mut App`，碰不到 `self`）
    fn snapshot(&self, metrics: RowMetrics) -> Vec<RowSnapshot> {
        let selected = self.app.list_state.selected();
        let select_mode = self.app.select_mode;
        let advance = style::ROW_SIZE * 0.6;
        let note_advance = style::NOTE_SIZE * 0.6;
        let elapsed = self.scroll_started.elapsed().as_secs_f32();
        self.app
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| RowSnapshot {
                index,
                columns: item.columns.clone(),
                badge: item.kind.badge(),
                color: style::badge_color(item.kind),
                chip_progress: self.chip_progress(index, selected),
                favorite: self.app.is_favorite(item),
                unavailable: matches!(item.control, crate::providers::Control::Unavailable),
                number: (select_mode && index < 9).then_some(index + 1),
                // 只有选中行会左右滚动（跟 TUI 一样），其余行裁掉就裁掉。
                // 用像素级 ping-pong + 时间基准，60fps 下是连续移动，不再是「一格一格跳」。
                shift: if selected == Some(index) {
                    let title = item.columns.first().map(String::as_str).unwrap_or("");
                    let overflow = display_width(title) as f32 * advance - metrics.title;
                    ping_pong(overflow, elapsed)
                } else {
                    0.0
                },
                note_shift: if selected == Some(index) {
                    let note = note_text(item);
                    let overflow = display_width(&note) as f32 * note_advance - metrics.note;
                    ping_pong(overflow, elapsed)
                } else {
                    0.0
                },
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
    favorite: bool,
    /// 开关类条目不可用（灰掉，提示是死项）
    unavailable: bool,
    /// 选择模式下的跳转序号（1–9）
    number: Option<usize>,
    /// chip 的点亮进度（0 = 灰底灰字，1 = 类型色底深色字）
    chip_progress: f32,
    /// 超宽时向左移动的像素（负 margin，实现左右滚动）
    shift: f32,
    /// 附注列同理
    note_shift: f32,
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
    let note = note_text_for_row(row);
    let fg = if row.unavailable { p.faint } else { p.fg };

    let card = div()
        .id(("row", row.index))
        .h(px(style::ROW_HEIGHT))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(style::ROW_GAP_X))
        .px(px(style::ROW_PAD_X))
        .rounded(px(style::ROW_RADIUS))
        .whitespace_nowrap()
        .hover(|style| style.bg(p.panel));

    // 选中底色与左侧绿条由列表容器里的「选框」（marker）统一画；
    // 绿条是 marker 的左边框，不占内容宽度，所以这里不再留占位（列表能整体左移）。
    let card = card
        .child(if let Some(n) = row.number {
            // 选择模式（Shift+Tab）下的 1-9 跳转序号，平时不占位
            div()
                .w(px(style::NUMBER_WIDTH))
                .flex_shrink_0()
                .text_size(px(style::NOTE_SIZE))
                .text_color(p.dim)
                .child(n.to_string())
                .into_any_element()
        } else {
            div().into_any_element()
        })
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
                .bg(style::mix(p.panel, row.color, row.chip_progress))
                .text_size(px(style::BADGE_SIZE))
                .text_color(style::mix(row.color, p.background, row.chip_progress))
                .child(row.badge),
        )
        .child(
            // 标题：外层定宽裁剪，内层用负 margin 左移 = 左右滚动
            div()
                .w(px(metrics.title))
                .flex_shrink_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(
                    div()
                        .ml(px(-row.shift))
                        .whitespace_nowrap()
                        .text_size(px(style::ROW_SIZE))
                        .text_color(fg)
                        .child(title),
                ),
        )
        .child(
            // 附注列：其余列拼起来，选中行超宽时同样左右滚动
            div()
                .w(px(metrics.note))
                .flex_shrink_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(
                    div()
                        .ml(px(-row.note_shift))
                        .whitespace_nowrap()
                        .text_size(px(style::NOTE_SIZE))
                        .text_color(p.dim)
                        .child(note),
                ),
        )
        .on_click({
            let panel = panel.clone();
            let index = row.index;
            move |event, _window, cx| {
                // 双击 = 回车：`MouseDownEvent.click_count` 由平台给，不用自己计时
                let double_click = matches!(
                    event,
                    gpui::ClickEvent::Mouse(mouse) if mouse.down.click_count >= 2
                );
                let _ = panel.update(cx, |panel, cx| {
                    panel.app.list_state.select(Some(index));
                    if double_click {
                        panel.run_selected(cx);
                    }
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
    height: f32,
) -> AnyElement {
    let head = row.columns.first().cloned().unwrap_or_default();
    let fg = if row.unavailable { p.faint } else { p.fg };
    let indent = style::ACCENT_BAR
        + 8.0 * 2.0
        + style::STAR_WIDTH
        + style::BADGE_WIDTH
        + 8.0;

    let block = div()
        .id(("detail", row.index))
        .w_full()
        .h(px(height))
        .flex()
        .flex_col()
        .gap(px(2.))
        .py(px(style::DETAIL_PAD_Y))
        .px(px(style::ROW_PAD_X))
        .rounded(px(style::ROW_RADIUS))
        .overflow_hidden()
        .whitespace_nowrap()
        .hover(|style| style.bg(p.panel));

    let mut block = block.child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
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
                    .bg(style::mix(p.panel, row.color, row.chip_progress))
                    .text_size(px(style::BADGE_SIZE))
                    .text_color(style::mix(row.color, p.background, row.chip_progress))
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
            move |event, _window, cx| {
                let double_click = matches!(
                    event,
                    gpui::ClickEvent::Mouse(mouse) if mouse.down.click_count >= 2
                );
                let _ = panel.update(cx, |panel, cx| {
                    panel.app.list_state.select(Some(index));
                    if double_click {
                        panel.run_selected(cx);
                    }
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
            // 先记下「上一行的选中项」，再更新 —— 顺序反了的话旧行永远不渐隐
            self.chip_from = self.last_selected;
            self.last_selected = selected;
            // 先熄灭 chip：不然新选中行会先用「点亮」状态渲染一帧，再被 animate 关掉 → 闪
            if self.animations {
                self.marker_moving = true;
            }
            // 旧选中行的 chip 从彩色渐隐回灰（`chip_from` 上面已经记好，别再覆盖成新值）
            self.chip_fade_start = std::time::Instant::now();
            // detail 模式没有选框滑动，直接从现在开始渐入，所以也记一个起点
            self.chip_settled_at = std::time::Instant::now();
            // 滚动动画从头开始（每换一行都从「贴左边」开始走）
            self.scroll_started = std::time::Instant::now();
        }

        // 从紧凑形态展开时，列表淡入
        let compact_now = self.compact() && !self.help;
        if self.was_compact && !compact_now {
            self.reveal_started = std::time::Instant::now();
        }
        self.was_compact = compact_now;

        if self.logged_rows == usize::MAX {
            debug_log(&format!(
                "window bounds={:?} scale={}",
                window.bounds(),
                window.scale_factor()
            ));
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

        self.scale = window.scale_factor().max(0.5);

        // 高度跟内容走（紧凑态只有输入行、出结果后向下展开）。
        // 用 `window.on_next_frame` 推后一帧再 resize：直接在 render 里调会触发
        // 同步 relayout/render → "cannot update Panel while it is already being updated"。
        // 尺寸是**逻辑像素**，GPUI 自己会处理 DPI（Win32 SetWindowPos 那条路走不通：
        // 请求 132px 实际窗口还是 66px，说明系统按 DPI 又缩了一次）。
        let wanted = self.desired_height();
        let current = f32::from(window.bounds().size.height);
        if (current - wanted).abs() > 1.0 {
            let width = window.bounds().size.width;
            window.on_next_frame(move |window, _cx| {
                window.resize(size(width, px(wanted)));
            });
        }
        let p = self.palette;
        let compact = compact_now;
        let reveal = (self.reveal_started.elapsed().as_secs_f32() / style::REVEAL_SECS).min(1.0);

        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(style::with_alpha(p.background, self.opacity))
            .rounded(px(style::RADIUS))
            .border(px(style::BORDER))
            .border_color(p.accent)
            .p(px(style::PADDING))
            .font_family(style::FONT)
            .font_weight(style::FONT_WEIGHT)
            .text_color(p.fg)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::handle_key))
            .child(self.input_row(window, p, cx));

        // 紧凑形态（刚呼出、还没输入）：只有一条输入行，分隔线 / 列表 / 状态栏都不画
        if !compact {
            let body = if self.help {
                self.help_panel(p)
            } else {
                self.body(window, cx, p)
            };
            // 展开时淡入（0 → 1，120ms）
            let body = if reveal < 1.0 {
                div().flex_1().w_full().opacity(reveal).child(body).into_any_element()
            } else {
                body
            };
            root = root
                .child(self.separator(p))
                .child(body)
                .child(self.status_bar(p));
        }

        root
    }
}

/// 左右滚动的位移（像素）：走到底 → 停一会儿 → 走回来 → 停一会儿，循环。
fn ping_pong(overflow: f32, elapsed: f32) -> f32 {
    if overflow <= 0.0 {
        return 0.0;
    }
    let speed = style::SCROLL_SPEED;
    let pause = style::SCROLL_PAUSE;
    let travel = overflow / speed;
    let cycle = (travel + pause) * 2.0;
    let t = elapsed % cycle;

    if t < travel {
        t * speed
    } else if t < travel + pause {
        overflow
    } else if t < travel + pause + travel {
        overflow - (t - travel - pause) * speed
    } else {
        0.0
    }
}

/// 除了第一列之外的所有列，用 " · " 拼起来
fn note_text(item: &crate::providers::Item) -> String {
    item.columns
        .iter()
        .skip(1)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" · ")
}

fn note_text_for_row(row: &RowSnapshot) -> String {
    row.columns
        .iter()
        .skip(1)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" · ")
}

/// 用真实字形排版量一段文本的宽度（不猜「一格多少像素」，`❯` 这种全角字符也准）
fn text_width(window: &Window, text: &str, size: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let run = gpui::TextRun {
        len: text.len(),
        font: gpui::font(style::FONT),
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = window
        .text_system()
        .shape_line(text.to_string().into(), px(size), &[run], None);
    f32::from(shaped.width)
}

/// 文本的「显示宽度」（宽字符算 2）
fn display_width(text: &str) -> usize {
    use unicode_width::UnicodeWidthChar;
    text.chars().map(|c| c.width().unwrap_or(0)).sum()
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

        tune_dwm(hwnd);
    }
}

/// 关掉系统圆角与 1px 窗口边框。
///
/// 内容自己画的是 12px 圆角，Win11 的 DWM 圆角半径比它小，两者叠在一起
/// 就会看到「边框没跟着圆角」的一圈弧线。索性让 DWM 不画，圆角交给内容。
#[cfg(windows)]
fn tune_dwm(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
        DwmSetWindowAttribute,
    };

    // DWMWCP_DONOTROUND = 1
    let corner: i32 = DWMWCP_DONOTROUND;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&corner as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
        );
    }

    // DWMWA_COLOR_NONE：不画窗口边框
    let no_border: u32 = 0xFFFF_FFFE;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR as u32,
            (&no_border as *const u32).cast(),
            std::mem::size_of::<u32>() as u32,
        );
    }
}
