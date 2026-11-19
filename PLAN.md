# PLAN —— 六项改造（2026-11-19 调研）

> 基线：`code 27c5494`，工作区干净。
> 本文写「现状结论 + 改哪里 + 怎么验」。做完一项勾一项；会话状态仍回写 `STATE.md`。
> 每项都可独立落地，**一项一项来**。

## 0. 调研数据（本机实测，2026-11-19）

| 测量 | 结果 |
|---|---|
| `--query "@pi"` | 0.99 / 1.01 / 1.06 s |
| `--query "@hist"` | 1.42 s |
| `--query "@app"` | 0.61 s |
| `--query ""`（开屏 home） | 0.49 s |
| 读 175 个 jsonl 的前 80 行 + JSON 解析 | **0.683 s / 88.4 MB** |
| 同一批文件只 `stat` | 0.0136 s |
| pi 会话规模 | 175 个 jsonl / **435 MB**；归档 254 个 |
| pi-sessions 索引 | 241 行（157 有 `session_name`，241 条都有 `first_user_prompt`），**0 条归档** |
| zoxide | 0.10.0，db 在 `%LOCALAPPDATA%\zoxide\db.zo`（1435 B、22 条） |
| `zoxide query --list --score` | ~0.05 s |

---

## 1. 去掉 `@pi`；clean 抽成独立 HTML（交 my-pi-plugins 做 `/clean`）

### 现状

- `@pi` 列表慢的根因：`src/providers/pi.rs::read_meta` 对**每个**会话文件都 `File::open` + 解析前 80 行
  （上面 0.683 s 那一行），而 `index.sqlite` 里 `session_name` / `first_user_prompt` / `cwd` /
  `message_count` / `modified_ts` 全都有 —— 索引只用来覆盖标题，没省掉任何 I/O。
- clean 的规则（`src/providers/clean.rs`，456 行）：
  - 窗口默认 **14 天**（`:clean:week` 7 / `:clean:month` 30），窗口内 `Protect`
  - 收藏 `Protect`；否则 `keywords 非空 || messages >= 8` → **Archive**，其余 → **Trash**
  - Archive / Restore 都是 `fs::rename`（可逆，恢复重名加 `-restored-HHmmss`）；Trash 走 `trash` CLI
  - 入口是 `@pi:clean` / `@pi:cleanall` / `@pi:clean:restore`
- 依赖点（删 `@pi` 要一起清，`rg` 已列全）：
  `providers::build()`、`Kind::Pi` / `Kind::Clean`（badge / columns / from_badge）、
  `Action::CleanSession` / `CleanBatch`、`CleanMode` / `CleanOp`、`exec.rs::run` 两个分支、
  `app.rs:508` 的「清理不隐藏面板」特例、`gui/glyphs.rs` 的 `PI` / `BOX_ARCHIVE` / `TRASH`、
  `i18n` 的 `desc_pi` + 全部 `clean_*`、`KNOWN_FLAGS` 里的 pi 专属 flag。

### tupa 侧（删）

**✅ 已完成（2026-11-19，代码 + 页面 + 契约）。** 待办只剩「my-pi-plugins 侧接线」。

| # | 改哪里 | 怎么确认 | 状态 |
|---|---|---|---|
| 1.1 | 删 `src/providers/pi.rs` / `pi_index.rs` / `clean.rs`；`build()` 去掉 `PiSessions` | `cargo build --release` 过；命令列表里没有 `pi` |
| 1.2 ✅ | 删 `Kind::Pi` / `Kind::Clean` + 三处同步（`badge` / `columns` / `from_badge`）；`Action::CleanSession` / `CleanBatch` + `exec` 分支 + `app.rs:508` 特例 + `CleanMode` / `CleanOp` | 编译过；`cargo test` 里相关用例一并删/改 |
| 1.3 ✅ | 清 `gui/glyphs.rs` 对应字形 + `i18n` 文案 + `KNOWN_FLAGS` | `cargo clippy --release --features gui` 零警告 |
| 1.4 ✅ | 文档：`README`、`docs/06-handover.md` §3/§6、`AGENTS.md`（pi-sessions 索引那条硬约定整条作废） | `rg -n "@pi|pi:clean|pi_index" docs README.md AGENTS.md` 只剩历史记录段落 |

### 新交付：clean 的 HTML 界面

`/clean` 的入口在 my-pi-plugins 那边，tupa 只交付**前端 + HTTP 契约**（推荐 A）。

- **✅ 定了方案 A**：`/clean` 起本地服务（`127.0.0.1:<port>`），
  - `GET /` → 单文件 `clean.html`（无构建、原生 JS）
  - `GET /api/sessions` → `{ path, title, project, ageDays, sizeBytes, messages, summary, windowDays, disposition }[]`
  - `POST /api/apply` → `{ ops: [{ path, mode: "archive"|"trash"|"restore" }] }` → 逐条结果
  - 页面：按窗口筛（week / month / 14d）、勾选、批量归档/删除、单独的 Restore 视图、结果 toast
  - 交付物：`web/clean/index.html` + `docs/09-clean-api.md`（字段语义 + 三条规则照抄 `classify()`）
- **方案 B**：tupa 加 `--clean-report <out.html>`，生成数据内嵌的**静态**页（只读 + 生成「要执行的清单」文本）。
- **方案 C**：tupa 零代码，规则文档化，TS 侧重写。

> 页面和契约已写好：`web/clean/index.html` + `docs/09-clean-api.md`。
> 剩下的是 my-pi-plugins 里实现 `/clean` 的服务端（`GET /api/sessions`、`GET /api/archive`、
> `POST /api/apply`），把页面托管起来。

---

## 2. 执行完再唤起，输入框没清空（bug）✅ 已完成

### 现状

- 隐藏面板走的是 `util::hide_panel` → `ShowWindow(SW_HIDE)`，**完全绕开 `App`**；
- `App::reset()`（`app.rs:918`，注释写着「隐藏前把状态清干净」）**全仓没有任何调用点**；
  `rg "\.reset\(\)"` 在 `src/gui/` 里 0 命中。
- 所以 `input` / `cursor` / `anchor` / `home` 状态全留着，下次唤起原样出现。

### 改哪里

1. `App::reset()` 补上 `input.clear()` / `cursor = 0` / `anchor = None` / `hint_open` 相关复位；
   把 `set_home()` 里的 `refresh()` 拆出来（隐藏时不必触发一次后台查询，下次显示时自然会 refresh）。
2. 三个隐藏点统一调 `self.app.reset()`：
   - `gui/mod.rs:567`（`handle_key` 里 `should_hide`）
   - `gui/mod.rs:647`（`run_selected` 里 `should_hide`）
   - `gui/mod.rs` Esc 隐藏那条（同文件，`hide_panel` 第二处）
   托盘/热键的 ShowWindow 不经过 `Panel`，所以**只能在隐藏侧清**，不能指望「显示时清」。

### 怎么确认

- 打开一个应用 → `Alt+\`` 再唤起 → 输入框空 + 列表是开屏（收藏/最近），不是上次的字；
- 单元测试：`app.reset()` 后 `input.is_empty() && cursor == 0 && items.is_empty()`；
- 回归：`Ctrl+E` 定位、`@set` 开关（不隐藏面板）**不受影响**。

---

## 3. 去掉 `@run` ✅ 已完成

### 现状

- `src/providers/run.rs`（46 行）：`@run <命令>` → `Kind::Command` + `Action::RunCommand`，
  flags `:admin` / `:bg`。你是要用 GlazeWM 的 `alt+shift+enter` 提权终端替代它。
- 牵连：`Action::RunCommand` 还被 **`history::line_for`**（生成 `@run ...` 可回放行）和
  `:blog` 等服务管理用；`Kind::Command` 也被 `:biog`/`@go:yazi` 之外的条目用（徽标 `run`）。

### 改哪里

| # | 内容 |
|---|---|
| 3.1 | 删 `src/providers/run.rs`，`build()` 里去掉 `Run` |
| 3.2 | **保留** `Action::RunCommand` + `exec.rs` 分支（历史里已有的 `@run xxx` 记录还要能跑） |
| 3.3 | `history::line_for` 不再产出新 `@run` 行（那个分支只保留读旧记录的兼容） |
| 3.4 | i18n `desc_run` / `run_note` 删；`KNOWN_FLAGS` 里 `admin` / `bg` 可留（老历史解析用） |

### 已知副作用（要接受）

历史里 `@run ipconfig` 这类行现在点在 `@hist` 上会补全成「命令列表」，不再自动执行 —— 
因为 `@run` 不再是 provider。若要保留可回放，得在 `parse` 里留一个「隐藏别名」。

### 怎么确认

- `--query "@run ipconfig"` → 命令列表（不再有一条可执行项）；
- 旧历史 `@run` 条目回车不 panic（给出 `cannot_reveal` 或命令列表提示）；
- `cargo test` 全绿。

---

## 4. 右键菜单（未开工）

### 现状

- GPUI 有 `MouseButton::Right`（`vendor/gpui/src/interactive.rs:289`）和
  `on_mouse_down`（`elements/div.rs:121`）；当前行只挂了 `on_click`（双击 = 回车），
  没有右键分支，也没有任何浮层组件。
- 可复用的既有能力：
  - **管理员运行** → `Action::Launch { elevated: true }` 已经实现（ShellExecute `runas`）
  - **打开位置** → `Action::reveal_target()` + `Action::Reveal` 已经实现
  - **收藏 / 取消收藏** → `favorites::toggle(&item)` 已实现（返回新的收藏状态）
  - **删除这一项** → `history::load/save` 有，但没有「按 action 删一条」的 API，需要新增

### 改哪里

| # | 内容 |
|---|---|
| 4.1 | **菜单状态**：`Panel` 加 `menu: Option<MenuState { row: usize, anchor: Point<Px>, items: Vec<MenuEntry> }>`；按 `Esc` / 点空白 / 滚轮关闭 |
| 4.2 | **命中**：行容器加 `on_mouse_down`（`MouseButton::Right` → 选中该行 + 按 item 类型生成菜单项），必须在 `on_click` 之前消费掉右键（否则右键会触发单击） |
| 4.3 | **绘制**：绝对定位浮层（复用 marker 的做法：`absolute()` + `left/top`），`w(180px)`、行高 26、hover 高亮、分隔线；层级在 marker 之上、状态栏之下 |
| 4.4 | **动作执行**：`App` 加 `apply_menu(entry)`，内部走 `exec::run` 或 `favorites::toggle`，执行后隐藏面板（`finish_open` 的语义） |
| 4.5 | **删除这一项**：新增 `history::remove_by_action(&Action)`；`@hist` / 开屏历史条目用它。`@web` 的源数据在 **Edge 的 History sqlite**，不能删 → 菜单文案写「从最近记录移除」，只删 tupa 的 `history.json` |

### 菜单内容（按 item 类型）

| 条目类型 | 菜单项 |
|---|---|
| 应用（`Kind::App`） | 管理员模式运行 / 打开所在位置 / 收藏（或取消收藏） |
| 收藏项 | 取消收藏 / 打开所在位置 |
| `@hist` 记录、开屏历史、`@web` 条目 | 从最近记录移除 / 打开所在位置（有路径时） |
| 其它（命令列表 / 控件） | 不弹菜单（或只有「复制」） |

### 怎么确认

- 右键 VSCode → 四个菜单项都在；「管理员模式运行」弹 UAC 且进程是 elevated 的（任务管理器看）；
- 右键收藏项 → 显示「取消收藏」，点了之后开屏列表里那条 ★ 消失；
- 右键 `@hist` 一条 → 「从最近记录移除」，点了后重新 `@hist` 不再出现，`history.json` 里对应条目已删；
- 右键后按 `Esc` 关闭菜单、不隐藏面板；左键双击仍然正常执行。

### 待定

> ⚠️ `@web`（Edge 历史）的「删除」只能是「从 tupa 的最近记录里移除」，**动不了 Edge 的库**。
> 确认这个语义能接受。

---

## 5. `@go` → `@z` ✅ 已完成

### 现状

- `src/providers/go.rs`（475 行）已经**在用 zoxide**：`zoxide_query()` 是 `Command::new("zoxide")
  .args(["query", "--list", "--", keyword])`（每次查询 spawn 一次 ~50 ms）。
- 已有链路：输入本身是存在的路径 → zoxide → Everything（路径包含 / 结尾匹配）→ 排序。
  空参数时读剪贴板。
- **空查询什么都不列**（`args` 空 → 从剪贴板取；剪贴板不是路径 → 空列表）。

### 改哪里

| # | 内容 |
|---|---|
| 5.1 | 关键字 `go` → `z`（`keyword()` + 命令列表 + i18n 描述；`KNOWN_FLAGS` 不动） |
| 5.2 | **空查询列出 zoxide 全部**：`zoxide query --list --score` 解析 `score\tpath`，按 score 降序，`max_results` 截断；`Item` 列 `[路径, rank]`（或 `[目录名, 完整路径]`，按现有 `Kind::Dir` 列） |
| 5.3 | 有查询词时保持现有链路（zoxide 精确查询 + Everything + 排序），行为不变 |
| 5.4 | 空查询**不再**无条件读剪贴板：剪贴板是路径时插一条 `Kind::Clip`（`@z` 头行），否则跳过 |
| 5.5 | 老历史里的 `@go ...` 行：建议在 `parse` 里把 `go` 当 `z` 的**别名**（`ProviderInfo` 加 `aliases: &[&str]`，只改 `matches()`），否则老记录全失效 |

### 怎么确认

- `--query "@z"` → 22 条（本机 zoxide 条数），按 rank 降序，第一条是 `C:\Users\Agustin\blog`；
- `--query "@z theme"` → 与改动前 `@go theme` 结果一致（抽 3 条对比）；
- `--query "@z:yazi"` / `@z:pi` / `@z:code` 行为不变（`test -d` 验证 `dir` 参数）；
- 剪贴板放了路径 → `@z` 第一行是 clip 条目。

### 决定

> 只留 `@z`，不做 `go` 别名（用户 2026-11-19 定：后来的右键菜单里可以删掉老历史）。
> 实测：`@z` 空查询 22 条按 rank 降序（`blog` 2.8 第一），`@z theme` 走原链路。

---

## 6. clip 反应慢 ✅ 已完成（并纠正了诊断）

### 现状

- 剪贴板读取本身是微秒级（`src/clipboard.rs`，裸 Win32，占用就立刻返回 `None`，不重试不阻塞）。
- 慢在**架构**：`home::query()`（`src/providers/home.rs:192`）在 **worker 线程**里跑，
  clip 条目要等「`favorites::load()` + `history::load()` + frecency 排序 + `Apps::query` + 
  其余段落」全部算完，才随整批 `Evt::Results` 一起显示。
- 也就是：呼出 → `queue_query` → worker 排 0.2–0.5 s → 列表一次性换掉，clip 是**其中之一**，
  不是最早的。
- 另：`history::load()` 每次都读 `history.json` 全量解析 + 排序，没有缓存。

### 改哪里（按收益顺序）

| # | 内容 |
|---|---|
| 6.1 ✅ | **量过了**：`home::query` 全程 **0.4–5ms**（`history::load` 首次 4ms / 热 0.16ms、`favorites` 0.2ms、剪贴板 29–809µs）。**所以 6.3 的缓存不需要做** —— 瓶颈不在这 |
| 6.2 ✅ | **clip 提前**：移到 `App::refresh_with`（`App::clip` + `merge_clip`），同步插到 `items[0]`；`home::query` 不再读剪贴板。`drain_events` 收到结果后再 merge 一次（去重）。实测呼出后 clip 行立刻在，且不抢默认选中（`2/6`） |
| 6.3 ❌ | 缓存：**不做**（6.1 实测不需要） |
| 6.4 | （可选）开屏空查询改**同步**执行（本地数据、无外部进程时），彻底去掉一次线程往返 —— 先看 6.1 的数据再决定 |

### 怎么确认

- `--query ""` 计时对比（改前 0.49 s）；
- GUI：冷启动后按 `Alt+\``，秒表/录屏看 clip 行出现的时间点（目标：与面板出现几乎同时）；
- 回归：clip 条目仍然**不抢默认选中**（`default_selection` 那条规则不变），
  `[general] clipboard = false` 时完全不出现。

---

## 7. 顺序（已完成 1/2/3/5/6；剩 4 与插件侧接线）

```
#2 输入框清空        ← 10 行级，立刻能给一个可用版本
#3 去掉 @run        ← 删代码，和 #1 的 tupa 侧一起编译验证
#1 tupa 侧删 @pi    ← 与 #3 同批；HTML 交付另行
#5 @go → @z         ← 独立，go.rs 改关键字 + 空查询
#6 clip             ← 先测量（6.1），再决定 6.2/6.3/6.4
#4 右键菜单         ← 最大，建议放在 Action 层清理完之后
#1 的 HTML          ← 与 my-pi-plugins 协作，可随时并行
```

## 8. 收尾（每项做完都走一遍）

```bash
cd ~/Projects/tupa-launcher
taskkill //IM tupa-gui.exe //F
cargo build  --release --features gui
cargo test   --release --features gui
cargo clippy --release --features gui      # 零警告
./target/release/tupa-launcher.exe --check # [[apps]] 全部可解析
```

改完写回 `STATE.md`（阶段 / 未完成 / 首行 `code`）；文档与代码不一致的改**文档**。
