# tupa-launcher 状态

updated: 2026-11-19 · code: `PENDING`（↑↓ 终端化 + 补全默认关 · 列表模式补键 · chip 只留渲染 · @hist 隐藏 · 右键菜单 · 删 @pi / @run · @go → @z · clip 同步出 · clean 迁到 HTML）

> 判断本文件是否过期：比较 `code` 与 `git log -1 --format=%h -- src Cargo.toml`。
> 只改文档的提交不算过期。

> 本文件只放**会过期**的事实：阶段 / 未完成 / 入口。
> 开工规矩、验证命令、「试过不行」清单见 [`AGENTS.md`](AGENTS.md)；代码结构、数据流、加功能的步骤、坑表见
> [`docs/06-handover.md`](docs/06-handover.md)；设计与阶段方案见 `docs/01`–`docs/08`。
>
> **每次会话收尾只需要改这一个文件的两节**（阶段 / 未完成），并更新首行。

## 阶段

- **chip 光标位置 + 鼠标切焦点（2026-11-19 晚，第四轮）**：
  - chip 只是渲染，光标能进块内 —— 但**胶囊是整体画的，光标在内部没地方放**。
    现在 `input_row` 里判断「光标 / 选区落进关键字范围」就把 chip 降级成普通文字重画：
    `@app` 按 `Home` → 胶囊消失、显示 `@app` 纯文本、光标在行首（原先被画到胶囊末尾）。
  - **鼠标点击切焦点**：点输入行 → 回输入框（`leave_list_mode` + 光标闪烁）；
    点列表行 / 列表空白 → `list_mode = true`（输入框失焦）。

- **Esc / ↑↓ 的两个修正（2026-11-19 晚，第三轮）**：
  - **Esc 收紧凑只在输入为空时生效**：原先 `@app` → `↓` → `Esc`（回输入）→ `Esc` 被
    「收紧凑」吃掉，要按两次才清空 （用户反馈）。现在非空输入的 Esc 直接清空。
  - **历史模式里 ↑↓ 归历史**：原先在历史模式里按 `↓` 会跳到列表模式；现在先判
    `app.in_history()`，`↑` 更早 / `↓` 更新（翻到底自动退出）。
  - `?` 帮助更新到最新键位（↓ 进列表、↑ 翻历史、Space/→ 开菜单、Home/End、Esc 四层）。

- **交互再细化（2026-11-19 晚，第二轮）**：
  - **补全默认关**：`[general] completion`（默认 `false`）。`App::update_completion` 里一次都不算，
    浮层 / 状态栏候选说明 / Tab 循环全没有；代码一行没删，配置改 `true` 就恢复。
  - **↑↓ 终端化**：输入模式 `↓` = 进列表模式并**直接选中下一项**（回车执行第 1 项）；
    `↑` = **翻输入历史**（`App::history_prev`），`Esc` 用 `App::in_history()` 判断先退出历史模式。
    **`Ctrl+↑/↓` 已删除**。列表模式里 `↑↓` 仍是移动选择。
  - **列表模式补键**：`Home` / `End` 到首尾（`App::select_first` / `select_last`）；
    `→` 开当前行菜单、`←` 关菜单（输入模式下这两个仍是光标移动）。
  - **列表模式打字自动回输入框**：`Panel::leave_list_mode()` 在 IME 的两个 handler 里调，
    中文组字时也会切回（原先是「完全忽略」）。
  - 菜单项「从最近记录移除」的内部方法改名 `App::history_has`（避免和 `in_history()` 撞名）。

- **四项交互调整（2026-11-19 晚）**：
  - **`@hist` 从 `@` 命令列表里隐藏**：`Provider::listed()`（默认 true）返回 false 只影响
    命令列表的 `metas`，不影响解析 —— `@hist` 照样能用。它跟开屏的「最近」重复，列出来是噪声。
  - **chip 不再原子**：只保留胶囊渲染（`chip_end` 仍由 `Parsed::Command` 决定），
    编辑上光标能进块内、退格逐字、`Home` 到行首、`Ctrl+Backspace` 按词边界走。
    删掉的四处：`snap_cursor`（+ 两处调用）、`word_boundary` 的反向特判、`backspace` 整块删、
    `gui/input.rs` 的 IME 插入点夹取。
  - **输入区 / 列表区分开**（用户定的交互模型）：`↓`（或 `↑`，顺手向上移）进 `Panel::list_mode`，
    输入框失焦（提示符变暗、光标隐藏、IME 与字符输入都被忽略）；列表模式下
    `↑↓` 移动、**`空格` 开当前行菜单**、`Enter` 执行、`Esc` 回输入框。
    `Esc` 现在逐层退：列表模式 → 输入框 → 紧凑 → 隐藏。
  - **补全候选浮层（GUI 之前一直缺着）**：右键菜单和补全共用 `Panel::popup_list`
    （`PopupPick` 区分点击行为）。补全竖排在输入框下方，`Tab` 立即套用（老行为）、
    `↑↓` 只移高亮（`App::cycle_completion`）、**`Enter` 套用高亮候选**（`App::complete_to`）、
    鼠标点击也能套用。菜单本身也支持键盘：`↑↓` + `Enter`。

- **右键菜单（2026-11-19 晚，PLAN #4）**：列表任意行右键 → 绝对定位浮层，菜单项按该行
  **实际能做的事**现取（`App::menu_for`，不缓存）：应用给「以管理员身份运行 / 打开所在位置 /
  收藏」，路径与收藏项给「打开所在位置 / 取消收藏」，在 `history.json` 里有记录的给
  「从最近记录移除」。`Action::Launch { elevated: true }` / `Action::reveal_target()` /
  `favorites::toggle()` 全部复用。**唯一新增语义**：「从最近记录移除」只改 tupa 的
  `history.json`（`@web` 的源在 Edge History 库里删不掉，所以文案是「最近记录」不是「删除」）。
  两条实现约束（已写进 AGENTS 硬约定）：行事件只在 `row_click_handler` /
  `row_right_click_handler` 一处写（卡片行与密度行共用）；右键也会走 `on_click`，
  左键处理器必须判 `MouseButton::Left`，否则右键双击会直接执行条目。

- **五项改造（2026-11-19 晚）**：一次会话做完 [`PLAN.md`](PLAN.md) 的 1 / 2 / 3 / 5 / 6 项。
  - **`@pi` 与 `@run` 已移除**（`pi.rs` / `pi_index.rs` / `clean.rs` / `run.rs` 删除）：
    `@pi` 每次查询要开 175 个 jsonl、读前 80 行（实测 **0.68s / 88MB**），而索引里
    `session_name` / `first_user_prompt` / `cwd` / `message_count` 全都有 —— 旧实现只用它
    覆盖标题、**没省掉 I/O**。既然不再需要就直接删。`Action::RunCommand` + `exec` 分支
    **保留**（老历史里的 `@run` 行还能执行，只是不再是输入入口）；`history::line_for`
    不再产出 `@run` 行，`LaunchInTerminal` 改成按目录回放成 `@z <dir>`。
  - **pi 会话清理迁出**：新交付 `web/clean/index.html`（单文件页面、无构建）+
    [`docs/09-clean-api.md`](docs/09-clean-api.md)（判定规则 + HTTP 契约）。
    `my-pi-plugins` 的 `/clean` 起本地服务托管页面即可，**tupa 侧零代码**。
  - **`@go` → `@z`**：关键字改 `z`，**空输入列出 zoxide 完整历史**
    （`zoxide query --list --score`，解析 `score path`、按 rank 降序，spawn 一次 ~50ms），
    剪贴板里是存在的路径时插第一行；有查询词时链路不变（存在路径 → zoxide → Everything → 重排）。
    **不做 `go` 别名**（老 `@go` 历史会变成命令列表提示，可用右键菜单删）。
  - **隐藏面板后清空输入**（bug）：TUI 的 `main.rs` 一直是 `app.reset()` + hide，
    GUI **漏了 `reset()`** → 执行完再唤起输入框还是上次那行字。现在 `Panel::hide()` 统一收尾
    （`reset()` + `hint_open` / `help` / 动画位置归零），两处 `should_hide` 都走它。
  - **紧凑态收回**：除 `handle_key` 那条即时复位外，轮询里再按「**非空 → 空**」的跳变复位
    一次（`Panel::was_empty`，覆盖 IME 提交 / 粘贴清空等不走 `handle_key` 的路径）。
    **不能**写成「空了就收回」—— 紧凑态按 ↓ 展开时输入本来就是空的。
  - **clip 行同步出**（以前要等 worker 整批结果）：`App::clip` 在 `refresh_with` 里同步算好、
    `merge_clip` 直插 `items[0]`，`home::query` 不再读剪贴板。实测 `home::query` 本身只要
    **0.4–5ms**（`history::load` 首次 4ms / 热 0.16ms、收藏 0.2ms、剪贴板 29–809µs）——
    慢的是「等整批 + 120ms 轮询」，不是查询本身。顺手把轮询初始值改成 `pending = true`。
  - 单测：`App` 在 `cfg!(test)` 下关闭剪贴板（`clipboard_on`），否则测试会读真实剪贴板，
    并行时还会被 `STATUS_HEAP_CORRUPTION` 崩（实测碰到过一次）。

- **输入框 provider chip（2026-11-19）**：关键字一命中 provider（`@pi`，**不必敲空格** ——
  列表就是那一刻切到 pi 会话的），输入框最前面那段就从纯文本变成原子胶囊
  （字形 + 关键字，本机 `@` 前缀下是 `(π pi)`），后面继续打的才是查询词。
  状态是 `App::chip_end`（**字符**下标，`0..chip_end`），在 `refresh_with` 里算：
  **只看 `Parsed::Command`**。曾经加过「必须跟空格」的条件，结果列表已经换了、chip 还
  没出现，看着错位（2026-11-19 用户反馈，已改掉）。代价：关键字打错（`@pix`）chip
  会消失回普通文字，只能整块删重打 —— 原子语义本意。
  **`providers::parse` 一行没动**：`--query "@pi"`、TUI 列表行为与改动前完全一致，
  变的只是 chip 跟着 parse 的结论走。
  编辑上真原子：`snap_cursor` 在 `refresh_with` 末尾统一夹光标（覆盖输入 / 粘贴 / IME），
  `Backspace` 贴着 chip 一次删掉整块（含分隔空格；`@pi` 无空格时直接清空），
  `Ctrl+Backspace` 反向跳词落到 0 而不是停在 `@pi` 中间，`Home` 落在 chip 之后。
  `@pi:clean` / `@pi:clean:week` 这类带修饰符的整体成一块，chip 跟着变长。
  GUI 在 `gui/mod.rs::input_row` 画胶囊（`style::CHIP_INPUT_*` 常量），光标位置用
  `text_width` 实量 chip 宽度 + 后续文本，`bounds_for_range`（IME 候选窗）也补了偏移；
  选中覆盖 chip 时底色亮一档，chip 不被当普通文字重画。TUI **仍只有黄色高亮**，
  但原子语义在 app 层，TUI 一样生效。
  实测：`@p` / `@pix`（未命中）不成块且回普通文字、`@pi`（无空格）chip 与列表同时出现、
  `@pi abc` chip + 光标贴字、删完 abc 再退格整块消失回开屏、`@pi` 时一次退格直接清空、
  Shift+Home 选中 chip 亮一档。
- **`@pi` 显示会话名（2026-11-18 深夜）** ← **已作废**（2026-11-19 删掉整个 `@pi`，
  相关源码 `pi.rs` / `pi_index.rs` 已删除，只留这段历史备查）：标题不再取「第一条用户消息」。pi 的自动标题
  （auto-title，每 4 轮重评）写在 jsonl 的 `{"type":"session_info","name":…}` 里，
  **位置不固定**（改标题就追加一条；本机 448 个会话里最后一条距末尾中位 8KB、p90 648KB、
  最大 33MB），读前 N 行 / 倒读都拿不全，全文件扫一次又是 652MB。
  权威源是 pi-sessions 的 `~/.pi/agent/pi-sessions/index.sqlite` 的 `sessions.session_name`
  （`src/providers/pi_index.rs`，只读打开）：每个 `turn_end` 由 hook 同步，pi 启动还会跟
  jsonl 对账 → **标题改了下一轮就生效，不需要自己缓存**。目录读 `settings.json` 的
  `sessions.index.dir`，任何失败（没装 / 重建中 / schema 升级 / 忙）都退化成空表。
  优先级：索引名 > jsonl 头 80 行的 `session_info` > `session-keywords.json` 的 summary >
  首条消息 > 文件名。**归档目录不在索引里**（本机 235 条索引 0 条归档）→
  `:pi:all` / `:pi:clean:restore` 走 jsonl 回退（老会话没 `session_info`，退化是正常的）。
  `rusqlite` 本来就在依赖里（`web.rs` 读 Edge History），零新增。
  实测：`--query "@pi"` 234 条全是会话名、热跑 597–599ms（改前 650ms）；GUI 截图确认。
- **图标 73/74 + 自定义 icon + 节能回退（2026-11-18 深夜）**：
  - `[[apps]] icon = "..."`（png/jpg/ico）→ `appicon::load_image`，给自动解析
    推不出图的条目用（本机 pi 跑在 wt.exe 里，用 SVG 转出的三色方块 `icons/pi.png`）
  - 开始菜单索引收 `.url`（Steam 只给游戏生成 `.url`，`IconFile=` 就是游戏 ico）
    → Big Walk 这类只有 `.url` 的游戏也出图；`How to Fish` 在 Steam 商店 CDN 上
    也是 404（没上架 / 没宣传图），只能等它上架
  - 节能模式 / 关闭透明效果 → `opaque_fallback` 改画不透明底色（1.5s 轮询）。
    判据：`EnableTransparency` + `SystemStatusFlag`（节电）+ **`EnergySaverState == 1`**
    （和 `:set` 的「节能模式」同一个来源）。**别用电源计划判断** —— 本机活动计划
    恒为节能计划 `a1841308-…`，跟那个开关不是一回事，会把亚克力永久判死。
    可用性变化后 `acrylic_refresh` 会重新下发 accent policy（否则切回来仍是黑底）
  - `--icons` 也会报自定义图标；`GenP` 条目已从配置删掉（指向的 exe 不存在），
    现在 74 条里只剩 `How to Fish` 没图
- **图标覆盖率 72/75（2026-11-18 晚）** ← 已被上文「图标 73/74」取代（2026-11-18）：
  `tupa-launcher.exe --icons` 能逐条报出
  每个 app 的图标来源。本轮修好的：MSIX（wt / pwsh / CeleClip → symlink 跟到包内
  真 exe → `shell:AppsFolder\<PFN>!App`）、Scoop shim（`shims\x.exe` → `apps\x\<ver>`）、
  UWP（`ms-windows-store:` / `ms-settings:` → AUMID）、Steam 封面语言变体
  （`library_600x900_schinese.jpg`）。开始菜单索引从 `Start Menu\Programs` 提到
  `Start Menu`（顶层 .lnk 也算），名字比对支持「去括号 / 取括号内」。
  剩 3 个：`GenP`（配置指向的 exe 已不存在）、`Big Walk` / `How to Fish`
  （本地无封面，走 CDN 兜底，见下）。
- **Steam 封面 CDN 兜底（2026-11-18 晚）**：本地 librarycache 没有图时，后台线程拉
  `cdn.akamai.steamstatic.com/steam/apps/<appid>/header.jpg`，缓存到
  `~/.config/tupa-launcher/icons/steam/<appid>.bin`（空文件 = 404 不重试）。
  `favicon::https_get` 是泛化后的 `http_get(host, port, secure, path)`，两边共用。
- **选框 / hover 改淡填充（2026-11-18 晚）**：`row_selected` 固定 alpha 0.2、
  `row_hover` = `opacity*0.35`（夹 0.1..0.3）。原因：叠色只能增加不透明度，
  0.6 的填充合成到 0.84，亚克力只剩 16%，肉眼就是「完全不透」；选中识别交给左侧绿条。
- **Steam 游戏图标（2026-11-18 晚）**：`steam://rungameid/<appid>` → 读注册表
  `HKCU\Software\Valve\Steam\SteamPath` → `userdata/<id>/config/grid/<appid>p.png`
  （用户自定义）优先，其次 `appcache/librarycache/<appid>/` 的
  `library_600x900.jpg` / `library_header.jpg` / `header.jpg`；居中裁方形缩到 96px。
  **不联网**，未安装的游戏回落 mini chip。`logo.png` 不用（横向文字 logo 缩到 20px 认不出）。
  相关：`windows` crate 的 features 加了 `Win32_System_Registry`。
- **hover / 选框透出亚克力 + MSIX 图标 + 提示自动回落（2026-11-18 晚）**：
  `Palette::with_opacity(opacity)` 让 `row_hover` / `row_selected` 带 alpha（否则实色
  盖住亚克力）；`appicon::exe_for` 跳过 `\WindowsApps\` 改走开始菜单（修 `wt.exe`
  的「白纸 + 蓝块」）；状态栏临时提示 `STATUS_HOLD = 2s` 后回落到 provider 描述。
- **`@exp` 已删除（2026-11-18 晚）**：`explorer_watch.rs` 保留，但不再作为 provider
  出现在命令列表（用户觉得不好用）。
- **非文件类条目也有图标了（2026-11-18 晚）**：`:set` 开关是 toggle、动作项是齿轮、
  `:run` / `:go:pi` 是终端、pi 会话是希腊字母 **π**、`:pi:clean` 是归档箱 / 垃圾桶、
  `:blog` 是服务器 / 停止圈 / 铅笔、命令列表是问号、其余 `FillInput` 是右箭头。
  决策逻辑在 `Panel::glyph_for`（纯函数，单测覆盖全分支），颜色仍走 `icon_fg`
  （未选中灰、选中类型色）。没有图标的条目（如 `steam://` 启动项）照旧回落 mini chip。
- **本机网页图标切回地球**：`config.toml` 加了 `[gui] web_icons = "globe"`（代码默认值
  不变，仍是 favicon）。favicon 有磁盘缓存（`~/.config/tupa-launcher/icons/<站点>.bin`，
  失败也写空文件、不重试），但**首次**遇到新站点仍会 spawn 线程走 WinHTTP（两次尝试
  加一次首页 HTML），站点一多就有体感卡顿。想换回来改这一行即可。
- **历史死链回填（2026-11-18）**：开屏 / `@hist` 里指向已删 `.lnk` 的应用条目（本机 OBS 的
  scoop 旧链、DeepSeek Harness 的旧链）现在是**展示前按标题回填当前 `[[apps]]`**
  （`Item::rebind_launch`，只动 `Launch`，且在去重之前调）—— 两条同名历史会被合并成一条，
  图标也顺带回来了。同时清了 `history.json` 里 5 条真死链（`@ev tools\timeline-tui.cmd`、
  `@ev my-dot.`、`@go Downloads/blog/Themes` 三条相对路径），备份 `history.json.bak-20261118-141813`。
  审计脚本：`python` 逐条看 `Action` 能不能解析（路径存在 / `which` / 协议前缀）。

- **图标全行覆盖 + favicon（2026-11-18 晚）**：类型列现在是三层来源（`RowIcon`）：
  **位图**（应用 SHELL 图标 / 网页 favicon）> **Nerd Font 字形**（文件 / 目录 / 网页地球）
  > **chip**（`cfg` / `run` / 老 `hist`）。文件图标不用图片 —— `JetBrains Maple Mono`
  自带完整 Nerd Font，`src/gui/glyphs.rs` 直接给 PUA 码点，**映射抄 eza**（按语言给专属
  图标：Rust / Python / Go / JS / TS / C / C++…）。网页图标走 `src/gui/favicon.rs`：
  `GET /favicon.ico` → 404 就翻首页的 `<link rel="icon">`（实测 `pwn.college` 就是 404），
  WinHTTP 在后台线程跑，缓存 `~/.config/tupa-launcher/icons/<站点>.bin`（失败写空文件），
  开关 `[gui] web_icons = "favicon"（默认）| "globe"`。新配置项已同步 `config.example.toml`。
  字形图标的颜色用 `icon_fg()`（`mix(p.dim, 类型色, chip_progress)`）：**未选中中性灰、
  选中才上类型色**（`chip_fg` 是 chip 文字专用的反白版，图标照抄会在选中时消失）；
  位图图标是彩色的不参与。
- **应用图标 + mini chip（2026-11-18 晚）**：卡片第一行图标顶替 chip，第二行图标下挂
  **mini chip**（34×14、字号 9，只在该行有图标时画，两行 `gap = 0` 贴着图标）；
  第二行用与第一行**镜像**的 flex 结构，副标题自动对齐（`card_indent()` 已删）。
  字形图标用 `GLYPH_PAD_LEFT = 17` 定位（字形 advance 是 20 逻辑像素、ink 还偏左 2；
  像素级标定：图标与 mini chip 的中心都是 139.5 物理像素）。
  单行密度模式只保留图标。
  应用图标三层兜底（`appicon::exe_for`）：直接 `.exe` → `.cmd` / 无扩展名 shim 向上一级
  找同名 `.exe`（VSCode `bin\code` → `Code.exe`，不做这步图标是「白纸 + 齿轮」）→
  开始菜单 `.lnk` 索引（按标题 / 命令名，`OnceLock` 建一次）。
- **窗口高度与提前滚动（2026-11-18）**：`MAX_HEIGHT_RATIO` → **0.68**（本机 960 逻辑高，
  实测窗口 638px、**11 条卡片**），`SCROLL_MARGIN_ROWS` → **2**（选框距边缘还有 2 条就开滚，
  实测 24/30 时下方仍露 2 条）。**可视条数不写死**：`max_visible_rows()` 用 `max_height`
  减去与 `desired_height()` 同一份 chrome 后除以 `row_height()` 反算，
  `CARD_VISIBLE_ROWS`（12）/ `MAX_VISIBLE_ROWS`（19）只当上限。

- **亚克力（窗口模糊）通了（2026-11-18）**：`[gui] acrylic = true` → 面板背后是**实时**模糊桌面
  （能采到别的窗口，不只是壁纸），实测棋盘格铺底只剩一片模糊明暗。
  实现只有两处：`gui::run` 把 `window_background` 设成 `Blurred`，
  `src/gui/acrylic.rs` 再补一次 `SetWindowCompositionAttribute`
  （**gradient alpha 必须是 1**；GPUI 0.2.2 传的是 0，会变成整窗黑玻璃）。
  **不用 fork GPUI、不用 Win2D、不写 blur shader。**
  ← 修正（2026-11-18 晚）：模糊本身确实没改 GPUI 渲染，但为了窗口 insets 后来还是
    vendor 了一份 gpui（`vendor/gpui`，见下一条），这句里的「不用 fork」只对 blur 成立。
  之前判成「做不到」的真因是**节能模式**（`GetSystemPowerStatus().SystemStatusFlag == 1`）
  或系统「透明效果」关闭：那时 DWM 把 Acrylic / Mica / host backdrop 全降级成不透明纯色，
  所有材质 API 都「调用成功但没效果」。排查顺序、探针对照表、Zed / ELYTerminal 的做法
  见 `docs/06-handover.md` §6「窗口模糊」。`examples/acrylic_probe.rs` 留着做回归
  （`host` 模式就是后备方案 —— WinRT host backdrop brush —— 的真模糊验证）。
- **窗口形状 / 圆角（2026-11-18）**：`vendor/gpui` + `[patch.crates-io]`，只改
  `events.rs::get_client_area_insets` 返回 0（客户区铺满窗口，内容能盖到边缘）；
  `[gui] rounded`（默认 true，外框 8px）控制圆角。
  **残留**：圆角外侧剩一点点方形亚克力 —— DWM 的 `SetWindowCompositionAttribute`
  按整个窗口矩形铺、系统圆角只裁内容，这是机制不是参数；要彻底干净只能上应用层模糊
  （呼出前截一帧 + `paint_image`，静态帧）。
  排查时**先确认窗口上没有残留的 `DWMWCP_DONOTROUND`**：`make_overlay_window` 里原来的
  `tune_dwm()` 设过它，会把之后所有 DWM 侧调整都盖掉（这次绕了几小时就是被它藏住）。
  `[gui] always_on_top`（默认 true）走 TOPMOST；
  **顺序**：`SWP_FRAMECHANGED` 会把 composition attribute 重置成 alpha = 0（黑玻璃），
  亚克力的 alpha = 1 补丁必须在 `frame::apply` 之后打（首帧里）。
- **失焦与光标（2026-11-18）**：窗口不激活时**光标隐藏**、且不推帧（轮询里读
  `window.is_window_active()`），重新聚焦后恢复 500ms 闪烁。
  本机 `[gui] opacity` 调到 0.6（纯配置，不在仓库）。
- **显示层：两行卡片（2026-11-18）**：普通模式从「标题 + 一条 ` · ` 拼起来的附注」
  改成**两行卡片**（第一行 标题 + 右侧状态/次数/时间，第二行 路径/分组 dim 小字），
  `Ctrl+Tab` 切**单行密度表**（一屏 12 条）。要点：
  - 卡片右列宽度按内容量（`card_parts` 拆 `trailing`），写死宽度会把
    「云上极速 · 13 小时前」裁成「云上极速」
  - 行高 / 可视条数 / 窗口高度统一到 `row_height()` + `max_visible_rows()` +
    `desired_height()` 三处，不一致会「最后一行被裁」或「选框越往下越偏」
  - 竖排的 `render_detail_row` 已删（GUI 不再用）；**TUI 的 Ctrl+Tab 仍是竖排详情**
    （`ui.rs`），所以 `detail_mode_on/off` 文案按 GUI 语义写，TUI 会显示同样的字
  - 卡片模式下 `App::detail` 语义变成「单行密度」，不再是「展开每列」

- **输入框编辑 / 滚动条 / 控件状态列（2026-11-18）**：
  - 输入框：选区锚点 `App::anchor` + `Shift+←/→/Home/End`；`Ctrl+←/→` 按词跳
    （路径里的 `\` `.` `:` 也算分隔，能逐段跳）；`Ctrl+A`；`Ctrl+C/X/V`
    （新增 `clipboard::set_text()`）；有选区时打字 / 退格 / 粘贴覆盖选区；
    `selected_text_range` 如实上报（之前永远零宽，IME 会把选区当零点）
  - **`Ctrl+C` 改成复制，退出统一 `Ctrl+Q`** —— `on_key` 是 TUI/GUI 共用的，两边一起变
  - 列表右侧 3px 滚动条：高度按「可视/内容」、位置跟 `scroll_y`，内容不足一屏不画；
    **只做指示**（不可拖、不淡入，这两项 2026-11-18 明确不做）
  - **超宽滚动补全**：`needs_scroll_raw()` 之前只看标题列，所以附注列 / detail 明细列
    超宽时压根不推帧，`ping_pong` 停在第一帧（看着像「只播一次」）。现在按模式检查
    所有会滚的列，detail 下每条明细列各自滚
  - **`@set` 的状态列回来了**：TUI 的「开 / 关 / 73%」是 `ui.rs` 按 `Control` 追加的，
    GUI 从没画过 → 只剩两列。现在普通模式单独一列（带颜色），detail 追加一行；
    行高与窗口高共用 `detail_extra_rows()`
  - **`?` 帮助**：中文输入法打的是全角「？」（`keys::to_key` 不认，字符会漏进输入框）
    → 归一 + 「输入框为空时 ?/？ 当帮助键」+ 50ms `text_block` 吞掉那次 `WM_CHAR`；
    帮助页补了新键位，`Alt+0` 改成不写死
  - **鼠标拖动选择不做**（2026-11-18 决定）：只支持键盘选区，要按字形宽度反查字符下标，
    收益不抵复杂度
  - 顺手修 clippy：`--features gui` 下 `icon::destroy_icon`（裸指针参数）报
    `not_unsafe_ptr_arg_deref` —— 这个 error 一直藏着，因为默认构建不编译 `src/gui/`

- **GPUI 面板全线打通（2026-11-17）**：自绘 GUI 从「winit 把字符网格贴成像素」换成 **GPUI** 重写。
  旧的 `src/gui/`（1266 行）与 winit / cosmic-text / softbuffer / unicode-script 四个依赖已删。
  现在是**两个 exe**（`src/lib.rs` 共享模块）：
  - `tupa-launcher.exe` —— TUI + 守护进程，**不链接 gpui**
  - `tupa-gui.exe` —— GPUI 面板，`required-features = ["gui"]`，默认 `cargo build` 不产出
  - 面板能力：无边框圆角 + 1px 草绿边框、`TOOLWINDOW`（不占任务栏/Alt-Tab）、可拖边缩放、
    `Esc` 隐藏、托盘图标（GDI 现画的草绿 ❯）+ 右键菜单、自带 `Alt+0` 热键（`[gui] hotkey`）
    ← 已改绑 `alt+\``（2026-11-17，见下面「主入口已切到 GUI」）
  - 交互：紧凑形态（呼出只有输入行，按 `↓` 拉下开屏列表）、`uniform_list` 虚拟化、
    选中跟手滚动、选中行超宽左右滚动（复用 `ui::scroll_offset`）、双击执行、hover 高亮、
    `Ctrl+Tab` detail、收藏 ★、选择模式 1–9 序号、`?` 键位表
    ← `uniform_list` 与 `ui::scroll_offset` 都已被取代（2026-11-17）：`scroll_to_item` 只能瞬跳，
    做不出平滑滚动；现在是自己算可视窗口 + 像素级插值，`scroll_offset` 只留给 TUI
  - 输入：中文输入法走 `EntityInputHandler`（预编辑画在光标处、候选窗贴光标）、光标跟手 + 闪烁
  - 视觉：chip 胶囊两档（灰底 + 类型色字 / 选中反白）、行高 28、半透明开关 `[gui] opacity`
    ← chip 已改成颜色渐变（2026-11-17）：`style::mix()` 在两组色之间插值，旧行 140ms 渐隐、
    新行渐亮；「两档」这个说法只对颜色终值成立
  - 与 WM 的关系：GlazeWM 里 `tupa-gui` 是 **`ignore`**（浮层不归 workspace 管 —— 被接管会被
    tiling 挤压、切 workspace 时被 `SW_HIDE`）；尺寸由 GPUI 按主屏 62%×55% 居中算
  - **主入口已切到 GUI**（2026-11-17 晚）：`[gui] hotkey = "alt+\`"`，开机自启是
    `启动	tupa-gui.lnk`；旧的 `tupa-launcher-daemon.lnk` 已**删到回收站**（2026-11-18；
    别用改扩展名的方式停用，原因见 `AGENTS.md` 硬约定「停用启动项要把它移出启动文件夹」）。
    守护进程在跑时会占着热键，切换前先停它
  - 踩过的坑（4 条 GPUI 行为 + 2 条崩溃）都记在 `AGENTS.md`，别重踩

- **动画体系（2026-11-17 晚）**：全部由 **vsync 驱动**（`render` 末尾按需注册
  `window.on_next_frame`，回调里推一帧 + notify，直到动画结束），不再用定时器 ——
  定时器 notify 会和 120Hz 屏的 vsync 抢节拍，表现是"卡卡的"。
  - 列表滚动：自己算可视窗口 + 垂直偏移，`scroll_y` 向目标插值（0.28）
  - 选框：独立绝对定位层，`marker_y` 插值（0.55）；坐标基准是 `marker_y - first*row_h`
    （inner 只渲染可视区那几行，第一行在 y=0）
  - chip：`marker_moving` 期间不点亮；新行按时间渐入、旧行按时间渐隐（140ms）
  - 光标：绝对定位，`caret_x` 插值（0.55）+ 折叠闪烁；IME 预编辑时跟在拼音后面
  - `[gui] animations = false` 一键全关（连 `on_next_frame` 都不注册，静止零重绘）
- **detail 模式并进统一列表（2026-11-17 晚）**：行高按「最大列数」统一后，和普通模式共用
  虚拟窗口 / 平滑滚动 / marker 滑动。之前它自己走 `overflow_y_scroll` + 手画选中行，
  所以既没有选框动画、也没有 chip 渐变。

- 版本 `Cargo.toml` **0.10.0**（GUI 拆成独立 exe `tupa-gui.exe`；之前 0.9.0 是 `:blog` 取代 `:new`）
- **TUI 面板稳定**：`app` `ev` `run` `pi` `go` `web` `blog` `fav` `cfg` `set` + 开屏 `home`（收藏 + 最近命令）
- **`:blog` 取代了旧的 `:new`（2026-10-25）**：`src/providers/blog.rs`。不带参数列三个入口
  （本地 `localhost:4000` / GitHub Pages / Cloudflare），`:blog:new <标题>` 在
  `<blog>/source/_posts` 写 front matter 并用 VSCode 打开（同名不覆盖、空参数列最近文章），
  `:blog:manage` 后台跑 `pnpm admin` + 1.5s 后打开 `127.0.0.1:5173`。
  目录与网址在 `config.toml` 的 `[blog]`（`dir` 留空 = `~/blog`）；
  旧的 `new` provider / `Action::CreateFile` / `general.new_files_dir` 已删干净
- **自绘 GUI 已能跑**：`src/gui/`（winit + cosmic-text + softbuffer），窗口 / 字体 / 键位 / 中文输入法（IME）/ daemon 接管都已实现；
  用 `--gui` 或在 `config.toml` 里设 `[daemon] panel = "gui"` 打开
  ← 「直接打开」这部分已被 2026-11-17「结构优化四连」取代：GUI 转入 feature gate，
  默认构建不含，要用得先 `cargo build --release --features gui`（其余描述仍有效）
- **但默认仍是 `tui`**：`daemon::launch_panel` 按 `[daemon] panel` 分流；
  **自绘 GUI 已冷冻（2026-11-06）**：圆角 / 无边框 / 居中这些由现成终端 + Win11 DWM + GlazeWM 覆盖，
  `src/gui/` 保留当资产，不再投时间（`docs/07-shell-handover.md` 文首已标停）
- **`:pi:clean` 已加入（2026-10-25）** ← **已作废**（2026-11-19 清理功能迁出到
  `my-pi-plugins` 的 `/clean`，见 `docs/09-clean-api.md`；`clean.rs` 已删）：清理视图挂在 `:pi` 上（`clean` 修饰符，
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
- 用户本机实际配置：`command_prefix = "@"`、`panel = "tui"`、`terminal = "auto"`（=WezTerm）、
  热键 `alt+\``、`default_provider = "all"`
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
- **新窗口执行修好了（2026-11-03）**：`:pi` 回车只开终端、不启动 pi，报
  `[错误 2147942402 (0x80070002)] 系统找不到指定的文件`。原因：`spawn_console` 把
  `Set-Location -LiteralPath '…'; <命令>` 当 `-Command` 参数交给 wt，而 **wt 把 `;` 当自己的
  命令分隔符（引号里也照切）**，第二段带前导空格的 `" pi --session …"` 被当成可执行文件。
  现在：起始目录用 `wt -d <目录>`，PowerShell 脚本用 `-EncodedCommand`（UTF-16LE base64，
  不含 `;` / 空格 / 引号）。`spawn_console` 是共用的，所以 `:run` 的可见窗口、
  `:go:yazi` / `:go:pi` 一并修好；坑已记入 `docs/06-handover.md` §4
- **开屏列版式单列一路（2026-11-04）**：`RowLayout::{Aligned,Natural}` 挂在
  `Provider::layout()`，随结果下发到 `App.layout`。**只有 `home` 是 `Natural`**（每段按自己
  长度排、段间 2 空格、行尾不填充，整行超宽才按 `Kind::columns()` 压缩）；其余列表仍是对齐表。
  版式来源统一到 `Kind::columns()`（引用各 provider 的 `COLUMNS` 常量，`pub(crate)`）
- **开屏最后一列 = 启动器使用次数（2026-11-04）**：`history::hits_of()`（同 `action` 多条相加）。
  web 行原来那列是 **Edge 浏览次数**（快照值、会过期），先摘掉再换。只有 ≤2 列的行补这一列，
  `cfg`（位置+说明）/ `pi`（目录+时间）不追加。**`@web` 里仍是 Edge 次数**
- **开屏点击会计数（2026-11-04）**：输入框空时 `execute()` 成功走 `history::record_open()`：
  按 `Action` 找旧记录 `hits+1`，找不到就用 `<前缀><provider> <完整标识>` 补一条（`Ctrl+↑` 可重放）。
  原 bug：`push` 拿空输入行当去重键，直接被丢弃。收藏的列同时跟历史走（**第一列保留收藏名**）、
  排序借 `history::score_of()`；`Item::normalize_web()` 懒修旧的「域名」列（不碰磁盘）
- **`:web` 列 `[4,6,1]`（2026-11-04）**：第二列从域名换成完整 URL（同站点不同页面分得清）；
  直开条目（`@web github.com/x`）只有两列 `URL | 直接打开`
- **`:blog` 升级为「入口 + 本地服务管理」（2026-11-05）**：不带参数列**五个入口**
  （新建文章 / 本地预览 / 管理页 / GitHub Pages / Cloudflare），前两条带**查询时现探的
  状态列**（已在运行 / 未启动）。新建文章条目只是 `Action::FillInput`（补成 `<前缀>blog:new `）。
  本文件上方那段「三个入口」的描述已作废
- **本地服务「按需启动 + 可停」，新增 `src/serve.rs`（2026-11-05）**：`port_of` /
  `is_listening`（**IPv4 与 IPv6 都探**，Node 不指定 host 时可能只绑 `::1`）/
  `wait_until_ready`（`READY_TIMEOUT` **20s**，实测 `pnpm server` 就绪 **5.2–5.6s**）/
  `listeners`（`netstat -ano` 取 PID，`0.0.0.0` 与 `[::]` 去重）/ `kill_pid`
  （`taskkill /T /F`，连子进程）。`exec::serve_and_open` 统一「在跑只开浏览器，没跑才
  spawn 并等端口」，修掉旧 `blog_admin` 无条件 spawn（连点两次端口冲突 → 白屏）与
  固定 `sleep 1.5s`（浏览器先到、页面没好）。停机：运行中的服务紧跟一条「停止本地预览 /
  停止管理页」（**只杀 node/pnpm/cmd**，端口被别人占着只报错不动手），`:blog:stop` 停全部。
  `stop` 已加进 `providers::KNOWN_FLAGS`（不加会被当成 args）
- **GitHub Pages 域名修正（2026-11-05）**：`https://nitsugao.github.io`（实测 **404**）→
  `https://nitsugaoo.github.io`（实测 **200**）；用户名 `NitsugaoO` 末尾是大写 O，域名里两个 o。
  改了 `config.rs` 默认值与 `config.example.toml`（本机 `config.toml` 无 `[blog]` 段，靠默认值生效）
- **`--help` 的 `:new <文件名>` 残留已删（2026-11-05）**：改为
  `:blog [new|manage|stop]`（i18n 中英两处）
- **TUI 面板宿主改为 WezTerm（2026-11-06）**：新增 `[daemon] terminal = auto|wezterm|wt`（默认 auto）。
  `launch_wezterm` 用 `wezterm-gui --config … start --always-new-process --class tupa-launcher -- <自身>`：
  `--class` 在 Windows 上就是 win32 窗口类名（实测生效），`find_panel` 对 Tui 只认类名、不再猜标题
  （WezTerm 的标题会跟着前台进程变，实测跑 ping 时是 `PING.EXE`）。`--config` 只作用于本实例、不动
  `wezterm.lua`：`enable_tab_bar=false` / padding 全 0 / `opacity=1.0` / `win32_system_backdrop='Disable'`
  （**没有 `None` 这个合法值**）。`wezterm_gui()` 按 PATH → `%LOCALAPPDATA%\Programs\WezTerm\wezterm-nightly`
  → Program Files 探测；找不到就退回 `wt -p tupa-launcher`（`find_panel` 两种类名都认，退回后不会反复重启）。
  实测：冷启动到窗口出现 698–918ms、热键显隐 63–69ms、`SW_HIDE`/`SW_SHOW` 连做三轮稳定。
  GlazeWM 规则已加 `window_class = 'tupa-launcher'`（WT 那条降为第三顺位兜底，权威源在 my_dot，已同步）
- **空参数执行也有具体历史了（2026-11-07）**：`:go` 读剪贴板 / `:pi` 列会话这类**没打参数**的执行，
  原来拿光秃秃的 `:go` 当 `history::push` 的去重键 —— 实测本机 `"@go"` hits=8
  却只剩最后一次（`preview.md`）的快照，前面跳过的 7 个目录全被覆盖；`Ctrl+↑` 回放还会
  **重读当前剪贴板**而不是当时那个目录。现在 `app::history_line`：`args` 为空时走
  `history::line_for`，按条目自身标识拼行（Open/Reveal → `:go <完整路径>`，`OpenUrl` → `:web`，
  `LaunchInTerminal`/`RunCommand` → `:run`），修饰符保留成 `:go:yazi`；编不出可重放行的
  （`:blog` 停服务 / `:set`）保持原样。开屏走 `bump`/`record_open` 那条路不变
- **开屏第一行「剪贴板」条目（2026-11-07）**：新增 `src/clipboard.rs`（Win32 `CF_UNICODETEXT`，
  从 `go.rs` 抽出来给两处共用）。剪贴板里是**存在的路径**或 **http(s) 网址**时，`home` 在最前插
  一条 `Kind::Clip`（徽标 `clip`）：路径回车载开/定位，网址回车载开浏览器。**不抢默认选中**
  （`app::default_selection` 遇到首条 clip 选第 1 条），想用按 `↑`。开关 `[general] clipboard`
  （默认 true）。识别顺序必须**先判 URL 再 `go::normalize`**（那函数会把 `/` 换成 `\`）
- **`:exp` 已加入（2026-11-08）**：`src/providers/exp.rs`，「最近打开的文件夹」。两个数据源合并：
  `src/destlist.rs` 自己解复合文档（CFB）读 `AutomaticDestinations\f01b4d95cf55d32a.…-ms`
  的 `DestList` 流（覆盖几个月历史），加 `src/explorer_watch.rs` 记的当前窗口（秒级）；
  同一路径取时间新的，按时间倒序，只留仍存在的目录，滤掉 Installer / Temp 噪音。
  **不靠跳转列表准实时** —— 那个文件实测能连续 26 小时不写（开窗口 75 秒 / 关窗口 /
  重启 explorer 都不变），所以实时数据只能自己盯：`SetWinEventHook` 监听 `CabinetWClass`
  窗口的 `EVENT_OBJECT_NAMECHANGE` / `EVENT_SYSTEM_FOREGROUND` / `EVENT_OBJECT_SHOW`，
  命中才枚举 `ShellWindows` 写到 `explorer.json`。**钩子区间必须窄** ——
  `(EVENT_SYSTEM_FOREGROUND, EVENT_OBJECT_SHOW)` 实际是 0x0003–0x8002 的大区间，
  等于把所有事件都订上，实测 2.6% CPU；改窄区间 + 只在命中时枚举后空闲 **0.05%**。
  监听起在**守护进程**里（面板进程要等第一次热键才存在），钩子注册在 explorer_watch
  自己的线程（`WINEVENT_OUTOFCONTEXT` 要靠 `DispatchMessage` 分发，主循环不干这个）
- **本机 `config.toml` 应用列表 66 → 72（2026-11-12，只改本机配置，未动代码）**：软件迁移到 scoop 后
  4 条 lnk 失效（DeepSeek Harness / Windhawk / Clash Party / OBS Studio）已改向（Windhawk 无 lnk，写 PATH 里的
  `windhawk`），`RBTray` 改 `D:\Softwares` 便携版（与开机 Run 键、实际进程一致），新增 6 条
  `D:\Softwares` 便携软件（Kdenlive / UWPSpy / StartMenuPin / nosleep / Who Shit On My C / GenP）；
  `--check` 72 项全通。备份 `config.toml.bak-20261112-005832`
- **WezTerm 面板前端回到默认（wgpu）（2026-11-12 晚修正）**：`src/daemon.rs::launch_wezterm`
  删掉了白天加的 `--config front_end='OpenGL'`。白天那个「wgpu 在 `detached_command`
  （`CREATE_NO_WINDOW` + stdin/stdout/stderr 全接 `NUL`）下**每次都** panic」只是相关、不是因果 ——
  真因是 **GlazeWM 的 `set_transparency` 给每个窗口无条件挂 `WS_EX_LAYERED`**，wgpu 的 DX12
  swapchain 在 layered 窗口上直接 `Validation Error`。已在 OhMyGlazeWM `ee149ae7` 修掉
  （不透明窗口不再动 layered），同一处修掉的还有腾讯会议「只剩一条标题栏 + 点击全穿透」。
  实测：`Alt+\`` 冷启动 3/3 存活（每轮先强杀 wezterm-gui 再唤起），面板渲染 / 中文 / 列宽正常。
  这台是 Intel Arc B390（驱动已从 `32.0.101.8356` 升到 `32.0.101.9150`，与本次故障无关）。
  **ElyTerminal（2026-11-11 装）已排除**。
- **本机配置 OBS 条目改向（2026-11-15，只改本机配置，未动代码）**：OBS 于当天 12:23 重装成官方版
  （`C:\Program Files\obs-studio\bin\64bit\obs64.exe`，快捷方式在 ProgramData），scoop 那份没了；
  `[[apps]]` 还指着 `…\Start Menu\Programs\Scoop Apps\OBS Studio.lnk`（已不存在）→ 点了打不开。
  改向 `C:\ProgramData\…\Start Menu\Programs\OBS Studio.lnk`，已重启守护进程。
  备份 `config.toml.bak-20261115-123804`。**注意 `--check` 的盲区**：路径不存在的条目会被当成
  `shell:<路径>` 打到 `?` 里，末尾仍报「全部可解析」，查失效项要 `--check | grep 'shell:'` 看是不是
  真 shell 命令（`ms-settings:` / `steam://`）。
- **本机 `config.toml` 新增 FilmCraft（2026-11-15，只改本机配置，未动代码）**：`[[apps]]` 72 → 73，
  插在 Kdenlive 与弹弹play 之间（影音组）。command 用 ArtCraft 安装器路径
  `C:\Users\Agustin\AppData\Local\Programs\ArtCraft\FilmCraft\filmcraft.exe`（**PATH 里没有 `filmcraft`**，
  scoop manifest 存在于 extras bucket 但本机没装，开始菜单的 `FilmCraft.lnk` 只是安装器产物）。
  `--query "@app filmcraft"` 命中 1 条，`--check` **73 项全通**。备份 `config.toml.bak-20261115-142134`。
  关联动作：右键「新建 → FilmCraft Project」的脚本在 `~/Projects/system-tweaks/context-menu/filmcraft-project/`
  （HKCU ProgID `FilmCraft.Project` + `.fcproj\ShellNew\Data` 内嵌 735 字节空项目；本机原先没有 `.fcproj` 关联）。
- **本机 `config.toml` 新增 CeleClip（2026-11-16 00:55，只改本机配置，未动代码）**：`[[apps]]` 73 → 74，
  `--check` 74 项全通。command 是 `wt` + `-d …\CeleClip` + `celeclip-tui.exe` —— **必须经真终端**：
  `celeclip-tui` 是控制台 TUI，tupa 直接 spawn 会走 `detached_command` 的 `CREATE_NO_WINDOW` + `stdout=null`，
  拿不到 tty 就静默退出。备份 `config.toml.bak-20261116-005523`
- **结构优化四连（2026-11-17，行为不变，148 测试全绿）**：
  ① **GUI 用 feature gate 隔离**：`[features] gui`（默认不编译，`--gui` 要
  `cargo build --release --features gui`，无 gui 构建会报 `i18n::gui_disabled`）。
  winit / cosmic-text / softbuffer / unicode-script 四个重依赖只有 `src/gui/` 在用，
  剔除后**全量构建 70s → 33–44s、二进制 6.8MB → 4.1MB**；代码保留当资产。
  ② **exec.rs 按动作域拆散**（737 → 533 行）：会话清理（`run_session`/`run_batch`）
  归 `providers/clean.rs`、文章创建（`create_post`）归 `providers/blog.rs`；
  serve 编排**留在 exec**（serve.rs 文档写明「只放纯逻辑」，不破坏分层）。
  ③ **「加 Action 同步 4 处」变 3 处**：定位知识收拢成 `Action::reveal_target()`
  （与枚举同文件），`app::reveal_selected` 只调方法不再枚举。
  ④ **内置 PowerShell 脚本抽到 `src/scripts/*.ps1`**（21 个，`include_str!` 嵌入，
  内容逐字节一致），actions.rs 1047 → 733 行；实测 `--query "@set"` 六个 get 全部正常。
  文档同步：AGENTS.md 硬约定改 3 处、README / docs/06 §2§3§6 / docs/07 文首 / config.example。
- **`Ctrl+E` 定位后的收尾与回车统一（2026-11-17）**：`app.rs::reveal_selected` 成功后原来只设一条
  status —— 面板不关、历史也不记，`@ev` 里定位过的东西永远进不了 `Ctrl+↑`。现在统一走
  `App::finish_open`（从 `execute` 成功分支抽出）：记历史 + `should_hide`，开屏时按条目累加次数。
  没有可定位路径的条目（`:web` 行 / 命令列表 / `:set` 控件）原来静默 `return`，现在给
  `i18n::cannot_reveal()` 红提示。`Ctrl+Enter` / `Alt+Enter` 是同一条路径，行为一并变化。
  顺带把历史落盘抽成 `App::save_history`（`#[cfg(not(test))]` 才写），单测不再覆盖真实 `history.json`
- **动作类条目不再 5s 超时（2026-11-17）**：用户报 `:set` → 「更新订阅」每次报
  「执行超时（5s）：powershell -NoProfile -NonInteract…」。`src/actions.rs` 原来所有脚本共用
  5s（`TIMEOUT`），而 Clash 更新是下载订阅 + 覆盖两个 yaml + 停进程 + 800ms + 起进程 + 2s，
  **实测全程 5.2s**，必炸。现在 `SpecKind::Action` 默认 `ACTION_TIMEOUT` = 60s，
  其余（`get` 这类每次打开 `:set` 都要跑的）仍 5s；`actions.toml` 新增可选 `timeout = <秒>`。
  超时改用 `serve::kill_pid`（`taskkill /T /F`）杀进程树 —— 只 `TerminateProcess` 会留下
  子进程继续写 yaml。报错里的命令改成 `display_command` 缩写（base64 一屏装不下）。
  实测：`--apply clash-subscription` 5.2s 返回「已更新并重启 Clash Party」，`updated` 刷新

## 未完成

- **打开应用时闪两下 cmd 窗口（未定位）**：`exec.rs::spawn_executable` 三路（`.exe` 直接
  CreateProcess / `.cmd` 走 `cmd.exe /C` / `.lnk` 走 `open::that_detached` → ShellExecuteExW），
  都带了不建控制台的标志。怀疑落在**条目本身**（scoop shim / 某 `.cmd` 再起 cmd）。
  定位办法：在 `launch` 里加 `tray::log` 打出「走了哪条路 + 路径」，复现一次看
  `%TEMP%	upa-gui.log`。兜底方案是 `.exe` / `.cmd` 也统一改 `ShellExecuteExW`。
- **`/clean` 服务端（my-pi-plugins 侧）**：页面 `web/clean/index.html` 与契约
  `docs/09-clean-api.md` 已交付，插件里要实现三个接口并托管页面。
- **窗口宽度记忆**：面板宽度固定按主屏 62%（高度随内容），拖边改过的宽度还没写回
  `gui.json`（`config.toml` 手写带注释，不能程序回写）。
- **TUI 侧没跟进的新交互**：紧凑形态、双击执行、chip 配色只在 GUI 有（输入框的
  provider chip 也不例外，TUI 只把前缀画成黄色加粗）；
  `Ctrl+Tab` 在 GUI 是「两行卡片 / 单行密度」切换，TUI 仍是竖排详情（`ui.rs`），
  而文案 `detail_mode_on/off` 按 GUI 语义写，TUI 会显示同样的字。

> 2026-11-17 清理：原 6 条全部是「自动验过、只缺手点」的验收项（`@exp`、`@blog` 本地链路、
> 开屏 / `@set` 对齐、WezTerm IME、剪贴板条目、FilmCraft 条目），按新规矩不算未完成，已删。

## 入口

| 项 | 位置 |
|---|---|
| 权威源码 | `~/Projects/tupa-launcher`（git → `NitsugaoO/tupa-launcher`） |
| GPUI 面板 | `target/release/tupa-gui.exe`（`cargo build --release --features gui` 才产出）。
  配置在 `config.toml` 的 `[gui]`：`hotkey` / `tray` / `opacity` / `acrylic`；托盘左键显隐、右键菜单 |
| GlazeWM 规则 | 权威源 `~/Projects/my_dot/dotfiles/glazewm/config.yaml`（`tupa-gui` → `ignore`），
  改完 `sync-dotfiles.ps1` + `Alt+Shift+R` 热重载 |
| 生效配置 | `~/.config/tupa-launcher/{config,actions,theme}.toml`（改完下次启动生效，**不用编译**） |
| 用户数据 | `~/.config/tupa-launcher/{favorites,history}.json` |
| 守护进程日志 | `%TEMP%\tupa-launcher-daemon.log` |
| 开机自启 | 启动文件夹 `tupa-gui.lnk`（GUI 面板）；旧的守护进程 lnk 已删到回收站（**停用启动项靠移出文件夹，不靠改扩展名**） |
| 面板窗口规则 | GlazeWM `window_rules`（浮动 / 62%×55% / 居中） |
