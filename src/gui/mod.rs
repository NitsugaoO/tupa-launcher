//! 自绘无边框窗口（`--gui`）。
//!
//! 复用 TUI 的全部逻辑：`ui::draw` 照常排版到离屏的 `TestBackend` 上，
//! 这里只负责开窗、把 Buffer 画成像素、转发键盘 / 输入法事件。
//!
//! ```text
//! tupa-launcher --gui        打开面板（已在运行时只是把它叫到前面）
//! ```

mod text;

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::KeyModifiers;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use softbuffer::{Context as SoftContext, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{ResizeDirection, Window, WindowLevel};

use crate::app::App;
use crate::config::Config;
use crate::theme::Theme;
use crate::ui;
use text::{Canvas, TextRenderer};

/// 窗口标题（daemon 按它找窗口，别随便改）
const WINDOW_TITLE: &str = "tupa-launcher";
/// Windows 窗口类名（重复启动时用它找已有窗口）
const WINDOW_CLASS: &str = "TupaLauncherWindow";

/// 面板占主显示器的比例
const WIDTH_RATIO: f64 = 0.62;
const HEIGHT_RATIO: f64 = 0.55;

/// 鼠标离窗口边缘多近算「拖拽缩放」（逻辑像素）
const EDGE_MARGIN: f64 = 6.0;

/// 内容四周留的空白（逻辑像素）：给窗口圆角让位，
/// 否则左上角的输入行会被圆角切掉一块，右/下也会紧贴边框
const PADDING: f32 = 7.0;

pub fn run(cfg: Config, theme: Theme) -> Result<()> {
    // 已经开着就把它叫到前面，别开第二个
    #[cfg(windows)]
    if focus_existing() {
        return Ok(());
    }

    let mut app = App::new(cfg, theme);
    app.refresh();

    let event_loop = EventLoop::new().context("创建事件循环失败")?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut gui = Gui {
        app,
        state: None,
        modifiers: ModifiersState::empty(),
        composing: false,
        hidden: false,
        cursor: PhysicalPosition::new(0.0, 0.0),
        dirty: true,
        // 调试用：`TUPA_GUI_SHOT=out.bmp` 渲一帧就存图退出（不需要看屏幕）
        shot: std::env::var_os("TUPA_GUI_SHOT").map(PathBuf::from),
    };

    event_loop.run_app(&mut gui).context("窗口循环异常退出")?;
    Ok(())
}

/// 窗口 / 画布 / 离屏终端
struct State {
    window: Arc<Window>,
    // 必须比 surface 活得久
    _context: SoftContext<Arc<Window>>,
    surface: Surface<Arc<Window>, Arc<Window>>,
    terminal: Terminal<TestBackend>,
    text: TextRenderer,
    /// 物理像素尺寸
    width: u32,
    height: u32,
    cols: u16,
    rows: u16,
}

struct Gui {
    app: App,
    state: Option<State>,
    modifiers: ModifiersState,
    /// 输入法正在组合（此时键盘事件交给输入法，不再当成普通按键）
    composing: bool,
    hidden: bool,
    /// 鼠标位置（物理像素），拖拽窗口时要用
    cursor: PhysicalPosition<f64>,
    /// 这一轮需要重绘
    dirty: bool,
    /// 渲一帧后存图退出（`TUPA_GUI_SHOT`，调试渲染用）
    shot: Option<PathBuf>,
}

impl ApplicationHandler for Gui {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        match State::new(event_loop) {
            Ok(state) => {
                self.dirty = true;
                self.state = Some(state);
            }
            Err(error) => {
                eprintln!("[tupa-launcher] 创建窗口失败：{error:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => self.quit(event_loop),

            WindowEvent::Resized(size) => self.resize(size),

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(state) = &mut self.state {
                    state.text.set_scale(scale_factor as f32);
                }
                self.dirty = true;
            }

            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return; // 松开不处理，否则 Windows 上会重复触发
                }
                // 输入法组合期间（拼音还没上屏）按键归输入法管
                if self.composing {
                    return;
                }
                if let Some(key) = to_key(&event, self.modifiers) {
                    self.app.on_key(key);
                    self.dirty = true;
                }
            }

            WindowEvent::Ime(ime) => self.ime(ime),

            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = position;
            }

            // 无边框窗口没有标题栏：自己判断「拖边缘 = 缩放」「拖顶部 = 移动」
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.begin_window_drag(),

            WindowEvent::RedrawRequested => self.redraw(),

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.app.should_quit {
            self.quit(event_loop);
            return;
        }

        // daemon 会用 ShowWindow 从外部显隐窗口，winit 收不到事件，只能自己对账
        if let Some(state) = &self.state {
            let visible = window_visible(&state.window);
            if visible == self.hidden {
                self.hidden = !visible;
                if visible {
                    self.dirty = true;
                } else {
                    self.app.reset();
                    self.app.preedit.clear();
                    self.composing = false;
                }
            }
        }

        if self.hidden {
            // 隐藏期间不渲染，只低频等 daemon 把窗口叫回来
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(200),
            ));
            return;
        }

        self.app.tick = self.app.tick.wrapping_add(1);
        self.app.drain_events();

        if self.app.should_hide {
            self.app.should_hide = false;
            self.hide();
        }

        if self.dirty || self.app.animate || self.app.pending {
            self.dirty = false;
            if let Some(state) = &self.state {
                state.window.request_redraw();
            }
        }

        // 滚动动画 / 等后台结果时用 30ms 一帧，否则降频省电（和 TUI 一致）
        let delay = if self.app.animate || self.app.pending {
            30
        } else {
            150
        };
        event_loop
            .set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(delay)));
    }
}

impl Gui {
    fn quit(&mut self, event_loop: &ActiveEventLoop) {
        self.app.should_quit = true;
        event_loop.exit();
    }

    fn hide(&mut self) {
        if let Some(state) = &self.state {
            state.window.set_visible(false);
        }
        self.app.reset();
        self.app.preedit.clear();
        self.composing = false;
        self.hidden = true;
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        let Some(state) = &mut self.state else {
            return;
        };
        if size.width == 0 || size.height == 0 {
            return;
        }
        state.width = size.width;
        state.height = size.height;
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        if let Err(error) = state.surface.resize(width, height) {
            eprintln!("[tupa-launcher] surface resize 失败：{error}");
        }
        self.dirty = true;
    }

    /// 鼠标左键按下：靠近边缘就进入系统缩放，顶部一条则可以拖动窗口
    fn begin_window_drag(&mut self) {
        let Some(state) = &self.state else {
            return;
        };
        let (width, height) = (state.width as f64, state.height as f64);
        let (x, y) = (self.cursor.x, self.cursor.y);

        // 缩放的"热区"宽度（物理像素，跟着 DPI 走）
        let edge = (EDGE_MARGIN * state.window.scale_factor()).max(4.0);
        let (west, east) = (x <= edge, x >= width - edge);
        let (north, south) = (y <= edge, y >= height - edge);
        let direction = match (west, east, north, south) {
            (true, _, true, _) => Some(ResizeDirection::NorthWest),
            (_, true, true, _) => Some(ResizeDirection::NorthEast),
            (true, _, _, true) => Some(ResizeDirection::SouthWest),
            (_, true, _, true) => Some(ResizeDirection::SouthEast),
            (true, _, _, _) => Some(ResizeDirection::West),
            (_, true, _, _) => Some(ResizeDirection::East),
            (_, _, true, _) => Some(ResizeDirection::North),
            (_, _, _, true) => Some(ResizeDirection::South),
            _ => None,
        };
        if let Some(direction) = direction {
            let _ = state.window.drag_resize_window(direction);
            return;
        }

        // 顶部一条（内边距 + 输入行）当标题栏用
        let band = f64::from(PADDING) * state.window.scale_factor() + f64::from(state.text.cell_h) / 2.0;
        if y <= band {
            let _ = state.window.drag_window();
        }
    }

    fn ime(&mut self, ime: Ime) {
        match ime {
            Ime::Enabled => {
                self.composing = true;
            }
            Ime::Preedit(text, _cursor) => {
                // 组合中：只更新预览文本，绝不触发 provider 查询
                self.composing = true;
                self.app.preedit = text;
                self.dirty = true;
            }
            Ime::Commit(text) => {
                self.composing = false;
                self.app.preedit.clear();
                if !text.is_empty() {
                    self.app.insert_str(&text);
                }
                self.dirty = true;
            }
            Ime::Disabled => {
                self.composing = false;
                self.app.preedit.clear();
                self.dirty = true;
            }
        }
    }

    fn redraw(&mut self) {
        let Self {
            app,
            state,
            shot,
            composing,
            ..
        } = self;
        let Some(state) = state.as_mut() else {
            return;
        };
        if state.width == 0 || state.height == 0 {
            return;
        }

        // 1) 按新的 cell 尺寸重算终端网格（扣掉四周留白）
        let padding = (PADDING * state.window.scale_factor() as f32).round() as i32;
        let usable_w = (state.width as f32 - 2.0 * padding as f32).max(1.0);
        let usable_h = (state.height as f32 - 2.0 * padding as f32).max(1.0);
        let cols = ((usable_w / state.text.cell_w).floor() as u16).max(1);
        let rows = ((usable_h / state.text.cell_h).floor() as u16).max(1);
        if cols != state.cols || rows != state.rows {
            state.cols = cols;
            state.rows = rows;
            // Terminal::resize 只改内部缓冲，离屏 backend 得自己改
            state.terminal.backend_mut().resize(cols, rows);
            if let Err(error) = state.terminal.resize(Rect::new(0, 0, cols, rows)) {
                eprintln!("[tupa-launcher] 终端尺寸调整失败：{error}");
                return;
            }
        }

        // 2) 照常排版（ui::draw 一行没改）
        app.tick = app.tick.wrapping_add(1);
        if let Err(error) = state.terminal.draw(|frame| ui::draw(frame, app)) {
            eprintln!("[tupa-launcher] 渲染失败：{error}");
            return;
        }

        let backend = state.terminal.backend();
        let (cursor, cursor_visible) = (backend.cursor_position(), backend.cursor_visible());
        let buffer = backend.buffer();
        let theme = app.theme;

        // 3) Buffer → 像素
        let mut surface = match state.surface.buffer_mut() {
            Ok(surface) => surface,
            Err(error) => {
                eprintln!("[tupa-launcher] 取帧缓冲失败：{error}");
                return;
            }
        };
        let (width, height) = (surface.width().get(), surface.height().get());
        {
            let mut canvas = Canvas {
                width,
                height,
                pixels: &mut surface,
            };
            // 先整块铺底色，网格铺不满的右/下边缘就是背景色而不是黑边
            canvas.fill_all(text::to_pixel(theme.background));
            state.text.render(buffer, theme, &mut canvas, (padding, padding));

            // 光标：一条竖线，比系统 IME 的组合框更贴合无边框窗口
            if cursor_visible {
                let (x, y) = state.text.cell_origin(cursor.x, cursor.y);
                let caret_w = (state.text.cell_w * 0.15).max(2.0) as u32;
                let caret_h = (state.text.cell_h - 6.0).max(4.0) as u32;
                canvas.fill_rect(
                    x + padding,
                    y + padding + 3,
                    caret_w,
                    caret_h,
                    text::to_pixel(theme.accent),
                );
            }
        }
        // 调试：把这一帧写成 BMP（`read` 工具能直接看）
        if let Some(path) = shot.take() {
            match save_bmp(&path, width, height, &surface) {
                Ok(()) => eprintln!("[tupa-launcher] 已保存截图 {}", path.display()),
                Err(error) => eprintln!("[tupa-launcher] 保存截图失败：{error:#}"),
            }
            app.should_quit = true;
        }

        // winit 要求上屏前通知一次，DWM 才会把这一帧真正合成到屏幕上
        state.window.pre_present_notify();
        if let Err(error) = surface.present() {
            eprintln!("[tupa-launcher] 上屏失败：{error}");
            return;
        }

        // 4) 让输入法候选框跟着光标走（winit 内部会再乘一次 scale_factor）
        if *composing {
            let (x, y) = state.text.cell_origin(cursor.x, cursor.y);
            state.window.set_ime_cursor_area(
                PhysicalPosition::new(x + padding, y + padding),
                PhysicalSize::new(
                    state.text.cell_w.round() as u32,
                    state.text.cell_h.round() as u32,
                ),
            );
        }
    }
}

impl State {
    fn new(event_loop: &ActiveEventLoop) -> Result<Self> {
        let (size, position) = geometry(event_loop);

        let attrs = Window::default_attributes()
            .with_title(WINDOW_TITLE)
            .with_decorations(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_resizable(true)
            .with_inner_size(size)
            .with_position(position);

        #[cfg(windows)]
        let attrs = {
            use winit::platform::windows::WindowAttributesExtWindows;
            // 注意：不要开 `with_undecorated_shadow`，它用 DWM 玻璃实现阴影，
            // 整个客户区会变成透明的（softbuffer 的像素压不住）
            attrs.with_class_name(WINDOW_CLASS).with_skip_taskbar(true)
        };

        let window = Arc::new(event_loop.create_window(attrs).context("创建窗口失败")?);
        window.set_ime_allowed(true);
        // 创建窗口时 winit 还不知道 DPI，位置会按 1:1 摆错，这里按真实屏幕重新居中
        center_window(&window);
        window.focus_window();

        #[cfg(windows)]
        {
            use winit::platform::windows::{CornerPreference, WindowExtWindows};
            window.set_corner_preference(CornerPreference::Round);
        }

        let scale = window.scale_factor() as f32;
        let text = TextRenderer::new(scale);

        let context = SoftContext::new(window.clone())
            .map_err(|error| anyhow::anyhow!("初始化 softbuffer 失败：{error}"))?;
        let surface = Surface::new(&context, window.clone())
            .map_err(|error| anyhow::anyhow!("创建绘制表面失败：{error}"))?;

        let inner = window.inner_size();
        let (width, height) = (inner.width.max(1), inner.height.max(1));
        let cols = ((width as f32 / text.cell_w).floor() as u16).max(1);
        let rows = ((height as f32 / text.cell_h).floor() as u16).max(1);

        let terminal = Terminal::new(TestBackend::new(cols, rows)).context("创建离屏终端失败")?;

        let mut state = State {
            window,
            _context: context,
            surface,
            terminal,
            text,
            width,
            height,
            cols,
            rows,
        };
        state.resize_surface();
        Ok(state)
    }

    fn resize_surface(&mut self) {
        if let (Some(width), Some(height)) =
            (NonZeroU32::new(self.width), NonZeroU32::new(self.height))
            && let Err(error) = self.surface.resize(width, height)
        {
            eprintln!("[tupa-launcher] surface resize 失败：{error}");
        }
    }
}

/// 按当前显示器重新居中（创建时的 position 在 DPI 未知时会偏）
fn center_window(window: &Window) {
    let Some(monitor) = window.current_monitor() else {
        return;
    };
    let screen = monitor.size();
    let origin = monitor.position();
    let size = window.outer_size();
    let x = origin.x + (screen.width.saturating_sub(size.width) / 2) as i32;
    let y = origin.y + (screen.height.saturating_sub(size.height) / 2) as i32;
    window.set_outer_position(PhysicalPosition::new(x, y));
}

/// 主显示器上 62% × 55%、居中
fn geometry(event_loop: &ActiveEventLoop) -> (PhysicalSize<u32>, PhysicalPosition<i32>) {
    let Some(monitor) = event_loop.primary_monitor().or_else(|| {
        let mut monitors = event_loop.available_monitors();
        monitors.next()
    }) else {
        return (PhysicalSize::new(1100, 640), PhysicalPosition::new(80, 80));
    };

    let screen = monitor.size();
    let origin = monitor.position();
    let width = (f64::from(screen.width) * WIDTH_RATIO).round() as u32;
    let height = (f64::from(screen.height) * HEIGHT_RATIO).round() as u32;
    let x = origin.x + (screen.width.saturating_sub(width) / 2) as i32;
    let y = origin.y + (screen.height.saturating_sub(height) / 2) as i32;

    (
        PhysicalSize::new(width.max(320), height.max(200)),
        PhysicalPosition::new(x, y),
    )
}

/// winit 的按键 → crossterm 的按键（`App::on_key` 只认后者）
fn to_key(
    event: &winit::event::KeyEvent,
    modifiers: ModifiersState,
) -> Option<crossterm::event::KeyEvent> {
    use crossterm::event::{KeyCode, KeyEvent};

    let mut mods = KeyModifiers::NONE;
    if modifiers.contains(ModifiersState::CONTROL) {
        mods |= KeyModifiers::CONTROL;
    }
    if modifiers.contains(ModifiersState::ALT) {
        mods |= KeyModifiers::ALT;
    }
    if modifiers.contains(ModifiersState::SHIFT) {
        mods |= KeyModifiers::SHIFT;
    }

    let code = match &event.logical_key {
        Key::Named(NamedKey::Enter) => KeyCode::Enter,
        Key::Named(NamedKey::Escape) => KeyCode::Esc,
        Key::Named(NamedKey::Backspace) => KeyCode::Backspace,
        Key::Named(NamedKey::Delete) => KeyCode::Delete,
        Key::Named(NamedKey::ArrowLeft) => KeyCode::Left,
        Key::Named(NamedKey::ArrowRight) => KeyCode::Right,
        Key::Named(NamedKey::ArrowUp) => KeyCode::Up,
        Key::Named(NamedKey::ArrowDown) => KeyCode::Down,
        Key::Named(NamedKey::Home) => KeyCode::Home,
        Key::Named(NamedKey::End) => KeyCode::End,
        Key::Named(NamedKey::PageUp) => KeyCode::PageUp,
        Key::Named(NamedKey::PageDown) => KeyCode::PageDown,
        // Shift+Tab：winit 给的是 Tab + SHIFT，crossterm 那边叫 BackTab
        Key::Named(NamedKey::Tab) if modifiers.contains(ModifiersState::SHIFT) => KeyCode::BackTab,
        Key::Named(NamedKey::Tab) => KeyCode::Tab,
        Key::Named(NamedKey::Space) => KeyCode::Char(' '),
        Key::Character(text) => {
            let mut chars = text.chars();
            let first = chars.next()?;
            if chars.next().is_some() {
                return None; // 一次给多个字符的（输入法残留）不当按键处理
            }
            // Alt / Ctrl 组合不产生文本（winit 的 `text` 为空）。
            // 万一修饰键状态没同步过来，这里补一个 ALT，
            // 免得 `alt+`` 热键把 ` 输进输入框
            if event.text.is_none() && !mods.contains(KeyModifiers::ALT) {
                mods |= KeyModifiers::ALT;
            }
            KeyCode::Char(first)
        }
        _ => return None,
    };

    Some(KeyEvent::new(code, mods))
}

/// 窗口现在是可见的吗（`ShowWindow` 从外部改动时 winit 自己不知道）
fn window_visible(window: &Window) -> bool {
    #[cfg(windows)]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;

        if let Ok(handle) = window.window_handle()
            && let RawWindowHandle::Win32(win32) = handle.as_raw()
        {
            return unsafe { IsWindowVisible(win32.hwnd.get() as _) } != 0;
        }
        true
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        true
    }
}

/// 调试用：把帧缓冲写成 32 位 BMP（顶朝下，不用管行序翻转）
fn save_bmp(path: &std::path::Path, width: u32, height: u32, pixels: &[u32]) -> Result<()> {
    use std::io::Write;

    let mut out = Vec::with_capacity(54 + (width as usize * height as usize) * 4);
    let size = 54 + width * height * 4;
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    out.extend_from_slice(&(-(height as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(width * height * 4).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]);
    for pixel in pixels {
        out.push((pixel & 0xff) as u8);
        out.push(((pixel >> 8) & 0xff) as u8);
        out.push(((pixel >> 16) & 0xff) as u8);
        out.push(0xff);
    }

    let mut file = std::fs::File::create(path).context("创建截图文件失败")?;
    file.write_all(&out).context("写截图文件失败")?;
    Ok(())
}

/// 把已经开着的面板叫到最前面（找不到就返回 false）
#[cfg(windows)]
fn focus_existing() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindow, SW_SHOW,
    };

    let class: Vec<u16> = WINDOW_CLASS.encode_utf16().chain([0]).collect();
    let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    if hwnd.is_null() {
        return false;
    }
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
    true
}
