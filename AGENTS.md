# tupa-launcher 开工规矩

> pi 在本目录（或子目录）启动时自动加载本节。状态（阶段 / 未完成 / 入口）**只看
> [`STATE.md`](STATE.md)**，不要在这里或 README 里重复。

## 开工第一步

**先读 [`STATE.md`](STATE.md)**（不超过一屏）。它首行的 `code` 是**最后一次改动代码的提交**，
用它判过期：

```bash
git log -1 --format=%h -- src Cargo.toml   # 不等于 STATE 里的 code → 代码已变，先核对「阶段」与「未完成」
```

只改文档的提交不算过期。若确实已经脱节，先按代码实际状态改 `STATE.md`，再动手。

## 先读哪里

| 想知道 | 看 |
|---|---|
| 现在做到哪、还有什么没做完 | `STATE.md` |
| 加 provider / 加 Action / 加文案 / 测试怎么写 | [`docs/06-handover.md`](docs/06-handover.md) §3 |
| **已经踩过的坑（20+ 条）** | `docs/06-handover.md` §4 —— 动手前扫一眼，能省一小时 |
| 套壳（GUI）/ 开关引擎 / 原生窗口的设计与步骤 | `docs/07-shell-handover.md` / `docs/04-switch-engine.md` / `docs/05-native-window.md` |
| 用户怎么用、配置项、命令语法 | `README.md` |

## 改完怎么生效 + 怎么验证（硬性，别忘）

面板进程常驻（`Esc` 只是隐藏），改完不重启就还是旧 exe：

```bash
cd ~/Projects/tupa-launcher
taskkill //IM tupa-launcher.exe //F
cargo test --release && cargo clippy --release && cargo build --release
powershell -NoProfile -Command "Start-Process (Join-Path \$env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup\tupa-launcher-daemon.lnk')"
```

```bash
cargo test --release      # 全绿（数字以输出为准，不写进文档）
cargo clippy --release    # 零警告
tupa-launcher.exe --check # 所有 `[[apps]]` 都能解析
```

## 硬约定

- **所有用户可见文案走 `src/i18n.rs`**（`t("中文", "English")`），业务代码里不写裸字符串。
- **加 `Action` 变体要同步 3 处**（2026-11-17 从 4 处收拢：定位知识移进
  `Action::reveal_target`，`app::reveal_selected` 不再枚举）：
  `providers/mod.rs` 的 `Action` + `Action::reveal_target`（无可定位路径的走 `_` 分支给
  `i18n::cannot_reveal()` 提示）+ `Item::detail_line`（后两者与 Action 同文件），
  再加 `exec.rs::run` 的执行分支（业务实现放各自的模块，别都堆进 exec）。
- **加 `Kind` 变体要同步 3 处**（编译不会报，行为会错）：`providers/mod.rs` 的 `badge()`
  （徽标）、`columns()`（漏了掉回默认两列，开屏混排里排版会歪）、`from_badge()`
  （漏了 `fav:<徽标>` 解析不到）。
- **加 provider**：`src/providers/xxx.rs` + `providers/mod.rs::build()` 里 push（顺序 = 命令列表顺序）。
- 命令关键字是 **`set`**（历史上叫 switch，已改名）；文档示例写默认前缀 `:`，
  用户本机 `config.toml` 改成了 `@`，别把两者搞混。
- **面板渲染只走 `ui::draw`**（TUI 与 GUI 共用），别在 GUI 里另写一套布局。
- **主入口是 `tupa-gui.exe`**（2026-11-17 晚起）：热键 `[gui] hotkey`（本机 `alt+\``）、
  开机自启 `启动	upa-gui.lnk`。守护进程 + TUI 已停用（`tupa-launcher-daemon.lnk.disabled`），
  TUI 只能手动跑。改 GUI 之前先确认守护进程没在跑，否则它会占着热键让 GUI 注册失败。
- **两个可执行文件，别混**（2026-11-17 起）：`src/lib.rs` 放全部模块（`pub mod`），
  `src/main.rs` → `tupa-launcher.exe`（TUI + 守护进程，不链接 gpui），
  `src/gui_main.rs` → `tupa-gui.exe`（GPUI 面板 + 托盘 + 热键，`required-features = ["gui"]`，
  默认 `cargo build` 不构建它）。`tupa-launcher --gui` 已删，只报一句提示。
  两个 exe 通过 `util::find_panel(PanelKind::Gui)`（进程名 `tupa-gui.exe`）协作。
- **GUI 换成 GPUI（2026-11-17 起）**：旧的 winit + cosmic-text + softbuffer 实现已整体删除
  （把 ratatui 字符网格逐像素贴窗口，拿不到真圆角/动效）。新 `src/gui/` 是可选 feature
  `gui`，默认构建不含。四条实测结论，改这块之前先记住：
  1. 窗口用 `WindowKind::Normal` + `titlebar.appears_transparent`（保住 `WS_THICKFRAME`
     才可拖边缩放），创建后再把 `WS_EX_APPWINDOW` 换成 `WS_EX_TOOLWINDOW`（不进任务栏）；
     `PopUp` 天然 TOOLWINDOW 但 `dwstyle = 0`，**不可缩放**。
  2. GPUI 窗口类名是全框架共用的 `Zed::Window`，跨进程 `GetWindowTextW` 读标题只回一个字符
     —— `util::find_panel` 的 GUI 分支只能按**进程名 + 有尺寸的顶层窗口**认人。
  3. `truncate()` **只对「有确定宽度」的元素生效**（`w(px(n))` / `max_w(px(n))`）；
     `flex_1` 的容器宽度由内容撑出来，截不了，文字会照画到父元素外（实测 A/B/C 三行对照）。
     列表列宽要给显式像素宽度再 `truncate()`（现在按 `window.bounds()` 宽度算）。
  4. GPUI 没有 channel 唤醒，后台结果靠 `cx.spawn` 轮询 `App::drain_events()`
     （查询中 16ms / 空闲 120ms），拿到结果再 `cx.notify()`。
  5. **不要在 `entity.update` 闭包里再 `update` 同一个 entity，也不要调 `Window::resize`**
     （2026-11-17 踩过两次，两次都是进程直接静默退出）：
     - 嵌套 `this.update(cx, ..)` → `cannot update Panel while it is already being updated`
     - `Window::resize` 会同步触发 relayout/render，同样重入
     改窗口尺寸用 `util::resize_panel_height`（Win32 `SetWindowPos`），在 `update` 之外调用。
     GUI 子系统下 panic 看不见 → `gui_main` 装了 panic hook 写 `%TEMP%	upa-gui.log`，
     排查"进程莫名消失"先看这个文件。
  6. **托盘图标**是运行时用 GDI 画的（`gui::icon::draw`，背景色方块 + 草绿 ❯），不依赖
     `.ico` 资源、也没有 `build.rs`；`HICON` 是裸指针，跨线程只能传 `isize`。
  7. **半透明**：`[gui] opacity` < 1.0 时窗口切 `WindowBackgroundAppearance::Transparent`。
     **没有模糊**，也别去接 Windhawk 的 translucent-windows —— 它靠 `DwmEnableBlurBehindWindow` /
     `SetWindowCompositionAttribute`，而 GPUI 的 `WS_EX_NOREDIRECTIONBITMAP` + DirectComposition
     窗口没有 DWM 背景可合成，实测无效。
- **输入框的文本入口是 `EntityInputHandler`，不是 `on_key_down`**：GPUI 在 `WM_CHAR`
  时调 `replace_text_in_range`、输入法组字时调 `replace_and_mark_text_in_range`，
  `bounds_for_range` 决定候选窗位置（Windows 端 GPUI 会 `ImmSetCandidateWindow`）。
  所以 `handle_key` 要**跳过无修饰键的字符键**，否则和 handler 重复插入；
  例外是选择模式的 `1`–`9`（走私有的 `App::on_key`）。
  注册点在 `gui::input_row` 里那个 **1×1 绝对定位 `canvas`**：`window.handle_input` 只能在
  paint 阶段调，而 0×0 的元素不会 paint（踩过，handler 就永远注册不上）。
- **托盘/热键在后台线程，别塞进 GPUI**：`Application::run` 占住主线程，所以 `src/tray.rs`
  自己开线程跑 message-only 窗口 + `Shell_NotifyIconW` + `RegisterHotKey`，面板显隐直接用
  `ShowWindow` 操作 HWND（`util::find_panel`），不碰 GPUI 状态。托盘菜单用
  `TPM_RETURNCMD` 直接拿返回值，`quit()` 先 `NIM_DELETE` 再 `process::exit`。
  `WNDCLASSW` / `RegisterClassW` 需要 windows-sys 的 `Win32_Graphics_Gdi` feature。
- **`config.example.toml` 保持精简示例（2026-11-03 决定）**：只教语法，不同步本机 55 项应用。
- **不要用「方向键 + `Space`」操作 Windows 系统 UI**（快速设置 / 通知中心）：`Space` 切的
  是**当前焦点项**，而系统 UI 的按钮布局会因用户增删固定项而变——必然存在「焦点落错行
  → 切错开关」的窗口（实战里误关过蓝牙 / 误开过飞行模式）。要操作这类面板，一律
  **坐标点击 + 读回状态校验**；点击前用 `GetCursorPos` 记光标、`finally` 里复位。
- **别一上来就想用 WindHawk 做实时事件**（2026-11-08 评估）：资源管理器导航这类需求，
  纯 Win32 的 `SetWinEventHook` + `ShellWindows` 已经够（见 `src/explorer_watch.rs`）。
  WindHawk mod 更精确（进程内 hook、零轮询、不漏事件），但要写 C++、要跨进程把数据
  传给 tupa、依赖 WindHawk 常驻，而且 Windows 更新改了符号就静默失效 —— 收益不抵维护成本。
- **WezTerm 面板的前端：默认（wgpu）可用，别再无条件加 `front_end='OpenGL'`**（2026-11-12 修正）：
  上午的「wgpu 在 `detached_command` 下每次都 panic」只对了一半。真因是 **GlazeWM 的
  `set_transparency` 给每个窗口无条件挂 `WS_EX_LAYERED`** —— wgpu 的 DX12 swapchain 在 layered
  窗口上直接 `Validation Error`。已在 OhMyGlazeWM `ee149ae7` 修掉（不透明窗口不再动 layered），
  `wezterm.lua` 的默认 `WebGpu` 冷启动 3/3 存活。以后再闪退，先 `glazewm query windows` 看该窗口
  是不是又被挂了 layered，别急着切 OpenGL。验证方式不变：按热键冷启动 + 强杀 wezterm 再唤起。
- **ElyTerminal 导致面板闪退 —— 已否决**（2026-11-12 查证）：装上时间与故障对得上，但默认终端
  GUID 仍是 Windows Terminal 的，注册表里没有任何 Ely 项，wgpu 崩也跟终端 delegate 无机制关联。
  真因是 GlazeWM 的 layered（见上一条），别再从终端软件方向找。
- **脚本超时是分层的，别调大全局 `TIMEOUT`**（2026-11-17）：普通条目（`get` / 开关）5 秒，
  `kind = "action"` 默认 60 秒，`actions.toml` 可用 `timeout = <秒>` 覆盖。`get` 在每次打开
  `:set` 时都要跑一遍，全局放宽会拖死面板；「下载 / 重启应用」这类动作 5 秒必然误报
  （Clash 订阅更新实测 5.2s）。超时必须杀进程树（`serve::kill_pid` → `taskkill /T /F`），
  只 `TerminateProcess` 会让 powershell 的子进程继续把 yaml 写到一半。
- **单测不要落盘到真实配置目录**（2026-11-17）：`~/.config/tupa-launcher/{history,favorites}.json`
  是用户真数据，跑一次 `cargo test` 就能被覆盖。要写盘的操作抽成可关闭的 helper
  （`App::save_history` 用 `#[cfg(not(test))]`），别在测试里直接调落盘路径。

## 收尾清单

1. `cargo test --release` + `cargo clippy --release` 全绿（数字以输出为准，**不要写死进文档**）。
2. 更新 `STATE.md` 的两节 + 首行 `updated` / `code`。
3. 若代码与文档不一致（命令改名、新增 provider、新增内置开关）→ 改**文档**；
   若只是"做到哪了" → 改 **STATE.md**。两者别混。
4. `git commit`（信息里写清"改了什么 + 为什么"）；需要时提醒用户 push。
