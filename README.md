# Tupa Launcher

> 一个**命令式**启动器：`功能 + 空格 + 参数`，跑在终端里。
>
> ```text
> ❯ :app vs          → 从列表里启动 Visual Studio Code
> ❯ :ev 报告 2026    → 用 Everything 找这个文件，回车直接打开
> ❯ :web rust async  → 在浏览器历史里找 rust async
> ```
>
> 版本：**0.9.0**（v0.8 套壳仍在进行中）· Rust · TUI 用 ratatui / GUI 实验用 winit + cosmic-text · 替代 PowerToys Command Palette
> 调研与可行性论证见 [`docs/01-research.md`](docs/01-research.md)，设计决策见 [`docs/02-design.md`](docs/02-design.md)
> 后续计划见 [`docs/03-roadmap.md`](docs/03-roadmap.md)
> **接着开发的入口：[`STATE.md`](STATE.md)（当前阶段 / 下一步）→ [`docs/06-handover.md`](docs/06-handover.md)（代码结构 / 坑表）**
> 用 pi 在本仓库开工时，规矩见 [`AGENTS.md`](AGENTS.md)

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
./target/release/tupa-launcher.exe --query ":set"  # 开关 / 选项 / 数值
./target/release/tupa-launcher.exe --apply brightness 50  # 直接执行一次开关写入
```

> 上面的 `:` 是默认前缀；如果你的 `config.toml` 里改成了别的（例如 `@`），
> `--query` 也要用那个前缀，否则整行会被当成默认 provider 的搜索词。

需要 `es.exe`（Everything 的命令行）—— 自动探测，探测不到就不注册 `ev`。
Everything 本体没装的话，`ev` 会用不了，其余功能正常。

## 全局热键

主入口是 **`tupa-gui.exe`** —— 独立程序，自带托盘图标 + 全局热键，**不需要守护进程**：

```bash
cargo build --release --features gui     # 默认 cargo build 不产出它
./target/release/tupa-gui.exe
```

热键 `Alt+`` `（`config.toml` 的 `[gui] hotkey` 可改），托盘图标左键显隐、右键菜单。

按一下唤起、再按一下隐藏；`Esc` 在输入框空的时候是隐藏（进程留着，下次唤起瞬时），
有内容时先清空输入；
`Enter` 启动应用后也是隐藏，把屏幕让出去。

已配置开机自启：启动文件夹里的 `tupa-gui.lnk`。
想停掉就 `taskkill /IM tupa-gui.exe /F`，或删掉那个快捷方式（右键托盘 → 退出 也行）。

守护进程 + TUI 的实现还在，可以手动跑（`./target/release/tupa-launcher.exe --daemon`），
但它已不是日常入口 —— **热键只能被一个进程注册**，两边同时跑会抢，所以旧的
`tupa-launcher-daemon.lnk` 已**移出启动文件夹**（删到回收站；不要用改扩展名的方式「停用」——
Explorer 会把启动文件夹里任何文件交给文件关联，本机 `.disabled` 会弹 NanaZip 报错）。它的热键在 `[daemon]` 里：

| 热键 | 怎么来的 |
|---|---|
| **`Alt+`` `** | `RegisterHotKey` 注册（默认值） |
| **`Copilot 键`** | 低级键盘钩子拦 `F23`（`copilot_key = true` 才启用） |

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

开屏显示**收藏 + 最近执行过的命令**（`home` provider，输入为空时生效）；命令本身**以 `:` 开头**
（开屏输入框是否预填 `:` 由 `[general] prefill_colon` 决定，默认 `false` = 空输入，
改成 `true` 则一开就在命令模式）：

如果剪贴板里恰好是**一个存在的路径**或 **http(s) 网址**，开屏第一行会多一条「剪贴板」
（徽标 `clip`）：回车直接跳过去 / 打开。它**不抢默认选中**，所以唤起后随手回车不会误跳，
想用它按一下 `↑`。`[general] clipboard = false` 可以关掉。

| 输入 | 行为 |
|---|---|
| （开屏 / 空输入） | 收藏 + 最近 25 条执行过的命令，回车直接执行；输入 `:` 则改为列出全部命令 |
| `:` | 用 `:` 开头进入命令模式（Tab 循环补全命令名） |
| `:app <名字>` | 从自定义列表里启动应用（模糊匹配）；`.lnk` 会递归解析到目标 exe 显示 |
| `:all <关键词>` | 默认搜索的显式写法：**应用 + 命令历史混排**（两边同一套模糊打分，同分时应用优先）；留空只列应用 |
| `:ev <关键词>` | Everything 搜文件，**支持原生语法**：`:ev ext:pdf 报告`、`:ev folder:Projects` |
| `:z <路径片段>` | 跳到目录 / 定位文件（zoxide 历史 + Everything 兜底）；**空输入列出 zoxide 的完整历史**（按 rank 降序），剪贴板里是路径时插在最前面；`:z:code` / `:z:yazi` / `:z:pi` 用指定程序打开 |
| `:web <网址 / 关键词>` | 搜 Edge 历史（默认**一周内**）；搜不到且输入像网址就直接打开（`:web:month` / `:web:all` 扩大范围，`:web:url` 强制打开） |
| `:hist [关键词]` | 搜**命令历史**（不在 `:` 命令列表里显示，避免和开屏的历史重复；照样能用）（`history.json`，最多 200 条）：模糊匹配当时的输入行 / 条目标题 / 副标题，带关键词时按相关度排序；回车同开屏——有快照直接再打开，老记录只把输入行填回输入框 |
| `:blog [new|manage|stop]` | 自己的博客：不带参数列**五个入口**——新建文章 / 本地预览 / 管理页 / GitHub Pages / Cloudflare。本地预览与管理页带状态列（「已在运行」/「未启动」）：**服务在跑就只开浏览器，没跑就先在后台起 `pnpm server` / `pnpm admin`，等端口真的监听上再打开**（不再重复起进程）。服务启动后是**静默后台进程（无窗口）**，所以在跑时会多出一条**停止本地预览 / 停止管理页**（只杀 node / pnpm / cmd，`taskkill /T` 连子进程；端口被别的程序占着时只报错不动手）；`:blog:stop` 一次停掉所有在跑的。`:blog:new <标题>` 在 `source/_posts` 新建文章并**用 VSCode 打开**（同名不覆盖，空参数列最近文章）；`:blog:manage` 直接到管理页 |
| `:fav [类型] [关键词]` | 列出收藏（`:fav app` / `:fav web` 按类型过滤），`Ctrl+D` 收藏 / 取消收藏 |
| `:cfg [关键词]` | 列出配置项（当前值 / 行号 / 说明），**回车在 VSCode 里跳到对应行** |
| `:set [关键词]` | 应用内开关 / 选项 / 数值（深色模式、亮度、电池养护……），回车应用、`←/→` 调整、**不隐藏面板** |
| `<名字>`（不带 `:`） | 整行交给默认 provider 当搜索词（默认 `all`：应用 + 历史混排；改成 `app` 就只搜应用） |

命令前缀（`:app` / `:web:url`）在输入框里会**高亮成黄色**，一眼能看出当前在哪个命令。

前缀本身**可以换**：`config.toml` 里 `general.command_prefix = "@"` 之后就是
`@app code`、`@web@url github.com`（修饰符分隔符跟着变；`@web:url` 这种写法也认）。

### pi 会话清理（已移出本程序）

`@pi` 与 `@pi:clean` 在 2026-11-19 整体移除。原因：`@pi` 每次查询要开 175 个 jsonl、读前 80 行
（实测 0.68s / 88MB），而清理本质是「一眼看完一批会话，挑一些归档 / 删掉」——
浏览器页面比 TUI 面板顺手。

现在由 `my-pi-plugins` 的 `/clean` 起一个本地服务 + 网页来做：

- 前端页面：`web/clean/index.html`（单文件、无构建）
- 规则与 HTTP 契约：`docs/09-clean-api.md`（时间窗口是前置条件；已总结 / 多轮的归档，
  瘦会话进回收站；归档是移动、可恢复）

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
- `:web:we` → 补全修饰符（`admin` / `week` / `month` / `all` / `url` / `code` / `yazi` / `pi`）
- 一旦敲了空格（进入参数区）就停止补全 —— 参数补全暂时不做
- 按 Tab 在候选里循环时，**状态栏会显示当前候选的说明**（比如 `admin · 提权执行，会弹 UAC`）

### 修饰符语法（`:flag`）

命令关键字后面可以继续用 `:` 加修饰符：

```text
:app:admin cmd             → 以管理员身份启动
:web:month 关键词          → 搜一个月内的浏览器历史
:z:yazi ~/Projects         → 在 yazi 里打开（可叠 ：z:code）
```

认得的修饰符只有这几个（不同命令各取所需，**没有同义词**）：

| 修饰符 | 作用 |
|---|---|
| `admin` | 提权启动 / 执行（弹 UAC）：`:app:admin` / `:all:admin` |
| `week` / `month` / `all` | `:web` 的搜索时间范围（默认 `week`）；`:set:all` 显示全部开关 |
| `url` / `title` | `:web` 强制直接打开网址 / 只按标题匹配 |
| `code` / `yazi` / `pi` | `:z` 用 VSCode / yazi / pi 打开目标 |
| `new` / `manage` / `stop` | `:blog` 的子命令 |

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

### 应用内开关：`:set`

把「打开 Vantage / 设置面板改一下再关掉」变成面板里的一次回车。

```text
:set              列出所有开关（读不到状态的项默认隐藏）
:set 电源         按标题 / 分组过滤
:set:all          连「不可用」的项一起列出
```

内置六项**不需要管理员权限**的开关：

- **深色模式**（写 `HKCU` 主题键 + 广播 `WM_SETTINGCHANGE`）
- **屏幕亮度**（WMI 的 `WmiMonitorBrightness` / `WmiMonitorBrightnessMethods`，0~100，步长 10）
- **电池养护模式**（反射调用 Lenovo Vantage 的 `IdeaBatteryAgent.dll`，`Storage` ↔ `Normal`）
- **节能模式**（把联想 Vantage 的「静音省电」和 Windows 11 的 Energy Saver 合成一个开关：
  开 = 两边都开，关 = Vantage 回智能模式 + 关掉 Energy Saver。Energy Saver 走
  `Win+A` 打开快速设置 → 鼠标点击「节能模式」按钮（不用方向键：Space 切的是焦点项，
  固定按钮一变就会误关蓝牙/开飞行模式），点完用注册表 `EnergySaverState` 校验）
- **保持唤醒**（`D:\Softwares\nosleep\nosleep.exe` 的托盘进程：开 = 启动并立即阻止睡眠，
  关 = 结束进程；`get` 直接看进程是否存在）
- **更新订阅**（Clash Party：读 `%APPDATA%\mihomo-party\profile.yaml` 的当前订阅与上次更新时间，
  回车下载并覆盖 `profiles/<id>.yaml`、改 `updated`、重启 Clash Party ——
  只有它自己会重新生成 `work/config.yaml`，单靠内核 API reload 会丢端口 / TUN / 规则集）

选中一行后：`←/→` 改本地值，`Enter` 写出去；状态显示在行尾（`开` / `关` / `80%`）。
写操作在后台 worker 里跑（普通条目 5 秒超时；`kind = "action"` 60 秒，`timeout` 可覆盖），
完成后自动重读状态并留在原来那一行。

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
timeout = 5                     # 可选：这一条最长跑几秒（默认 5；action 类默认 60）
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

- 命令统一走 `cmd /C`，在 worker 线程执行，**普通条目 5 秒超时**；
  下载 / 重启应用这类「真干活」的条目用 `kind = "action"`（默认 60 秒），
  也可以用 `timeout = <秒>` 单独调；失败时状态栏显示 stderr 第一行。
  超时会 `taskkill /T` 连子进程一起收，不会留半截下载在后台继续写文件。
- `{value}` 会替换成实际值（选项是文本，数值是数字）；也可以直接读环境变量 `TUPA_VALUE`。
- `get` 失败（脚本报错 / 设备不支持）→ 该行在 `:set:all` 里显示「不可用」。

> **电池养护模式**已经内置（id = `battery-care`），原理是反射调用 Vantage 的
> `IdeaBatteryAgent.dll`，不需要管理员。`LENOVO_GAMEZONE_DATA` 的 Win 键锁 / 风扇接口
> 在 ThinkBook 上实测不可用（方法调用报「无效的对象」），这类功能才需要管理员/驱动。

### 命令历史

动作**执行成功**后自动记录（失败的、只查询没执行的都不记），存在
`~/.config/tupa-launcher/history.json`，去重、最多 200 条。

**空参数执行**（`:z` 这种）记的是**实际那个目标**，不是光秃秃的 `:z`：
历史里是 `:z C:\Users\me\Themes`。
所以每次跳转都是独立一条、`Ctrl+↑` 回放能直接跳回那个目录，次数也分开算 ——
不会把八次跳转压成一条、快照互相覆盖。

每条还带**执行次数**与**最后执行时间**，开屏（最近 25 条）与 `@app` 的空查询按
`次数 × 新鲜度` 排序（一小时内的权重 400 / 一天 200 / 一周 100 / 一月 50 / 更早 25），
所以常用的会自动浮到前面；`[general] frecency = false` 可以关掉。

| 键 | 作用 |
|---|---|
| `Ctrl+↑` | 上一条（更早的）输入 |
| `Ctrl+↓` | 下一条（更新的）输入；到底时回到进入历史前正在输入的草稿 |

任何手动编辑都会退出历史浏览模式。

### `z` 的打开方式

```text
:z:code 项目        → 用 VSCode 打开（文件也可以，直接定位到它）
:z:yazi 项目        → 新终端窗口里用 yazi 打开所在目录
:z:pi   项目        → 新终端窗口里在目标目录启动 pi
```

### `z` 的路径写法

```text
:z                              → 空输入：列出 zoxide 的完整历史（按 rank 降序）
:z ~/Projects                   → ~ 展开为用户目录
:z "C:\Users\<你的用户名>\Projects" → 引号（单/双）自动去掉
:z file:///C:/Users/My%20Docs   → file:// 前缀 + %XX 解码
:z projects\term               → 半截路径，走 zoxide + Everything 补全
:z D:\Legacy\report.pdf        → 文件：在资源管理器里定位
```

空列表的第一行可能是**剪贴板**里的路径 / 网址（徽标 `clip`），它不抢默认选中 ——
想用它按一下 `↑`。`[general] clipboard = false` 可以关掉。

匹配顺序：**存在的完整路径 → zoxide 历史 → Everything 路径匹配**。
和终端的 `z` 命令**共用同一份 zoxide 数据库**：终端里跳过的目录这里能搜到，
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
- `Ctrl+Tab`（或 `Ctrl+O`）切「详细信息」：把每列拆成单独一行、缩进对齐到标题列，超宽同样省略 + 滚动，**不换行**；上下键仍然按条目移动（这段是 **TUI 面板**的行为；`tupa-gui.exe` 里同一快捷键切的是「两行卡片 / 单行密度」，见上面「全局热键」一节）
- 详细信息最后还会多一行**完整目标**：`web` 显示完整 URL、`ev`/`go` 显示完整路径、`run` 显示命令行（列里已经有的内容不重复）

```text
紧凑：     ▸ app   终端 (Windows Terminal)   wt
详细：     ▸ app   终端 (Windows Terminal)
                     wt
```

按键：

| 键 | 作用 |
|---|---|
| `↓` | 进列表模式并选中**下一项**（回车执行第 1 项 → ↓ 就是第 2 项） |
| `↑` | 翻**输入历史**（像终端）：更早一条；再按 `↓` 往回，`Esc` 退出 |
| `Enter` | 打开文件 / 启动程序（成功后自动退出，把屏幕让出去）；**开关 / 选项 / 数值行只应用新值，不退出** |
| `←` `→` | 开关 / 选项 / 数值行：调整本地值（输入光标不动），回车才写出 |
| `Ctrl+E` / `Alt+Enter` | 在文件资源管理器里定位（**app 也能用**：`.lnk` 会跟到目标 exe）；**成功后和回车一样退出面板并记入历史**，失败才留在面板里报错 |
| `Tab` | 循环补全命令名 / 修饰符（候选在输入框下方浮一层，竖排显示） |
| `Shift+Tab` | 进入 / 退出**选择模式**：`Tab` 下一个、`1`~`9` 向下移动 n 项 |
| `Ctrl+Tab` / `Ctrl+O` | **显示密度切换**：默认**两行卡片**（第一行标题 + 右侧状态/次数，第二行路径/分组），切到**单行对齐表**（字号小、同屏多一倍的条目）。`Ctrl+Tab` 在 Windows Terminal 里默认被占用，需要解绑；`Ctrl+O` 是备用键 |
| `Ctrl+D` | 收藏 / 取消收藏当前选中项（`★` 表示已收藏） |
| `Ctrl+↑` `Ctrl+↓` | 上一条 / 下一条历史输入 |
| `Ctrl+W` / `Ctrl+Backspace` | 删掉光标前的一个词 |
| `Shift+←` `Shift+→` / `Shift+Home` `Shift+End` | 选中文字；有选区时打字 / 退格 / 粘贴直接**替换选区** |
| `Ctrl+←` `Ctrl+→` | 按词移动光标（路径里的 `\` `.` `:` 也算分隔，能逐段跳） |
| `Ctrl+A` / `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | 全选 / 复制 / 剪切 / 粘贴（没选区时复制、剪切整行；多行粘贴会压成一行） |
| `Esc` | **逐层退**：输入历史 → 列表模式 → 清空输入 → 收成紧凑 → 隐藏面板（进程保留） |
| `Ctrl+Q` | 真正退出（销毁面板进程）。**`Ctrl+C` 现在是复制**，不再退出 |

### 输入区 / 列表区（焦点分离）

呼出面板后焦点在**输入框**，所有键都是输入（空格就是空格）。要操作列表时：

| 键 | 作用 |
|---|---|
| `↓` | **进列表模式**并选中下一项 |
| `↑` | 进列表模式 |
| `↑` `↓`（已在列表模式） | 移动选择 |
| `Home` / `End` | 选中**第一条 / 最后一条** |
| `Space` | 打开**当前行的菜单**（等价于鼠标右键） |
| `→` / `←` | 打开 / 关闭当前行的菜单 |
| `Enter` | 执行当前行 |
| `Tab` / `Shift+Tab` | 选择模式 / 详情（和以前一样） |
| `Esc` | **回到输入框**（不清空、不隐藏面板） |
| 打字 | 自动切回输入框，字照常打进去 |

列表模式下输入框是「失焦」的：提示符变暗、光标隐藏。这样空格才能当菜单键 ——
输入模式下空格仍然是参数分隔符（`:blog:new 我的 文章`）。

`Esc` 是逐层退：**输入历史 → 列表模式 → 输入框 → 收成紧凑 → 隐藏面板**（哪层在就退哪层）。

### 补全候选浮层（`[general] completion`，**默认关**）

2026-11-19 起默认关掉：实际用下来收益不大，还把 `↑↓` 两个键占住了。想要就在
`config.toml` 里写 `completion = true`（代码一行没删）。

打开后：输入 `:` / `@` 之后打命令名或修饰符（`@web:we`）会出现候选浮层（输入框下方的竖排列表）：

| 键 | 作用 |
|---|---|
| `Tab` | 循环候选并**立即套用**到输入框（老行为不变） |
| `↑` `↓` | 只移动高亮，**不改输入** |
| `Enter` | 套用当前高亮的候选（有候选时回车不会执行列表项） |
| 鼠标点击 | 直接套用该候选 |

状态栏右侧仍然显示光标停在哪个候选上、它是什么意思（`admin · 提权执行，会弹 UAC`）。

### 右键菜单

在列表任意一行上**右键**，菜单按这一行**实际能做的事**生成（做不到的项不会出现）：

| 条目 | 菜单项 |
|---|---|
| 应用（`:app` / `:all`）（**也包括开屏里的）** | 以管理员身份运行（弹 UAC）/ 打开所在位置 / 收藏 |
| 目录、文件（`:z` / `:ev` / 剪贴板条目） | 打开所在位置 / 收藏 |
| 已收藏的条目 | 取消收藏 / 打开所在位置 |
| 在 `history.json` 里有记录的条目 | 从最近记录移除 |

- 「从最近记录移除」只删 **tupa 自己的 `history.json`**；`:web` 那类条目的数据源在
  Edge 的浏览历史里，删不掉（所以文案是「最近记录」而不是「删除」）。
- 菜单打开时：`↑↓` 移高亮、`Enter` 执行、`Esc` 只关菜单（不隐藏面板）；
  滚轮 / 点别处 / 打字也会关掉它。
- 执行「管理员运行」「打开位置」后和回车一样**隐藏面板**；收藏 / 删除不隐藏，面板里给状态提示。

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
| `default_provider` | `all` | 不带 `:` 前缀时整行交给谁（`all` = 应用 + 历史混排；`app` 只搜应用） |
| `max_results` | `30` | 单个 provider 最多返回多少条 |
| `everything_path` | 自动探测 | `es.exe` 路径 |
| `prefill_colon` | `false` | 开屏输入框是否预填命令前缀（`true` = 一开就在命令模式；`false` = 空输入，但结果列表仍是命令列表） |
| `command_prefix` | `:` | 命令前缀，也是修饰符分隔符；想用 `@` 就改成 `"@"` |
| `language` | `zh` | 界面语言：`zh` / `en`（所有界面文案、命令描述、`--help` 都会跟着变） |
| `show_hints` | `true` | 右下角的快捷键提示是否显示（`false` 只留左侧状态） |
| `frecency` | `true` | 开屏历史与 `@app` 空查询按「使用频率 × 新鲜度」排序（关掉后：开屏按时间、`@app` 按配置顺序） |
| `clipboard` | `true` | 剪贴板里是存在的路径 / http(s) 网址时，开屏第一行多一条「跳过去」（徽标 `clip`，默认不选中） |

`[blog]` 段（`:blog` 用）：

| 键 | 默认 | 说明 |
|---|---|---|
| `dir` | `~/blog` | 博客仓库根目录（`source/_posts` 与 `package.json` 在这里） |
| `local_url` | `http://localhost:4000` | 本地预览（`pnpm server`，Hexo 默认端口） |
| `github_url` | `https://nitsugaoo.github.io` | GitHub Pages（用户名 `NitsugaoO`，域名里是**两个 o**） |
| `cloudflare_url` | `https://blog.wooyun.workers.dev` | Cloudflare Workers |
| `admin_url` | `http://127.0.0.1:5173` | `pnpm admin` 起来的管理页 |

`[glazewm]` 段（配合 [GlazeWM](https://github.com/glzr-io/glazewm) 窗口管理器）：

| 键 | 默认 | 说明 |
|---|---|---|
| `focus_browser` | `true` | 打开网页后，如果浏览器窗口在别的 workspace / 被 WM 隐藏，让 GlazeWM 切过去（`glazewm query workspaces` → `focus --workspace`，失败静默）。没装 GlazeWM 时自动忽略 |

为什么需要它：`open::that_detached` 只在浏览器里新建一个标签，窗口仍 `displayState=hidden`、
工作区也不切，面板一隐藏就什么都看不到。

## 架构

```
src/
├── main.rs              入口 + 事件循环 + --query/--check 调试模式
├── app.rs               状态机：输入解析、键位、补全、选择、与 worker 通信
├── ui.rs                纯渲染：分列布局 / 详细信息 / 选中行滚动 / 状态列
├── exec.rs              执行动作：打开文件 / 启动程序 / 定位 / 打开网址 + 本地服务编排
├── glazewm.rs           GlazeWM 集成：打开网页后把浏览器窗口切到前台
├── destlist.rs         「最近打开的文件夹」数据源：解复合文档 + DestList 流
├── explorer_watch.rs   实时记录资源管理器当前打开的目录（WinEventHook 事件驱动）
├── actions.rs           actions.toml 解析 + 脚本执行（超时）+ 内置开关（脚本在 scripts/*.ps1）
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
    ├── all.rs           all  —— 默认搜索：应用 + 历史混排（共用 app 的列表）
    ├── everything.rs    ev   —— 调用 es.exe + 本地相关性重排
    ├── web.rs           web  —— URL 判断 + Edge 历史（rusqlite）
    ├── hist.rs          hist —— 搜索命令历史（history.json：输入行 + 执行快照）
    ├── pi.rs            pi   —— pi 会话列表（标题来自 pi-sessions 索引，回退 jsonl）
    ├── pi_index.rs      pi 的标题来源：读 pi-sessions 的 index.sqlite（rusqlite）
    ├── favorites.rs     fav  —— 列出收藏 + 按类型过滤
    ├── commands.rs      `:`  —— 命令列表（也是补全的数据源）
    ├── blog.rs           blog —— 新建文章 / 本地预览 / 管理页 / 站点入口（文章创建的执行也在这）
    ├── go.rs            go   —— zoxide + Everything + 剪贴板
    ├── run.rs           run  —— 执行命令
    ├── clean.rs         :pi:clean 的实现（清理 pi 会话：归档 / 回收站 / 恢复，含执行）
    ├── switch.rs        set —— 开关 / 选项 / 数值（actions.toml）
    ├── config_items.rs  cfg  —— 列出配置项
    └── home.rs          home —— 开屏内容（收藏 + 最近命令；无关键字，不进命令列表）
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
- 全局热键默认 `Alt+`` `（`config.toml` 的 `[daemon] hotkey` 可改）；TUI 面板默认跑在
  **WezTerm** 里（`[daemon] terminal = "auto"`，装了就用），没装则退回 Windows Terminal。
  `Ctrl+Tab` 两种终端都自己占着，所以「详细信息」还准备了 `Ctrl+O`。
- WezTerm 面板靠 `--class tupa-launcher` 的**窗口类名**识别（全机唯一），比 WT 靠标题前缀稳；
  WezTerm 的窗口标题会跟着前台进程变，不能拿去匹配。面板用 `--config` 内联覆盖了用户的
  `wezterm.lua`：关 tab 栏、padding 归零、背景不透明。
- 退回 WT 时需要 `config.toml` 里有个叫 `tupa-launcher` 的 profile；两种宿主都做不到无边框，
  "居中 / 62%×55%" 由 GlazeWM `window_rules` 负责。

## 路线图

- [x] **v0.1** `app` + `ev` 两个 provider、主题可覆盖、自定义 app 列表、异步查询
- [x] **v0.2** 守护进程 + `Win+S` 全局热键 + 居中面板窗口
- [x] 集成 GlazeWM `window_rules`（浮动/居中/尺寸交给 WM）
- [ ] 主题并入 `~/Themes/generator/generate.py` 流水线（在 `~/.config/tupa-launcher/theme.toml` 里生成）
- [ ] 智能识别并打开文件夹（`PathPal` 的 "字面 → 祖先回退 → Everything 兜底" 三级策略）
- [x] `hist`（历史记录搜索，独立 provider）
- [ ] 使用频率统计，把常用项自动排前（frecency）
- [x] **v0.3** `web`（Edge 历史）+ `fav`（收藏）+ 命令历史（`Ctrl+↑/↓`）
- [x] **v0.4** 分列显示（份数/最短/最长、选中行滚动）+ 详细信息模式 + `:` 命令前缀 + Tab 补全 + 选择模式
- [x] **v0.6** 中英文（`general.language`）+ 文案精简；`:go:code` / `:go:yazi` / `:go:pi`；`:run` 默认 PowerShell；状态栏显示位置与后台忙碌
- [x] **v0.5** `:web` 时间范围（默认一周 / month / all）+ 搜不到才直接打开；`:run` 默认新窗口执行；命令前缀可配置 + 去掉同义修饰符；`:new` 新建文件
- [x] **v0.7** `:set` 应用内开关 / 选项 / 数值（`actions.toml` 驱动 + 内置深色模式 / 亮度 / 电池养护 / 节能 / 保持唤醒 / 更新订阅）
- [x] **v0.9** `:blog`（新建文章 / 本地预览 / 管理页 / 站点入口，配置在 `[blog]` 段；取代旧的 `:new`）
- [x] **v0.8** 自建无边框窗口，脱离 Windows Terminal（设计见 [`docs/05-native-window.md`](docs/05-native-window.md))
      —— 旧 winit 实现**已于 2026-11-17 整体删除**（依赖重、能力天花板低），正换成 **GPUI** 重写。
      面板现在是**独立程序 `tupa-gui.exe`**（自带托盘图标 + 全局热键，不依赖守护进程）：
      `cargo build --release --features gui` 才产出，默认构建不含。
      原来的守护进程 + TUI 仍可手动跑，但开机自启已改为 GUI。
      面板默认宿主是 WezTerm（`[daemon] terminal`），`WindowsTerminal` 只在找不到 WezTerm 时兜底
      （开工说明见 [`docs/07-shell-handover.md`](docs/07-shell-handover.md)，当前进度见 [`STATE.md`](STATE.md)）
