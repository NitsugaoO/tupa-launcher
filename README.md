# Tupa Launcher

> 一个**命令式**启动器：`功能 + 空格 + 参数`，跑在终端里。
>
> ```text
> ❯ app vs          → 从列表里启动 Visual Studio Code
> ❯ ev 报告 2026    → 用 Everything 找这个文件，回车直接打开
> ```
>
> 状态：**v0.2 可用** · Rust + ratatui · 替代 PowerToys Command Palette
> 调研与可行性论证见 [`docs/01-research.md`](docs/01-research.md)，设计决策见 [`docs/02-design.md`](docs/02-design.md)
> 待实现功能的交接设计见 [`docs/03-roadmap.md`](docs/03-roadmap.md)

## 快速开始

```bash
cd ~/Projects/tupa-launcher
cargo build --release
./target/release/tupa-launcher.exe
```

调试用（不进界面，直接把结果打到 stdout）：

```bash
./target/release/tupa-launcher.exe --query "ev curl"
./target/release/tupa-launcher.exe --query "app vsc"
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

| 输入 | 行为 |
|---|---|
| `app <名字>` | 从自定义列表里启动应用（模糊匹配）；`.lnk` 会递归解析到目标 exe 显示 |
| `ev <关键词>` | Everything 搜文件，**支持原生语法**：`ev ext:pdf 报告`、`ev folder:Projects` |
| `run <命令>` | 执行命令（后台，不弹窗） |
| `go <路径>` | 跳到目录 / 定位文件；**空输入时自动读剪贴板** |
| `cfg [关键词]` | 列出配置项（行号 / 当前值 / 说明），**回车在 VSCode 里跳到对应行** |
| `<名字>` | 不带关键字时走默认 provider（当前是 `app`） |

### 什么时候算“命令”

**只有关键字后面跟了空格、或带了冒号，才算命令**：

| 输入 | 行为 |
|---|---|
| `ev` | ❌ 不算命令 → 当普通搜索词，能搜到 **Everything 这个应用** |
| `ev ` / `ev: ` | ✅ 算命令 → 进入 ev provider |
| `ev 123` / `ev: 123` / `ev:123` | ✅ 参数是 `123` |
| `run:admin ipconfig` | ✅ `admin` 是修饰符，`ipconfig` 是参数 |
| `report` | ❌ 不是关键字 → 默认 provider |

命令关键字在输入框里会**高亮成黄色**，一眼能看出当前在哪个 provider。
`:` 后面只认 `admin` / `elevated` / `sudo` 三个修饰符，其他内容会当作参数。

### 修饰符语法（`:flag`）

关键字后面可以用 `:` 加修饰符，多个用 `:` 继续接：

```text
run:admin ipconfig        → 提权执行（弹 UAC）
app:admin cmd             → 以管理员身份启动
run:admin:verbose foo     → 多个修饰符（目前只认 admin / elevated / sudo）
```

没命中关键字的 `:` 修饰符不生效（整行当参数交给默认 provider）。

### `go` 的路径写法

```text
go ~/Projects                  → ~ 展开为用户目录
go "C:\Users\<你的用户名>\Projects"  → 引号（单/双）自动去掉
go file:///C:/Users/My%20Docs  → file:// 前缀 + %XX 解码
go projects\term              → 半截路径，走 zoxide + Everything 补全
go                            → 空输入，读剪贴板
go D:\Legacy\report.pdf        → 文件：在资源管理器里定位
```

匹配顺序：**存在的完整路径 → zoxide 历史 → Everything 路径匹配**。
这里和终端的 `z` 命令**共用同一份 zoxide 数据库**：终端里跳过的目录这里能搜到，
这里打开过的目录也会写回去（`zoxide add`）。

按键：

| 键 | 作用 |
|---|---|
| `↑` `↓` / `Ctrl+P` `Ctrl+N` | 移动选择 |
| `Enter` | 打开文件 / 启动程序（成功后自动退出，把屏幕让出去） |
| `Ctrl+E` / `Alt+Enter` | 在文件资源管理器里定位（**app 也能用**：`.lnk` 会跟到目标 exe） |
| `Tab` | 补全关键字（`ev` → `ev `） |
| `Ctrl+W` | 删除一个词 |
| `Esc` | 有输入时**清空输入**；没输入时**隐藏面板**（进程保留，下次唤起是瞬时的） |
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

## 架构

```
src/
├── main.rs              入口 + 事件循环 + --query/--check 调试模式
├── app.rs               状态机：输入解析、键位、选择、与 worker 通信
├── ui.rs                纯渲染（无状态）
├── exec.rs              执行动作：打开文件 / 启动程序 / 定位
├── daemon.rs            常驻守护进程：低级键盘钩子 + 面板窗口管理（Windows）
├── config.rs            config.toml 加载
├── theme.rs             主题：内置默认 + 两层覆盖
├── util.rs              子进程 / 编码 / PATH 查找
└── providers/
    ├── mod.rs           Provider trait、Item/Action、解析、**后台 worker**
    ├── apps.rs          app —— 自定义应用列表 + 模糊匹配
    └── everything.rs    ev  —— 调用 es.exe + 本地相关性重排
```

### 加一个新功能（比如 `go <目录>` 走 zoxide）

1. 在 `src/providers/` 下新建 `zoxide.rs`，实现 `Provider`（`keyword` / `description` / `query` 三个方法）；
2. 在 `providers/mod.rs` 的 `build()` 里 push 进去。

就这样 —— 不用打包、不用重装、不用改 UI。`query()` 在后台线程里跑，可以放心阻塞。

### 为什么不卡

查询走一个单线程 worker：主线程只发"最新一次请求"，worker 会**丢弃排队中的旧请求**，
所以快速打字不会堆积一堆 `es.exe` 进程，也不会让 UI 掉帧。
结果带回一个自增序号，序号过期的直接扔掉。

## 已知限制

- 只在 Windows 上验证过（`exec.rs`、`daemon.rs` 和 `everything.rs` 有 Windows 专属逻辑，其余跨平台）。
- `es.exe` 通过管道输出的是**系统 ANSI 代码页**（中文系统 = GBK），代码里有 UTF-8 → GBK 的回退解码。
- 守护进程占用 `Win+S`：这会屏蔽掉系统自带的搜索面板。不想要就删掉启动文件夹里的快捷方式。
- 面板窗口是 Windows Terminal 窗口，做不到无边框浮层；“居中”是守护进程用 `SetWindowPos` 摆出来的。

## 路线图

- [x] **v0.1** `app` + `ev` 两个 provider、主题可覆盖、自定义 app 列表、异步查询
- [x] **v0.2** 守护进程 + `Win+S` 全局热键 + 居中面板窗口
- [x] 集成 GlazeWM `window_rules`（浮动/居中/尺寸交给 WM）
- [ ] 主题并入 `~/Themes/generator/generate.py` 流水线（在 `~/.config/tupa-launcher/theme.toml` 里生成）
- [ ] `go <目录>`（zoxide）、`run <命令>`、`hist`（历史记录搜索）
- [ ] 智能识别并打开文件夹（`PathPal` 的 "字面 → 祖先回退 → Everything 兜底" 三级策略）
- [ ] 使用频率统计，把常用项自动排前（frecency）
