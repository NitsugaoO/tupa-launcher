//! 中 / 英文案。
//!
//! 语言来自 `config.toml` 的 `general.language`（`zh` / `en`），启动时设置一次，
//! 之后全进程读全局值。所有用户可见的字符串都走这里，方便统一改文案。

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

static LANG: OnceLock<Lang> = OnceLock::new();

pub fn lang() -> Lang {
    *LANG.get().unwrap_or(&Lang::Zh)
}

pub fn set_lang(value: Lang) {
    let _ = LANG.set(value);
}

pub fn is_zh() -> bool {
    lang() == Lang::Zh
}

/// 按当前语言取一段文本
/// 按配置里的 `general.language` 设定界面语言（任何输出之前调用）
pub fn apply(cfg: &crate::config::Config) {
    set_lang(if cfg.general.language.eq_ignore_ascii_case("en") {
        Lang::En
    } else {
        Lang::Zh
    });
}

pub fn t(zh: &'static str, en: &'static str) -> &'static str {
    match lang() {
        Lang::Zh => zh,
        Lang::En => en,
    }
}

/// 按当前语言拼一段带参数的文本
pub fn s(zh: impl FnOnce() -> String, en: impl FnOnce() -> String) -> String {
    match lang() {
        Lang::Zh => zh(),
        Lang::En => en(),
    }
}

// ---------------------------------------------------------------------------
// provider 描述（命令列表 / 状态栏）
// ---------------------------------------------------------------------------

pub fn desc_app() -> &'static str {
    t("启动应用", "Launch apps")
}

pub fn desc_all() -> &'static str {
    t("应用 + 历史（默认搜索）", "Apps + history")
}

pub fn desc_ev() -> &'static str {
    t("搜索文件", "Search files")
}

pub fn desc_go() -> &'static str {
    t("跳转目录", "Jump to a folder")
}

pub fn age_now() -> &'static str {
    t("刚刚", "now")
}

pub fn desc_web() -> &'static str {
    t("浏览历史", "Browse history")
}

pub fn desc_hist() -> &'static str {
    t("命令历史", "Command history")
}

pub fn desc_exp() -> &'static str {
    t("最近打开的文件夹", "Recently opened folders")
}

pub fn desc_blog() -> &'static str {
    t(
        "博客：新建文章 / 管理页 / 站点入口",
        "Blog: new post, admin, site links",
    )
}

pub fn blog_local_preview() -> &'static str {
    t("本地预览", "Local preview")
}

pub fn blog_github() -> &'static str {
    t("GitHub Pages", "GitHub Pages")
}

pub fn blog_cloudflare() -> &'static str {
    t("Cloudflare", "Cloudflare")
}

pub fn blog_admin() -> &'static str {
    t("管理页", "Admin page")
}

pub fn blog_post_exists() -> &'static str {
    t("已存在，直接打开", "Exists, opening it")
}

pub fn blog_posts_dir() -> &'static str {
    t("source/_posts", "source/_posts")
}

pub fn desc_fav() -> &'static str {
    t("收藏", "Favorites")
}

pub fn desc_cfg() -> &'static str {
    t("查看配置", "Config")
}

pub fn desc_switch() -> &'static str {
    t("开关 / 选项 / 数值", "Toggles & values")
}

pub fn desc_home() -> &'static str {
    t("收藏与最近", "Favorites & recent")
}

pub fn desc_commands() -> &'static str {
    t("命令列表", "Commands")
}

// ---------------------------------------------------------------------------
// 执行结果 / 状态栏消息
// ---------------------------------------------------------------------------

pub fn opened(name: &str) -> String {
    s(|| format!("已打开 {name}"), || format!("Opened {name}"))
}

pub fn revealed(name: &str) -> String {
    s(|| format!("已定位 {name}"), || format!("Revealed {name}"))
}

pub fn launched(command: &str) -> String {
    s(
        || format!("已启动 {command}"),
        || format!("Launched {command}"),
    )
}

pub fn ran(command: &str) -> String {
    s(|| format!("已执行 {command}"), || format!("Ran {command}"))
}

pub fn blog_post_created(name: &str) -> String {
    s(
        || format!("已新建文章 {name}"),
        || format!("Created post {name}"),
    )
}

/// 本地服务已经在跑（只开浏览器，不重复起进程）
pub fn serve_already_running(url: &str) -> String {
    s(
        || format!("已在运行，正在打开 {url}"),
        || format!("Already running, opening {url}"),
    )
}

/// 刚启动的本地服务已就绪
pub fn serve_started(url: &str) -> String {
    s(
        || format!("服务已就绪，正在打开 {url}"),
        || format!("Ready, opening {url}"),
    )
}

/// 进程起来了但端口没监听上（编译报错 / 依赖没装）
pub fn serve_timeout(port: u16, command: &str, secs: u64) -> String {
    s(
        || format!("{command} 已启动，但 {secs} 秒内端口 {port} 没监听上（看终端日志排查）"),
        || format!("Started {command}, but port {port} never listened within {secs}s"),
    )
}

/// `:blog` 列表里本地服务的状态列
pub fn serve_running() -> &'static str {
    t("已在运行", "running")
}

pub fn serve_state_idle() -> &'static str {
    t("未启动，回车开", "stopped, Enter starts")
}

// --- 停止静默后台的本地服务 ---

pub fn serve_stop_label(name: &str) -> String {
    s(|| format!("停止{name}"), || format!("Stop {name}"))
}

pub fn serve_stop_hint(port: u16) -> String {
    s(
        || format!("杀掉 :{port} 上的进程（含子进程）"),
        || format!("kill the process tree on :{port}"),
    )
}

pub fn serve_stop_all() -> &'static str {
    t("停止本地服务", "Stop local services")
}

pub fn serve_stop_all_hint(count: usize) -> String {
    s(
        || format!("一次停掉 {count} 个在跑的服务"),
        || format!("stop all {count} running services"),
    )
}

pub fn serve_stopped(name: &str, pids: &str) -> String {
    s(
        || format!("已停止{name}（{pids}）"),
        || format!("Stopped {name} ({pids})"),
    )
}

pub fn serve_stop_nothing(name: &str) -> String {
    s(
        || format!("{name} 没有在运行"),
        || format!("{name} is not running"),
    )
}

/// 端口被别的东西占着（不是我们起的 node）—— 不能顺手杀
pub fn serve_not_ours(process: &str, pid: u32) -> String {
    let process = if process.is_empty() { "?" } else { process };
    s(
        || format!("端口被 {process}(PID {pid}) 占用，不是本地服务，没动它"),
        || format!("port held by {process} (PID {pid}), not a local service - left it alone"),
    )
}

pub fn serve_stop_failed(pid: u32, error: &str) -> String {
    s(
        || format!("停 PID {pid} 失败：{error}"),
        || format!("Failed to kill PID {pid}: {error}"),
    )
}

pub fn serve_stopped_all(count: usize) -> String {
    s(
        || format!("已停止 {count} 个本地服务"),
        || format!("Stopped {count} local services"),
    )
}

pub fn serve_stop_batch(done: usize, failed: &str) -> String {
    s(
        || format!("已停止 {done} 个；失败：{failed}"),
        || format!("Stopped {done}; failed: {failed}"),
    )
}

pub fn serve_stop_none() -> &'static str {
    t("没有在运行的本地服务", "No local services are running")
}

pub fn serve_stop_detail(port: u16) -> String {
    s(
        || format!("停掉监听 :{port} 的进程"),
        || format!("kill whatever listens on :{port}"),
    )
}

pub fn serve_stop_detail_all(ports: &str) -> String {
    s(
        || format!("停掉 {ports} 上的本地服务"),
        || format!("stop local services on {ports}"),
    )
}

pub fn serve_pids(list: &str) -> String {
    s(|| format!("PID {list}"), || format!("PID {list}"))
}

/// 拼接多条提示时的分隔符（中文用全角分号）
pub fn list_separator() -> &'static str {
    t("；", "; ")
}

pub fn blog_new_post_hint() -> &'static str {
    t(
        "回车后输入标题（写入 source/_posts）",
        "press Enter, then type the title (written to source/_posts)",
    )
}

pub fn blog_dir_missing(path: &str) -> String {
    s(
        || format!("博客目录不存在：{path}"),
        || format!("Blog folder not found: {path}"),
    )
}

pub fn opened_in_editor(name: &str, line: usize) -> String {
    s(
        || format!("已在编辑器打开 {name}:{line}"),
        || format!("Opened {name}:{line} in editor"),
    )
}

/// 「无法<动词> <目标>」
pub fn cannot(zh_verb: &str, en_verb: &str, target: &str) -> String {
    s(
        || format!("无法{zh_verb} {target}"),
        || format!("Cannot {en_verb} {target}"),
    )
}

pub fn filled(text: &str) -> String {
    s(|| format!("已填入 {text}"), || format!("Filled: {text}"))
}

pub fn shortcut_no_args(path: &str) -> String {
    s(
        || format!("{path} 是快捷方式，不支持附加参数"),
        || format!("{path} is a shortcut, extra arguments are not supported"),
    )
}

pub fn explorer_failed() -> &'static str {
    t("无法启动资源管理器", "Cannot start Explorer")
}

pub fn vscode_failed() -> &'static str {
    t("无法启动 VSCode", "Cannot start VSCode")
}

pub fn elevate_failed(code: isize) -> String {
    s(
        || format!("提权失败，ShellExecute 返回 {code}"),
        || format!("Elevation failed, ShellExecute returned {code}"),
    )
}

#[allow(dead_code)] // 只在 non-Windows 编译时用到
pub fn elevate_windows_only() -> &'static str {
    t("提权只在 Windows 上支持", "Elevation is Windows-only")
}

#[allow(dead_code)] // 只在无 gui feature 的构建里用到
/// GPUI 面板里 `?` 打开的键位表（`(按键, 说明)`）
pub fn loading() -> &'static str {
    t("加载中…", "Loading…")
}

pub fn gui_help() -> Vec<(&'static str, &'static str)> {
    match lang() {
        Lang::Zh => vec![
            ("↓", "进列表模式并选中下一项；列表模式里是向下移"),
            ("↑", "翻输入历史（更早一条）；列表模式里是向上移"),
            ("Enter", "执行当前项"),
            ("Space / →", "列表模式：打开当前行菜单（← 关闭）"),
            ("Home / End", "列表模式：第一条 / 最后一条"),
            ("Tab", "补全命令 / 参数（`[general] completion`，默认关）"),
            ("Shift+← / →", "选中文字（也可以 Shift+Home / End）"),
            ("Ctrl+← / →", "按词移动光标"),
            ("Ctrl+A", "全选输入框"),
            ("Ctrl+C / X / V", "复制 / 剪切 / 粘贴"),
            ("Ctrl+E", "在文件管理器中定位"),
            ("Ctrl+D", "收藏 / 取消收藏"),
            ("Ctrl+Tab", "两行卡片 / 单行密度 切换"),
            ("Shift+Tab", "选择模式（1-9 跳转）"),
            ("Esc", "逐层退：输入历史 → 列表 → 清空输入 → 隐藏面板"),
            ("打字", "列表模式下打字＝自动回输入框"),
            ("Ctrl+Q", "退出面板进程"),
            ("全局热键", "显示 / 隐藏面板（`[gui] hotkey`）"),
            ("?", "打开 / 关闭这份说明"),
        ],
        Lang::En => vec![
            ("↓", "enter list mode + next item; move down in list mode"),
            ("↑", "input history (older); move up in list mode"),
            ("Enter", "run the selected item"),
            ("Space / →", "list mode: open the row menu (← closes)"),
            ("Home / End", "list mode: first / last item"),
            ("Tab", "complete command / argument (`[general] completion`, off by default)"),
            ("Shift+← / →", "select text (or Shift+Home / End)"),
            ("Ctrl+← / →", "move by word"),
            ("Ctrl+A", "select all"),
            ("Ctrl+C / X / V", "copy / cut / paste"),
            ("Ctrl+E", "reveal in file explorer"),
            ("Ctrl+D", "toggle favorite"),
            ("Ctrl+Tab", "two-line cards / dense single line"),
            ("Shift+Tab", "select mode (1-9 jumps)"),
            ("Esc", "step back: history → list → clear input → hide"),
            ("typing", "in list mode, typing jumps back to the input"),
            ("Ctrl+Q", "quit the panel process"),
            ("global hotkey", "show / hide the panel (`[gui] hotkey`)"),
            ("?", "toggle this help"),
        ],
    }
}

pub fn gui_moved() -> &'static str {
    t(
        "GUI 面板已经是独立程序：运行同目录的 tupa-gui.exe",
        "The GUI panel is its own executable now: run tupa-gui.exe",
    )
}

pub fn gui_disabled() -> &'static str {
    t(
        "本构建未编译自绘 GUI：cargo build --release --features gui",
        "This build has no GUI: cargo build --release --features gui",
    )
}

pub fn not_found(what: &str) -> String {
    s(|| format!("找不到 {what}"), || format!("Not found: {what}"))
}

pub fn cannot_reveal() -> &'static str {
    t(
        "该条目没有可以定位的路径",
        "This item has no path to reveal",
    )
}

/// 复制 / 剪切时剪贴板写不进去（被别的程序占着等）
pub fn clipboard_failed() -> &'static str {
    t("剪贴板被占用，写入失败", "Clipboard busy, write failed")
}

/// 剪贴板里不是文本 / 读不到
pub fn clipboard_empty() -> &'static str {
    t("剪贴板里没有可粘贴的文本", "No text in the clipboard")
}

/// 没选中也没输入时按 Ctrl+C
pub fn nothing_to_copy() -> &'static str {
    t("没有可复制的内容", "Nothing to copy")
}

pub fn favorited(title: &str) -> String {
    s(
        || format!("已收藏 {title}"),
        || format!("Favorited {title}"),
    )
}

pub fn unfavorited(title: &str) -> String {
    s(
        || format!("已取消收藏 {title}"),
        || format!("Unfavorited {title}"),
    )
}

pub fn cannot_favorite_command() -> &'static str {
    t("命令列表不能收藏", "Command entries can't be favorited")
}

// ---------------------------------------------------------------------------
// 右键菜单
// ---------------------------------------------------------------------------

pub fn menu_run_as_admin() -> &'static str {
    t("以管理员身份运行", "Run as administrator")
}

pub fn menu_reveal() -> &'static str {
    t("打开所在位置", "Show in Explorer")
}

pub fn menu_favorite() -> &'static str {
    t("收藏", "Add to favorites")
}

pub fn menu_unfavorite() -> &'static str {
    t("取消收藏", "Remove from favorites")
}

pub fn menu_remove_history() -> &'static str {
    t("从最近记录移除", "Remove from recent")
}

pub fn menu_empty() -> &'static str {
    t("这一行没有可用的操作", "No actions for this row")
}

/// 正在翻输入历史时状态栏的提示
pub fn hint_history_mode() -> &'static str {
    t(
        "输入历史：↑ 更早 · ↓ 更新 · Esc 退出",
        "Input history: ↑ older · ↓ newer · Esc to exit",
    )
}

/// 列表模式（输入框失焦）时状态栏的提示
pub fn hint_list_mode() -> &'static str {
    t(
        "列表模式：↑↓ 选择 · 空格 菜单 · Enter 打开 · Esc 回到输入",
        "List mode: ↑↓ move · Space menu · Enter open · Esc to input",
    )
}

pub fn menu_removed_history(title: &str) -> String {
    s(
        || format!("已从最近记录移除 {title}"),
        || format!("Removed {title} from recent"),
    )
}

pub fn detail_mode_on() -> &'static str {
    t("单行密度模式", "Dense single-line mode")
}

pub fn detail_mode_off() -> &'static str {
    t("两行卡片模式", "Two-line card mode")
}

pub fn select_mode() -> &'static str {
    t(
        "选择模式 Tab 下一个 1-9 跳转 Esc 退出",
        "Select mode: Tab next, 1-9 jump, Esc exit",
    )
}

pub fn status_summary(
    position: usize,
    total: usize,
    description: &str,
    detail: bool,
    pending: bool,
) -> String {
    let dots = if pending { " …" } else { "" };
    if detail {
        s(
            || format!(" {position}/{total} · {description} · 详情{dots}"),
            || format!(" {position}/{total} · {description} · detail{dots}"),
        )
    } else {
        s(
            || format!(" {position}/{total} · {description}{dots}"),
            || format!(" {position}/{total} · {description}{dots}"),
        )
    }
}

pub fn shortcut_hint() -> &'static str {
    t(
        "Enter 打开 · ^E 定位 · ^D 收藏 · ^Tab 详情 · Esc 隐藏 ",
        "Enter open · ^E reveal · ^D favorite · ^Tab detail · Esc hide ",
    )
}

/// 选中开关 / 选项 / 数值时的提示（回车不隐藏面板）
pub fn shortcut_hint_control() -> &'static str {
    t(
        "Enter 切换 · ←→ 调整 · ^Tab 详情 · Esc 隐藏 ",
        "Enter apply · Left/Right adjust · ^Tab detail · Esc hide ",
    )
}

/// 状态栏里显示「候选 · 说明」
pub fn completion_description(candidate: &str, description: &str) -> String {
    s(
        || format!("{candidate} · {description}"),
        || format!("{candidate} · {description}"),
    )
}

/// 修饰符的说明（补全时显示在状态栏）
pub fn flag_description(flag: &str) -> &'static str {
    match flag {
        "admin" => t("提权执行，会弹 UAC", "Run elevated, UAC prompt"),
        "bg" => t("后台执行，输出丢弃", "Run in background"),
        "week" => t(
            "一周（:web 搜索范围 / :pi:clean 时间窗口）",
            "One week (:web range / :pi:clean window)",
        ),
        "month" => t(
            "一个月（:web 搜索范围 / :pi:clean 时间窗口）",
            "One month (:web range / :pi:clean window)",
        ),
        "clean" => t(
            "清理会话：时间窗口外的归档 / 删到回收站",
            "Clean sessions outside the time window",
        ),
        "cleanall" => t(
            "列出 1 条汇总项，回车一次处理全部候选",
            "One row to apply every candidate at once",
        ),
        "restore" => t(
            "列出归档会话，回车恢复（配 :pi:clean）",
            "List archived sessions and restore (with :pi:clean)",
        ),
        "all" => t(
            "不限时间 / 显示不可用项",
            "No time limit / show unavailable",
        ),
        "url" => t("只在网址里搜索", "Search in URLs only"),
        "title" => t("只在标题里搜索", "Search in titles only"),
        "yazi" => t("用 yazi 打开所在目录", "Open the folder with yazi"),
        "pi" => t("在目标目录启动 pi", "Start pi in that folder"),
        "code" => t("用 VSCode 打开", "Open with VSCode"),
        _ => "",
    }
}

pub fn completion_label() -> &'static str {
    t("Tab", "Tab")
}

// ---------------------------------------------------------------------------
// 空结果提示
// ---------------------------------------------------------------------------

pub fn hint_commands() -> &'static str {
    t("没有匹配的命令", "No such command")
}

pub fn hint_ev() -> &'static str {
    t("输入文件名开始搜索", "Type a file name")
}

pub fn hint_app() -> &'static str {
    t(
        "还没有应用，去 config.toml 添加",
        "No apps yet, add some in config.toml",
    )
}

pub fn hint_switch() -> &'static str {
    t(
        "没有匹配的开关，检查 ~/.config/tupa-launcher/actions.toml",
        "No matching toggles, check ~/.config/tupa-launcher/actions.toml",
    )
}

// ---------------------------------------------------------------------------
// 开关 / 选项 / 数值
// ---------------------------------------------------------------------------

pub fn control_on() -> &'static str {
    t("开", "on")
}

pub fn control_off() -> &'static str {
    t("关", "off")
}

pub fn control_unavailable() -> &'static str {
    t("不可用", "n/a")
}

pub fn control_unavailable_hint() -> &'static str {
    t(
        "读不到当前状态（设备不支持或脚本失败）",
        "Cannot read the current state (unsupported device or script error)",
    )
}

/// 状态栏：`亮度 · 80%`
pub fn control_value(title: &str, value: &str) -> String {
    s(
        || format!("{title} · {value}（回车应用）"),
        || format!("{title} · {value} (Enter to apply)"),
    )
}

/// 写操作进行中
pub fn control_applying(title: &str) -> String {
    s(
        || format!("正在设置 {title}…"),
        || format!("Applying {title}…"),
    )
}

/// 写操作完成
pub fn control_applied(title: &str, value: &str) -> String {
    s(
        || format!("{title} 已设为 {value}"),
        || format!("{title} set to {value}"),
    )
}

pub fn action_not_found(id: &str) -> String {
    s(
        || format!("找不到开关 {id}"),
        || format!("Toggle not found: {id}"),
    )
}

pub fn action_no_set(title: &str) -> String {
    s(
        || format!("{title} 没有配置 set 命令"),
        || format!("{title} has no set command"),
    )
}

pub fn action_empty_command() -> &'static str {
    t("命令为空", "Empty command")
}

pub fn script_timeout(command: &str, seconds: u64) -> String {
    s(
        || format!("执行超时（{seconds}s）：{command}"),
        || format!("Timed out after {seconds}s: {command}"),
    )
}

pub fn script_failed(command: &str, message: &str) -> String {
    if message.is_empty() {
        s(
            || format!("执行失败：{command}"),
            || format!("Command failed: {command}"),
        )
    } else {
        s(
            || format!("执行失败：{message}"),
            || format!("Command failed: {message}"),
        )
    }
}

// 内置开关的标题（id 不变，翻译不影响 `SetControl`）
pub fn action_dark_mode() -> &'static str {
    t("深色模式", "Dark mode")
}

pub fn action_appearance() -> &'static str {
    t("系统主题", "System theme")
}

pub fn action_group_appearance() -> &'static str {
    t("外观", "Appearance")
}

pub fn action_brightness() -> &'static str {
    t("屏幕亮度", "Brightness")
}

pub fn action_battery_care() -> &'static str {
    t("电池养护模式", "Battery conservation")
}

pub fn action_energy_saver() -> &'static str {
    t("节能模式", "Energy saver")
}

pub fn action_group_power() -> &'static str {
    t("电源", "Power")
}

pub fn action_clash_subscription() -> &'static str {
    t("更新订阅", "Update subscription")
}

pub fn action_group_network() -> &'static str {
    t("网络", "Network")
}

pub fn action_keep_awake() -> &'static str {
    t("保持唤醒", "Keep awake")
}

pub fn action_display() -> &'static str {
    t("内置屏幕", "Built-in display")
}

pub fn action_group_display() -> &'static str {
    t("显示", "Display")
}

// ---------------------------------------------------------------------------
// 其它
// ---------------------------------------------------------------------------

pub fn web_direct() -> &'static str {
    t("直接打开", "Open directly")
}

pub fn web_visits(count: i64) -> String {
    s(|| format!("{count} 次"), || format!("{count} visits"))
}

/// 开屏最后一列：这个条目在启动器里被执行过几次（`history.json` 的 hits），
/// 与 [`web_visits`]（浏览器访问次数）是两回事。
pub fn uses(count: u32) -> String {
    s(|| format!("{count} 次"), || format!("{count} uses"))
}

/// 开屏里「剪贴板」条目的来源列
pub fn from_clipboard() -> &'static str {
    t("剪贴板", "Clipboard")
}

pub fn config_line(line: Option<usize>) -> String {
    match line {
        Some(number) => s(|| format!("第 {number} 行"), || format!("line {number}")),
        None => t("未写入配置文件", "not in config").to_string(),
    }
}

pub fn config_default_value() -> &'static str {
    t("默认", "default")
}

// ---------------------------------------------------------------------------
// 配置项说明（`:cfg`）
// ---------------------------------------------------------------------------

pub fn hint_cfg_default_provider() -> &'static str {
    t(
        "不带命令前缀时走哪个 provider",
        "Provider used when there's no command prefix",
    )
}

pub fn hint_cfg_max_results() -> &'static str {
    t("每个 provider 最多返回多少条", "Max results per provider")
}

pub fn hint_cfg_everything_path() -> &'static str {
    t("es.exe 路径，留空自动探测", "Path to es.exe, empty = auto")
}

pub fn hint_cfg_blog_dir() -> &'static str {
    t("博客仓库根目录（留空 ~/blog）", "Blog repo root, empty = ~/blog")
}

pub fn hint_cfg_command_prefix() -> &'static str {
    t(
        "命令前缀，修饰符也用它",
        "Command prefix, also used for flags",
    )
}

pub fn hint_cfg_prefill_colon() -> &'static str {
    t("开屏是否预填命令前缀", "Prefill the command prefix on open")
}

pub fn hint_cfg_show_hints() -> &'static str {
    t(
        "右下角是否显示快捷键提示",
        "Show shortcut hints in the status bar",
    )
}

pub fn hint_cfg_language() -> &'static str {
    t("界面语言 zh 或 en", "UI language: zh or en")
}

pub fn hint_cfg_hotkey() -> &'static str {
    t(
        "唤起热键，改完重启守护进程",
        "Global hotkey, restart the daemon after changing",
    )
}

pub fn hint_cfg_panel() -> &'static str {
    t(
        "热键唤起哪种面板 tui（终端）或 gui（自绘窗口）",
        "Panel on hotkey: tui (terminal) or gui (native window)",
    )
}

pub fn hint_cfg_copilot_key() -> &'static str {
    t(
        "是否接管 Copilot 键 true 或 false",
        "Take over the Copilot key: true or false",
    )
}

pub fn hint_cfg_theme_background() -> &'static str {
    t(
        "背景色，留空用内置默认",
        "Background color, empty = built-in",
    )
}

pub fn hint_cfg_theme_accent() -> &'static str {
    t("强调色，留空用内置默认", "Accent color, empty = built-in")
}

pub fn hint_cfg_theme_fg() -> &'static str {
    t(
        "正文色，留空用内置默认",
        "Foreground color, empty = built-in",
    )
}

// ---------------------------------------------------------------------------
// 调试 / 帮助
// ---------------------------------------------------------------------------

pub fn check_ok(count: usize) -> String {
    s(
        || format!("{count} 项全部可解析"),
        || format!("All {count} entries resolved"),
    )
}

pub fn check_missing(count: usize) -> String {
    s(
        || format!("{count} 项无法解析，检查 config.toml"),
        || format!("{count} entries could not be resolved, check config.toml"),
    )
}

/// `--help` 全文
pub fn help() -> String {
    if is_zh() {
        r#"tupa-launcher   命令式启动器

用法
  tupa-launcher                    启动界面（TUI）
  tupa-launcher --gui              启动自绘窗口
  tupa-launcher --query "@ev 日志"  直接打印结果
  tupa-launcher --check            检查应用列表
  tupa-launcher --daemon           常驻，热键唤起
  tupa-launcher --resolve <路径>   查看快捷方式解析结果
  tupa-launcher --reveal <路径>    在资源管理器里定位

命令（前缀默认 :，可配置）
  :app <名字>      启动应用
  :ev <关键词>     搜索文件
  :run <命令>      执行命令
  :go <路径>       跳转目录
  :web <关键词>    搜索浏览历史
  :hist <关键词>   搜索命令历史
  :exp <关键词>    最近打开的文件夹
  :blog [new|manage|stop]  博客入口（预览 / 管理页 / 新建文章 / 停止本地服务）
  :fav [类型]      收藏
  :cfg [关键词]    查看配置
  :set [关键词]    开关 / 选项 / 数值
  不带前缀         交给默认 provider

修饰符
  admin   提权
  bg      后台执行
  week / month / all   浏览历史的时间范围
  url / title          :web 只在网址 / 标题里搜索
  yazi / pi / code     用指定程序打开目录

按键
  Enter            打开
  Tab / Shift+Tab  补全 / 选择模式
  Ctrl+E           定位
  Ctrl+D           收藏
  Ctrl+Tab         详细信息
  Ctrl+↑ / Ctrl+↓  历史输入
  ← / →            开关 / 数值时调整
  Esc              清空 / 隐藏
"#
        .to_string()
    } else {
        r#"tupa-launcher   a command launcher

Usage
  tupa-launcher                     start the UI (TUI)
  tupa-launcher --gui               start the native window
  tupa-launcher --query "@ev log"   print results
  tupa-launcher --check             check the app list
  tupa-launcher --daemon            run resident, wake with a hotkey
  tupa-launcher --resolve <path>    show shortcut resolution
  tupa-launcher --reveal <path>     reveal in Explorer

Commands (prefix defaults to :, configurable)
  :app <name>       launch an app
  :ev <keyword>     search files
  :run <command>    run a command
  :go <path>        jump to a folder
  :web <keyword>    search browser history
  :hist <keyword>   search command history
  :exp <keyword>    recently opened folders
  :blog [new|manage|stop]  blog entries (preview / admin / new post / stop)
  :fav [kind]       favorites
  :cfg [keyword]    inspect config
  :set [keyword]   toggles & values
  no prefix         goes to the default provider

Flags
  admin   elevate
  bg      run in background
  week / month / all   history time range
  url / title          :web searches URLs / titles only
  yazi / pi / code     open the folder with that program

Keys
  Enter            open
  Tab / Shift+Tab  complete / select mode
  Ctrl+E           reveal
  Ctrl+D           favorite
  Ctrl+Tab         detail mode
  Ctrl+Up / Down   input history
  Left / Right     adjust toggles & values
  Esc              clear / hide
"#
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_chinese() {
        assert!(is_zh());
        assert_eq!(t("中文", "English"), "中文");
    }

    #[test]
    fn formats_with_current_language() {
        assert_eq!(opened("x"), "已打开 x");
        assert_eq!(web_visits(3), "3 次");
        assert_eq!(uses(2), "2 次");
    }

    #[test]
    fn help_is_not_empty() {
        assert!(help().contains("tupa-launcher"));
    }
}
