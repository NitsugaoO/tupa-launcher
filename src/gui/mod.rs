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
mod acrylic;
mod frame;
mod input;
mod keys;
mod style;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use gpui::{
    AnyElement, AppContext as _, Bounds, Context, FocusHandle, KeyDownEvent, Render, Rgba,
    ScrollDelta, ScrollWheelEvent, TitlebarOptions, WeakEntity, Window,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point, prelude::*, px,
    relative, size,
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
    // 亚克力：GPUI 只负责把窗口设成 `Blurred`，alpha = 1 的修正由
    // `acrylic::enable` 在我们自己手里补（见 `src/gui/acrylic.rs`）。
    let acrylic_on = cfg.gui.acrylic;
    let always_on_top = cfg.gui.always_on_top;
    let rounded = cfg.gui.rounded;
    let app = App::new(cfg, theme);

    gpui::Application::new().run(move |cx| {
        // 尺寸与 TUI 面板对齐（屏幕的 62%×55%），居中放在主屏上。
        // GlazeWM 那边这条规则是 `ignore`（面板是浮层，不该进 tiling / 不归 workspace 管），
        // 所以位置和大小由这里说了算。
        let display = cx.primary_display();
        let (bounds, max_height) = match &display {
            Some(display) => {
                let area = display.bounds();
                let screen_w = f32::from(area.size.width);
                let screen_h = f32::from(area.size.height);
                let width = screen_w * style::WIDTH_RATIO;
                let height = screen_h * style::HEIGHT_RATIO;
                // 不居中：像命令面板那样靠上（居中时窗口一展开就滑到屏幕中间，
                // 与鼠标/视线焦点差得远）。水平居中，顶部留 12% 屏高。
                let origin = point(
                    area.origin.x + px((screen_w - width) / 2.0),
                    area.origin.y + px(screen_h * style::TOP_RATIO),
                );
                (
                    Bounds::new(origin, size(px(width), px(height))),
                    screen_h * style::MAX_HEIGHT_RATIO,
                )
            }
            None => (
                Bounds::centered(
                    None,
                    size(px(style::DEFAULT_WIDTH), px(style::DEFAULT_HEIGHT)),
                    cx,
                ),
                style::DEFAULT_HEIGHT,
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
                // 只用 Transparent（纯 alpha）/ Blurred（亚克力）。`Blurred` 走的是
                // `SetWindowCompositionAttribute(ACCENT_ENABLE_ACRYLICBLURBEHIND)`，DWM 画的
                // 是真模糊；但 GPUI 0.2.2 传的 gradient alpha = 0，会变成黑玻璃，
                // 所以开亚克力时再补一次 `acrylic::enable`（alpha = 1）。
                window_background: if acrylic_on {
                    WindowBackgroundAppearance::Blurred
                } else if opacity < 1.0 {
                    WindowBackgroundAppearance::Transparent
                } else {
                    WindowBackgroundAppearance::Opaque
                },
                ..Default::default()
            },
            move |window, cx| {
                cx.new(|cx| {
                    Panel::new(
                        app,
                        Surface {
                            opacity,
                            animations,
                            always_on_top,
                            acrylic: acrylic_on,
                            rounded,
                        },
                        max_height,
                        window,
                        cx,
                    )
                })
            },
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
    /// 窗口外观（圆角 / 置顶）是否已经应用过；首帧渲染时才做
    frame_applied: bool,
    /// `[gui] always_on_top`
    always_on_top: bool,
    /// `[gui] acrylic`
    acrylic_on: bool,
    /// `[gui] rounded`：外框是否圆角
    rounded: bool,
    /// 选框滑动的当前 y / 目标 y（像素）
    marker_y: f32,
    marker_target: f32,
    /// 本次滑动的起点与起始时刻（固定时长 ease-out）
    marker_from: f32,
    marker_start: std::time::Instant,
    /// 滚动同理：起点与起始时刻。**与选框用同一个进度**，否则两者会脉动。
    scroll_from: f32,
    scroll_start: std::time::Instant,
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
    /// 滚动条 / 列表用的「累积不到一行就先存着」（滚轮 + 触控板）
    wheel_accum: f32,
    /// `?` 打开的快捷键帮助浮层
    help: bool,
    /// 帮助浮层切隐的瞬间屏蔽一下文本输入。
    ///
    /// `?` 是 Shift+/ 打出来的：`WM_KEYDOWN` 先到 `handle_key`（打开帮助），
    /// 紧接着同一按键的 `WM_CHAR` 会走 `EntityInputHandler::replace_text_in_range`
    /// 把 `?` 插进输入框。设一个短窗口（60ms）把这个字符吞掉——
    /// 既盖住「`?` 打开帮助」，也盖住「帮助开着时按任意键关闭」。
    text_block: Option<std::time::Instant>,
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
    /// 窗口高度上限（逻辑像素；屏幕的 55%，再高就顶到任务栏了）
    max_height: f32,
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
}

/// 面板的外观开关（`[gui]` 里那几项），打包传进来免得 `Panel::new` 参数爆炸。
#[derive(Clone, Copy)]
struct Surface {
    opacity: f32,
    animations: bool,
    always_on_top: bool,
    acrylic: bool,
    rounded: bool,
}

impl Panel {
    fn new(
        mut app: App,
        surface: Surface,
        max_height: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Surface {
            opacity,
            animations,
            always_on_top,
            acrylic: acrylic_on,
            rounded,
        } = surface;
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
            loop {
                // 只负责后台查询结果的轮询；动画改由 `on_next_frame` 跟着 vsync 跑
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
                    // 光标闪烁：到点就翻一次（移动光标后 `blink_at` 会被推后）
                    let now = std::time::Instant::now();
                    if now >= panel.blink_at {
                        panel.cursor_on = !panel.cursor_on;
                        panel.blink_at = now + Duration::from_millis(500);
                        cx.notify();
                    }

                    panel.app.tick += 1;

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
            scroll_y: 0.0,
            scroll_target: 0.0,
            scroll_viewport: 0.0,
            scroll_max: 0.0,
            animations,
            frame_applied: false,
            always_on_top,
            rounded,
            acrylic_on,
            marker_y: 0.0,
            marker_target: 0.0,
            marker_from: 0.0,
            marker_start: std::time::Instant::now(),
            scroll_from: 0.0,
            scroll_start: std::time::Instant::now(),
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
            text_block: None,
            wheel_accum: 0.0,
            cursor_on: true,
            blink_at: std::time::Instant::now() + Duration::from_millis(500),
            preedit: None,
            hint_open: false,
            row_metrics: RowMetrics {
                title: 0.0,
                note: 0.0,
                state: 0.0,
                secondary: 0.0,
            },
            scale: 1.0,
            opacity,
            max_height,
            scroll_started: std::time::Instant::now(),
            reveal_started: std::time::Instant::now(),
            was_compact: true,
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
            self.text_block = Some(std::time::Instant::now() + Duration::from_millis(50));
            window.focus(&self.focus);
            cx.notify();
            return;
        }
        if question {
            self.help = true;
            self.text_block = Some(std::time::Instant::now() + Duration::from_millis(50));
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

        // 选框/滚动立即起步：`animate()` 平时在后台轮询里跑（空闲时 120ms 一轮），
        // 按键后干等那一轮会明显「迟钝」。这里先把目标算好、立刻推一帧。
        if let Some(index) = self.app.list_state.selected() {
            self.aim_marker(index as f32 * self.row_height());
        }
        self.animate();

        // 焦点别丢（Esc 清空输入、点列表之后都要能继续打字）
        window.focus(&self.focus);
        cx.notify();
    }

    /// 帮助浮层期间（以及刚切隐的那一下）不要往输入框插字
    fn text_blocked(&self) -> bool {
        self.help
            || self
                .text_block
                .is_some_and(|until| std::time::Instant::now() < until)
    }

    /// 设选框目标。目标没变就什么都不做（每帧重置起点会让动画永远走不完）。
    /// 位移超过一屏时直接瞬移（循环回第一条时选框不该扫过整屏）。
    fn aim_marker(&mut self, target: f32) {
        if (target - self.marker_target).abs() < 0.5 {
            return;
        }
        let distance = (target - self.marker_y).abs();
        self.marker_from = self.marker_y;
        self.marker_start = std::time::Instant::now();
        self.marker_target = target;
        if distance > self.scroll_viewport.max(1.0) {
            self.marker_y = target;
            self.marker_from = target;
        }
    }

    /// 设滚动目标（变化时才记起点，并把起点时刻对齐到选框的，保证两者同步）
    fn aim_scroll(&mut self, target: f32) {
        let target = target.clamp(0.0, self.scroll_max);
        if (target - self.scroll_target).abs() < 0.5 {
            return;
        }
        let distance = (target - self.scroll_y).abs();
        self.scroll_from = self.scroll_y;
        // 与选框同一个起始时刻：滚动目标比选框晚一帧算出，用选框的时刻才能追上进度
        self.scroll_start = self.marker_start;
        self.scroll_target = target;
        if distance > self.scroll_viewport.max(1.0) {
            self.scroll_y = target;
            self.scroll_from = target;
        }
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
        let (before, _after) = self.app.input.split_at(at);
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
            );

        // 提示符
        row = row.child(
            div()
                .text_size(px(style::INPUT_SIZE))
                .text_color(p.accent)
                .child(style::PROMPT),
        );

        // 输入行按「选区边界」切成假设干段：普通段不着色，选中段给一层 `overlay` 底色。
        // 光标可能落在选区中间（Shift+← 往左就一直如此），所以前后两半都要按 cursor 再切一次。
        //
        // 光标之前：普通 → 选中 → 普通
        {
            let chars: Vec<char> = self.app.input.chars().collect();
            let n = chars.len();
            let c = self.app.cursor.min(n);
            let (sel_start, sel_end) = self.app.selection().unwrap_or((c, c));
            let cut = |a: usize, b: usize| -> String {
                let a = a.min(n);
                let b = b.min(n).max(a);
                chars[a..b].iter().collect()
            };
            let mut parts: Vec<(String, bool)> = Vec::new();

            let before_sel = sel_start.min(c);
            if before_sel > 0 {
                parts.push((cut(0, before_sel), false));
            }
            let before_sel_end = sel_end.min(c);
            if before_sel_end > before_sel {
                parts.push((cut(before_sel, before_sel_end), true));
            }
            if c > before_sel_end {
                parts.push((cut(before_sel_end, c), false));
            }

            for (text, selected) in parts {
                if text.is_empty() {
                    continue;
                }
                let mut segment = div()
                    .text_size(px(style::INPUT_SIZE))
                    .text_color(p.fg)
                    .child(text);
                if selected {
                    segment = segment.bg(p.overlay);
                }
                row = row.child(segment);
            }
        }

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

        // 光标之后：普通 → 选中 → 普通（同样的切法，起点是 `cursor`）
        {
            let chars: Vec<char> = self.app.input.chars().collect();
            let n = chars.len();
            let c = self.app.cursor.min(n);
            let (sel_start, sel_end) = self.app.selection().unwrap_or((c, c));
            let cut = |a: usize, b: usize| -> String {
                let a = a.min(n);
                let b = b.min(n).max(a);
                chars[a..b].iter().collect()
            };
            let mut parts: Vec<(String, bool)> = Vec::new();

            let after_sel = sel_start.max(c);
            if after_sel > c {
                parts.push((cut(c, after_sel), false));
            }
            let after_sel_end = sel_end.max(c);
            if after_sel_end > after_sel {
                parts.push((cut(after_sel, after_sel_end), true));
            }
            if n > after_sel_end {
                parts.push((cut(after_sel_end, n), false));
            }

            for (text, selected) in parts {
                if text.is_empty() {
                    continue;
                }
                let mut segment = div()
                    .text_size(px(style::INPUT_SIZE))
                    .text_color(p.fg)
                    .child(text);
                if selected {
                    segment = segment.bg(p.overlay);
                }
                row = row.child(segment);
            }
        }
        row
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
            // 精确像素（触控板）：按行高折算，刻度小的地方会累积到一整行才动
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / 24.0,
            ScrollDelta::Lines(delta) => delta.y,
        };
        // **一格滚轮只走一行**：系统默认一次滚 3 行，启动器里一跳三条太快了
        // （用户反馈「往上滚感觉滚多了」）。不足一行的部分累积起来，触控板也能用。
        self.wheel_accum += lines;
        let whole = self.wheel_accum.trunc();
        self.wheel_accum -= whole;
        let steps = (whole as i32).clamp(-1, 1);
        debug_log(&format!(
            "wheel: delta={:?} lines={lines:.2} steps={steps}",
            event.delta
        ));
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
        if let Some(index) = self.app.list_state.selected() {
            self.aim_marker(index as f32 * self.row_height());
        }
        self.animate();
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

        // 选框与列表滚动共用「固定时长 ease-out」：进度一致 → 内容看着是跟着选框走的。
        // （以前两者各自指数逼近，120Hz 下两三帧就冲完，而且容易一快一慢）
        let t = (self.marker_start.elapsed().as_secs_f32() / style::MOVE_SECS).min(1.0);
        let ease = 1.0 - (1.0 - t) * (1.0 - t);
        let was_moving = self.marker_moving;
        if t < 1.0 {
            self.marker_y = self.marker_from + (self.marker_target - self.marker_from) * ease;
            self.marker_moving = true;
            moving = true;
        } else {
            self.marker_y = self.marker_target;
            self.marker_moving = false;
        }

        // 滚动用自己的进度（起点时刻对齐选框的，算目标比选框晚一帧）
        let st = (self.scroll_start.elapsed().as_secs_f32() / style::MOVE_SECS).min(1.0);
        let sease = 1.0 - (1.0 - st) * (1.0 - st);
        if st < 1.0 {
            self.scroll_y = self.scroll_from + (self.scroll_target - self.scroll_from) * sease;
            moving = true;
        } else {
            self.scroll_y = self.scroll_target;
        }
        self.scroll_y = self.scroll_y.clamp(0.0, self.scroll_max);

        // 光标仍用指数插值（独立元件，跟手优先）
        let diff = self.caret_target - self.caret_x;
        if diff.abs() > style::SCROLL_SNAP {
            self.caret_x += diff * style::MOVE_EASE;
            moving = true;
        } else if self.caret_x != self.caret_target {
            self.caret_x = self.caret_target;
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
                // detail 模式没有选框滑动，按「选中变化的时刻」0 → 1 渐入。
                // 这里**不能用 `chip_settled_at`**：marker 落地时会重置它，
                // 渐入到一半被重置回 0 就是那个「闪」。
                return (self.chip_fade_start.elapsed().as_secs_f32() / fade).clamp(0.0, 1.0);
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

    /// 还有动画在跑吗（决定要不要注册下一帧回调）
    fn animating(&self) -> bool {
        if !self.animations {
            return false;
        }
        self.marker_moving
            || (self.scroll_target - self.scroll_y).abs() > style::SCROLL_SNAP
            || (self.caret_target - self.caret_x).abs() > style::SCROLL_SNAP
            || self.chip_fade_start.elapsed().as_secs_f32() < style::CHIP_FADE_SECS
            || self.chip_settled_at.elapsed().as_secs_f32() < style::CHIP_FADE_SECS
            || self.reveal_started.elapsed().as_secs_f32() < style::REVEAL_SECS
            || self.needs_scroll_raw()
    }

    /// 注册下一帧：推进动画 + 重绘，链条一直到动画结束
    fn schedule_animation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.animating() {
            return;
        }
        let entity = cx.entity().downgrade();
        window.on_next_frame(move |_window, cx| {
            let _ = entity.update(cx, |panel, cx| {
                panel.animate();
                cx.notify();
            });
        });
    }

    /// 当前模式最多显示多少条（卡片两行一条，能放的条数是一半多）
    fn max_visible_rows(&self) -> usize {
        if self.app.detail {
            style::MAX_VISIBLE_ROWS
        } else {
            style::CARD_VISIBLE_ROWS
        }
    }

    /// 当前列表的行高（含行间距）。
    ///
    /// - `detail = true`：单行对齐表（密度模式，`Ctrl+Tab`）
    /// - 默认：两行卡片（标题 + 次要信息）
    fn row_height(&self) -> f32 {
        if self.app.detail {
            style::ROW_HEIGHT + style::ROW_GAP
        } else {
            style::CARD_HEIGHT + style::ROW_GAP
        }
    }

    /// 选中的那行是不是宽到需要左右滚动（等宽字体估算，只读）
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

        let row_advance = style::ROW_SIZE * 0.6;
        let note_advance = style::NOTE_SIZE * 0.6;

        // 标题列
        let title = item.columns.first().map(String::as_str).unwrap_or("");
        if display_width(title) as f32 * row_advance > self.row_metrics.title {
            return true;
        }

        // 其余列也要算：之前只看标题列，所以「标题不超宽、附注 / 第二行超宽」时
        // 压根不会推帧，滚动动画就停在第一帧（看着像「只播一次」）。
        if self.app.detail {
            let note = note_text(item);
            display_width(&note) as f32 * note_advance > self.row_metrics.note
        } else {
            let secondary = card_parts(item).0;
            display_width(&secondary) as f32 * note_advance > self.row_metrics.secondary
        }
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

        let visible = self.app.items.len().min(self.max_visible_rows()) as f32;
        let rows = visible.max(1.0);

        // 底部多留一截：滚到底时最后一行不贴着状态栏（看着像「没有底边」）
        base + style::LIST_TOP * 2.0 + rows * self.row_height() + style::STATUS_HEIGHT
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
        // 右侧尾巴列：密度模式只放控件状态；卡片模式还要放「次数 / 时间」。
        // 宽度按**实际内容**量（不能写死）：Clash 那行是「云上极速 · 3 小时前」，
        // 写死 72px 会把时间那半截裁掉，看着像「只显示了订阅名」。
        let card_mode = !self.app.detail;
        let trailing_width = {
            let widest = self
                .app
                .items
                .iter()
                .filter_map(|item| {
                    if item.control.is_control() {
                        Some(item.control.label())
                    } else if card_mode {
                        card_parts(item).1
                    } else {
                        None
                    }
                })
                .map(|text| display_width(&text))
                .max()
                .unwrap_or(0) as f32;
            if widest > 0.0 {
                (widest * style::NOTE_SIZE * 0.6 + 14.0).clamp(style::STATE_WIDTH, 260.0)
            } else {
                0.0
            }
        };
        let state_width = trailing_width;
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
        // 密度模式：标题 + 拼接附注 + 状态；卡片模式：标题 + 右侧尾巴，第二行全宽
        let note_width = if card_mode {
            0.0
        } else {
            (total * 0.32)
                .clamp(80.0, 240.0)
                .min((total - fixed - state_width - 120.0).max(60.0))
        };
        let metrics = RowMetrics {
            title: (total - fixed - note_width - state_width).max(120.0),
            note: note_width,
            state: state_width,
            secondary: if card_mode {
                (total - card_indent() - style::ROW_PAD_X).max(80.0)
            } else {
                0.0
            },
        };
        self.row_metrics = metrics;

        let rows = Arc::new(self.snapshot(metrics));
        let panel = cx.entity().downgrade();

        // 行高：普通模式固定；detail 模式按「最大列数」统一（所有行等高才能复用
        // 虚拟窗口 + 平滑滚动 + 选框滑动这套，多算的几像素对少列条目只是留白）
        let detail = self.app.detail;
        let row_h = self.row_height();

        // 自己算「可视窗口 + 垂直偏移」，这样才能做平滑滚动
        // （GPUI 的 `uniform_list` 只能瞬间 scroll_to_item，没法插值）
        let visible_rows = rows.len().min(self.max_visible_rows());
        self.scroll_viewport = visible_rows as f32 * row_h;
        self.scroll_max = (rows.len() as f32 * row_h - self.scroll_viewport).max(0.0);

        // 选中项保持在可视区内。
        //
        // 两个细节（都是「看着怪」的真因）：
        // 1. **不要留余量**：之前用 `margin = row_h`，每次触底滚动都多滚一行
        // 2. **拿实际视口 `scroll_y` 判断**，不是 `scroll_target`：跟着目标值走时，
        //    快速连按会提前停住（目标已经推出去了，实际视口还没到）
        if let Some(index) = self.app.list_state.selected() {
            let top = index as f32 * row_h;
            let bottom = top + row_h;
            // 用**目标位置**判断，不看动画中的 `scroll_y`：后者滞后，向上滚时
            // 会因为「视图还没回滚」而漏判，选框会先跑出视口。
            let mut target = self.scroll_target;
            if top < target {
                target = top.max(0.0);
            } else if bottom > target + self.scroll_viewport {
                target = (bottom - self.scroll_viewport).min(self.scroll_max);
            }
            self.aim_scroll(target);
        }
        self.scroll_target = self.scroll_target.clamp(0.0, self.scroll_max);
        self.scroll_y = self.scroll_y.clamp(0.0, self.scroll_max);

        {
            use std::sync::atomic::{AtomicIsize, Ordering};
            static LAST: AtomicIsize = AtomicIsize::new(isize::MAX);
            let sel = self
                .app
                .list_state
                .selected()
                .map_or(-1isize, |index| index as isize);
            if LAST.swap(sel, Ordering::Relaxed) != sel {
                debug_log(&format!(
                    "scroll: sel={:?} rows={} row_h={:.1} vp={:.1} max={:.1} y={:.1} target={:.1}",
                    self.app.list_state.selected(),
                    rows.len(),
                    row_h,
                    self.scroll_viewport,
                    self.scroll_max,
                    self.scroll_y,
                    self.scroll_target
                ));
            }
        }

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
        // 选框目标：目标没变就不重置起点（每帧都重置会让动画永远走不完）
        if let Some(index) = self.app.list_state.selected() {
            self.aim_marker(index as f32 * row_h);
        } else {
            self.aim_marker(0.0);
        }

        // 只渲染可视区附近的行
        let first = ((self.scroll_y / row_h).floor() as usize).saturating_sub(1);
        let count = (self.scroll_viewport / row_h).ceil() as usize + 3;
        let last = (first + count).min(rows.len()).max(first);
        let mut inner = div()
            .absolute()
            // **必须减掉 `first * row_h`**：inner 里只堆 `first..last`，第一行落在
            // inner 的 y=0，而它在内容坐标系里的位置是 `first * row_h`。
            // 只写 `-scroll_y` 时偏差 = `first * row_h`，滚得越深偏得越多，
            // 到底部整片推到容器上方 → 列表一片空白（2026-11-18 修）。
            .top(px(first as f32 * row_h - self.scroll_y))
            .left(px(0.))
            .w_full()
            .flex()
            .flex_col();

        // 选框：一个独立的绝对定位块，位置由 `marker_y` 插值 —— 这样换行时是滑过去的。
        //
        // 注意坐标基准：inner 里只渲染 `first..last` 这几行，**第一行落在 y=0**，
        // 而 `marker_y` 是「整个列表的内容坐标」= index × row_h。滚动到下面（first > 0）时
        // 必须减掉 `first × row_h`，否则选框会一路往下漂（越按越偏）。
        if self.app.list_state.selected().is_some() {
            inner = inner.child(
                div()
                    .absolute()
                    // 行高 31 而卡片 28，卡片是居中的（上下各 ROW_GAP/2），marker 要跟着偏
                    .top(px(self.marker_y - first as f32 * row_h + style::ROW_GAP / 2.0))
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
                // 密度模式（`Ctrl+Tab`）：单行对齐表
                render_row(row, p, metrics, &panel)
            } else {
                render_card_row(row, p, metrics, &panel)
            });
        }

        let mut area = div()
            .id("list")
            .relative()
            .flex_1()
            .w_full()
            .overflow_hidden()
            .pb(px(style::LIST_TOP))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(inner);

        // 滚动条（只做指示，不能拖）：轨道铺满列表区右边，滑块高度 = 可视/内容。
        //
        // 用百分比（`relative`）而不是像素：列表区是 `flex_1`，高由窗口决定，
        // 而窗口高是固定的（主屏 55%），与「最多 12 行」的 `scroll_viewport` 不是一回事。
        // 算像素的话滑块只占顶部一小段，看着像没画满。
        // 轨道 = 上方空白 + 滑块，两者比例相加 ≤ 1，滚到底时正好贴底。
        let content_h = rows.len() as f32 * row_h;
        if content_h > self.scroll_viewport + 0.5 {
            let thumb = (self.scroll_viewport / content_h).clamp(style::SCROLLBAR_MIN_RATIO, 1.0);
            let fraction = if self.scroll_max > 0.0 {
                (self.scroll_y / self.scroll_max).clamp(0.0, 1.0)
            } else {
                0.0
            };
            area = area.child(
                div()
                    .absolute()
                    .right(px(1.))
                    .top(px(0.))
                    .bottom(px(0.))
                    .w(px(style::SCROLLBAR_WIDTH))
                    .child(div().h(relative(fraction * (1.0 - thumb))).w_full())
                    .child(
                        div()
                            .h(relative(thumb))
                            .w_full()
                            .rounded(px(style::SCROLLBAR_WIDTH / 2.0))
                            .bg(style::with_alpha(p.fg, style::SCROLLBAR_ALPHA)),
                    ),
            );
        }

        area.into_any_element()
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
                selected: selected == Some(index),
                color: style::badge_color(item.kind),
                chip_progress: self.chip_progress(index, selected),
                favorite: self.app.is_favorite(item),
                unavailable: matches!(item.control, crate::providers::Control::Unavailable),
                number: (select_mode && index < 9).then_some(index + 1),
                state: item.control.is_control().then(|| {
                    (
                        item.control.label(),
                        control_color(&item.control, &self.palette),
                    )
                }),
                secondary: card_parts(item).0,
                trailing: card_parts(item).1,
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
                    // 密度模式滚拼接的附注；卡片模式滚第二行
                    let (text, width) = if self.app.detail {
                        (note_text(item), metrics.note)
                    } else {
                        (card_parts(item).0, metrics.secondary)
                    };
                    let overflow = display_width(&text) as f32 * note_advance - width;
                    ping_pong(overflow, elapsed)
                } else {
                    0.0
                },
                // detail 模式的每条明细列都单独算：超宽了就滚，没超就不动。
                // 之前这里只有「第一列 + 附注列」两路，detail 下除表头以外的列
                // 全走 `truncate()` 硬切，选中也看不到后半段。
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
    /// 是否是选中行（选中行不做 hover 高亮，否则会盖住 marker）
    selected: bool,
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
    /// 控件行的状态（文本 + 颜色）：`开` / `关` / 选项 / 数值 / 动作说明。
    /// 普通条目为 `None`（不占列宽）。
    state: Option<(String, Rgba)>,
    /// 卡片第二行：路径 / 分组等次要信息（已去掉用作右侧的那一列）
    secondary: String,
    /// 卡片右侧的尾巴：没控件时用最后一列（次数 / 时间），否则 `None`
    trailing: Option<String>,
}

/// 行内固定列的宽度（`number` 只在选择模式下占位）
#[derive(Clone, Copy)]
struct RowMetrics {
    title: f32,
    note: f32,
    /// 右侧尾巴 / 状态列宽度
    state: f32,
    /// 卡片模式第二行可用的宽度
    secondary: f32,
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
        .whitespace_nowrap();

    // 选中行的底色来自 marker（背景层），这里再挂 hover 会把它盖掉，所以只给未选中行加
    let card = if row.selected {
        card
    } else {
        card.hover(|style| style.bg(p.panel))
    };

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
        .child(
            // 状态列（`:set` / 开关）：TUI 里由 `ui.rs` 按 `Control` 追加，
            // GUI 这边同样单独占一列（之前根本没画，所以只剩「标题 + 附注」两列）。
            div()
                .w(px(metrics.state))
                .flex_shrink_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(style::NOTE_SIZE))
                .text_color(row.state.as_ref().map_or(p.dim, |(_, color)| *color))
                .child(
                    row.state
                        .as_ref()
                        .map(|(label, _)| label.clone())
                        .unwrap_or_default(),
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

/// 卡片第二行的缩进（对齐到 chip 右侧）
fn card_indent() -> f32 {
    style::ACCENT_BAR + 8.0 * 2.0 + style::STAR_WIDTH + style::BADGE_WIDTH + 8.0
}

/// 两行卡片（默认形态）：第一行标题 + 右侧尾巴，第二行路径 / 分组（dim 小字）。
///
/// `Ctrl+Tab` 会切回 [`render_row`] 的单行对齐表（密度模式）。
fn render_card_row(
    row: &RowSnapshot,
    p: Palette,
    metrics: RowMetrics,
    panel: &WeakEntity<Panel>,
) -> AnyElement {
    let title = row.columns.first().cloned().unwrap_or_default();
    let fg = if row.unavailable { p.faint } else { p.fg };

    // 右侧：控件状态（开 / 关 / 数值）优先，否则次数 / 时间
    let trailing = row
        .state
        .clone()
        .or_else(|| row.trailing.clone().map(|text| (text, p.dim)));

    let head = div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(style::ROW_GAP_X))
        .child(if let Some(n) = row.number {
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
            // 标题：外层定宽裁剪，内层负 margin 左移 = 左右滚动
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
            div()
                .w(px(metrics.state))
                .flex_shrink_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(style::NOTE_SIZE))
                .text_color(trailing.as_ref().map_or(p.dim, |(_, color)| *color))
                .child(
                    trailing
                        .map(|(text, _)| text)
                        .unwrap_or_default(),
                ),
        );

    // 第二行：路径 / 分组（超宽时跟标题一样左右滚动）
    let sub = div()
        .w_full()
        .pl(px(card_indent()))
        .overflow_hidden()
        .whitespace_nowrap()
        .child(
            div()
                .ml(px(-row.note_shift))
                .whitespace_nowrap()
                .text_size(px(style::NOTE_SIZE))
                .text_color(p.dim)
                .child(row.secondary.clone()),
        );

    let card = div()
        .id(("card", row.index))
        .h(px(style::CARD_HEIGHT))
        .w_full()
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(2.))
        .px(px(style::ROW_PAD_X))
        .rounded(px(style::ROW_RADIUS))
        .whitespace_nowrap();

    // 选中行的底色由 marker 画，未选中行只挂 hover（卡片不铺底色——用户要的是
    // 两行结构，不是一张张色块）。
    let card = if row.selected {
        card
    } else {
        card.hover(|style| style.bg(p.panel))
    };

    div()
        .id(("card-row", row.index))
        .w_full()
        .h(px(style::CARD_HEIGHT + style::ROW_GAP))
        .flex()
        .items_center()
        .child(card.child(head).child(sub))
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

        // 窗口外观要等**首帧渲染**时再设：`open_window` 的那个闭包里调太早，
        // GPUI 之后还会 show / resize，把 DWM 的 frame 状态（边框、阴影）重置回去。
        if !self.frame_applied {
            self.frame_applied = true;
            if let Err(error) = frame::apply(window, self.always_on_top, self.rounded) {
                crate::tray::log(&format!("[tupa-gui] 窗口外观设置失败：{error:#}"));
            }
            // 必须在 `frame::apply`（里面有 `SWP_FRAMECHANGED`）**之后**：
            // 先设的话会被 frame change 重置回 gpui 的 alpha = 0（黑玻璃）。
            if self.acrylic_on
                && let Err(error) = acrylic::enable(window)
            {
                crate::tray::log(&format!("[tupa-gui] 亚克力没挂上：{error:#}"));
            }
        }

        // 高度跟内容走（紧凑态只有输入行、出结果后向下展开）。
        // 用 `window.on_next_frame` 推后一帧再 resize：直接在 render 里调会触发
        // 同步 relayout/render → "cannot update Panel while it is already being updated"。
        // 尺寸是**逻辑像素**，GPUI 自己会处理 DPI（Win32 SetWindowPos 那条路走不通：
        // 请求 132px 实际窗口还是 66px，说明系统按 DPI 又缩了一次）。
        let wanted = self.desired_height().min(self.max_height);
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
            // `rounded` 时圆角交给 DWM（窗口级），内容自己不再画一圈：
            // 内容在客户区里（比窗口小一圈 insets），再画 12px 圆角会在
            // DWM 圆角与内容圆角之间露出一条弧形亚克力缝。
            .rounded(px(if self.rounded { style::WINDOW_RADIUS } else { 0.0 }))
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

        // 动画由 vsync 驱动：还有东西在动就注册「下一帧」回调，回调里推进动画并重绘。
        // 用定时器 notify 会和屏幕刷新（这台 120Hz）互相挤，表现就是卡卡的。
        self.schedule_animation(window, cx);

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
/// 状态列的颜色（和 TUI 的 `ui::state_style` 一致：开=绿、关=灰、选项/数值=强调色）
fn control_color(control: &crate::providers::Control, p: &Palette) -> Rgba {
    use crate::providers::Control;
    match control {
        Control::Toggle { value: true } => p.good,
        Control::Toggle { value: false } => p.dim,
        Control::Choice { .. } | Control::Number { .. } => p.accent,
        Control::Action { .. } => p.muted,
        Control::Unavailable => p.faint,
        Control::Run => p.dim,
    }
}

/// 卡片两行怎么拆：`columns[0]` 是标题；最后一列当右侧尾巴（次数 / 时间），
/// 剩下的拼成第二行。控件行不抽尾巴（右侧显示 control 的 label）。
fn card_parts(item: &crate::providers::Item) -> (String, Option<String>) {
    let title = item.columns.first().map(String::as_str).unwrap_or("");
    let mut rest: Vec<&str> = item.columns.iter().skip(1).map(String::as_str).collect();

    let trailing = if !item.control.is_control() && item.columns.len() >= 3 {
        rest.pop().map(str::to_string)
    } else {
        None
    };

    let secondary = rest
        .into_iter()
        .filter(|value| !value.is_empty() && *value != title)
        .collect::<Vec<_>>()
        // 用竖线分隔而不是「 · 」：第二行往往有 2–3 段（路径 / 分组 / 时间），
        // 中点跟汉字挤在一起时边界看不清。
        .join(" │ ");
    (secondary, trailing)
}

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

    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{Action, Item, Kind};
    use std::path::PathBuf;

    fn item(columns: &[&str]) -> Item {
        Item::new(
            Kind::App,
            columns.iter().map(|s| s.to_string()).collect(),
            Action::Open(PathBuf::from("x")),
        )
    }

    /// 最后一列当右侧尾巴（次数 / 时间），其余进第二行
    #[test]
    fn card_parts_takes_last_column_as_trailing() {
        let (secondary, trailing) = card_parts(&item(&["Code", "code", "19 次"]));
        assert_eq!(secondary, "code");
        assert_eq!(trailing.as_deref(), Some("19 次"));
    }

    /// 第二行多段之间用竖线分隔（中点跟汉字挤在一起看不清边界）
    #[test]
    fn card_parts_separates_segments_with_a_bar() {
        let (secondary, trailing) = card_parts(&item(&["标题", r"D:\a", "网络", "3 天前"]));
        assert_eq!(secondary, r"D:\a │ 网络");
        assert_eq!(trailing.as_deref(), Some("3 天前"));
    }

    /// 第二行与标题重复（home 里 web 条目的 URL 出现了两次）就不显示
    #[test]
    fn card_parts_drops_duplicate_of_title() {
        let (secondary, _) = card_parts(&item(&["https://x", "https://x", "5 次"]));
        assert_eq!(secondary, "");
    }

    /// 两列时没有尾巴，整列都是第二行
    #[test]
    fn card_parts_keeps_two_column_items_intact() {
        let (secondary, trailing) = card_parts(&item(&["Downloads", "D:\\"]));
        assert_eq!(secondary, "D:\\");
        assert_eq!(trailing, None);
    }
}
