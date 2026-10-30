# tupa-launcher 开工规矩

> pi 在本目录（或子目录）启动时自动加载本节。状态（阶段 / 下一步 / 待验证）**只看
> [`STATE.md`](STATE.md)**，不要在这里或 README 里重复。

## 开工第一步

**先读 [`STATE.md`](STATE.md)**（不超过一屏）。它首行的 `code` 是**最后一次改动代码的提交**，
用它判过期：

```bash
git log -1 --format=%h -- src Cargo.toml   # 不等于 STATE 里的 code → 代码已变，先核对「阶段」与「下一步」
```

只改文档的提交不算过期。若确实已经脱节，先按代码实际状态改 `STATE.md`，再动手。

## 先读哪里

| 想知道 | 看 |
|---|---|
| 现在做到哪、下一步是什么 | `STATE.md` |
| 加 provider / 加 Action / 加文案 / 测试怎么写 | [`docs/06-handover.md`](docs/06-handover.md) §3 |
| **已经踩过的坑（20+ 条）** | `docs/06-handover.md` §4 —— 动手前扫一眼，能省一小时 |
| 套壳（GUI）/ 开关引擎 / 原生窗口的设计与步骤 | `docs/07-shell-handover.md` / `docs/04-switch-engine.md` / `docs/05-native-window.md` |
| 用户怎么用、配置项、命令语法 | `README.md` |

## 改完怎么生效（硬性，别忘）

面板进程常驻（`Esc` 只是隐藏），改完不重启就还是旧 exe：

```bash
cd ~/Projects/tupa-launcher
taskkill //IM tupa-launcher.exe //F
cargo test --release && cargo clippy --release && cargo build --release
powershell -NoProfile -Command "Start-Process (Join-Path \$env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Startup\tupa-launcher-daemon.lnk')"
```

## 硬约定

- **所有用户可见文案走 `src/i18n.rs`**（`t("中文", "English")`），业务代码里不写裸字符串。
- **加 `Action` 变体要同步 4 处**（漏了编译不过或行为不对）：`providers/mod.rs` 的 `Action`、
  `exec::run`、`app::reveal_selected`、`Item::detail_line`。
- **加 provider**：`src/providers/xxx.rs` + `providers/mod.rs::build()` 里 push（顺序 = 命令列表顺序）。
- 命令关键字是 **`set`**（历史上叫 switch，已改名）；文档示例写默认前缀 `:`，
  用户本机 `config.toml` 改成了 `@`，别把两者搞混。
- 面板渲染只走 `ui::draw`（TUI 与 GUI 共用），别在 GUI 里另写一套布局。
- **不要用「方向键 + `Space`」操作 Windows 系统 UI**（快速设置 / 通知中心）：`Space` 切的
  是**当前焦点项**，而系统 UI 的按钮布局会因用户增删固定项而变——必然存在「焦点落错行
  → 切错开关」的窗口（实战里误关过蓝牙 / 误开过飞行模式）。要操作这类面板，一律
  **坐标点击 + 读回状态校验**；点击前用 `GetCursorPos` 记光标、`finally` 里复位。

## 收尾清单

1. `cargo test --release` + `cargo clippy --release` 全绿（数字以输出为准，**不要写死进文档**）。
2. 更新 `STATE.md` 的三节 + 首行 `updated` / `based_on`。
3. 若代码与文档不一致（命令改名、新增 provider、新增内置开关）→ 改**文档**；
   若只是"做到哪了" → 改 **STATE.md**。两者别混。
4. `git commit`（信息里写清"改了什么 + 为什么"）；需要时提醒用户 push。
