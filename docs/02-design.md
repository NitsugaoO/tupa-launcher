# 设计说明

> 记录 v0.1 的几个关键决策和理由。市场调研见 [`01-research.md`](01-research.md)。

## 1. 为什么是 TUI

需求里最硬的一条是**"功能类型 + 空格 + 具体操作"**（`ev 报告`、`app code`）——
这是 CLI 的心智模型，不是 GUI 搜索框的心智模型。

现成的 GUI 启动器（Flow Launcher / Wox / Keypirinha…）虽然也支持 action keyword，
但有两个绕不开的问题：

1. 主题是它们自己的格式 → 无法复用 `~/Themes` 的 Everforest 角色表；
2. 扩展仍然活在别人的生态里，加一个自己的小功能要跟着人家的规矩走。

TUI 则天然贴合：**终端本来就是这一套语法的主场**，
而且主题、按键、数据源全都由我们自己控制。

代价也要认：TUI 不能像 CmdPal 那样"从屏幕顶部飘出来"，
必须靠 Windows Terminal 的 Quake 模式 + `globalSummon` 全局热键来补这个体验（见 §6）。

## 2. 为什么是 Rust + ratatui

| 候选 | 否决理由 |
|---|---|
| Python + Textual | 冷启动 0.3–1s，与"按热键即现"直接冲突 |
| C# + Spectre/Terminal.Gui | 会 C#，但 TUI 库不如 ratatui 灵活，项目结构也重 |
| Go + bubbletea | 很合适，纯属个人偏好上的第二选择 |

Rust 的选择理由：
- release 冷启动 **~10ms 级**，单二进制，无运行时；
- ratatui 是当前 TUI 生态事实标准（yazi、gitui 都用）；
- 项目规模小（约 900 行），不涉及复杂所有权，适合作为第一个 Rust 项目。

## 3. 交互模型

```
输入行 ──┬─ "app vs"   → (provider=app, args="vs")
         ├─ "ev 报告"  → (provider=ev,  args="报告")
         └─ "vs"       → (default_provider, args="vs")
```

解析规则刻意做得**极简**：第一个空格切一刀，前半段查 provider 关键字表。
没命中就把整行当参数交给默认 provider —— 这样"想搜什么直接打"永远不会失败，
不需要用户记住任何前缀。

`Tab` 把命中的关键字补全成 `关键字 + 空格`，完成"从裸输入到显式语法"的过渡。

## 4. 并发模型：一个 worker + 丢弃旧请求

```
主线程                              worker 线程
  │                                    │
  │ 每次输入变化：                        │
  ├── Cmd::Query{gen, provider, args} ──►│
  │                                    ├─ 先把队列里排着的旧请求全部丢掉，
  │                                    │  只执行最后一条（防止 es.exe 堆积）
  │◄── Evt::Results{gen, items} ────────┤
  │   gen != 当前 gen → 丢弃             │
  │                                    │
  └─ 30ms 轮询一次 crossterm 事件 ────────┘
```

- **为什么不用 async runtime**：只有一个阻塞数据源，一个线程 + channel 就够，
  省掉 tokio 依赖和 1MB+ 的运行时体积。
- **为什么要 generation**：worker 执行期间用户可能又敲了几个字，
  旧结果到达时不能覆盖新结果。
- **为什么 worker 只执行最后一条**：`es.exe` 单次 10–50ms，
  快速打字若逐条串行执行会排出几百毫秒的延迟。

## 5. `ev` 的相关性重排

Everything 只按名称/路径排序，不区分"我的文件"和"系统文件"，
直接拿前 30 条会得到一堆 `C:\Windows\WinSxS\...`。

所以流程改成：

```
es.exe -n (max*8, 上限 400)   ← 多拉候选
        ↓
本地打分排序（见 providers/everything.rs 的 score()）
        ↓
截断到 max
```

打分直觉（不是精密的排序算法，够用就行）：

| 信号 | 分值 |
|---|---|
| 在用户主目录下 | **+120** |
| 文件名 == 查询词，或以 `查询词.` 开头（`curl` → `curl.exe`） | **+260** |
| 文件名以查询词开头 / 包含查询词 | +120 / +60 |
| 是 `.exe` `.lnk` `.cmd` `.bat` `.ps1`（比同名目录更可能是目标） | +60 |
| `\winsxs\`、`\appdata\local\assembly\`、`\node_modules\`、`\site-packages\`… | −30 ~ −150 |
| 路径每深一层 | −4 |
| 路径每长 10 个字符 | −1 |

`ext:pdf`、`folder:` 这类 Everything 语法不参与打分（它们不是"词"）。

## 6. 全局热键：为什么是自己写守护进程

最初打算用 Windows Terminal 的 Quake 模式 / `globalSummon`，实测后放弃，两个原因：

1. **`RegisterHotKey` 抢不到 Win 键组合**。
   [微软文档](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey)明确写着
   “涉及 Windows 键的快捷键由操作系统保留”，而 shell 已经用同一个 API 注册了 Win+S。
   PowerToys 能做到是因为它走的是**低级键盘钩子**，不是 `RegisterHotKey`。
2. **WT 1.24 的 `GlobalSummonAction` 只有 5 个参数**
   （`name` / `desktop` / `monitor` / `dropdownDuration` / `toggleVisibility`，见官方 schema），
   **没有 `commandline` 也没有 `profile`** —— 召唤出来的窗口只会跑默认 profile（一个 PowerShell），
   没法让它跑 term-palette。

所以 `--daemon` 自己做两件事：

```
低级键盘钩子（WH_KEYBOARD_LL）              主线程消息循环
  Win+S 按下                                  │
  ├─ PostThreadMessageW ────────────────────►├─ 找面板窗口（类名 + 标题）
  └─ return 1（吞掉按键，系统搜索不会弹出）   │   ├─ 找到 → 可见且前台？隐藏 : 显示+置前
                                              │   └─ 没有 → wt -p term-palette 启动，
                                              │            等窗口出现后用 SetWindowPos 摆到工作区中央
```

窗口靠 **类名 + 标题** 识别：`CASCADIA_HOSTING_WINDOW_CLASS` +
标题包含 `term-palette`（由 WT profile 的 `tabTitle` 固定）。
面板退出 → 窗口消失 → 下次热键重新拉起，正好是“呼之即来、用完即走”。

### 踩过的坑

- **`FreeConsole()` 之后 `Command::spawn` 报 “句柄无效 (os error 6)”**：
  守护进程脱离控制台后 stdin/stdout/stderr 失效，子进程继承这些句柄会让
  `CreateProcess` 直接失败。修法：`util::detached_command()` 统一给子进程
  指定 `Stdio::null()`。
- **hwnd 不能用 `AtomicU32` 存**：64 位下 `HWND` 是指针，会截断。
- **枚举窗口而非精确匹配标题**：WT 的窗口标题可能是 `term-palette`、
  `term-palette - PowerShell` 等多种形式。

## 7. 编码坑（已解决）

`es.exe` 把 stdout 重定向到管道时，输出的是**系统 ANSI 代码页**（中文系统 = GBK/936），
而 TUI 是 UTF-8 渲染的。直接用 `String::from_utf8_lossy` 会让中文路径变成一串 `�`。

处理：先按 UTF-8 严格解码，失败再回退 GBK（`everything.rs::decode_output`）。
反方向（把中文查询词传给 es.exe）没问题：Windows 在 `CreateProcessW` 里会自动转成 ANSI。

## 8. 与 Everforest 主题体系的对接（待做）

现在程序的默认色是硬编码的 Everforest(hard)，与 `tokens.yaml` 一致但**不联动**。
计划：在 `~/Themes/generator/` 里加一个 `gen_term_palette.py`，
和生成 VSCode / yasb 主题一样，把 `tokens.yaml` 渲染成
`~/.config/term-palette/theme.toml`，从而做到"改一次色板，这个面板也跟着变"。

需要的映射（角色名 → 我们的字段）：

```
背景→background  面板→panel  浮层→overlay  边框→border
正文→fg          次要→muted  更淡→dim      最淡→faint
强调→accent      正常→good   注意→warn     危险→bad
```
