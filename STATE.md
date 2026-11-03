# tupa-launcher 状态

updated: 2026-11-03 · code: `a6814c9`（最后一次改动代码的提交）· main 与 origin/main 已同步（无未推送提交）

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
- 用户本机实际配置：`command_prefix = "@"`、`panel = "tui"`、热键 `alt+\``
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

## 下一步

1. **`:web` 打开网页不会唤出浏览器（2026-11-01 发现，GlazeWM 环境）**：`src/exec.rs` 的
   `Action::OpenUrl` 只做 `open::that_detached`，于是已有浏览器实例在**别的 workspace** 里新建
   标签，窗口保持 GlazeWM 的 `displayState=hidden`，工作区不切 → 用户看不到。实测证据：
   Edge 窗口在 workspace `1`(all)、当前在 `2`(web)。`:blog` 三入口与收藏里的网址走同一个
   `Action::OpenUrl`，修一处全受益。
   - 方案 A（推荐）：`open` 后 `glazewm query windows` 按 `processName` 找浏览器窗口 →
     `glazewm command focus --container-id <id>`；加配置开关，非 GlazeWM 环境退化为现状。
     代价：每次多 spawn 两次 `glazewm.exe`（~30–80ms）
   - 方案 B **别用**：纯 Win32 `SetForegroundWindow` + `ShowWindow`，窗口显示了但 GlazeWM 的
     `isDisplayed` 不同步，之后 layout 会错乱
   - 方案 C：常驻订阅 GlazeWM IPC 事件（命名管道），效果最好但工作量大
2. **frecency 排序（现在只按「最近」）**：`history::Entry` 加 `hits: u32` 与 `last_used`（unix 秒），
   全部 `#[serde(default)]` 兼容旧 `history.json`（老记录 hits 记 1）；`push` 里命中同一条就
   `hits += 1`。排序改成 `hits × 新鲜度衰减`，影响两处：开屏那 25 条历史、`@app`/空查询的列表。
   注意别把 `history::LIMIT = 200` 撑大，也别让排序打乱开屏的「收藏在前」结构
3. **GUI 打磨**（[`docs/07-shell-handover.md`](docs/07-shell-handover.md) §1「还没做的」）：
   圆角 / 阴影 / 淡入、鼠标点击、滚动条、DPI 切换实测、窗口尺寸配置项
4. 实机跑一段时间 GUI 后，再把 `[daemon] panel` 的默认值从 `tui` 改成 `gui`（保留 TUI 兜底）
5. 主题并入 `~/Themes/generator/generate.py`（现在 `theme.toml` 是手改的，权威色板是
   `~/Themes/themes/everforest/tokens.yaml`）：让生成器直接产出 `~/.config/tupa-launcher/theme.toml`，
   跟博客 / pi / 终端主题同一个来源
6. **`config.example.toml` 仍只有 7 个示例应用**（本机已 55 项）：倾向**保持精简示例**
   （只教语法）；若决定同步，改的是**仓库文件**，改完需提交推送

> 2026-11-03 删掉了三条「待实机验」（节能模式 / `:blog` / `:pi:clean`）：都是已经在用的功能，
> 真出问题时再按报错查。相关坐标与调用链仍留在「阶段」节里备查。同日做完了 `hist` provider，
> 开屏历史也从 5 条改成了 25 条（填满面板）。

## 待验证

```bash
cargo test --release      # 全绿（2026-11-03 实测 93 passed，数字以输出为准）
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
