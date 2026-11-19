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

**主入口是 `tupa-gui.exe`**（`src/gui/`，GPUI）。面板进程常驻（`Esc` 只是隐藏），
改完不重启就还是旧 exe：

```bash
cd ~/Projects/tupa-launcher
taskkill //IM tupa-gui.exe //F            # 单实例：不杀旧的，新起的只会把旧窗口叫到前面就退出
cargo build --release --features gui      # 默认 cargo build 不产出 tupa-gui.exe
powershell -NoProfile -Command "Start-Process .\target\release\tupa-gui.exe"
```

三个前提（踩过）：

- **热键只属于一个进程**：`tupa-launcher --daemon` 在跑会先占住 `alt+\``，GUI 就注册失败
  （托盘图标在、按热键没反应）。改 GUI 前先 `tasklist | findstr tupa-launcher` 确认没在跑。
- **不带 `--features gui` 时 `src/gui/` 一行都不编译** —— 只跑默认的 `build` / `clippy` /
  `test` 拿到的「全绿」不包含 GUI 代码，改了等于没改。动 `src/gui/` / `gui_main.rs` 一律带 feature。
- **进程莫名消失**（GUI 子系统下 panic 没有控制台）：先看 `%TEMP%\tupa-gui.log`，
  `gui_main` 装了 panic hook 写这里。

```bash
cargo test  --release --features gui     # 全绿（数字以输出为准，不写进文档）
cargo clippy --release --features gui    # 零警告；必须带 feature，否则 GUI 代码不参与
./target/release/tupa-launcher.exe --check  # 所有 `[[apps]]` 都能解析（默认构建就有这个 exe）
```

只改 lib / TUI 侧（`providers` / `exec` / `ui`）时 `cargo build --release` 就够；
`tupa-launcher.exe` 手动 `cargo run --release` 跑（守护进程 + TUI 已停用，见下面硬约定）。

**无人值守看面板：别模拟热键，直接 ShowWindow**（2026-11-18 踩过）：`keybd_event` 造的
`Alt+\`` 不会触发 `RegisterHotKey`（面板根本不出现，`SendKeys` 的字符还会漏进前台终端）。
正确做法：`EnumWindows` 按 PID 找那个**类名首字符是 `Z`、宽度 > 200** 的顶层窗口
（GPUI 的窗口类名跨进程只读回首字符），`ShowWindow(h, SW_SHOW)` + `SetForegroundWindow`
之后再用截图脚本：`pwsh -NoProfile -File "...\screenshot.ps1" -Mode window -Target tupa-gui`。

## 硬约定

- **不要在这个仓库跑 `cargo fmt`**（2026-11-18 实测）：代码是手写排版（不少行超过 100 列也不折），`cargo fmt` 会一口气重排 20+ 个文件，diff 直接淹没本次改动。要格式化只手动整自己碰过的那几行。
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
- **`@pi` / `@run` 已移除（2026-11-19）**：`@pi` 每次查询要开 175 个 jsonl、读前 80 行
  （实测 0.68s / 88MB），`@run` 的提权终端交给 GlazeWM 的 `alt+shift+enter`。
  pi 会话清理改为 `my-pi-plugins` 的 `/clean`：规则 + HTTP 契约在
  [`docs/09-clean-api.md`](docs/09-clean-api.md)，前端 `web/clean/index.html`。
  不要再把 pi 的读取逻辑搬回 provider 层。
- 命令关键字是 **`set`**（历史上叫 switch，已改名）；文档示例写默认前缀 `:`，
  用户本机 `config.toml` 改成了 `@`，别把两者搞混。
- **面板渲染只走 `ui::draw`**（TUI 与 GUI 共用），别在 GUI 里另写一套布局。
- **主入口是 `tupa-gui.exe`**（2026-11-17 晚起）：热键 `[gui] hotkey`（本机 `alt+\``）、
  开机自启 `Startup\tupa-gui.lnk`。守护进程 + TUI 已停用（旧的 `tupa-launcher-daemon.lnk`
  **已移出启动文件夹、删到回收站**，见下面那条坑），
  TUI 只能手动跑。改 GUI 之前先确认守护进程没在跑，否则它会占着热键让 GUI 注册失败。
- **停用启动项要把它移出启动文件夹，别改扩展名**（2026-11-18 坑）：Explorer 枚举
  `Startup\` 时对目录里**每个文件**都 ShellExecute，`.lnk.disabled` 不是快捷方式 → 走文件关联
  （本机 `.disabled` 只登记过 NanaZip → 每次登录弹「无法作为压缩包打开」）。
  旧守护进程那份已删到回收站；判断「是不是又被执行了」看
  `HKCU\...\RunNotification\StartupTNoti<文件名> = 4`。
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
     GUI 子系统下 panic 看不见 → `gui_main` 装了 panic hook 写 `%TEMP%\tupa-gui.log`，
     排查"进程莫名消失"先看这个文件。
  6. **托盘图标**是运行时用 GDI 画的（`gui::icon::draw`，背景色方块 + 草绿 ❯），不依赖
     `.ico` 资源、也没有 `build.rs`；`HICON` 是裸指针，跨线程只能传 `isize`。
  7. **亚克力是能用的，别再以为做不到**（2026-11-18 实测跑通，`[gui] acrylic = true`）：
     - 做法：`window_background = Blurred` → GPUI 调 `SetWindowCompositionAttribute`，
       但 **0.2.2 传的 gradient alpha = 0 会变黑玻璃**；`src/gui/acrylic.rs` 在窗口创建后
       再补一次同参数、**alpha = 1** 的调用（照上游 `if is_acrylic && color.3 == 0 { color.3 = 1 }`）。
       **不用 fork GPUI**，也不用 Win2D 的 `GaussianBlurEffect`。
     - **排查顺序（上次就是栽在这里，浪费了一整轮）**：先看**节能模式**
       （`GetSystemPowerStatus().SystemStatusFlag`，1 = 省电开着）和
       「设置 → 个性化 → 颜色 → 透明效果」。两者任一为真时 DWM 会把 Acrylic/Mica
       **降级成不透明纯色**（黑玻璃 / 纯灰块），此时所有材质 API 都「调用成功但没效果」。
     - 判据不要靠壁纸：用**棋盘格**铺底再截图（`copyfromscreen`，`PrintWindow` 抓不到
       DComp 合成结果）。探针在 `examples/acrylic_probe.rs`（`backdrop` / `host` /
       `sb2..sb4`），细节与结论见 `docs/06-handover.md` §6「窗口模糊」。
     - **窗口形状 / 圆角（2026-11-18 结案）**：`vendor/gpui` 把
       `events.rs::get_client_area_insets` 改成 0（客户区铺满窗口，内容能盖到窗口边缘），
       `[gui] rounded` 控制外框圆角（8px）。
       **残留**：圆角外侧会剩一点点方形亚克力 —— 这是 DWM 的机制
       （`SetWindowCompositionAttribute` 按整个窗口矩形铺，系统圆角只裁内容），
       不是参数问题。要彻底干净只能走应用层（呼出前截一帧 + 模糊 + `paint_image`）。
     - **排查这类问题先看有没有残留的 `DWMWCP_DONOTROUND`**：`src/gui/mod.rs` 原来在
       `make_overlay_window` 里调 `tune_dwm()` 设了它，DWM 就彻底不管这个窗口的圆角，
       而且之后所有 DWM 侧调整（corner preference / region / insets）都会被它盖掉 ——
       这次绕了几小时就是被这一行藏住。已删。
     - **别再试的**（全部实测无效或更差）：`SetWindowRgn`（带
       `WS_EX_NOREDIRECTIONBITMAP` 时 DWM 静默忽略；去掉那个 flag 标题栏会冒出来）、
       `DwmExtendFrameIntoClientArea(-1)`、`DWMWA_NCRENDERING_POLICY`、
       `DWMWA_BORDER_COLOR = COLOR_NONE`、`DWMWA_VISIBLE_FRAME_BORDER_THICKNESS = 0`、
       运行时子类化 `WM_NCCALCSIZE`（`WindowBorderOffset` 失配，窗口 resize 会越来越大，
       966 → 1551）。
     - 独立结论（仍然成立，别再试）：`CreateHostBackdropBrush` 在 Win32 上依赖
       `DWMWA_USE_HOSTBACKDROPBRUSH` 且自带模糊，可作为**后备方案**（要换 WinRT 合成树）；
       `CreateBackdropBrush` 只「透」不糊；`DWMWA_SYSTEMBACKDROP_TYPE` 在 DComp 窗口上
       只有纯色。Zed 上游 / ELYTerminal 的 Windows 实现就是 SWCA acrylic，没有别的魔法。
  8. **动画一律 vsync 驱动，别用定时器 notify**（2026-11-17）：定时器（8ms / 16ms）和屏幕
     刷新是两个独立节拍，互相挤 → 用户感受就是"卡卡的"。做法是在 `render` 末尾按需
     `window.on_next_frame(...)`，回调里 `animate()` + `cx.notify()`，链条自然停在动画结束那帧。
     动画帧率因此等于屏幕刷新率（本机 120Hz）。
  9. **marker（选框）的坐标基准**：`inner` 里只渲染 `first..last` 这几行，第一行落在 y=0，
     所以 `top = marker_y - first * row_h`。漏掉这个减项时「只看第一屏永远正确」，
     滚到第二屏才暴露成"选框一路往下漂"。
 10. **选中行不要挂 `hover` 底色**：行画在 marker 之后，hover 会把选框盖住。
     选中态只由 marker 负责，hover 只表示「鼠标在这行但没选中」。
 11. **chip 渐变的两个时间戳别混用**：`chip_settled_at` 会在 marker 落地时被重置，
     拿它算渐入进度会导致"渐到一半归零再渐"（看起来是闪）；渐入起点用 `chip_fade_start`
     （只在选中项变化时设），`chip_settled_at` 只负责「落地后从 85% 补满到 100%」。
 12. **控件状态列 GUI 要自己画**：TUI 的「开 / 关 / 73%」是 `ui.rs::draw` 按
     `Control` 追加的一列，GUI 没有对应实现 → `@set` 曾经只剩「标题 + 附注」两列。
     现在普通模式单独占 `metrics.state` 那一列（颜色见 `control_color`），detail 模式
     追加一行；行高和窗口高度都用 `detail_extra_rows()`，别两边各算一套。
 13. **`needs_scroll_raw()` 决定「要不要继续推帧」**：只看标题列时，附注列 / detail
     明细列超宽就没有动画帧，`ping_pong` 停在第一帧（表现为「只播一次」或干脆不动）。
     新增任何可滚列都要同步这里。
 14. **`?` 是 Shift+/，会走两条路**：`WM_KEYDOWN` → `handle_key`，以及 `WM_CHAR` →
     `replace_text_in_range`。只拦前者的话字符照样进输入框；中文输入法打的还是全角
     「？」，`keys::to_key` 得归一。现在靠 `Panel::text_blocked()`（help 期间 +
     切隐后 50ms）在 input handler 里吞掉那次提交，并补了「输入框为空时 ? 当帮助键」。
 15. **两种列表形态共用一套虚拟滚动**：默认两行卡片（`render_card_row`，标题行 +
     路径/分组行，行高 `CARD_HEIGHT + ROW_GAP`），`Ctrl+Tab` 切单行对齐表（`render_row`，
     `ROW_HEIGHT + ROW_GAP`）。
     **可视条数不写死**：`max_visible_rows()` 用 `max_height`（= `MAX_HEIGHT_RATIO` × 屏高）
     减去与 `desired_height()` 同一份 chrome（PADDING / BORDER / INPUT / LIST_TOP / STATUS）
     后除以 `row_height()` 反算，`CARD_VISIBLE_ROWS` / `MAX_VISIBLE_ROWS` 只当上限。
     改动这三处（`row_height()` / `max_visible_rows()` / `desired_height()`）时必须口径一致，
     否则就是「最后一行被窗口底边裁掉」或「选框越往下偏得越多」。
     **选框提前一行滚动**：`style::SCROLL_MARGIN_ROWS`（= 1）只当**触发阈值**，
     目标是「刚好留出 margin」再 `clamp` 到 `scroll_max`。把 margin 直接加进
     `bottom - viewport` 又不 clamp，触底就会多滚出一行空白（旧版就是这么搞的）。
     卡片右列（状态 / 次数 / 时间）宽度按内容量，**不能写死**——「云上极速 · 13 小时前」
     写死 72px 就只剩「云上极速」。**卡片本身不铺底色**（2026-11-18）：要的是两行结构，
     不是一张张色块；底色只给 hover 和 marker。
 16. **虚拟滚动的 `inner` 偏移必须减 `first * row_h`**（2026-11-18 踩过）：
     `list_area` 只渲染 `first..last` 这几行，第 `first` 行落在 inner 的 y=0，而它在
     内容坐标系里是 `first * row_h`。写成 `-scroll_y` 时偏差 = `first * row_h`：
     浅滚看不出来，滚到底部**整片内容被推到容器外 → 列表一片空白**（`30/30` 复现）。
     marker 同理要减 `first * row_h`（它已经减了，别改成只写 `marker_y`）。
 17. **滚轮一格只走一行**（2026-11-18）：Windows 默认一格滚 3 行，启动器里一跳三条
     太粗；`on_scroll` 只取 ±1 步，不足一行的部分累积（触控板细滚也能用）。
     滚动判据用**目标位置 `scroll_target`**，不能用动画中的 `scroll_y`——
     后者滞后，向上滚时会漏判、选框先跑出视口。
- **输入框的文本入口是 `EntityInputHandler`，不是 `on_key_down`**：GPUI 在 `WM_CHAR`
  时调 `replace_text_in_range`、输入法组字时调 `replace_and_mark_text_in_range`，
  `bounds_for_range` 决定候选窗位置（Windows 端 GPUI 会 `ImmSetCandidateWindow`）。
  所以 `handle_key` 要**跳过无修饰键的字符键**，否则和 handler 重复插入；
  例外是选择模式的 `1`–`9`（走私有的 `App::on_key`）。
  注册点在 `gui::input_row` 里那个 **1×1 绝对定位 `canvas`**：`window.handle_input` 只能在
  paint 阶段调，而 0×0 的元素不会 paint（踩过，handler 就永远注册不上）。
 18. **图标有三层来源，`RowIcon` 枚举（2026-11-18）**：位图（应用 / favicon）> Nerd Font
     字形（文件 / 目录 / 网页）> chip（`cfg` / `run` / 老 `hist`）。行数据里已经有
     `Action`，所以 `RowSnapshot.icon` 在 `snapshot()` 里算就行，TUI 一行不用改。
     五个必须记住的点（细节见 `docs/06-handover.md` §6「图标」）：
     - 取图用 `IShellItemImageFactory::GetImage`（可指定尺寸），不用 `SHGetFileInfoW`（只有 32px）
     - `GetDIBits` 里 `biHeight` 给**负数**（自顶向下），32bpp 出来就是 GPUI 要的 BGRA；
       **别再 swap 一次红蓝**（favicon 解码同理）
     - 老式图标 alpha 全 0 必须当不透明处理（不然整块透明，像图标丢失）
     - 只给「可视区附近」的行提取（`Panel::prefetch_icons`），且**失败也缓存**；
       别在 `snapshot()` 里遍历全部条目
     - 图标顶替 chip 但**列宽不变**（仍是 `BADGE_WIDTH`）；卡片第二行的 mini chip 用与
       第一行镜像的 flex 结构（number / star / `BADGE_WIDTH` + 同一个 gap），副标题因此自动对齐
     - 图标源有三层兜底（`appicon::exe_for`）：直接 `.exe` → `.cmd` / **无扩展名 shim**
       向上找同名 `.exe`（不做这步 VSCode 会是「白纸 + 齿轮」）→ 开始菜单 `.lnk` 索引
     - **`\WindowsApps\` 下的 exe 别直接取图**（2026-11-18）：MSIX 包目录受保护，
       SHELL 只给通用「应用」图标（白纸 + 蓝块，`wt.exe` / `pwsh.exe` 都中招）。
       先跟随 symlink 到包内真 exe，再拼 `shell:AppsFolder\<PackageFamilyName>!App`；
       `AppData\Local\Microsoft\WindowsApps\pwsh.exe` 那种别名目录已是 FamilyName，
       直接拼 `!App`。UWP 的 `ms-settings:` / `ms-windows-store:` → AUMID 同理
     - **Scoop shim 的图标是假的**：`<scoop>\shims\x.exe` 是通用代理程序，要回
       `<scoop>\apps\x\current\`（或版本目录）找真 exe，否则 `windhawk` 这类没图标
     - **Steam 游戏只有 `.url` 没有 `.lnk`**：开始菜单索引要收 `.url`
       （Internet Shortcut，`IconFile=` 指向 Steam 缓存的游戏 ico），且对 `.url`
       不做 `resolve_shortcut`，直接拿它自己取图（`Big Walk` 靠这条才有图）
   19. **文件图标不用图片，用 Nerd Font 字形**（2026-11-18）：字体 `JetBrains Maple Mono`
     自带完整 Nerd Font 图标集（18MB 的原因），`src/gui/glyphs.rs` 里直接给 PUA 码点。
     **码点抄 eza**（`src/output/icons.rs`，按语言给专属图标），别自己拍脑袋；
     换字体要重量（字里没有的字形显示成豆腐块 —— PowerShell + GDI+ 把码点画成 PNG
     看一眼即可，再加单测兜住）。已知坑：`F60F`（fa-markdown）在这个字体里是斜线方块，
     用 `F48A`。
   20. **favicon 只能异步、用 WinHTTP、必须缓存失败**（2026-11-18）：
     `src/gui/favicon.rs` 在 `prefetch_icons` 里 spawn 线程跑（WinHTTP 阻塞，
     放渲染线程会把面板卡住），结果走 mpsc 回轮询里 drain。取图顺序
     `/favicon.ico` → 首页 `<link rel="icon">`（实测 `pwn.college` 前者是 404，
     少了第二步一个图标都拿不到）。缓存存原始字节、**失败写空文件**否则每次呼出都重试；
     站点 key 要带**非默认端口**（`127.0.0.1:5173` 与 `:4000` 是两个服务）。
     开关是 `[gui] web_icons = "favicon" | "globe"`（`globe` 完全无网络请求）。
- **输入框的 provider chip 是原子的**（2026-11-19）：关键字一命中 provider（`@z`，不必敲空格），
  `input` 的 `0..chip_end` 段（`App::chip_end`，**字符**下标）在 GUI 里画成
  「字形 + 关键字」的胶囊，编辑上不许光标进入。三个坑：
  - **chip 的判据就是 `Parsed::Command`，不要再加空格条件** —— `providers::parse` 本来就
    不要求空格，列表在 `@z` 就已经切到目录跳转了；chip 要是等空格，就会出现「列表已经
    换了、输入框还是纯文字」的错位（2026-11-19 用户反馈，别改回去）。
  - **也试过反过来：改 `parse` 让列表也等空格 —— 否决**（2026-11-19）。那会让 `@z` + 回车
    变成「补成 `@z `、要按两次回车才列目录」，还把 `--query "@z"` 从列目录改成列命令。
    代价不对等：错位靠 chip 提前就能解决，不用动解析层。
  - **原子语义在 `app` 层，不在 GUI**：`snap_cursor` 在 `refresh_with` 末尾统一夹一次
    （覆盖输入 / 粘贴 / IME / 各条编辑路径），`backspace` 贴着 chip 一次删掉整块含分隔
    空格，`word_boundary` 反向不许停在 `@z` 中间。TUI 没画 chip，但**这些行为同样生效**。
  - **新增 provider 要补 `gui/glyphs.rs::for_keyword`**，漏了只是回落到问号（没有编译错误）。
    码点用已有常量；新增码点先用 GDI+ 画一张 PNG 确认不是豆腐块。
- **`[[apps]] icon = "..."` 给的图要用 `image` 解，别过 SHELL**（2026-11-18）：
  `SIIGBF_ICONONLY` 对 png / jpg 返回的是「图片文件」通用图标（白纸），不是图本身。
  用 `appicon::load_image`（`image::open` + RGBA→BGRA），给自动解析推不出图的条目用
  （本机 pi 跑在 `wt.exe` 里，图标是从用户给的 SVG 画出来的三色方块）。
  排查用 `tupa-launcher.exe --icons`，它会列出每条实际走的哪条路。
- **亚克力可用的判据别用电源计划**（2026-11-18 实测）：本机 `ActivePowerScheme`
  恒为节能计划 `a1841308-3541-4fab-bc81-f71556f20b4a`，跟 `:set` 里那个「节能模式」
  开关**不是一回事** —— 拿它判断会永远判成不可用，关节能后亚克力也回不来。
  正确判据三条：`EnableTransparency != 0`、`SYSTEM_POWER_STATUS::SystemStatusFlag != 1`
  （节电）、`HKLM\SYSTEM\CurrentControlSet\Control\Power\EnergySaverState != 1`
  （和 `energy_saver_get_combined.ps1` 同源；实测关着时是 `2`）。
  不可用时画不透明主题色（`#272e33`），并且**要重新下发 accent policy**
  （`acrylic::disable` / `enable`），只改底色 DWM 那边还挂着旧的、切回来仍是黑底。
- **行的鼠标交互只在两个地方写**（2026-11-19）：`row_click_handler`（左键：单击选中 /
  双击 = 回车）与 `row_right_click_handler`（右键：选中该行 + 弹菜单）。卡片行（`render_card_row`）
  和密度行（`render_row`）**共用这一对**，别在两边各写一份。
  - 右键也会走 `on_click`，所以左键处理器里必须判 `mouse.down.button == MouseButton::Left`，
    否则右键双击会直接执行条目。
  - 菜单项**每次现取**（`App::menu_for(row)`），不缓存：收藏状态、「历史里还有没有这条」
    都是实时的。做不到的项就不给（能不能定位问 `Action::reveal_target()`）。
  - 「从最近记录移除」只删 `history.json`（`App::run_menu` 里直接改内存 + `save_history`）；
    `:web` 的数据在 Edge 的 History 库里，**删不掉**，文案要写「最近记录」。
  - 菜单是 root 的最后一个 child + `absolute()`，坐标超出窗口就往里夹；`Esc` 只关菜单。
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
