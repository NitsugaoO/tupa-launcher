# 开发手册（代码结构 / 扩展点 / 坑表）

> 本文是**稳定知识**：代码结构、加功能的路径、踩过的坑、调试入口。
> **当前状态（阶段 / 下一步 / 待验证）不在这里** —— 见 [`../STATE.md`](../STATE.md)；
> 开工前的硬性规矩（改完怎么生效、收尾清单）见 [`../AGENTS.md`](../AGENTS.md)。
>
> 设计文档：[`05-native-window.md`](05-native-window.md)（套壳）、
> [`04-switch-engine.md`](04-switch-engine.md)（开关引擎）、
> [`07-shell-handover.md`](07-shell-handover.md)（套壳的具体步骤与待确认决策）。

## 1. 状态与“改完怎么生效”（已搬走）

- 现在做到哪、下一步、待验证命令 → [`../STATE.md`](../STATE.md)
- 改完代码怎么让它生效（重启守护进程的三条命令）→ [`../AGENTS.md`](../AGENTS.md)

本文不再维护这两项，避免和 STATE.md / AGENTS.md 漂移。

## 2. 代码结构

```
src/
├── main.rs        入口 / 事件循环 / --query --check --resolve --reveal --apply --daemon --gui
├── app.rs         状态机：解析、键位、补全、选择、历史、收藏、与 worker 通信
├── ui.rs          纯渲染：分列布局、详细信息、滚动、状态栏
├── exec.rs        执行动作（打开/启动/定位/提权/新窗口/新建文章/
│                  用指定程序打开/开关写入）
├── actions.rs     actions.toml 解析 + 脚本执行（超时）+ 内置开关
├── config.rs      config.toml
├── i18n.rs        中英文案（所有用户可见字符串必须走这里）
├── favorites.rs   收藏读写
├── history.rs     输入历史
├── theme.rs       颜色
├── util.rs        .lnk 解析 / PATH / 编码 / 面板窗口
├── daemon.rs      热键 + 面板显隐（Windows）
├── gui/           GUI 实验：mod.rs（winit 窗口 / 事件循环 / 渲染）、text.rs（cosmic-text 排版）
└── providers/
    ├── mod.rs     Provider trait / Item / Action / Column / 解析 / worker
    ├── apps.rs    app
    ├── everything.rs  ev
    ├── run.rs     run
    ├── go.rs      go（zoxide + Everything + 剪贴板 + yazi/pi/code）
    ├── pi.rs      pi（pi 会话列表 + 关键词搜索 + -session 打开）
    ├── web.rs     web（Edge 历史，rusqlite）
    ├── blog.rs      blog（新建文章 / 管理页 / 站点入口）
    ├── favorites.rs   fav
    ├── config_items.rs cfg
    ├── clean.rs     :pi:clean 的实现（时间窗口 + 归档 / 回收站 / 恢复）
    ├── switch.rs     set（actions.toml 的开关/选项/数值；命令名 2026-10 从 switch 改成 set）
    ├── home.rs       home（开屏：收藏 + 最近命令；无关键字、不进命令列表）
    └── commands.rs    命令列表（keyword 是 `:`，不参与匹配）
```

### 数据流（记这张图就够）

```
键盘 → App::on_key
        ├─ 编辑输入 → App::refresh → providers::parse → Cmd::Query → worker 线程
        │                                              └─ Evt::Results → App::drain_events
        ├─ 补全 → App::compute_completion（主线程，纯内存）
        ├─ 控件 ←/→ → 只改 App::items 里的本地值（不写）
        ├─ 控件 Enter → Cmd::Apply → worker（exec::run → actions::apply）
        │                             └─ Evt::Applied → App 重查 + 保留选中行
        └─ 普通 Enter → exec::run(Action)
渲染 → ui::draw(App)
```

- **provider 的 `query()` 在 worker 线程跑**，可以阻塞（SQLite / es.exe / 文件 IO / PowerShell）
- worker 只执行「排队中最后一条」**查询**，快速打字不会堆积子进程；
  `Cmd::Apply` 例外，一条都不会丢
- `App::tick` 每帧 +1，选中行的滚动偏移由它推导

### 关键类型

```rust
Item { kind, columns: Vec<String>, action, control: Control }
                                                // control = 开关/选项/数值
Action::{Open, OpenUrl, Launch, LaunchInTerminal, RunCommand, Reveal, OpenInEditor, CreateBlogPost, BlogAdmin, FillInput, SetControl}
Column { weight, min, max }                      // 每列的份数/最短/最长
Control::{Run, Toggle{value}, Choice{options,index}, Number{value,min,max,step,suffix}, Unavailable}
```

- 详情模式的「完整目标」行由 `Item::detail_line()` 从 `Action` 推导（已在列里的不重复）
- 状态列由 `ui` 根据 `Control::label()` 追加，**provider 的 columns 不用管它**
- 收藏去重键是 `kind + title + action`，**不要**把会变的列（如访问次数）算进去；
  `control` 不参与序列化比较（旧收藏文件读出默认 `Run`）

## 3. 加东西的时候

### 加一个 provider（约 30 行）

1. `src/providers/xxx.rs`，实现 `Provider`（`keyword` / `description` / `query`，按需 `columns()`）
2. 描述用 `crate::i18n::desc_xxx()`，文案加在 `i18n.rs`
3. `providers/mod.rs::build()` 里 push（顺序 = 命令列表顺序）
4. 需要新参数：把词加进 `KNOWN_FLAGS`，在 `query()` 里读 `flags`

### 加一个动作（**4 处要同步，漏了会编译不过/行为不对**）

1. `providers/mod.rs` 的 `Action` 加变体
2. `exec.rs::run` 加分支
3. `app.rs::reveal_selected` 加分支（不支持的写 `=> return`）
4. `providers/mod.rs::Item::detail_line` 加分支（详情模式显示什么）

### 加一句用户可见文案

**必须**加到 `src/i18n.rs`（`t("中文", "English")` 或 `s(|| …, || …)`），不要在业务代码里写裸字符串。
`--help`、状态栏、空提示、错误消息、`:cfg` 的说明都覆盖了。

### 测试习惯

- 纯逻辑（解析、列宽、补全、格式化）→ 单测，放在同文件 `#[cfg(test)] mod tests`
- 布局和渲染 → `ui::render_tests` 用 `TestBackend` 把屏幕取成 `Vec<String>` 再断言
  （注意宽字符会占两格，测试数据尽量用 ASCII）
- `App::new` 在测试里**不读**真实的 `favorites.json` / `history.json`（`cfg!(test)` 分支）

## 4. 已经踩过的坑（别重踩）

| 坑 | 现象 / 正确做法 |
|---|---|
| `explorer /select` 引号 | 必须 `raw_arg("/select,\"路径\"")`，引号只包路径 |
| `es.exe` 输出编码 | 用 `util::decode_output`（UTF-8 优先，回退 GBK） |
| daemon `FreeConsole` 后 spawn | 用 `util::detached_command`（`Stdio::null()`），否则 `Ok` 但不建窗口 |
| 低级键盘钩子 | 300ms 超时，已弃用；热键走 `RegisterHotKey`，**永远不要吞修饰键抬起** |
| WT 窗口识别 | 标题可能带后缀，用「类名 + 标题前缀」；**不能用 contains**（pi 的 `π - tupa-launcher` 会被误判） |
| Edge 历史被独占 | 必须先复制到临时目录再用 SQLite 打开 |
| `last_visit_time` 单位 | 是 **Chromium 微秒**（1601 起），`/1_000_000 - 11_644_473_600`；网上的 100ns 是错的 |
| `Ctrl+Tab` | 被 Windows Terminal 默认占用，面板收不到；备用键 `Ctrl+O` |
| 状态栏提示宽度 | 英文比中文宽，必须按 `UnicodeWidthStr::width` 算，写死会截断 |
| PowerShell 路径 | 新窗口里切目录用 `Set-Location -LiteralPath '…'`，cmd 用 `cd /d "…" && …` |
| 中文字形/光标 | 光标列要按显示宽度算（`ui::cursor_column`），输入超宽时用 `trim_left` 跟随 |
| `cmd /C` 里的引号 / 管道 | 内置 PowerShell 用 `-EncodedCommand`（UTF-16LE base64）绕开转义；用户脚本自己小心 |
| `root\wmi` 实例权限 | 普通用户 `Get-CimInstance` → `0x80041003`；管理员下 ThinkBook 的 `LENOVO_GAMEZONE_DATA` 方法又报「无效的对象」（不是 Gaming 系列） |
| 电池养护没有 WMI/BIOS 入口 | `Lenovo_BiosSetting` 无相关项，`GetBiosSelections` 返回空；走 Vantage 的 `IdeaBatteryAgent.dll`（反射调用，免提权） |
| Vantage Addin 反射 | 程序集在 `%ProgramData%\Lenovo\Vantage\Addins\IdeaNotebookAddin\<版本>\`，要先加载契约/工具 DLL；`New-Object` 得到的 PSObject 不能直接传给反射 `Invoke`，用 `[Activator]::CreateInstance` |
| 24H2 Energy Saver | `EnergySaverState` 写入要重启才生效；`PowerManager.EnergySaverStatus` 在 24H2 手动开启时也返回 `Off`（API 不可靠） |
| Energy Saver 的 WNF | `WNF_PO_ENERGY_SAVER_STATE` 可读，但写：普通/管理员 `ACCESS_DENIED`；SYSTEM 写入成功但读回空（Session 0 与用户会话隔离） |
| 快速设置的 UIAutomation | 它是 WinUI3，UIA2（PowerShell 5.1）`FromHandle` 后子元素为 0；pwsh 7 的 UIA3 COM `FindAll` 会挂死。最终用 `Win+A` + `Home/→→↓/Space` |
| 快速设置方向键 | `Home` 回第一个按钮，`→→↓` 到第二行第三个（节能）；比 Tab 顺序稳定，但依赖按钮布局 |
| 快速设置点击回退 | 坐标相对屏幕右下角的逻辑偏移（当前 -89,-311，200% DPI），仅在方向键失败时用 |
| `cmd /C` 命令行长度 | 长脚本的 `-EncodedCommand` base64 会超 8191 字符；内置脚本改为直接 spawn `powershell.exe` |
| 节能电源计划被隐藏 | `powercfg /list` 不显示，但 `a1841308-3541-4fab-bc81-f71556f20b4a` 仍可用 `/setactive` 直接切换（普通用户，即时） |
| `Evt::Applied` 丢了 | 等待期间用户又输入会让 `generation` 过期；**无论是否过期都要复位 `applying`**，否则以后按回车再也发不出写请求 |
| 空格不能当控件切换键 | 空格是搜索词/参数分隔符（`:set 电源`），拿去切换会在打过滤词时误触发写操作 |
| `TestBackend` 宽字符 | 中文在 buffer 里占两格、字符串断言会掺空格；测试里只断言 ASCII 部分或用 `column_of` |
| `pnpm` 是 `.cmd` | 不能 `Command::new("pnpm")`；走 `pwsh -Command "pnpm admin"` + `.current_dir()`（和 `RunCommand` 共用 `shell_args`） |
| 管理页白屏 | `pnpm admin` 要几百毫秒才监听 5173，立刻开浏览器会白屏；延迟 1.5s 在**后台线程**打开，不卡面板 |

## 5. 开关引擎（v0.7 已完成）

设计见 [`04-switch-engine.md`](04-switch-engine.md)。机制一句话：`Item.control` +
`Action::SetControl` + `Cmd::Apply` / `Evt::Applied`；`actions.toml` 驱动（toggle / choice /
number，5 秒超时）；内置项免提权（深色模式 / 亮度 / 电池养护 / 节能 / 保持唤醒 / 更新订阅）。

**没做**：计划任务提权。本来打算用它做电池养护，后来发现直接调 Vantage 的
`IdeaBatteryAgent.dll` 就能读写（不需要管理员），所以没往系统里写任务。
真正需要管理员的功能（BIOS 写入、GameZone Win 键锁）在这台 ThinkBook 上 WMI 不可用，
要做得先解决驱动/提权。

## 6. 套壳（GUI）：进行中

设计取舍见 [`05-native-window.md`](05-native-window.md)，具体步骤 / 待确认决策见
[`07-shell-handover.md`](07-shell-handover.md)，**当前进度见 [`../STATE.md`](../STATE.md)**。

现状：`src/gui/` 已能显示与输入（`--gui` 或 `[daemon] panel = "gui"`），默认仍是
`wt -p tupa-launcher` 拉起的 Windows Terminal 窗口。

选的是 **winit + cosmic-text + softbuffer**（保留 ratatui 的 layout/render 代码）。

关键复用点：

- `ui::draw(&mut App)` 只依赖 `ratatui::Frame`，换成自己把 `Buffer` 画成像素即可
- 输入层只要把 `winit::event::KeyEvent` 映射成 `crossterm::event::KeyEvent` 喂给 `App::on_key`
- 面板本身是个 Windows Terminal 窗口，做不到无边框浮层，`--gui` 才是出路

剩下的主要都是打磨项（圆角 / 阴影 / 淡入、鼠标点击、滚动条、DPI 实测、窗口尺寸配置）。
**中文输入法（IME）已接入**：`Ime::Preedit` 画预编辑、`Ime::Commit` 插入、
`set_ime_cursor_area` 定位候选窗 —— 要改就先读 `src/gui/mod.rs` 的 `ime()`。

## 7. 调试入口

```bash
tupa-launcher.exe --query "@ev 报告"      # 打印 provider 结果
tupa-launcher.exe --query "@"             # 命令列表
tupa-launcher.exe --query "@set"         # 开关列表（会真的执行 get 脚本）
tupa-launcher.exe --apply brightness 50   # 直接写一次开关（调试 actions.toml）
tupa-launcher.exe --check                 # 配置里每个 app 能否解析
tupa-launcher.exe --resolve "C:\x.lnk"    # 快捷方式解析成什么
tupa-launcher.exe --reveal "C:\x.exe"     # 测「在资源管理器里定位」
tupa-launcher.exe --daemon --debug        # 守护进程 + 日志
```

日志：`%TEMP%\tupa-launcher-daemon.log`

## 8. 已知可以不做的小事

- `KNOWN_FLAGS` 是全局的（`@app:week` 里的 `week` 会被当修饰符吞掉）—— 语法代价，按 provider 过滤 flag 不值
- `favorites::toggle` 有两次小文件 IO（收藏不是高频操作）
- 面板隐藏时每 200ms 轮询一次窗口可见性（改成就绪通知要碰窗口消息）
- `Item.columns` 在每帧渲染时会分配 `String`（30fps 下可接受，真要优化就改成借用）
