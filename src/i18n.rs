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

pub fn desc_ev() -> &'static str {
    t("搜索文件", "Search files")
}

pub fn desc_run() -> &'static str {
    t("执行命令", "Run a command")
}

pub fn desc_go() -> &'static str {
    t("跳转目录", "Jump to a folder")
}

pub fn desc_pi() -> &'static str {
    t("pi 会话", "pi sessions")
}

pub fn hint_pi() -> &'static str {
    t("输入关键词搜索 pi 会话", "Search pi sessions")
}

pub fn age_now() -> &'static str {
    t("刚刚", "now")
}

pub fn desc_web() -> &'static str {
    t("浏览历史", "Browse history")
}

pub fn desc_new() -> &'static str {
    t("新建文件", "New file")
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

pub fn created(name: &str) -> String {
    s(|| format!("已新建 {name}"), || format!("Created {name}"))
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

pub fn created_but_cannot_open(path: &str) -> String {
    s(
        || format!("已新建，但无法打开 {path}"),
        || format!("Created, but cannot open {path}"),
    )
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

pub fn not_found(what: &str) -> String {
    s(|| format!("找不到 {what}"), || format!("Not found: {what}"))
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

pub fn detail_mode_on() -> &'static str {
    t("详细信息模式", "Detail mode")
}

pub fn detail_mode_off() -> &'static str {
    t("紧凑模式", "Compact mode")
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
        "week" => t("最近一周的历史", "History from the past week"),
        "month" => t("最近一个月的历史", "History from the past month"),
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
// provider 内部文案
// ---------------------------------------------------------------------------

pub fn run_note(elevated: bool, background: bool) -> &'static str {
    match (elevated, background) {
        (true, true) => t("管理员后台执行", "Admin, in background"),
        (true, false) => t("管理员新窗口执行", "Admin, new window"),
        (false, true) => t("后台执行", "In background"),
        (false, false) => t("新窗口执行", "In a new window"),
    }
}

pub fn web_direct() -> &'static str {
    t("直接打开", "Open directly")
}

pub fn web_visits(count: i64) -> String {
    s(|| format!("{count} 次"), || format!("{count} visits"))
}

pub fn new_file_exists() -> &'static str {
    t("已存在，不覆盖", "Exists, not overwriting")
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

pub fn hint_cfg_new_files_dir() -> &'static str {
    t("new 新建文件放在哪", "Where `new` creates files")
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
  :new <文件名>    新建文件
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
  :new <file>       create a file
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
    }

    #[test]
    fn help_is_not_empty() {
        assert!(help().contains("tupa-launcher"));
    }
}
