# 自建无边框窗口（脱离 Windows Terminal）——设计稿

> 现在面板是 `wt -p tupa-launcher` 拉起来的 Windows Terminal 窗口。这份文档讨论
> 换成自绘窗口的可行性、方案和工作量。
>
> 状态：**设计稿**（选定方向：winit + 自绘；v0.7 待实现）
> 具体开工步骤、当前代码接口、坑和待确认决策见
> [`docs/07-shell-handover.md`](07-shell-handover.md)。

## 1. 现在的问题

| 问题 | 影响 |
|---|---|
| 窗口有边框 / 标题栏 | 不像启动器，倒像个终端 |
| 依赖 WT 的 profile 和设置 | 别人装不了；用自己的配置容易冲突 |
| `Ctrl+Tab` 被 WT 占用 | 「详细信息」只能改用 `Ctrl+O` |
| 首次唤起 400~700ms | 冷启动 WT；之后进程常驻所以只慢一次 |
| 位置/尺寸依赖 GlazeWM | 没跑 WM 时窗口位置很随机 |

## 2. 方案对比

### A. winit + 自绘（保留 ratatui 的布局/渲染代码）

- `winit` 创建无边框、置顶、不在任务栏显示的窗口
- 自己把 `ratatui::buffer::Buffer` 的每个 cell 画成「背景矩形 + 字形」
- 字体：`cosmic-text`（支持中文、字体回退、字形缓存）
- 光栅化：`swash` 或 cosmic-text 自带；像素上传用 `softbuffer`（CPU）或 `wgpu`（GPU）
- 输入法：需要自己处理 `Ime` 事件（预编辑文本、候选窗定位 `set_ime_cursor_area`）

**优点**：完全掌控，能真正做「浮层启动器」；ratatui 的 widget 代码基本能留
**缺点**：字体渲染/IME 工作量大，估计 1500~2000 行 + 调优

### B. egui 重写 UI 层

- 状态机（`App`）保留，只把 `ui.rs` 换成 egui 绘制
- egui 自带字体、IME、滚动、动画、焦点
- `winit` 集成成熟（`eframe` 或 `egui-winit`）

**优点**：IME、字体、滚动都是现成的，开发最快（估计 600~900 行）
**缺点**：放弃 ratatui 的渲染代码；依赖体积变大（~5MB）；界面风格要重新设计

### C. 继续用 Windows Terminal

**优点**：零成本，中文输入法/滚动/复制粘贴都现成
**缺点**：第 1 节的全部问题

### 结论

- 想要「真正的启动器体验」→ **A**
- 想最快看到效果、愿意重画界面 → **B**
- 怕 IME 坑、只想把功能做厚 → **C**

> 你选的是 **A**。下面按 A 拆阶段。

## 3. 分阶段实施（A 方案）

### 阶段 1：窗口与渲染骨架（独立分支，不影响现有功能）

- [x] `winit` 创建窗口：`decorations(false)`、`always_on_top`、`skip_taskbar`
- [x] `softbuffer` 拿到窗口 surface，把 `Buffer` 画成像素（`src/gui/text.rs`）
- [x] 字形：`cosmic-text` 排版 + 统一基线 + 按字符宽度选字体（FiraCode / 雅黑）
- [x] 用 `--gui` 子命令启动（和 `--daemon`/TUI 并存）

验收：窗口里能显示命令列表，窗口无边框、居中、置顶。

### 阶段 2：输入

- [x] `winit` 的 `KeyboardInput` → 转成 `crossterm::event::KeyEvent`，直接喂 `App::on_key`
- [x] 键位表补齐（`Ctrl+Tab`、`Shift+Tab`、方向键、Home/End…）
- [x] 光标：读 `TestBackend` 的光标位置，自绘一条竖线

验收：能打字、上下选、Enter 执行、Esc 隐藏。

### 阶段 3：中文输入法（最大的坑）

- [x] `Window::set_ime_allowed(true)`
- [x] 处理 `Ime::Preedit`（画在输入行、带下划线）和 `Ime::Commit`（插进 `App::input`）
- [x] `set_ime_cursor_area` 把候选窗定位到光标处
- [x] `Preedit` 期间不触发 provider 查询（只在 Commit 时刷新）
- [ ] 中文模式下直接上屏的标点（不走组合）实测是否重复

验收：能输入中文、候选框位置正确、全角字符光标不错位。

### 阶段 4：隐藏/唤起与常驻

- [x] 唤醒热键沿用现有 daemon（`RegisterHotKey` + Copilot 键）
- [x] daemon 不再 `wt -p tupa-launcher`，改为拉起同目录 `tupa-launcher.exe --gui`
- [x] Esc → 隐藏（`window.set_visible(false)`），不是退出
- [x] 单实例保护（窗口类名查找，重复执行只唤起已有窗口）

### 阶段 5：细节

- [ ] 小尺寸（62%×55%）、居中、圆角/阴影（DWM `DwmExtendFrameIntoClientArea` + 自绘圆角）
- [ ] 高分屏缩放（DPI 变化时重建字形缓存）
- [ ] 动画（淡入 + 轻微上移，200ms）

## 4. 风险与对策

| 风险 | 对策 |
|---|---|
| 中文输入法候选窗跑偏 | 每次 Preedit 都 `set_ime_cursor_area`（winit 0.30 支持） |
| 字体渲染糊 / 符号缺字 | cosmic-text 的字体回退 + 用系统等宽字体（Cascadia Mono / 微软雅黑） |
| 性能（每帧重画 6 万个矩形） | 只重画脏区；字形走 atlas；用 `softbuffer` 的 `buffer_mut` + `present` |
| winit 版本 API 变动 | 固定 `winit 0.30` + `softbuffer 0.4`，写在一层 `Presenter` 里隔离 |
| 与现有 TUI 冲突 | 保留 TUI，GUI 用 `--gui` 启动；`ui::draw` 产出的 Buffer 两边共用 |

## 5. 依赖与体积

```toml
winit = "0.30"
softbuffer = "0.4"
cosmic-text = "0.14"
```

体积估计 +2 MB（release 后）。

## 6. 备选：如果 IME 太难

- 用 `egui` 只做「输入行 + 列表」两块，其余逻辑复用（B 方案），IME 问题由 egui 解决
- 或者继续用 WT，把精力放在功能上（C 方案）

---

**下一步**：先确认是否接受「新增 3 个依赖、约 1500 行」的成本；接受的话按阶段 1 起一个分支。
