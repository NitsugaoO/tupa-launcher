# tupa-launcher 状态

updated: 2026-11-03 · code: `8f67bb8`（最后一次改动代码的提交）· 同步状态以 `git status -sb` 为准

> 判断本文件是否过期：比较 `code` 与 `git log -1 --format=%h -- src Cargo.toml`。
> 只改文档的提交不算过期。

> 本文件只放**会过期**的事实：阶段 / 下一步 / 待验证 / 入口。
> 开工规矩见 [`AGENTS.md`](AGENTS.md)；代码结构、数据流、加功能的步骤、坑表见
> [`docs/06-handover.md`](docs/06-handover.md)；设计与阶段方案见 `docs/01`–`docs/08`。
>
> **每次会话收尾只需要改这一个文件的三节**（阶段 / 下一步 / 待验证），并更新首行。

## 阶段

- 版本 `Cargo.toml` **0.9.0**（`:blog` 删了旧 `:new`，升了一个次版本）
- **TUI 面板稳定**：`app` `ev` `run` `pi` `go` `web` `blog` `fav` `cfg` `set` + 开屏 `home`（收藏 + 最近命令）
- **`:blog` 取代了旧的 `:new`（2026-10-25）**：`src/providers/blog.rs`。不带参数列三个入口
  （本地 `localhost:4000` / GitHub Pages / Cloudflare），`:blog:new <标题>` 在
  `<blog>/source/_posts` 写 front matter 并用 VSCode 打开（同名不覆盖、空参数列最近文章），
  `:blog:manage` 后台跑 `pnpm admin` + 1.5s 后打开 `127.0.0.1:5173`。
  目录与网址在 `config.toml` 的 `[blog]`（`dir` 留空 = `~/blog`）；
  旧的 `new` provider / `Action::CreateFile` / `general.new_files_dir` 已删干净
- **自绘 GUI 已能跑**：`src/gui/`（winit + cosmic-text + softbuffer），窗口 / 字体 / 键位 / 中文输入法（IME）/ daemon 接管都已实现；
  用 `--gui` 或在 `config.toml` 里设 `[daemon] panel = "gui"` 打开
- **但默认仍是 `tui`**：`daemon::launch_panel` 按 `[daemon] panel` 分流，TUI 走 `wt -p tupa-launcher`；
  GUI 尚未打磨到可以当默认
- **`:pi:clean` 已加入（2026-10-25）**：清理视图挂在 `:pi` 上（`clean` 修饰符，
  实现在 `src/providers/clean.rs`）。时间窗口是前置条件（默认两周，
  `:pi:clean:week` / `:pi:clean:month`），窗口内与收藏的**不列出**；已总结或消息数 ≥ 8 →
  归档到 `session-archive/`，其余瘦会话 → 系统回收站；`:pi:clean:restore` 列出归档，
  回车恢复回 `sessions/` 原目录（重名加 `-restored-` 后缀）。回车执行后不隐藏面板、
  就地刷新（可连续处理）。批量：`:pi:cleanall[:week|:month]` 压成一条汇总项，
  回车一次处理全部（失败不中断，最后汇报）。调试：`--query "@pi:clean"`（只读，前缀按本机 `@`）
- **节能模式改为纯鼠标点击（2026-10-31）**：不再「键盘优先、鼠标回退」。原来的
  `Home` → `→→↓` → `Space` 切的是**当前焦点项**，快速设置的固定按钮一增删
  （蓝牙 / 飞行模式 / 亮度），`Space` 就会误关蓝牙或开飞行模式。现在只有
  `Click-Energy`（相对屏幕右下角 `-89, -311`，按 DPI 缩放；200% 下 `(2894,1298)`），
  点完读 `EnergySaverState` 校验，没变就报错 → **最坏是「没生效」，不会「切错别的东西」**；
  点击前记光标、`finally` 里复位。代价：一次约 4 秒（原 ~1 秒）
- **默认搜索改成「应用 + 历史」混排（2026-11-03）**：新增 `src/providers/all.rs`（keyword `all`），
  两边都用 `SkimMatcherV2` 打分（同一尺度）后混排，**同分时应用优先**；按 `Action` 去重
  （历史里已记为 `Launch code` 的不会在应用结果后再出现）；空查询不混（只列应用，开屏另有 `home`）。
  `all` 与 `app` 共用同一份 `Arc<Apps>`（不重复解析 55 个 `.lnk`）。`[general] default_provider`
  默认值 `app` → **`all`**；`@app code` 仍然只搜应用。配套把 `Apps::query` / `Hist::query`
  拆出 `ranked()`（返回 `(分数, Item)`）给混排用
- 用户本机实际配置：`command_prefix = "@"`、`panel = "tui"`、热键 `alt+\``、`default_provider = "all"`
- **应用列表扩到 55 项（2026-11-01，只动本机配置，未改仓库代码）**：`[[apps]]` 18 → 55，
  按常用度分五组重排（天天用 12 / 工作沟通 9 / 桌面系统 10 / 网络 4 / 影音杂项 20）；
  `general.max_results` 30 → **60** —— `src/providers/apps.rs` 空查询走 `take(self.max)`，
  条数超过它会被**静默截断**（配了但空输入看不到）。备份 `config.toml.bak-20261101-160208`。
  QQ / DeepSeek Harness / Radmin VPN / Where To Study / piik-app 属「装了但没在开始菜单
  建快捷方式」或漏看，已手工补；`Join QR Code` 明确不加
- **`:hist` 已加入（2026-11-03）**：`src/providers/hist.rs`，`@hist [关键词]` 搜 `history.json`
  （最多 200 条，再按 `general.max_results` 截断）：模糊匹配**输入行 / 快照标题 / 快照副标题**，
  带关键词按得分排序、不带关键词「最近在前」，按 `action` 去重。回车行为同开屏：有快照
  直接再打开，老记录用 `Action::FillInput` 填回输入框。`history::to_item()` 已抽成公共函数
  给 `home` 与 `hist` 共用（开屏历史条目从 3 列变 2 列，布局按最多的行算，不受影响）
- **`:web` 打开网页会自己切到浏览器（2026-11-03）**：新增 `src/glazewm.rs`。
  `Action::OpenUrl`（`:web` 选中项 / `:blog` 三入口 / 收藏里的网址）开完网页后在**后台线程**里
  `glazewm query workspaces` → 按 `processName` 白名单找浏览器窗口 → 先
  `focus --workspace <名字>` 让 WM 自己切（实测：Edge 从 `hidden` 变 `shown` 且拿到焦点），
  300ms 复查没落到目标窗口才补 `focus --container-id`，400ms 后再复查一次防「跳过去又弹回来」。
  开关 `[glazewm] focus_browser`（默认 `true`；没装 GlazeWM 自动忽略，失败静默）。
  顺序与复查逻辑照搬 `~/.pi/my-plugins/plugins/pi-notify` 的 `glaze.ts` / `focus.runtime.mjs`：
  **先 workspace 后 container 不能反** —— 跨 workspace 直接 `--container-id` 会先把 WM
  内部焦点改掉，之后 `--workspace` 变成 no-op，窗口还是不显示（那边 2026-11-01 实测）；
  也没用裸 `SetForegroundWindow`（只改显示不改前台，`isDisplayed` 不同步，layout 会乱）
- **frecency 排序（2026-11-03）**：`history::Entry` 加了 `hits` / `last_used`（都 `#[serde(default)]`，
  老 `history.json` 读出来 hits=1、last_used=0，不会解析失败），`push` 命中同一条就 `hits += 1`
  并刷新时间。`history::score()` = 次数 × 新鲜度权重（×100 定点：1h 内 400 / 1d 200 / 1w 100 /
  1m 50 / 更早 25），`sort_frecency()` 同分按 last_used 降序。作用两处：开屏那 25 条历史、
  `@app` 空查询（历史里 `Kind::App` 的 `Action::Launch` 按 `command + args` 聚合成总分，
  `elevated` 不参与匹配）。开关 `[general] frecency`（默认 true，关掉就回时间序 / 配置顺序）。
  `@hist` 不动（仍按相关度 / 最近）。顺带修了 `HISTORY_LIMIT` 被回退成 5 的问题（现 25，
  加了单测钉住 ≥20）

## 下一步

1. **GUI 打磨**（[`docs/07-shell-handover.md`](docs/07-shell-handover.md) §1「还没做的」）：
   圆角 / 阴影 / 淡入、鼠标点击、滚动条、DPI 切换实测、窗口尺寸配置项
2. 实机跑一段时间 GUI 后，再把 `[daemon] panel` 的默认值从 `tui` 改成 `gui`（保留 TUI 兜底）
3. 主题并入 `~/Themes/generator/generate.py`（现在 `theme.toml` 是手改的，权威色板是
   `~/Themes/themes/everforest/tokens.yaml`）：让生成器直接产出 `~/.config/tupa-launcher/theme.toml`，
   跟博客 / pi / 终端主题同一个来源
4. **`config.example.toml` 仍只有 7 个示例应用**（本机已 55 项）：倾向**保持精简示例**
   （只教语法）；若决定同步，改的是**仓库文件**，改完需提交推送

> 2026-11-03 删掉了三条「待实机验」（节能模式 / `:blog` / `:pi:clean`）：都是已经在用的功能，
> 真出问题时再按报错查。相关坐标与调用链仍留在「阶段」节里备查。同日做完了 `hist` provider、
> `:web` 唤出浏览器、frecency，开屏历史也从 5 条改成了 25 条（填满面板）。

## 待验证

```bash
cargo test --release      # 全绿（2026-11-03 实测 102 passed，数字以输出为准）
cargo clippy --release    # 零警告（2026-11-03 实测）
tupa-launcher.exe --check # 55 项应用全部可解析、空输入列 55 条（2026-11-01 实测）
```

改完代码必须**重启守护进程**，否则跑的还是旧 exe —— 步骤见 [`AGENTS.md`](AGENTS.md)。

## 入口

| 项 | 位置 |
|---|---|
| 权威源码 | `~/Projects/tupa-launcher`（git → `NitsugaoO/tupa-launcher`） |
| 生效配置 | `~/.config/tupa-launcher/{config,actions,theme}.toml`（改完下次启动生效，**不用编译**） |
| 用户数据 | `~/.config/tupa-launcher/{favorites,history}.json` |
| 守护进程日志 | `%TEMP%\tupa-launcher-daemon.log` |
| 开机自启 | 启动文件夹 `tupa-launcher-daemon.lnk` |
| 面板窗口规则 | GlazeWM `window_rules`（浮动 / 62%×55% / 居中） |
