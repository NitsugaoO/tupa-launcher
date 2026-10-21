# 套壳交接（自建无边框窗口）

> **这份文档是给下一个会话的开工说明。** 设计取舍看
> [`docs/05-native-window.md`](05-native-window.md)；项目整体现状看
> [`docs/06-handover.md`](06-handover.md)。
>
> 最后更新：阶段 1~3 已跑通（`--gui` 可用，daemon 已切换到自绘窗口）

## 0. 一句话目标

把面板从 `wt -p tupa-launcher` 换成 **`tupa-launcher.exe --gui`** 的自绘无边框窗口：

- 复用 `App` 状态机、`ui::draw`、所有 provider
- 保留现有 TUI（`--gui` 并存，先不改默认）
- daemon 的热键 / 显隐 / Copilot 键逻辑不动

## 1. 当前状态

**已经实现的（`src/gui/`，约 900 行）：**

| 文件 | 内容 |
|---|---|
| `src/gui/mod.rs` | winit 窗口 + softbuffer 上屏 + 键位映射 + IME + 显隐/单实例 |
| `src/gui/text.rs` | `ratatui::Buffer` → 像素：cosmic-text 排版 + swash 光栅化 + 字形缓存 |

- `tupa-launcher --gui` 打开自绘无边框窗口；重复执行会把已有窗口叫到前面
- 窗口：无边框、置顶、跳过任务栏、主显示器 62%×55% 居中
- 字体：英文 `FiraCode Nerd Font Mono`，中文按字符宽度切到 `Microsoft YaHei UI`
- 行高 / 基线按字体实测（不再硬编码），中英文共用一条基线
- `Esc` 隐藏进程常驻；daemon 的 `ShowWindow` 能从外部再次唤起
- `daemon::launch_panel` 已改为拉起同目录 `tupa-launcher.exe --gui`
- `util::find_panel` 已改为 **进程名 `tupa-launcher.exe` + 标题前缀** 识别

**还没做的：** 圆角/阴影/淡入、鼠标点击、滚动条、DPI 切换的实测、窗口尺寸配置项。

### 两套面板并存

`[daemon] panel` 决定热键唤起哪一版：

```toml
[daemon]
panel = "tui"   # Windows Terminal 里的版本（默认，当前线上用的就是它）
# panel = "gui" # 自绘无边框窗口（仍在打磨）
```

- `tupa-launcher`（不带参数）= TUI，仍然可用
- `tupa-launcher --gui` = 自绘窗口
- 两者共用同一份 `App` / `ui::draw` / provider，daemon 只是 `launch_panel` 分叉

### GlazeWM 规则（本机已配好）

`~/.glzr/glazewm/config.yaml` 的 `window_rules` 里加了两条（旧的 WT 那条保留，TUI 也能用）：

```yaml
# 自绘窗口：进程名 + winit 窗口类名
- commands: ['set-floating', 'size --width 62% --height 55%', 'position --centered']
  match:
    - window_process: { equals: 'tupa-launcher' }
      window_class: { equals: 'TupaLauncherWindow' }

# 旧版 TUI 面板（Windows Terminal 标签页）
- commands: ['set-floating', 'size --width 62% --height 55%', 'position --centered']
  match:
    - window_class: { equals: 'CASCADIA_HOSTING_WINDOW_CLASS' }
      window_title: { regex: '^tupa-launcher($| - |: )' }
```

有了这条规则，GUI 自己那次 `center_window` 会被 WM 覆盖（最终位置由 GlazeWM 决定）。
没跑 WM 时 `center_window` 仍然保证窗口居中。

- 版本 `Cargo.toml` 0.8.0，68 个测试通过，clippy 零警告
- 命令：`:app` `:ev` `:run` `:go` `:pi` `:web` `:new` `:fav` `:cfg` `:switch`
- 面板进程：`wt.exe -p tupa-launcher`（Windows Terminal）
- daemon：`RegisterHotKey`（默认 Alt+`` ` ``）+ 窗口显隐 + Copilot 键钩子

### 关键接口（套壳要用到的）

| 位置 | 作用 |
|---|---|
| `src/ui.rs::draw(frame: &mut Frame, app: &mut App)` | 纯渲染：输入行 / 列表 / 状态栏；用 `frame.area()`、`frame.set_cursor_position` |
| `src/app.rs::App::new(cfg, theme)` | 状态机 + 启动 worker；`app.refresh()` 发首次查询 |
| `src/app.rs::App::on_key(crossterm::event::KeyEvent)` | 所有键位入口（编辑、补全、选择、执行） |
| `src/app.rs::App::drain_events()` | 收 worker 结果 |
| `src/app.rs::App::tick` | 每帧 +1，选中行滚动用 |
| `src/app.rs::App::animate` / `pending` | 是否 30ms 一帧（否则 150ms） |
| `src/app.rs::App::should_hide` / `reset()` | Esc 隐藏面板；隐藏时清空状态 |
| `src/main.rs::run` | TUI 主循环（渲染 → poll 30/150ms → on_key → drain_events） |
| `src/daemon.rs::launch_panel` | `ShellExecuteW("open", "wt.exe", "-p tupa-launcher")` |
| `src/util.rs::find_panel` | 按「WT 类名 + 标题前缀 `tupa-launcher`」找面板窗口 |
| `src/util.rs::hide_panel/show_panel/panel_visible` | `ShowWindow` / `IsWindowVisible` |

### 数据流（不变）

```
键盘 → App::on_key
        ├─ 编辑 → App::refresh → provider → worker → Evt::Results → drain_events
        ├─ 补全（主线程）
        └─ 执行 → exec::run / Cmd::Apply
渲染 → ui::draw(App)
```

## 2. 已定方案

**A：winit + cosmic-text + softbuffer**（详见 docs/05）：

- `winit 0.30`（`ApplicationHandler`）：无边框、置顶、跳过任务栏
- `softbuffer 0.4`：CPU 像素上传
- `cosmic-text 0.14`：字体/中文 fallback/字形缓存
- 不引入 `wgpu`；先 CPU 渲染，性能不够再换
- 依赖用 `cargo add` 取当前兼容版本，不要盲目锁死；docs/05 的版本是 2026-10 的参考

## 3. 复用策略（最重要）

### 3.1 不改 `ui::draw`

用 ratatui 的离屏 `Terminal<TestBackend>` 跑现有 draw：

```rust
use ratatui::backend::TestBackend;
use ratatui::Terminal;

let backend = TestBackend::new(cols, rows);
let mut terminal = Terminal::new(backend)?;
terminal.draw(|frame| ui::draw(frame, &mut app))?;
let buffer = terminal.backend().buffer();
// 把 buffer 画成像素
```

- 先确认 release 构建能用 `TestBackend`（它是 ratatui 的公开 backend，不是 test-only）
- 如果每帧分配 Buffer 导致 30fps 掉帧，写一个 `MemoryBackend` 实现 `ratatui::backend::Backend`（约 100 行），
  或者直接复用 `Terminal` 的 `current_buffer_mut`。不要一开始就重写 `ui`。
- `terminal.get_cursor_position()` 拿输入光标（`ui::draw` 里已经 `frame.set_cursor_position`）

### 3.2 不改 `App::on_key`

winit 的 `KeyEvent` → `crossterm::event::KeyEvent`：

```rust
fn to_key(key: &winit::event::KeyEvent) -> Option<crossterm::event::KeyEvent> {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use winit::keyboard::{Key, NamedKey};

    let code = match &key.logical_key {
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
        Key::Named(NamedKey::Tab) => KeyCode::Tab,
        Key::Character(s) if s.chars().count() == 1 => KeyCode::Char(s.chars().next()?),
        _ => return None,
    };

    let mut mods = KeyModifiers::NONE;
    if key.modifiers.contains(winit::keyboard::ModifiersState::CONTROL) {
        mods |= KeyModifiers::CONTROL;
    }
    // ALT / SHIFT 同理

    // Shift+Tab：winit 可能给 Tab + Shift，也可能给 BackTab；按实测补
    let code = if code == KeyCode::Tab && key.modifiers.contains(ModifiersState::SHIFT) {
        KeyCode::BackTab
    } else {
        code
    };

    Some(KeyEvent::new(code, mods))
}
```

- 只处理 `ElementState::Pressed`，忽略 Released（否则 Windows 上会重复触发）
- `Ctrl+Tab` 在 GUI 里不再被 WT 占用，可以保留
- `Ime::Commit` 的文本不要走 `on_key`，直接 `App::insert_str`

### 3.3 主循环

```rust
// 每帧
app.tick = app.tick.wrapping_add(1);
terminal.draw(|frame| ui::draw(frame, &mut app))?;   // 得到 Buffer
present(buffer, &mut surface, &mut renderer);

let delay = if app.animate || app.pending { 30 } else { 150 };
// AboutToWait 里 request_redraw + ControlFlow::WaitUntil(now + delay)

app.drain_events();

if app.should_hide {
    app.should_hide = false;
    app.reset();
    window.set_visible(false);   // 不退出
}
```

## 4. 分阶段开工

### 阶段 1：窗口 + 骨架（第一个会话先做这个）

1. `cargo add winit softbuffer cosmic-text`
2. 新建 `src/gui.rs`，`pub fn run(cfg: Config, theme: Theme) -> Result<()>`
3. `main.rs` 加 `--gui` 分支（在 `--query` 之后、TUI 之前）
4. `winit 0.30` 的 `ApplicationHandler`：
   - `resumed`：创建窗口（`decorations(false)`、`window_level(AlwaysOnTop)`、
     可选 `skip_taskbar`）、`softbuffer::Context/Surface`
   - `window_event`：`Resized` 重算 cols/rows、`KeyboardInput` → `App::on_key`、
     `CloseRequested` → 退出、`RedrawRequested` → 渲染
   - `about_to_wait`：`request_redraw` + `ControlFlow::WaitUntil`
5. 初版渲染：只画 cell 背景 + 字符方块（先不接 cosmic-text），验证布局/尺寸
6. 验收：`cargo run -- --gui` 出现无边框、置顶窗口，能看到命令列表

> 阶段 1 不要碰 daemon；先让 `--gui` 能独立运行。

### 阶段 2：字体渲染

- `FontSystem` + `SwashCache`；字体候选：
  - 等宽：`Cascadia Mono` / `Consolas`
  - 中文 fallback：`Microsoft YaHei UI` / `Sarasa Mono SC`（可选）
- 每个 cell：背景填充 + glyph 绘制；用 `UnicodeWidthStr::width` 处理宽字符（占 2 列，第二列跳过）
- 颜色：`ratatui::style::Color` → `(u8,u8,u8)`；`Color::Reset` 用主题背景/前景
- 光标：`terminal.get_cursor_position()` → 画竖线/方块
- 验收：中英文、选中行、光标位置

### 阶段 3：输入 / 键位

- `to_key` 映射（见 3.2）
- `Ctrl+Tab` / `Shift+Tab` / `Tab` 补全 / `Esc`
- `Enter` 执行、`should_hide` 隐藏
- 验收：打字、上下选、Enter、Esc、Tab 补全、Ctrl+Tab 详情

### 阶段 4：中文输入法（最大风险）

- `window.set_ime_allowed(true)`
- `App` 加 `preedit: String`（或在 `gui` 里保存）并让 `ui::draw_input` 显示：
  - `Preedit(text, cursor)`：把 preedit 画在输入光标前，带下划线；**不要** `app.refresh()`
  - `Commit(text)`：`App::insert_str(&text)`（新增方法，只 refresh 一次）
- 每次 Preedit/光标移动都 `window.set_ime_cursor_area(PhysicalPosition, PhysicalSize)`，
  位置 = 光标 cell 的物理像素 `(x*cell_w, y*cell_h)`，size = cell 尺寸
- 验收：中文输入、候选框位置、全角字符光标

### 阶段 5：daemon 接管

- `daemon::launch_panel` 改为启动同目录 `tupa-launcher.exe --gui`（不再 `wt`）
- `util::find_panel` 改为按 **进程 exe 名 + 标题** 识别：
  - `GetWindowThreadProcessId` → `QueryFullProcessImageNameW`，文件名 `tupa-launcher.exe`
  - 标题以 `tupa-launcher` 开头
  - **不要**再用 WT 类名；也不要 `contains`（pi 的 `π - tupa-launcher` 是 `WindowsTerminal.exe`，按进程名就能排除）
- `ShowWindow` / `IsWindowVisible` 对 winit 窗口仍然有效
- 单实例：`CreateMutexW("Local\\TupaLauncherGui")`；已存在则显示已有窗口并退出
- 隐藏：`window.set_visible(false)`，进程常驻；下次热键 `ShowWindow` + `SetForegroundWindow`
- GlazeWM 的 `window_rules` 里那条 WT 规则要改成新窗口（按进程名/标题），或者干脆让 GUI 自己居中/定尺寸

### 阶段 6：细节（可选）

- 62%×55% 尺寸、居中、圆角/阴影（DWM `DwmExtendFrameIntoClientArea`）
- 淡入动画 200ms
- DPI 变化（`ScaleFactorChanged`）重建字体 metrics / cell 尺寸
- 任务栏隐藏、托盘退出

## 5. 关键实现细节

### 5.1 Buffer → 像素

```rust
let area = buffer.area;
for y in area.top()..area.bottom() {
    for x in area.left()..area.right() {
        let cell = &buffer[(x, y)];
        let bg = rgb(cell.style().bg.unwrap_or(Color::Reset), theme);
        fill_rect(framebuffer, x, y, cell_w, cell_h, bg);
        let fg = rgb(cell.style().fg.unwrap_or(Color::Reset), theme);
        draw_text(cell.symbol(), x, y, fg, cell.style());
    }
}
```

- `softbuffer` 的像素是 `u32`（`0x00RRGGBB`），不是 RGBA；要透明/圆角得靠 DWM，不要指望 alpha
- 宽字符：`cell.symbol()` 是 `"中"` 时占 2 列，下一列通常是空 string；按 `UnicodeWidthStr` 跳过
- 只重绘脏区可以后置；先全量重绘（62% 屏约 100×30 cell）

### 5.2 字体 metrics

- 逻辑字号 ~16px，行高 ~20px；`cell_w` 用等宽字体的 advance（半角），`cell_h` 用行高
- 窗口逻辑尺寸 62%×55%（例如 1536×960 屏幕 → 952×528 → cols≈119, rows≈26）
- `winit` 的窗口尺寸是物理像素，乘/除 `scale_factor` 换算；高 DPI 下字体要按 scale 放大

### 5.3 窗口属性（winit 0.30）

```rust
let attrs = Window::default_attributes()
    .with_title("tupa-launcher")
    .with_decorations(false)
    .with_window_level(WindowLevel::AlwaysOnTop)
    .with_resizable(true);
// Windows 扩展：skip_taskbar / 居中 由 WindowAttributesExtWindows 或 SetWindowPos 处理
let window = event_loop.create_window(attrs)?;
```

（具体 trait/method 名以 winit 0.30 编译为准；不要把不确定的 API 写死。）

### 5.4 IME 事件

```rust
WindowEvent::Ime(Ime::Preedit(text, cursor)) => {
    gui.preedit = text;
    // 不 refresh，只重绘；更新候选窗位置
    window.set_ime_cursor_area(pos, size);
}
WindowEvent::Ime(Ime::Commit(text)) => {
    app.insert_str(&text);
    gui.preedit.clear();
}
```

### 5.5 隐藏 / 唤起

- `Esc` 在 `App::on_key` 里设置 `should_hide`；GUI 在帧尾处理
- 隐藏时 `app.reset()` + `window.set_visible(false)`；不要 drop `App`（worker 要保留）
- daemon 的 `panel_visible()` 用 `IsWindowVisible`，对 winit 窗口有效
- 如果窗口被 `ShowWindow(SW_HIDE)` 隐藏，winit 可能收不到事件；下次 `SW_SHOW` 后要 `request_redraw`

## 6. 坑（按优先级）

| 坑 | 说明 |
|---|---|
| winit 0.30 API | 用 `ApplicationHandler` + `run_app`，不是旧的闭包式 `EventLoop::run`；`softbuffer::Surface` 要在 `resumed` 里建 |
| `TestBackend` 性能 | 每帧分配 Buffer；先量 30fps，掉帧就写 `MemoryBackend` |
| 中文 fallback | Cascadia Mono 没有中文；cosmic-text 需要加载系统字体，中文会落到 YaHei，等宽可能不整齐 |
| 宽字符 | Buffer 每个 cell 一个 symbol，宽字符占两列；不要重复画第二列 |
| DPI | 窗口物理像素 / `scale_factor` 换算 cell 数；`ScaleFactorChanged` 要重建 metrics |
| Ctrl+Tab / Shift+Tab | winit 的 logical key 可能是 `Tab` + modifiers 或 `BackTab`，实测后补映射 |
| `find_panel` 误判 | pi 的 `π - tupa-launcher` 是 `WindowsTerminal.exe`；新 GUI 是 `tupa-launcher.exe`，按进程名 + 标题前缀，别再用 contains |
| Preedit 触发查询 | Preedit 期间不要 `refresh()`，否则每敲一个拼音都跑一次 provider |
| 隐藏即销毁 | `Esc` 只 `set_visible(false)`，别 drop 窗口/App；daemon 还要热键唤起 |
| GlazeWM 规则 | 旧规则按 WT 类名 + 标题；新窗口类名/进程不同，需要更新规则或让 GUI 自己定位 |
| 焦点 | 无边框窗口要确保能拿到键盘焦点；`SetForegroundWindow` 后 `window.focus_window()` |
| 最小化/零尺寸 | `inner_size()` 为 0 时跳过渲染/除零 |
| 文本输入 | `Key::Character` 在中文输入法下可能是 preedit，不要把 preedit 当普通字符插入 |

## 6.5 实现期踩的坑（都已修，别再踩）

| 现象 | 原因 / 修法 |
|---|---|
| 窗口下半部分是黑的 | `Terminal::resize()` 只改内部缓冲，**不会**改 backend。必须同时 `terminal.backend_mut().resize(cols, rows)` |
| 整块背景变成米色 | `cell.style().bg` 常常是 `Some(Color::Reset)`，不能当具体颜色用，要回落到主题背景 |
| 选中行高亮在汉字处断开 | 宽字符第二列 cell 的样式被 reset，背景要沿用左邻 |
| **文字上下错位、每行压到下一行** | `placement.top` 是「基线到字形顶部」的距离。逐字符排版时 baseline 由各自字体决定，必须**自己定一条统一基线**：`top = baseline - placement.top`；行高 = `max(ascent) + max(descent)` |
| 汉字被渲染成日文字体 | cosmic-text 的 Windows fallback 不保证选雅黑（实测落到 `MS Gothic`）。我们按**字符显示宽度**选字体：宽字符 → `Microsoft YaHei UI`，其余 → `FiraCode Nerd Font Mono` |
| 窗口内容"透"出来 | 别开 `with_undecorated_shadow(true)`：它用 DWM 玻璃实现阴影，客户区会透明，softbuffer 的像素压不住 |
| 窗口位置跑到右下角 | 创建窗口时 winit 还不知道 DPI，`with_position(PhysicalPosition)` 会被按 1:1 摆放。创建后要用真实屏幕重新 `set_outer_position` 居中 |
| 上屏延迟 / 偶发不刷新 | `surface.present()` 前要 `window.pre_present_notify()` |
| 截图截到 pi 的窗口 | pi 的终端标题是 `π - tupa-launcher`，按标题匹配会命中它。用 `screenshot` skill 的 `-Mode region`，或先 `-List` 看进程名 |
| 打开后窗口被挤成竖条 | GlazeWM 会平铺新窗口。调试时先关 GlazeWM，或给它加规则（规则还没写） |

**调试技巧**：`TUPA_GUI_SHOT=C:\path\out.bmp tupa-launcher --gui` 会渲一帧存成 BMP 后退出，
不依赖屏幕截图就能验证渲染管线（`read` 工具能直接看 BMP）。

## 7. 测试 / 验收

- `cargo test --release` 全过；TUI 的 `--query` / `--daemon` 不回归
- `cargo run -- --gui`：无边框、置顶、能输入/选择/执行/Esc 隐藏
- 中文输入：拼音 preedit、候选框位置、Commit 后插入
- 高 DPI（200%）：字体不糊、光标对齐
- `--daemon`：热键唤起/隐藏、只开一个实例、隐藏后进程常驻
- GlazeWM 下：新窗口不被误平铺/误浮动（更新规则后）

## 8. 开工前需要确认的决策

1. `--gui` 先作为可选，默认仍是 TUI？（建议是；稳定后再切默认）
2. 窗口是否无边框 + 置顶 + 跳过任务栏？（docs/05 选的是）
3. 字体：`Cascadia Mono` + `Microsoft YaHei UI` 是否接受？要不要 Sarasa Mono SC？
4. 圆角/阴影/淡入第一阶段做还是后置？（建议后置）
5. 窗口定位交给 GlazeWM 还是 GUI 自己 `SetWindowPos` 居中？（建议 GUI 自己居中，GlazeWM 规则只做 fallback）
6. 是否把 daemon 的热键合并进 GUI？（建议不合并，daemon 继续负责热键/Copilot）

## 9. 参考

- [`docs/05-native-window.md`](05-native-window.md)：方案对比、阶段划分、依赖
- [`docs/06-handover.md`](06-handover.md)：项目整体约定、坑、测试习惯
- winit 0.30 `ApplicationHandler` / `WindowEvent::Ime`
- cosmic-text `FontSystem` / `SwashCache`
- softbuffer `Context` / `Surface`
