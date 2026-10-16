# 调研：Windows 上的命令式启动器 / TUI 面板

> 调研日期：2026-10-16
> 结论：**没有现成项目能完整满足需求；自研一个 TUI 面板是可行的，且工作量可控。**

## 0. 需求清单（从原始诉求归纳）

| 编号 | 需求 | 说明 |
|---|---|---|
| R1 | **命令式语法** | `<功能> <空格> <参数>`，例：`ev 报告.pdf`、`app code`、`go ~/Projects` |
| R2 | **主题可自定义** | 至少要能换主题色，最好复用现有 Everforest 角色表 |
| R3 | **可增长的功能面** | 历史记录搜索、运行命令、智能识别并打开文件夹 |
| R4 | **app 列表可自定义** | 最常用功能，要能手工维护/pin，而不是全盘扫描 |
| N1 | 启动快、常驻占用低 | 相对于 PowerToys 常驻 |
| N2 | 扩展/配置改动**零打包开销** | 不要 msix、不要重复安装、改完即刻生效 |

## 1. 候选总览

### 1.1 GUI 启动器（PowerToys CmdPal 同类）

| 项目 | 技术栈 | 插件语言 | 插件开发体验 | 占用 | 备注 |
|---|---|---|---|---|---|
| **PowerToys Command Palette** | C#/.NET + WinUI | C#（Extension SDK） | ❌ 差：需打包 msix、无热重载、易出现重复安装 | 高（PowerToys 全家桶常驻） | 当前方案，被淘汰 |
| **[Flow Launcher](https://github.com/Flow-Launcher/Flow.Launcher)** | C#/WPF | C# / **Python** / JS(JSON-RPC) | ✅ 最好：`plugin.json` + `query()`，改完 reload 即可 | 中（~100–150MB） | 有 Everything 插件、程序插件，支持 `action keyword + 空格 + 参数` 语法 |
| **[Wox v2](https://github.com/Wox-launcher/Wox)** | 自绘 UI（非 Electron），跨平台 | Node / Python / 脚本 | ✅ 好，带插件商店 | 中（~150MB） | 有 macOS/Linux 版 |
| **[Keypirinha](https://keypirinha.com/)** | C++ 核心 + Python3 插件 | Python 3（SDK） | ⚠️ 中：无 GUI 设置，全靠 ini/文件夹 | ✅ 最低、冷启动最快 | 语法是 `keyword: 参数`，非常接近 R1；**但上游基本停更** |
| **[ueli](https://github.com/oliverschwendener/ueli)** | Electron/TS | TypeScript | ⚠️ 中 | 高（Electron） | 内置功能多、插件生态小 |
| **[Fluent Search](https://fluentsearch.net/)** | C# | C# | ⚠️ 中 | 中 | 功能强，闭源 |
| **Listary / Launchy** | 闭源/老旧 | 无 | — | 低 | 不满足 R2/R3 |

### 1.2 TUI / 终端系（更贴近 R1 的"手感"）

| 项目 | 栈 | 定位 | 能否当启动器 |
|---|---|---|---|
| **[television (`tv`)](https://github.com/alexpasmantier/television)** | Rust | 可扩展的 **fuzzy finder**，核心概念是 **channel**（一个 TOML 文件描述数据源 + 预览 + 动作） | ⭕ 最接近：`tv apps` / `tv files` 就是一个 channel；但它是"查找器"而非"启动器"，自带的动作模型偏弱，主题只能调少量颜色 |
| **[launchr](https://github.com/Cod-e-Codes/launchr)** | Rust + ratatui | 终端"任务控制台"：扫 PATH 出 app 列表、fuzzy 搜索、启动/杀进程、书签、计算器 | ⭕ 可用但**功能很杂**（正是你要避免的冗余），且配置与主题体系不对接 |
| **[sunbeam](https://github.com/pomdtr/sunbeam)** | Go + bubbletea | 通用命令启动器（raycast/rofi + fzf 混合），UI 由脚本/扩展描述 | ⭕ 概念契合，但要自己写扩展脚本，等于还是自研 |
| **[fastlauncher](https://github.com/fastlauncher/fastlauncher)** | Go | rofi/wofi 的 TUI 替代 | ❌ Windows 支持 WIP |
| **[walker](https://github.com/abenz1267/walker)** | Go | 多用途启动器、插件、fuzzy | ❌ 实际是 Linux/Wayland only |
| rofi / wofi / fuzzel / tofi | C | Linux 启动器 | ❌ Linux only；但它们是**交互模型**的参照物 |
| **fzf / skim** | Go / Rust | 通用 fuzzy 过滤器 | ⭕ 作为 UI 内核可用，但主题只有 `--color` 少量槽位，做不出"自定义主题色"的层级 |

### 1.3 传统文件搜索（作为后端，不是竞品）

Everything + `es.exe`（已装，`~/AppData/Local/Everything/es`）——**这是最好的后端**，直接当 R1 里 `ev` 的实现，比任何启动器自带的文件索引都快。

## 2. 为什么都不合适（三个硬缺口）

1. **交互形态不匹配**：GUI 启动器是"悬浮输入框"，TUI 系又都是"查找器"而非"命令分发器"。
   你要的 `功能 + 空格 + 参数` 本质是 **CLI 的心智模型**，只有 Keypirinha 最像，但它已停更。
2. **主题是黑盒**：所有 GUI 启动器的主题都存在自己的格式里（json/xaml），**无法复用你的 `~/Themes/themes/everforest/tokens.yaml` 角色表**。
   你已有的 `generate.py` 体系是"改一处全应用同步"，新工具的配色应当并入这条流水线，而不是成为第 N 个游离的色板。
3. **可扩展性 = 打包 + 重启**：CmdPal 最严重（msix、重复安装、无热重载）。
   Flow Launcher 已经好很多，但它依然是另一个 GUI 生态，且引入 .NET WPF 常驻。

**补充**：Flow Launcher 是"不想自研"时的最优退路（Python 插件 + 大量现成插件，1 小时能上手），
建议把它记为 **Plan B**，而不是无视。

## 3. 结论：自研 TUI 可行

### 3.1 为什么可行

核心逻辑其实很小。一个命令面板 = 5 个模块：

| 模块 | 内容 | 复杂度 |
|---|---|---|
| 输入解析 | 一行文本 → `(provider, args)` | 低 |
| Provider 注册表 | `ev` / `app` / `go` / `run` / `hist` 各自的"取数据 + 过滤 + 渲染" | 中（每个 provider 独立文件，加功能=加文件） |
| 交互层 | 列表 + 选中 + 键位（jk/方向键/Enter/Esc/Tab） | 中（选对 TUI 库就省一大半） |
| 动作层 | 打开文件 / 启动程序 / 复制路径 / 打开所在目录 | 低（Windows 上就是 `ShellExecuteW`） |
| 配置与主题 | `config.toml` + 从 `tokens.yaml` 生成配色 | 低（可直接接你现有 generate.py） |

**关键点：provider 就是一个函数**，新增功能不需要打包、不需要重启框架，配置改完重开即可——直接解决 N2。

### 3.2 可复用的现成资产

- `es.exe`（Everything CLI）→ `ev` provider，0 成本
- `zoxide`（已装）→ `go <目录名>` provider
- `fzf`/`rg`/`fd`（已装）→ 其他 provider 的数据源
- `~/Themes/generator/generate.py` + `tokens.yaml` → 主题生成
- 已有 C# / Python 经验（PathPal）、已装 dotnet 10 / node / uv

### 3.3 风险与代价（必须提前认清）

| 风险 | 说明 | 对策 |
|---|---|---|
| **全局热键唤起** | TUI 跑在终端里，做不到 CmdPal 那种"从屏幕顶部飘出来"。 | 用 **Windows Terminal 的 Quake 模式 + `globalSummon` 快捷键**（已确认装了 WT）；或 GlazeWM 键位绑定 `shell-exec` 起一个专用窗口；Windhawk/AHK 兜底。 |
| 主观体验有落差 | 终端有等宽字体、行列限制，视觉不如 WinUI 精致。 | 用 Nerd Font + 主题色 + 边框（你已有 yazi 的经验）。 |
| 启动延迟 | 决定"按热键到出现"的手感。 | 排除 Python+Textual（冷启动 0.3–1s）；选 Rust/Go/C#(AOT)。 |
| 功能蔓延 | 容易做成又一个庞然大物。 | provider 严格按需加，宁缺毋滥。 |

### 3.4 顺带收益

- 跨平台（Linux 上跑得通）——对正在学 Linux 的你来说是加分项，也是一个很好的系统编程练习（事件循环、ANSI、进程管理、Windows API）。
- 主题与 Everforest 体系打通后，是这个工具集里**唯一"可编程"的入口**。

## 4. PowerToys 退役路径

不用为了这一个面板继续养 PowerToys 全家桶。除 CmdPal 外的两个"其实只在用"的功能：

| 功能 | 替代 | 说明 |
|---|---|---|
| **Awake**（防睡眠） | **[NoSleep](https://github.com/JohnXu22786/nosleep)**（C，GPL-3，托盘 + 完整 CLI）或 **[keepawake-rs](https://github.com/segevfiner/keepawake-rs)**（Rust，`--display/--idle/--sleep`，可 `keepawake -- cmd` 挂着命令） | 若要"任务结束自动解除"，选 **WakeWhile**（90KB 单文件）。三者都比 Awake 轻 |
| **File Locksmith** | **[ShowWhatProcessLocksFile](https://github.com/PolarGoose/ShowWhatProcessLocksFile)**（开源，明确以 File Locksmith 为参照的轻量克隆，含右键菜单） | CLI 口味可选 **[whoholds](https://github.com/Yortw/winix)**（dotnet tool，支持 `:端口` 查询）或 Sysinternals `handle.exe`；要 GUI 全能选 **[System Informer](https://systeminformer.sourceforge.io/)** |

> 注：未提权时任何工具都只能看到自己用户进程的手柄，这是 Windows 的限制，不是工具的锅。

→ PowerToys 可整包卸载（保留 `Everything`、`Windhawk`、`tacky-borders` 等独立工具即可）。

## 5. 参考链接

- Flow Launcher：[仓库](https://github.com/Flow-Launcher/Flow.Launcher) · [Python 插件 wrapper](https://garulf.github.io/pyFlowLauncher/) · [插件样例](https://github.com/Flow-Launcher/plugin-samples)
- Keypirinha：[官网](https://keypirinha.com/) · [API](https://keypirinha.com/api/module.html)
- television：[仓库](https://github.com/alexpasmantier/television) · [channel 规范](https://alexpasmantier.github.io/television/reference/channel-spec/) · [Windows 频道示例](https://alexpasmantier.github.io/television/community/channels-windows/)
- 其他：[launchr](https://github.com/Cod-e-Codes/launchr) · [sunbeam](https://github.com/pomdtr/sunbeam) · [Wox](https://github.com/Wox-launcher/Wox)
- 替代品：[NoSleep](https://github.com/JohnXu22786/nosleep) · [keepawake-rs](https://github.com/segevfiner/keepawake-rs) · [ShowWhatProcessLocksFile](https://github.com/PolarGoose/ShowWhatProcessLocksFile) · [whoholds](https://github.com/Yortw/winix)
