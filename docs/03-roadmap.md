# 待实现功能设计（交接文档）

> 写给下一个开发会话：当前项目已经能跑（v0.7，已有 `:switch`），本文档描述**接下来要做的三件事**
> 的设计方案、踩坑记录和测试方法。
>
> 日期：2026-10-17（v0.3 已实现本文档中的三个功能，实现备注见各节末尾）
>
> **接着开发的入口请先看 [`06-handover.md`](06-handover.md)**（当前状态、约定、坑、下一步）。

## 已完成（本轮）

- **v0.4 架构改动**（分列 + 命令前缀 + 补全 + 选择模式）：
  1. **分列显示**：`Item { kind, columns: Vec<String>, action }`，每列宽度由
     `Provider::columns()` 用 `Column { weight, min, max }` 声明，
     `ui::layout_columns()` 按份数分配、夹到 `[min, max]`、窄屏按 `min` 比例压缩。
  2. **省略 + 滚动**：非选中行超宽用 `…` 截断（按 `unicode-width` 算显示宽度）；
     选中行每一列左右滚动（`ui::scroll_offset`，2 帧走 1 列，到底停 24 帧）。
     滚动步进来自 `App::tick`（main 循环每帧 +1）。
  3. **详细信息模式**（`Ctrl+Tab` / `Ctrl+O`）：每列拆成单独一行，缩进对齐到标题列，
     超宽仍然省略 + 滚动（不换行）；多行 `ListItem`，上下键仍按条目移动。
  4. **命令前缀改成 `:`**：`providers::parse()` 取代 `resolve()` / `command_prefix_end()`，
     返回 `Parsed::{Default, Command, CommandList}`。不带 `:` 的整行走默认 provider。
  5. **命令列表 provider**（`src/providers/commands.rs`，keyword = `:`）：
     开屏输入 `:` 时就列出所有命令；回车是 `Action::FillInput`，只把输入补成
     `:命令 `，不执行任何动作（也不进历史）。
  6. **Tab 补全**：`App::compute_completion()` —— 光标前的词里最后一个 `:` 之后
     补命令名或修饰符，候选显示在输入行下面一行，Tab 循环替换。
     出现空格（进入参数区）就停止补全。
  7. **选择模式**（`Shift+Tab`）：`Tab` 循环下一项，`1`~`9` 相对下移 n 项，`Esc` 退出。
  8. 收藏文件改成 `columns` 数组；旧的 `title` / `subtitle` 格式读取时自动转换。
  9. `Provider::bare_trigger()` 已删除（`:` 前缀后不再需要）。

- **v0.3 新增**：`web` / `fav` / 命令历史（`Ctrl+↑` / `Ctrl+↓`）
  - 新文件：`src/providers/web.rs`、`src/providers/favorites.rs`、`src/favorites.rs`、`src/history.rs`
  - 依赖新增：`rusqlite`（bundled）、`serde_json`
  - 与原设计的差异：
    1. `fav` **单独一个词也触发**（`Provider::bare_trigger()`），因为收藏列表需要“打 `fav` 回车”就能看；
       而 `ev` / `app` 保留原语义（单打一个词是搜索词）。
    2. 历史匹配改成「标题模糊匹配 + URL 只对去掉 query/fragment 的部分做子串匹配」，
       否则 `?q=...&cvid=...` 里的随机串会让任何关键词都命中（实测 `rust` 会召回一堆无关页面）。
    3. 新增 `Kind::Web`（徽标 `web`），收藏可按 `fav:web` 过滤。
    4. 列表里已收藏的条目带 `★`（`App::is_favorite` + 内存快照，避免每帧读文件）。
    5. 历史/收藏文件写入用「临时文件 + rename」原子替换。

- **改名**：`term-palette` → **Tupa Launcher**
  - 目录：`~/Projects/tupa-launcher`
  - 包名 / exe：`tupa-launcher` / `tupa-launcher.exe`
  - 配置目录：`~/.config/tupa-launcher/`（已从旧的迁移过来）
  - WT profile 名 / tabTitle：`tupa-launcher`
  - 自启：启动文件夹里的 `tupa-launcher-daemon.lnk`

## 原计划（v0.3 已全部实现，保留设计记录）

### 1. `web` —— 打开网页 / 搜浏览器历史（✅ 已实现）

**输入**

```text
web github.com              → 像 URL，直接用浏览器打开
web https://example.com/a   → 同上
web rust async              → 不像 URL，搜 Edge 历史记录
```

**判断规则**（简单够用）

- 以 `http://` / `https://` 开头 → URL
- 无空格，且匹配 `^[\w-]+(\.[\w-]+)+(/.*)?$`（含至少一个点）→ 当作 URL 补 `https://` 前缀
- 其他 → 搜历史

**Edge 历史读取**

- 位置：`%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\History`
  - 多 profile 时还有 `Profile 1\History`、`Profile 2\History`，建议全部扫一遍
- **坑 1**：Edge 运行时该文件被独占锁住 → **必须先复制到临时目录**再用 SQLite 打开
  ```rust
  let tmp = std::env::temp_dir().join("tupa-edge-history.sqlite");
  std::fs::copy(&history_path, &tmp)?;
  ```
- **坑 2**：`last_visit_time` 是 Chromium 时间戳（1601-01-01 起、单位**微秒**），
  不是 Unix 时间。实测单位是微秒（不是网上常说的 100ns）：
  `unix_secs = last_visit_time / 1_000_000 - 11_644_473_600`
- 表结构（实测 Edge/Chromium 通用）：
  ```sql
  SELECT url, title, visit_count, last_visit_time
  FROM urls
  ORDER BY last_visit_time DESC
  LIMIT 3000
  ```
- **建议**：SQL 只做「取最近 N 条」，关键词过滤放在 Rust 侧做
  （避免拼 SQL 注入，也能用上已有的 `fuzzy-matcher`）

**依赖**

```toml
rusqlite = { version = "0.3x", features = ["bundled"] }
```

`bundled` 会从源码编译 SQLite，首次编译多 1~2 分钟，之后增量很快。

**新增代码**

- `src/providers/web.rs`：`Provider` 实现
- `Action::OpenUrl(String)`：在 `exec.rs` 里用 `open::that_detached(url)` 打开
- `Item` 的 subtitle 建议显示 `域名 · 访问 N 次`
- 在 `providers/mod.rs::build()` 里注册

---

### 2. `fav` —— 收藏（✅ 已实现）

**交互**

| 操作 | 行为 |
|---|---|
| `Ctrl+D`（在任意结果上） | 收藏 / 取消收藏当前选中项 |
| `fav` | 列出所有收藏 |
| `fav:app` | 只列出 app 类型的收藏 |
| `fav` 里 `Ctrl+D` | 从收藏里移除 |

**存储**

`~/.config/tupa-launcher/favorites.json`：

```json
[
  {
    "kind": "App",
    "title": "Visual Studio Code",
    "subtitle": "code",
    "action": { "Launch": { "command": "code", "args": [], "elevated": false } }
  }
]
```

**需要改的地方**

1. 给 `Kind` 和 `Action` 加 `#[derive(Serialize, Deserialize)]`
   （`providers/mod.rs`；`PathBuf` 自带 serde 支持）
2. `Item` 也加 serde（或者单独定义一个 `Favorite` 结构体，更干净）
3. 新建 `src/favorites.rs`：
   - `load() -> Vec<Item>` / `save(&[Item])`
   - `toggle(item) -> bool`（返回是否已收藏）
   - 去重键：`(kind, title, action)`，`Action` 需要 `PartialEq`
4. `App` 里加 `favorites: Vec<Item>` 和 `Ctrl+D` 的处理
5. 新建 `src/providers/favorites.rs`：
   - `query(args, _flags)`：`args` 为空列全部；`args = "app"` / `"cfg"` / `"dir"` 等按 `Kind` 过滤

**注意**

- `Ctrl+D` 在终端里通常表示 EOF，但 TUI 自己处理按键，不冲突
- 收藏的是「Item 快照」，不追踪原条目的变化（比如 `ev` 结果路径失效就失效了）——
  这对收藏来说可以接受
- 状态栏应给反馈：「已收藏：xxx」/「已取消收藏：xxx」

---

### 3. 命令历史（`Ctrl+↑` / `Ctrl+↓`）（✅ 已实现）

**行为**

- `Ctrl+↑` 上一条输入（更早），`Ctrl+↓` 下一条（更新）
- 到底/到头时停住，或回到「当前正在输入的内容」

**记录时机**

在 `App::execute()` 成功执行动作后，把 `self.input` 存进历史
（失败的不存，避免污染）。也建议记录 `Esc` 隐藏前的内容？—— 不需要。

**存储**

`~/.config/tupa-launcher/history.json`：

```json
["app vs", "ev 报告", "go ~/Projects"]
```

- 去重（新的放最前，重复的移到最前）
- 上限 200 条

**实现**

```rust
pub struct App {
    history: Vec<String>,      // 最新的在前
    history_cursor: Option<usize>, // None = 正在编辑新输入
    draft: String,             // 进入历史浏览前的输入，用于 Ctrl+Down 回到原位
}
```

- `Ctrl+Up`：`cursor = min(cursor + 1, len-1)`，`input = history[cursor]`
- `Ctrl+Down`：`cursor` 减到 `None` 时恢复 `draft`
- 任何普通输入（字符/删除）都把 `cursor` 重置为 `None`

---

## 代码结构速查

```
src/
├── main.rs              入口 / 事件循环 / 调试参数
├── app.rs               状态机：输入解析、键位、选择、与 worker 通信、历史浏览
├── ui.rs                渲染（纯函数）：分列布局 / 详细信息 / 滚动
├── i18n.rs              中英文案
├── exec.rs              执行动作：打开 / 启动 / 定位 / 提权 / 编辑器跳转 / 打开网址
├── config.rs            config.toml 加载
├── favorites.rs         收藏：favorites.json 读写 + toggle
├── history.rs           命令历史：history.json 读写
├── theme.rs             主题（内置 Everforest + 两层覆盖）
├── util.rs              .lnk 解析 / PATH 查找 / 编码 / 面板窗口
├── daemon.rs            常驻：RegisterHotKey + Copilot 键钩子 + 窗口显隐
└── providers/
    ├── mod.rs           Provider trait / Item / Action / 解析 / 后台 worker
    ├── apps.rs          app
    ├── everything.rs    ev
    ├── go.rs            go（zoxide + Everything + 剪贴板）
    ├── web.rs           web（Edge 历史 + rusqlite）
    ├── favorites.rs     fav
    ├── commands.rs      命令列表（keyword = `:`）
    ├── new.rs           new
    ├── run.rs           run
    └── config_items.rs  cfg
```

### 加一个 provider 的步骤

1. `src/providers/xxx.rs` 实现 `Provider`：
   ```rust
   fn keyword(&self) -> &str;        // 触发词
   fn description(&self) -> &str;    // 状态栏描述
   fn query(&self, args: &str, flags: &[String]) -> Vec<Item>;  // 后台线程执行，可阻塞
   ```
   （可选）`fn columns(&self) -> &'static [Column]`：声明各列的份数 / 最短 / 最长。
2. `providers/mod.rs::build()` 里 `list.push(Box::new(xxx))`
3. 需要新动作时：`Action` 加变体 + `exec.rs::run` 加分支

## 开发约定

- **provider 的 `query` 在后台线程跑**，可以放心做阻塞 IO（es.exe、SQLite、读文件）
- **不要在主线程做耗时操作**：`App::reveal_selected` 里的 `.lnk` 解析是例外（按键触发、可接受）
- 用户可见的字符串用中文
- 提交前跑 `cargo clippy --release`，保持零警告
- 改完记得 `cargo build --release` 并重启守护进程（它占着 exe）：
  ```bash
  taskkill //IM tupa-launcher.exe //F
  cargo build --release
  powershell -NoProfile -Command "Start-Process (Join-Path \$env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup\tupa-launcher-daemon.lnk')"
  ```

## 已知的坑（别重踩）

| 坑 | 现象 | 正确做法 |
|---|---|---|
| `explorer /select` 的引号 | 定位跳到 `D:\Documents` 之类的默认文件夹 | 必须 `raw_arg("/select,\"路径\"")`，引号只包路径 |
| `es.exe` 输出编码 | 中文路径变 `�` | `util::decode_output`（UTF-8 优先，回退 GBK） |
| daemon `FreeConsole` 后 spawn | `Ok` 但不创建窗口 / 句柄无效 | 用 `util::detached_command`（`Stdio::null()`） |
| 低级键盘钩子 | 300ms 超时，主线程一忙就漏键 | 已弃用钩子抢 Win+S；改用 `RegisterHotKey` |
| 吞修饰键抬起 | Win/Shift 卡死，全大写、点任务栏开新窗口 | **永远不要吞修饰键的抬起事件** |
| WT 窗口识别 | 标题可能带后缀 | `util::find_panel` 用「类名 + 标题前缀」；**不能 contains**，否则 pi 的 `π - tupa-launcher` 会被误判 |
| 面板冷启动慢 | 重开 WT 要 400~700ms | `Esc` 是隐藏不是退出（进程常驻） |

## 调试入口

```bash
tupa-launcher.exe --query "app code"     # 打印 provider 结果（不进 TUI）
tupa-launcher.exe --check                # 检查配置里每个 app 能否解析
tupa-launcher.exe --resolve "C:\x.lnk"   # 看 .lnk / 命令解析成什么
tupa-launcher.exe --reveal "C:\x.exe"    # 测「在资源管理器里定位」
tupa-launcher.exe --daemon --debug       # 守护进程 + 详细日志
```

守护进程日志：`%TEMP%\tupa-launcher-daemon.log`
