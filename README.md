# Tupa Launcher

> 一个**命令式**启动器：`功能 + 空格 + 参数`，跑在终端里。
>
> ```text
> ❯ :app vs          → 从列表里启动 Visual Studio Code
> ❯ :ev 报告 2026    → 用 Everything 找这个文件，回车直接打开
> ❯ :web rust async  → 在浏览器历史里找 rust async
> ```
>
> 状态：**v0.6** · Rust + ratatui · 替代 PowerToys Command Palette
> 调研与可行性论证见 [`docs/01-research.md`](docs/01-research.md)，设计决策见 [`docs/02-design.md`](docs/02-design.md)
> 后续计划见 [`docs/03-roadmap.md`](docs/03-roadmap.md)，
> **接着开发的入口见 [`docs/06-handover.md`](docs/06-handover.md)**

## 快速开始

```bash
cd ~/Projects/tupa-launcher
cargo build --release
./target/release/tupa-launcher.exe
```

调试用（不进界面，直接把结果打到 stdout）：

```bash
./target/release/tupa-launcher.exe --query ":ev curl"
./target/release/tupa-launcher.exe --query ":app vsc"
./target/release/tupa-launcher.exe --query ":"        # 命令列表
./target/release/tupa-launcher.exe --query ":switch"  # 开关 / 选项 / 数值
./target/release/tupa-launcher.exe --apply brightness 50  # 直接执行一次开关写入
```

需要 `es.exe`（Everything 的命令行）—— 自动探测，探测不到就不注册 `ev`。
Everything 本体没装的话，`ev` 会用不了，其余功能正常。

## 全局热键

```bash
./target/release/tupa-launcher.exe --daemon    # 常驻
```

两个热键，任选（都可在 `config.toml` 的 `[daemon]` 里改）：

| 热键 | 怎么来的 |
|---|---|
| **`Alt+`` `** | `RegisterHotKey` 注册（默认值） |
| **`Copilot 键`** | 低级键盘钩子拦 `F23`（`copilot_key = true` 才启用） |

按一下唤起、再按一下隐藏；`Esc` 在输入框空的时候是隐藏（进程留着，下次唤起瞬时），
有内容时先清空输入；
`Enter` 启动应用后也是隐藏，把屏幕让出去。选完以后想彻底结束进程按 `Ctrl+C`。

已配置开机自启：启动文件夹里的 `tupa-launcher-daemon.lnk`。
想停掉就 `taskkill /IM tupa-launcher.exe /F`，或删掉那个快捷方式。

### 为什么不用裸 `Win+S`

试过，是个死结（完整推理见 [`docs/02-design.md`](docs/02-design.md) §6）：
`Win+S` 是系统保留键 → `RegisterHotKey` 注册不了 → 换低级键盘钩子能吞掉按键，
但 shell 判断 Win+S 看的是**异步按键状态**（`GetAsyncKeyState`），吞按键拦不住 →
注入假抬起把异步状态抹平，shell 又认为「只按了一下 Win 键」而弹出开始菜单 →
要阻止开始菜单就得吞掉真实的 Win 抬起，而那样**系统的 Win 键会一直处于按下状态**：
打字全大写（Shift 同理）、点任务栏变成 `Win+点击`、Win 键失去响应。

而 `Alt+`` ` 这类组合不在系统保留范围内，`RegisterHotKey` 直接就能注册，
于是完全不需要钩子，也就没有钩子超时、修饰键卡死这些坑。
Copilot 键是个例外（它的原样组合被 shell 占着），但用钩子**只拦 F23 这一个键**、

不碰任何修饰键，风险可控。

> 面板窗口的位置和尺寸不在守护进程里算，而是交给窗口管理器：
> GlazeWM 的 `window_rules` 里有一条 `set-floating` +
> `size --width 62% --height 55%` + `position --centered`。
> 没跑 GlazeWM 时窗口就用 Windows Terminal 自己的默认位置。

## 用法

开屏显示**命令列表**；命令本身**以 `:` 开头**（开屏输入框是否预填 `:` 由
`[general] prefill_colon` 决定，默认 `false` = 空输入但仍列命令）：

| 输入 | 行为 |
|---|---|
| （开屏 / 空输入） | 命令列表；回车把 `:命令 ` 填进输入框 |
| `:` | 用 `:` 开头进入命令模式（Tab 循环补全命令名） |
| `:app <名字>` | 从自定义列表里启动应用（模糊匹配）；`.lnk` 会递归解析到目标 exe 显示 |
| `:ev <关键词>` | Everything 搜文件，**支持原生语法**：`:ev ext:pdf 报告`、`:ev folder:Projects` |
| `:run <命令>` | 开一个新终端窗口执行（默认 PowerShell；`:run:bg` 后台，`:run:admin` 提权） |
| `:go <路径>` | 跳到目录 / 定位文件；空参数读剪贴板；`:go:code` / `:go:yazi` / `:go:pi` 用指定程序打开 |
| `:pi [关键词]` | 列出所有 pi 会话（标题 / 项目 / 时间），**回车在新终端里 `pi --session` 打开**；`:pi:all` 包含 `session-archive` 里的归档 |
| `:web <网址 / 关键词>` | 搜 Edge 历史（默认**一周内**）；搜不到且输入像网址就直接打开（`:web:month` / `:web:all` 扩大范围，`:web:url` 强制打开） |
| `:new <文件名>` | 在 `~/Projects/New Files` 里新建文件并打开（没写扩展名默认 `.md`，同名不覆盖） |
| `:fav [类型] [关键词]` | 列出收藏（`:fav app` / `:fav web` 按类型过滤），`Ctrl+D` 收藏 / 取消收藏 |
| `:cfg [关键词]` | 列出配置项（当前值 / 行号 / 说明），**回车在 VSCode 里跳到对应行** |
| `:switch [关键词]` | 应用内开关 / 选项 / 数值（深色模式、亮度、电池养护……），回车应用、`←/→` 调整、**不隐藏面板** |
| `<名字>`（不带 `:`） | 整行交给默认 provider 当搜索词（当前是 `app`） |

命令前缀（`:app` / `:run:admin`）在输入框里会**高亮成黄色**，一眼能看出当前在哪个命令。

前缀本身**可以换**：`config.toml` 里 `general.command_prefix = "@"` 之后就是
`@app code`、`@run@admin ipconfig`（修饰符分隔符跟着变；`@run:admin` 这种写法也认）。

### `:web` 的时间范围与直接打开

```text
:web                  → 最近一周访问过的页面
:web:month rust       → 一个月内
:web:all rust         → 不限时间（最近 3000 条）
:web github.com       → 先在历史里找 github.com；找不到就直接打开 https://github.com
:web:url github.com   → 不看历史，直接打开
```

历史来自 Edge 的 `User Data\{Default,Profile N}\History`：Edge 运行时那个文件被独占，
所以**先复制到临时目录**再打开；标题用模糊匹配、URL 只用「去掉查询串后的部分」做子串匹配，
避免 `?q=...&cvid=...` 里的随机串乱命中。

`last_visit_time` 是 Chromium 时间戳（1601 起、单位**微秒**）—— 网上很多资料写的
100ns 是错的，差 10 倍。

### 命令补全

光标前那个词里有 `:` 时，输入框下面会多出一行候选，`Tab` 在候选里**循环**：

```text
❯ :ap
   Tab  app  web  ...
```

- `:` / `:ap` → 补全命令名
- `:run:ad` → 补全修饰符（`admin` / `bg` / `week` / `month` / `all` / `url`）
- 一旦敲了空格（进入参数区）就停止补全 —— 参数补全暂时不做
- 按 Tab 在候选里循环时，**状态栏会显示当前候选的说明**（比如 `admin · 提权执行，会弹 UAC`）

### 修饰符语法（`:flag`）

命令关键字后面可以继续用 `:` 加修饰符：

```text
:run:admin ipconfig        → 提权执行（弹 UAC）
:app:admin cmd             → 以管理员身份启动
:run:admin:bg foo          → 多个修饰符
```

认得的修饰符只有这几个（不同命令各取所需，**没有同义词**）：

| 修饰符 | 作用 |
|---|---|
| `admin` | 提权执行（弹 UAC） |
| `bg` | `:run` 在后台执行（输出丢弃） |
| `week` / `month` / `all` | `:web` 的搜索时间范围（默认 `week`） |
| `url` | `:web` 强制直接打开网址 |
| `code` / `yazi` / `pi` | `:go` 用 VSCode / yazi / pi 打开目标 |

不在这个列表里的内容会当作参数。

### 收藏

`Ctrl+D` 收藏 / 取消收藏当前选中项（已收藏的条目在列表里带一个 `★`）。
收藏存的是「条目快照」，在 `~/.config/tupa-launcher/favorites.json`：

```json
[
  {
    "kind": "App",
    "columns": ["Visual Studio Code", "code"],
    "action": { "Launch": { "command": "code", "args": [], "elevated": false } }
  }
]
```

> 旧格式（`title` / `subtitle` 两个字段）仍能读，下次保存会自动转成 `columns`。

不追踪原条目变化（比如一个文件的路径后来失效了，收藏里也会失效）。

### 应用内开关：`:switch`

把「打开 Vantage / 设置面板改一下再关掉」变成面板里的一次回车。

```text
:switch              列出所有开关（读不到状态的项默认隐藏）
:switch 电源         按标题 / 分组过滤
:switch:all          连「不可用」的项一起列出
```

内置五项**不需要管理员权限**的开/关：

- **深色模式**（写 `HKCU` 主题键 + 广播 `WM_SETTINGCHANGE`）
- **屏幕亮度**（WMI 的 `WmiMonitorBrightness` / `WmiMonitorBrightnessMethods`，0~100，步长 10）
- **电池养护模式**（反射调用 Lenovo Vantage 的 `IdeaBatteryAgent.dll`，`Storage` ↔ `Normal`）
- **节能模式**（Windows 11 的 Energy Saver：`Win+A` 打开快速设置 → `Home` + `→→↓` + `Space`
  切到「节能模式」→ 关闭；键盘失败时回退到鼠标点击。用注册表 `EnergySaverState` 校验）
- **保持唤醒**（`D:\Softwares\nosleep\nosleep.exe` 的托盘进程：开 = 启动并立即阻止睡眠，
  关 = 结束进程；`get` 直接看进程是否存在）

选中一行后：`←/→` 改本地值，`Enter` 写出去；状态显示在行尾（`开` / `关` / `80%`）。
写操作在后台 worker 里跑（带 5 秒超时），完成后自动重读状态并留在原来那一行。

#### 自己的开关：`actions.toml`

`~/.config/tupa-launcher/actions.toml`（不存在就只用内置项；同 `id` 覆盖内置）：

```toml
# 开关
[[items]]
id = "fan-boost"                # 不写默认用标题
kind = "toggle"
title = "风扇全速"
subtitle = "Legion"
group = "性能"
get = 'powershell -NoProfile -File C:\Users\me\.config\tupa-launcher\scripts\fan-get.ps1'
on  = 'powershell -NoProfile -File C:\Users\me\.config\tupa-launcher\scripts\fan-set.ps1 -Value 1'
off = 'powershell -NoProfile -File C:\Users\me\.config\tupa-launcher\scripts\fan-set.ps1 -Value 0'
on_value = "1"                  # get 输出这个字符串算「开」（默认认 1/true/on/开/yes/enabled）

# 选项
[[items]]
kind = "choice"
title = "性能模式"
options = ["安静", "均衡", "性能"]
get = 'powershell -NoProfile -File ...\perf-get.ps1'
set = 'powershell -NoProfile -File ...\perf-set.ps1 -Mode "{value}"'

# 数值
[[items]]
kind = "number"
title = "风扇转速"
min = 0
max = 100
step = 10
suffix = "%"
get = '...'
set = '... -Value {value}'
```

- 命令统一走 `cmd /C`，在 worker 线程执行，**5 秒超时**；失败时状态栏显示 stderr 第一行。
- `{value}` 会替换成实际值（选项是文本，数值是数字）；也可以直接读环境变量 `TUPA_VALUE`。
- `get` 失败（脚本报错 / 设备不支持）→ 该行在 `:switch:all` 里显示「不可用」。

> **电池养护模式**已经内置（id = `battery-care`），原理是反射调用 Vantage 的
> `IdeaBatteryAgent.dll`，不需要管理员。`LENOVO_GAMEZONE_DATA` 的 Win 键锁 / 风扇接口
> 在 ThinkBook 上实测不可用（方法调用报「无效的对象」），这类功能才需要管理员/驱动。

### 命令历史

动作**执行成功**后自动记录（失败的、只查询没执行的都不记），存在
`~/.config/tupa-launcher/history.json`，去重、最多 200 条。

| 键 | 作用 |
|---|---|
| `Ctrl+↑` | 上一条（更早的）输入 |
| `Ctrl+↓` | 下一条（更新的）输入；到底时回到进入历史前正在输入的草稿 |

任何手动编辑都会退出历史浏览模式。

### `go` 的打开方式

```text
:go:code 项目        → 用 VSCode 打开（文件也可以，直接定位到它）
:go:yazi 项目        → 新终端窗口里用 yazi 打开所在目录
:go:pi   项目        → 新终端窗口里在目标目录启动 pi
```

### `go` 的路径写法

```text
:go ~/Projects                  → ~ 展开为用户目录
:go "C:\Users\<你的用户名>\Projects" → 引号（单/双）自动去掉
:go file:///C:/Users/My%20Docs  → file:// 前缀 + %XX 解码
:go projects\term              → 半截路径，走 zoxide + Everything 补全
:go                             → 空参数，读剪贴板
:go D:\Legacy\report.pdf       → 文件：在资源管理器里定位
```

匹配顺序：**存在的完整路径 → zoxide 历史 → Everything 路径匹配**。
这里和终端的 `z` 命令**共用同一份 zoxide 数据库**：终端里跳过的目录这里能搜到，
这里打开过的目录也会写回去（`zoxide add`）。

### 界面

列表是**分列**的（徽标 + 若干列）。每列用「份数 + 最短 + 最长」描述：

```rust
// 例如 web：标题（6 份，最少 12 列）· 域名（2 份，最多 40）· 访问次数（1 份，最多 12）
const COLUMNS: &[Column] = &[
    Column::new(6, 12, 0),
    Column::new(2, 8, 40),
    Column::new(1, 0, 12),
];
```

- 可用宽度按份数比例分给各列，再夹到 `[min, max]`；终端太窄时按 `min` 的比例压缩
- 放不下的列内容用 `…` 省略；**指针所在行**的每一列会左右滚动把完整内容走一遍（滚到底停一会儿再回到开头）
- `Ctrl+Tab`（或 `Ctrl+O`）切「详细信息」：把每列拆成单独一行、缩进对齐到标题列，超宽同样省略 + 滚动，**不换行**；上下键仍然按条目移动
- 详细信息最后还会多一行**完整目标**：`web` 显示完整 URL、`ev`/`go` 显示完整路径、`run` 显示命令行（列里已经有的内容不重复）

```text
紧凑：     ▸ app   终端 (Windows Terminal)   wt
详细：     ▸ app   终端 (Windows Terminal)
                     wt
```

按键：

| 键 | 作用 |
|---|---|
| `↑` `↓` / `Ctrl+P` `Ctrl+N` | 移动选择 |
| `Enter` | 打开文件 / 启动程序（成功后自动退出，把屏幕让出去）；**开关 / 选项 / 数值行只应用新值，不退出** |
| `←` `→` | 开关 / 选项 / 数值行：调整本地值（输入光标不动），回车才写出 |
| `Ctrl+E` / `Alt+Enter` | 在文件资源管理器里定位（**app 也能用**：`.lnk` 会跟到目标 exe） |
| `Tab` | 循环补全命令名 / 修饰符（补全行显示候选） |
| `Shift+Tab` | 进入 / 退出**选择模式**：`Tab` 下一个、`1`~`9` 向下移动 n 项 |
| `Ctrl+Tab` / `Ctrl+O` | **详细信息模式**：每列纵向铺开（`Ctrl+Tab` 在 Windows Terminal 里默认被占用，需要解绑；`Ctrl+O` 是备用键） |
| `Ctrl+D` | 收藏 / 取消收藏当前选中项（`★` 表示已收藏） |
| `Ctrl+↑` `Ctrl+↓` | 上一条 / 下一条历史输入 |
| `Ctrl+W` | 删除一个词 |
| `Esc` | 选择模式里退出选择模式；否则清空输入回到开屏（`:` 或空串）；已经在开屏状态时**隐藏面板**（进程保留） |
| `Ctrl+C` | 真正退出（销毁面板进程） |

## 配置

配置文件：`~/.config/tupa-launcher/config.toml`（即 `C:\Users\<你的用户名>\.config\tupa-launcher\config.toml`）
—— 改完下次启动生效，**不需要重新编译**。

主题色可以写在同一个文件的 `[theme]` 段，或者单独的 `~/.config/tupa-launcher/theme.toml`。
不写就用内置的 Everforest(hard)。

> 色值与 Everforest 官方色板的角色名对齐。作者本地用一套 `~/Themes` 脚本统一生成
> 各应用的配色（`tokens.yaml` → 各应用主题），所以这里的角色名和它保持一致；
> 对其他人来说这一段可以忽略，直接改 `[theme]` 段就行。

```
background panel overlay border fg muted dim faint accent good warn bad
```

模板见 [`config.example.toml`](config.example.toml)。

`[general]` 里几个常用开关：

| 键 | 默认 | 说明 |
|---|---|---|
| `default_provider` | `app` | 不带 `:` 前缀时整行交给谁 |
| `max_results` | `30` | 单个 provider 最多返回多少条 |
| `everything_path` | 自动探测 | `es.exe` 路径 |
| `prefill_colon` | `false` | 开屏输入框是否预填命令前缀（`true` = 一开就在命令模式；`false` = 空输入，但结果列表仍是命令列表） |
| `command_prefix` | `:` | 命令前缀，也是修饰符分隔符；想用 `@` 就改成 `"@"` |
| `language` | `zh` | 界面语言：`zh` / `en`（所有界面文案、命令描述、`--help` 都会跟着变） |
| `show_hints` | `true` | 右下角的快捷键提示是否显示（`false` 只留左侧状态） |
| `new_files_dir` | `~/Projects/New Files` | `:new` 新建文件放哪个目录 |

## 架构

```
src/
├── main.rs              入口 + 事件循环 + --query/--check 调试模式
├── app.rs               状态机：输入解析、键位、补全、选择、与 worker 通信
├── ui.rs                纯渲染：分列布局 / 详细信息 / 选中行滚动 / 状态列
├── exec.rs              执行动作：打开文件 / 启动程序 / 定位 / 打开网址
├── actions.rs           actions.toml 解析 + 脚本执行（超时）+ 内置开关
├── daemon.rs            常驻守护进程：低级键盘钩子 + 面板窗口管理（Windows）
├── config.rs            config.toml 加载
├── favorites.rs         收藏：favorites.json 读写 + toggle
├── history.rs           命令历史：history.json 读写
├── i18n.rs              中英文案（所有用户可见字符串都在这里）
├── theme.rs             主题：内置默认 + 两层覆盖
├── util.rs              子进程 / 编码 / PATH 查找
└── providers/
    ├── mod.rs           Provider trait、Item/Action、解析、**后台 worker**
    ├── apps.rs          app  —— 自定义应用列表 + 模糊匹配
    ├── everything.rs    ev   —— 调用 es.exe + 本地相关性重排
    ├── web.rs           web  —— URL 判断 + Edge 历史（rusqlite）
    ├── favorites.rs     fav  —— 列出收藏 + 按类型过滤
    ├── commands.rs      `:`  —— 命令列表（也是补全的数据源）
    ├── new.rs           new  —— 新建文件
    ├── go.rs            go   —— zoxide + Everything + 剪贴板
    ├── run.rs           run  —— 执行命令
    ├── switch.rs        switch —— 开关 / 选项 / 数值（actions.toml）
    └── config_items.rs  cfg  —— 列出配置项
```

### 加一个新功能（比如 `go <目录>` 走 zoxide）

1. 在 `src/providers/` 下新建 `zoxide.rs`，实现 `Provider`（`keyword` / `description` / `query` 三个方法，按需覆盖 `columns()` 声明列宽）；
2. 在 `providers/mod.rs` 的 `build()` 里 push 进去。

就这样 —— 不用打包、不用重装、不用改 UI。`query()` 在后台线程里跑，可以放心阻塞。

### 为什么不卡

查询走一个单线程 worker：主线程只发"最新一次请求"，worker 会**丢弃排队中的旧查询**，
所以快速打字不会堆积一堆 `es.exe` 进程，也不会让 UI 掉帧。
结果带回一个自增序号，序号过期的直接扔掉。
开关的写操作（`Apply`）不参与丢弃，按顺序执行完才继续处理查询。

## 已知限制

- 只在 Windows 上验证过（`exec.rs`、`daemon.rs` 和 `everything.rs` 有 Windows 专属逻辑，其余跨平台）。
- `es.exe` 通过管道输出的是**系统 ANSI 代码页**（中文系统 = GBK），代码里有 UTF-8 → GBK 的回退解码。
- 全局热键默认 `Alt+`` `（`config.toml` 的 `[daemon] hotkey` 可改）；面板依赖 Windows Terminal，
  `Ctrl+Tab` 被 WT 默认占用，所以「详细信息」还准备了 `Ctrl+O`。
- 面板窗口是 Windows Terminal 窗口，做不到无边框浮层；“居中”是守护进程用 `SetWindowPos` 摆出来的。

## 路线图

- [x] **v0.1** `app` + `ev` 两个 provider、主题可覆盖、自定义 app 列表、异步查询
- [x] **v0.2** 守护进程 + `Win+S` 全局热键 + 居中面板窗口
- [x] 集成 GlazeWM `window_rules`（浮动/居中/尺寸交给 WM）
- [ ] 主题并入 `~/Themes/generator/generate.py` 流水线（在 `~/.config/tupa-launcher/theme.toml` 里生成）
- [ ] 智能识别并打开文件夹（`PathPal` 的 "字面 → 祖先回退 → Everything 兜底" 三级策略）
- [ ] `hist`（历史记录搜索，独立 provider）
- [ ] 使用频率统计，把常用项自动排前（frecency）
- [x] **v0.3** `web`（Edge 历史）+ `fav`（收藏）+ 命令历史（`Ctrl+↑/↓`）
- [x] **v0.4** 分列显示（份数/最短/最长、选中行滚动）+ 详细信息模式 + `:` 命令前缀 + Tab 补全 + 选择模式
- [x] **v0.6** 中英文（`general.language`）+ 文案精简；`:go:code` / `:go:yazi` / `:go:pi`；`:run` 默认 PowerShell；状态栏显示位置与后台忙碌
- [x] **v0.5** `:web` 时间范围（默认一周 / month / all）+ 搜不到才直接打开；`:run` 默认新窗口执行；命令前缀可配置 + 去掉同义修饰符；`:new` 新建文件
- [x] **v0.7** `:switch` 应用内开关 / 选项 / 数值（`actions.toml` 驱动 + 内置深色模式 / 亮度 / 电池养护）
- [ ] **v0.8** 自建无边框窗口，脱离 Windows Terminal（设计见 [`docs/05-native-window.md`](docs/05-native-window.md)）
