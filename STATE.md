# tupa-launcher 状态

updated: 2026-10-23 · code: `194892a`（最后一次改动代码的提交）· 整理时 main 领先 origin/main **13** 个提交（未推送）

> 判断本文件是否过期：比较 `code` 与 `git log -1 --format=%h -- src Cargo.toml`。
> 只改文档的提交不算过期。

> 本文件只放**会过期**的事实：阶段 / 下一步 / 待验证 / 入口。
> 开工规矩见 [`AGENTS.md`](AGENTS.md)；代码结构、数据流、加功能的步骤、坑表见
> [`docs/06-handover.md`](docs/06-handover.md)；设计与阶段方案见 `docs/01`–`docs/08`。
>
> **每次会话收尾只需要改这一个文件的三节**（阶段 / 下一步 / 待验证），并更新首行。

## 阶段

- 版本 `Cargo.toml` **0.8.0**
- **TUI 面板稳定**：`app` `ev` `run` `pi` `go` `web` `new` `fav` `cfg` `set` + 开屏 `home`（收藏 + 最近命令）
- **自绘 GUI 已能跑**：`src/gui/`（winit + cosmic-text + softbuffer），窗口 / 字体 / 键位 / 中文输入法（IME）/ daemon 接管都已实现；
  用 `--gui` 或在 `config.toml` 里设 `[daemon] panel = "gui"` 打开
- **但默认仍是 `tui`**：`daemon::launch_panel` 按 `[daemon] panel` 分流，TUI 走 `wt -p tupa-launcher`；
  GUI 尚未打磨到可以当默认
- **`:clean` 已加入（2026-10-25）**：pi 会话清理 provider（`src/providers/clean.rs`）。
  时间窗口是前置条件（默认两周，`:week` / `:month`），窗口内与收藏的**不列出**；
  已总结或消息数 ≥ 8 → 归档到 `session-archive/`，其余瘦会话 → 回收到系统回收站；
  `:clean:restore` 列出归档，回车恢复回 `sessions/` 原目录（重名加 `-restored-` 后缀）。
  回车执行后不隐藏面板、就地刷新（可连续处理）。调试：`--query ":clean"`（只读）
- 用户本机实际配置：`command_prefix = "@"`、`panel = "tui"`、热键 `alt+\``

## 下一步

1. **`:clean` 待实机验**（2026-10-25）：热键唤起 → `@clean` → 回车归档一条 → `@clean:restore`
   搜回来，确认路径与目录结构；再试一条 `[删除]` 看是否进回收站
2. **GUI 打磨**（[`docs/07-shell-handover.md`](docs/07-shell-handover.md) §1「还没做的」）：
   圆角 / 阴影 / 淡入、鼠标点击、滚动条、DPI 切换实测、窗口尺寸配置项
3. 实机跑一段时间 GUI 后，再把 `[daemon] panel` 的默认值从 `tui` 改成 `gui`（保留 TUI 兜底）
4. 剩下的小项（不阻塞套壳）：主题并入 `~/Themes/generator/generate.py`；`hist` provider；frecency 排序

## 待验证

```bash
cargo test --release      # 全绿（2026-10-23 实测 73 passed，数字以输出为准）
cargo clippy --release    # 零警告
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
