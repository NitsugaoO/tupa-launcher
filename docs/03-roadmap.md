# 待实现功能设计（交接文档）

> 写给下一个开发会话：当前项目已经能跑（v0.2），本文档描述**接下来要做的三件事**
> 的设计方案、踩坑记录和测试方法。
>
> 日期：2026-10-17

## 已完成（本轮）

- **改名**：`term-palette` → **Tupa Launcher**
  - 目录：`~/Projects/tupa-launcher`
  - 包名 / exe：`tupa-launcher` / `tupa-launcher.exe`
  - 配置目录：`~/.config/tupa-launcher/`（已从旧的迁移过来）
  - WT profile 名 / tabTitle：`tupa-launcher`
  - 自启：启动文件夹里的 `tupa-launcher-daemon.lnk`

## 待实现

### 1. `web` —— 打开网页 / 搜浏览器历史

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
- **坑 2**：`last_visit_time` 是 Windows FILETIME（1601-01-01 起、单位 100ns），
  不是 Unix 时间。如果需要显示日期：
  `unix_secs = last_visit_time / 10_000_000 - 11_644_473_600`
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

### 2. `fav` —— 收藏

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

### 3. 命令历史（`Ctrl+↑` / `Ctrl+↓`）

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
├── app.rs               状态机：输入解析、键位、选择、与 worker 通信
├── ui.rs                渲染（纯函数）
├── exec.rs              执行动作：打开 / 启动 / 定位 / 提权 / 编辑器跳转
├── config.rs            config.toml 加载
├── theme.rs             主题（内置 Everforest + 两层覆盖）
├── util.rs              .lnk 解析 / PATH 查找 / 编码 / 面板窗口
├── daemon.rs            常驻：RegisterHotKey + Copilot 键钩子 + 窗口显隐
└── providers/
    ├── mod.rs           Provider trait / Item / Action / 解析 / 后台 worker
    ├── apps.rs          app
    ├── everything.rs    ev
    ├── go.rs            go（zoxide + Everything + 剪贴板）
    ├── run.rs           run
    ├── config_items.rs  cfg
    └── （待加）web.rs / favorites.rs
```

### 加一个 provider 的步骤

1. `src/providers/xxx.rs` 实现 `Provider`：
   ```rust
   fn keyword(&self) -> &str;        // 触发词
   fn description(&self) -> &str;    // 状态栏描述
   fn query(&self, args: &str, flags: &[String]) -> Vec<Item>;  // 后台线程执行，可阻塞
   ```
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
| WT 窗口识别 | 标题可能带后缀 | `util::find_panel` 用「类名 + 标题包含」 |
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
