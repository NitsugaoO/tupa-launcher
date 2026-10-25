//! Provider：一个关键字对应一类功能。
//!
//! **加功能 = 加一个 provider 文件**，不需要打包、不需要重启框架
//! （需求 N2）。实现 [`Provider`] trait 后在 [`build`] 里注册即可。
//!
//! 输入以 `:` 开头时进入命令模式（`:app code`）；不带 `:` 的整行是默认
//! provider 的搜索词。命令列表本身也是一个 provider（见 [`commands`]）。

pub mod apps;
pub mod blog;
pub mod clean;
pub mod commands;
pub mod config_items;
pub mod everything;
pub mod favorites;
pub mod go;
pub mod home;
pub mod pi;
pub mod run;
pub mod switch;
pub mod web;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use serde::{Deserialize, Serialize};

use crate::config::Config;

/// 结果条目的大类，决定徽标显示
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    App,
    File,
    Dir,
    Setting,
    Command,
    /// 网页（浏览器历史 / 用户直接给的 URL）
    Web,
    /// 命令列表里的命令（回车只是把输入补成 `:<命令> `）
    Help,
    /// 开关 / 选项 / 数值（`:set`）
    Switch,
    /// pi 会话
    Pi,
    /// pi 会话清理（归档 / 删除 / 恢复）
    Clean,
    /// 最近执行过的命令（开屏列表用）
    History,
}

impl Kind {
    pub fn badge(self) -> &'static str {
        match self {
            Kind::App => "app",
            Kind::File => "file",
            Kind::Dir => "dir",
            Kind::Setting => "cfg",
            Kind::Command => "run",
            Kind::Web => "web",
            Kind::Help => "cmd",
            Kind::Switch => "set",
            Kind::Pi => "pi",
            Kind::Clean => "clean",
            Kind::History => "rec",
        }
    }

    /// 从徽标反查（`fav:app` 的参数解析用）
    pub fn from_badge(badge: &str) -> Option<Kind> {
        [
            Kind::App,
            Kind::File,
            Kind::Dir,
            Kind::Setting,
            Kind::Command,
            Kind::Web,
            Kind::Help,
            Kind::Switch,
            Kind::Pi,
            Kind::Clean,
        ]
        .into_iter()
        .find(|kind| kind.badge() == badge)
    }
}

/// 一列的宽度规格。
///
/// 所有列先拿到 `min`，剩下的宽度按 `weight` 的比例分配，分配时不超过 `max`
/// （`max = 0` 表示不限）。具体实现在 `ui::layout_columns`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    /// 占多少「份」
    pub weight: u16,
    /// 最少给多少列（终端太窄时会被压缩）
    pub min: u16,
    /// 最多给多少列；`0` = 不限
    pub max: u16,
}

impl Column {
    pub const fn new(weight: u16, min: u16, max: u16) -> Self {
        Column { weight, min, max }
    }
}

/// 默认两列：标题（5 份、最少 12 列）+ 副标题（3 份）
pub const DEFAULT_COLUMNS: &[Column] = &[Column::new(5, 12, 0), Column::new(3, 0, 0)];

/// 条目的交互类型。
///
/// 默认（[`Control::Run`]）就是「回车执行、然后隐藏面板」的老行为；
/// 开关 / 选项 / 数值类条目在执行后**不隐藏**，只刷新当前列表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Control {
    /// 普通动作：回车执行并隐藏面板
    #[default]
    Run,
    /// 空格 / 回车切换开 / 关
    Toggle { value: bool },
    /// ←→ 在选项里切换
    Choice { options: Vec<String>, index: usize },
    /// ←→ 按 `step` 调整，回车提交
    Number {
        value: i64,
        min: i64,
        max: i64,
        step: i64,
        suffix: String,
    },
    /// 只读信息 + 一个动作：状态列显示 `detail`，回车执行（例如「更新订阅」）
    Action { detail: String },
    /// 读不到当前状态（脚本失败 / 设备不支持）
    Unavailable,
}

impl Control {
    /// 是不是需要面板内交互的条目（不是普通动作）
    pub fn is_control(&self) -> bool {
        !matches!(self, Control::Run)
    }

    /// 状态列显示的文本（普通条目为空）
    pub fn label(&self) -> String {
        match self {
            Control::Run => String::new(),
            Control::Toggle { value } => {
                if *value {
                    crate::i18n::control_on().to_string()
                } else {
                    crate::i18n::control_off().to_string()
                }
            }
            Control::Choice { options, index } => options.get(*index).cloned().unwrap_or_default(),
            Control::Number { value, suffix, .. } => format!("{value}{suffix}"),
            Control::Action { detail } => detail.clone(),
            Control::Unavailable => crate::i18n::control_unavailable().to_string(),
        }
    }

    /// ←→ 调整本地值。返回是否真的变了（`Unavailable` / `Run` 不处理）。
    pub fn adjust(&mut self, delta: isize) -> bool {
        match self {
            Control::Toggle { value } => {
                if delta == 0 {
                    return false;
                }
                *value = !*value;
                true
            }
            Control::Choice { options, index } => {
                if options.is_empty() {
                    return false;
                }
                let len = options.len() as isize;
                *index = (*index as isize + delta).rem_euclid(len) as usize;
                true
            }
            Control::Number {
                value,
                min,
                max,
                step,
                ..
            } => {
                *value = (*value + delta as i64 * (*step).max(1)).clamp(*min, *max);
                true
            }
            Control::Run | Control::Action { .. } | Control::Unavailable => false,
        }
    }
}

/// 选中一条结果后要执行的动作
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// 用系统默认程序打开（文件 / 文件夹）
    Open(PathBuf),
    /// 用默认浏览器打开一个网址
    OpenUrl(String),
    /// 启动一个程序
    Launch {
        command: String,
        args: Vec<String>,
        /// `true` 时通过 ShellExecute 的 `runas` 提权（会弹 UAC）
        elevated: bool,
    },
    /// 执行一条命令
    RunCommand {
        command: String,
        elevated: bool,
        /// `true` = 后台执行（输出丢弃）；`false` = 开一个可见终端窗口
        background: bool,
    },
    /// 在新终端窗口里运行一个 TUI 程序（yazi / pi）
    LaunchInTerminal {
        command: String,
        /// 启动前先切到这个目录
        dir: Option<PathBuf>,
    },
    /// 在文件资源管理器里定位
    Reveal(PathBuf),
    /// 在编辑器里打开某个文件的指定行
    OpenInEditor { file: PathBuf, line: usize },
    /// 新建一篇博客文章（已存在则不覆盖），然后在编辑器里打开
    CreateBlogPost { path: PathBuf, title: String },
    /// 后台跑 `pnpm admin`，稍后打开管理页
    BlogAdmin { dir: PathBuf, url: String },
    /// 把面板输入替换成这段文本（命令列表用，不执行任何东西）
    FillInput(String),
    /// 把 `actions.toml` 里的某个开关 / 选项 / 数值设成新值
    SetControl { id: String, value: String },
    /// 处理一个 pi 会话：归档 / 删除到回收站 / 从归档恢复
    CleanSession { path: PathBuf, mode: CleanMode },
    /// 一次处理全部候选（`:pi:cleanall`）：回车才执行
    CleanBatch { ops: Vec<CleanOp> },
}

/// 批量清理里的一项
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanOp {
    pub path: PathBuf,
    pub mode: CleanMode,
}

/// [`Action::CleanSession`] 的处置方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CleanMode {
    /// 移到 `session-archive/`
    Archive,
    /// 进系统回收站
    Trash,
    /// 从 `session-archive/` 移回 `sessions/`
    Restore,
}

/// 一条结果：徽标（由 [`Kind`] 决定）+ 若干列文本 + 动作
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub kind: Kind,
    pub columns: Vec<String>,
    pub action: Action,
    /// 交互类型。旧收藏文件里没有这个字段，读出来就是 [`Control::Run`]。
    #[serde(default)]
    pub control: Control,
}

impl Item {
    pub fn new(kind: Kind, columns: Vec<String>, action: Action) -> Self {
        Item {
            kind,
            columns,
            action,
            control: Control::Run,
        }
    }

    /// 带交互类型的条目（`:set` 用）
    pub fn with_control(
        kind: Kind,
        columns: Vec<String>,
        action: Action,
        control: Control,
    ) -> Self {
        Item {
            kind,
            columns,
            action,
            control,
        }
    }

    /// 第一列（标题）
    pub fn title(&self) -> &str {
        self.columns.first().map(String::as_str).unwrap_or("")
    }

    /// 第二列（副标题）
    pub fn subtitle(&self) -> &str {
        self.columns.get(1).map(String::as_str).unwrap_or("")
    }

    /// 详细信息模式下额外显示的一行：完整的网址 / 路径 / 命令行。
    ///
    /// 已经出现在列里的内容不重复显示（比如 app 的命令行本来就在第二列）。
    pub fn detail_line(&self) -> Option<String> {
        let text = match &self.action {
            Action::Open(path) | Action::Reveal(path) => path.display().to_string(),
            Action::OpenUrl(url) => url.clone(),
            Action::Launch { command, args, .. } => {
                if args.is_empty() {
                    command.clone()
                } else {
                    format!("{command} {}", args.join(" "))
                }
            }
            Action::RunCommand { command, .. } => command.clone(),
            Action::LaunchInTerminal { command, dir } => match dir {
                Some(dir) => format!("{command} · {}", dir.display()),
                None => command.clone(),
            },
            Action::OpenInEditor { file, line } => format!("{}:{line}", file.display()),
            Action::CreateBlogPost { path, .. } => path.display().to_string(),
            Action::BlogAdmin { dir, url } => format!("pnpm admin · {} · {url}", dir.display()),
            Action::SetControl { id, value } => format!("{id} = {value}"),
            Action::CleanSession { path, .. } => path.display().to_string(),
            Action::CleanBatch { .. } => return None,
            Action::FillInput(_) => return None,
        };

        if text.is_empty() || self.columns.iter().any(|column| column == &text) {
            return None;
        }
        Some(text)
    }

    /// 从一个文件系统路径构造条目（Everything 的结果走这里）
    pub fn from_path(path: PathBuf) -> Self {
        let is_dir = path.is_dir();
        let title = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let subtitle = path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        Item::new(
            if is_dir { Kind::Dir } else { Kind::File },
            vec![title, subtitle],
            Action::Open(path),
        )
    }
}

/// 一个功能模块
pub trait Provider: Send {
    /// 触发关键字（`:` 之后输入的第一个词）
    fn keyword(&self) -> &str;
    /// 一句话描述，显示在界面上
    fn description(&self) -> &'static str;
    /// 查询。**在后台线程里调用**，可以放心做阻塞 IO。
    ///
    /// - `args`：关键字后面（空格之后）的内容
    /// - `flags`：写在关键字上的修饰符，如 `:run:admin ipconfig` 里的 `admin`
    fn query(&self, args: &str, flags: &[String]) -> Vec<Item>;

    /// 这一组结果有几列、各自怎么分宽度（见 [`Column`]）
    fn columns(&self) -> &'static [Column] {
        DEFAULT_COLUMNS
    }

    /// 这个 provider 认识的 `:flag` 修饰符（只用于输入补全，解析仍走 [`KNOWN_FLAGS`]）。
    ///
    /// 给空表示它不认任何修饰符（例如 `:ev`）。
    fn flags(&self) -> &'static [&'static str] {
        &[]
    }
}

/// 主线程只需要知道 provider 的元信息（真正的 provider 在 worker 线程里）
pub struct ProviderInfo {
    pub keyword: String,
    pub description: String,
    /// 该 provider 支持的 `:flag` 修饰符（补全用）
    pub flags: &'static [&'static str],
}

impl ProviderInfo {
    /// 输入的词是否命中这个 provider
    pub fn matches(&self, word: &str) -> bool {
        self.keyword == word
    }
}

/// 注册所有 provider（顺序 = 界面上的优先级）
pub fn build(cfg: &Config) -> Vec<Box<dyn Provider>> {
    let mut list: Vec<Box<dyn Provider>> = Vec::new();
    list.push(Box::new(apps::Apps::new(cfg)));
    if let Some(ev) = everything::Everything::new(cfg) {
        list.push(Box::new(ev));
    }
    list.push(Box::new(run::Run));
    list.push(Box::new(pi::PiSessions));
    list.push(Box::new(go::Go::new(cfg)));
    list.push(Box::new(web::Web::new(cfg)));
    list.push(Box::new(blog::Blog::new(cfg)));
    list.push(Box::new(favorites::Favorites::new(cfg)));
    list.push(Box::new(config_items::ConfigItems));
    list.push(Box::new(switch::Switch));

    // 开屏内容（收藏 + 最近命令）：没有关键字，所以不进命令列表
    list.push(Box::new(home::Home::new(cfg)));

    // 命令列表放在最后：它要把前面所有 provider 的关键字列出来
    let metas = list
        .iter()
        .filter(|p| p.keyword() != home::KEYWORD)
        .map(|p| commands::Meta {
            keyword: p.keyword().to_string(),
            description: p.description().to_string(),
        })
        .collect();
    list.push(Box::new(commands::Commands::new(metas, cfg)));

    list
}

pub fn infos(providers: &[Box<dyn Provider>]) -> Vec<ProviderInfo> {
    providers
        .iter()
        .map(|p| ProviderInfo {
            keyword: p.keyword().to_string(),
            description: p.description().to_string(),
            flags: p.flags(),
        })
        .collect()
}

/// 命令列表 provider 在 `infos` 里的下标
pub fn commands_index(infos: &[ProviderInfo]) -> Option<usize> {
    infos
        .iter()
        .position(|info| info.keyword == commands::KEYWORD)
}

/// 认得的修饰符。不在这个列表里的修饰符内容会被当成**参数**。
///
/// 各 provider 只解释自己关心的那几个（`admin` 提权、`bg` 后台、
/// `week`/`month`/`all` 时间范围、`url` 直接打开网址）。
pub const KNOWN_FLAGS: &[&str] = &[
    "admin", "bg", "week", "month", "all", "url", "title", "yazi", "pi", "code", "clean",
    "cleanall", "restore", "new", "manage",
];

/// 输入行的解析结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// 不带 `:` 前缀：整行交给默认 provider 当搜索词
    Default,
    /// 命中了命令，`args` 已经去掉前缀和关键字
    Command {
        provider: usize,
        flags: Vec<String>,
        args: String,
    },
    /// `:` 开头但没命中命令 → 显示命令列表（`query` = 已经输入的部分）
    CommandList { query: String },
}

/// 解析输入行。
///
/// - 不以 `:` 开头 → [`Parsed::Default`]（整行是默认 provider 的搜索词）
/// - `:` 后面没有内容 → [`Parsed::CommandList`]
/// - `:app code` / `:run:admin ipconfig` → [`Parsed::Command`]
/// - `:xxx` 没命中任何命令 → [`Parsed::CommandList`]，列出近似命令
pub fn parse(infos: &[ProviderInfo], line: &str, prefix: char) -> Parsed {
    // 空输入（开屏）就是命令列表
    if line.trim().is_empty() {
        return Parsed::CommandList {
            query: String::new(),
        };
    }

    let Some(rest) = line.strip_prefix(prefix) else {
        return Parsed::Default;
    };

    let head_end = rest.find(' ').unwrap_or(rest.len());
    let head = &rest[..head_end];
    if head.is_empty() {
        return Parsed::CommandList {
            query: String::new(),
        };
    }

    // 修饰符分隔符：命令前缀本身，或者 `:`（两种写法都接受）
    let mut pieces = head.split([prefix, ':']);
    let keyword = pieces.next().unwrap_or("");
    let mut flags: Vec<String> = Vec::new();
    let mut inline_args: Vec<String> = Vec::new();
    for piece in pieces.filter(|piece| !piece.is_empty()) {
        let lowered = piece.to_ascii_lowercase();
        if KNOWN_FLAGS.contains(&lowered.as_str()) {
            flags.push(lowered);
        } else {
            inline_args.push(piece.to_string());
        }
    }

    let Some(index) = infos.iter().position(|info| info.matches(keyword)) else {
        return Parsed::CommandList {
            query: head.to_string(),
        };
    };

    let mut args = inline_args.join(" ");
    if rest.len() > head_end {
        let tail = &rest[head_end + 1..];
        if !args.is_empty() && !tail.is_empty() {
            args.push(' ');
        }
        args.push_str(tail);
    }

    Parsed::Command {
        provider: index,
        flags,
        args,
    }
}

/// 输入里「命令前缀」的结束位置（字节下标），用于高亮。
///
/// `:run:admin ipconfig` → 高亮 `:run:admin`；`:` / `:xx`（命令列表）→ 整行高亮。
pub fn prefix_end(line: &str, prefix: char) -> Option<usize> {
    let rest = line.strip_prefix(prefix)?;
    let head_end = rest.find(' ').unwrap_or(rest.len());
    Some(prefix.len_utf8() + head_end)
}

// ---------------------------------------------------------------------------
// 后台查询线程
// ---------------------------------------------------------------------------

pub enum Cmd {
    Query {
        generation: u64,
        provider: usize,
        args: String,
        flags: Vec<String>,
    },
    /// 开关 / 选项 / 数值的写操作（可能跑几百毫秒的脚本，放 worker 里）
    Apply { generation: u64, action: Action },
}

pub enum Evt {
    Results {
        generation: u64,
        items: Vec<Item>,
        columns: &'static [Column],
    },
    /// 写操作完成（`Err` 里是要显示在状态栏的错误）
    Applied {
        generation: u64,
        result: Result<String, String>,
    },
}

/// 单线程 worker：串行执行查询，并且**只执行排到最前面的那一条**，
/// 这样快速打字时不会堆积一堆 `es.exe` 进程。
pub fn spawn_worker(providers: Vec<Box<dyn Provider>>) -> (Sender<Cmd>, Receiver<Evt>) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
    let (evt_tx, evt_rx) = mpsc::channel::<Evt>();

    thread::spawn(move || {
        while let Ok(first) = cmd_rx.recv() {
            // 清空队列：`Apply` 必须一条条执行（不能丢），查询只保留最后一条，
            // 这样快速打字不会堆积一堆 `es.exe`。
            let mut latest_query = None;
            let mut queued = Some(first);
            while let Some(cmd) = queued {
                match cmd {
                    Cmd::Apply { generation, action } => {
                        let result =
                            crate::exec::run(&action).map_err(|error| format!("{error:#}"));
                        if evt_tx.send(Evt::Applied { generation, result }).is_err() {
                            return; // 主线程已退出
                        }
                    }
                    query @ Cmd::Query { .. } => latest_query = Some(query),
                }
                queued = cmd_rx.try_recv().ok();
            }

            let Some(Cmd::Query {
                generation,
                provider,
                args,
                flags,
            }) = latest_query
            else {
                continue;
            };

            let (items, columns) = match providers.get(provider) {
                Some(p) => (p.query(&args, &flags), p.columns()),
                None => (Vec::new(), DEFAULT_COLUMNS),
            };

            if evt_tx
                .send(Evt::Results {
                    generation,
                    items,
                    columns,
                })
                .is_err()
            {
                break; // 主线程已退出
            }
        }
    });

    (cmd_tx, evt_rx)
}

/// 按前缀过滤候选（前缀匹配优先，其次模糊匹配）。补全和命令列表共用。
pub fn match_prefix<'a>(candidates: impl IntoIterator<Item = &'a str>, query: &str) -> Vec<String> {
    let query = query.trim();
    if query.is_empty() {
        return candidates.into_iter().map(str::to_string).collect();
    }
    let lowered = query.to_lowercase();

    let mut hits: Vec<(i64, String)> = Vec::new();
    let matcher = SkimMatcherV2::default();
    for candidate in candidates {
        let lower = candidate.to_lowercase();
        let score = if lower.starts_with(&lowered) {
            1000 - (lower.len() as i64)
        } else if let Some(score) = matcher.fuzzy_match(&lower, &lowered) {
            score
        } else {
            continue;
        };
        hits.push((score, candidate.to_string()));
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    hits.into_iter().map(|(_, name)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn infos() -> Vec<ProviderInfo> {
        ["app", "ev", "web", ":"]
            .into_iter()
            .map(|keyword| ProviderInfo {
                keyword: keyword.to_string(),
                description: String::new(),
                flags: &[],
            })
            .collect()
    }

    #[test]
    fn parses_commands() {
        let infos = infos();
        assert_eq!(parse(&infos, "code", ':'), Parsed::Default);
        assert_eq!(
            parse(&infos, "", ':'),
            Parsed::CommandList {
                query: String::new()
            }
        );
        assert_eq!(
            parse(&infos, ":app code", ':'),
            Parsed::Command {
                provider: 0,
                flags: vec![],
                args: "code".to_string()
            }
        );
        assert_eq!(
            parse(&infos, ":app", ':'),
            Parsed::Command {
                provider: 0,
                flags: vec![],
                args: String::new()
            }
        );
        assert_eq!(
            parse(&infos, ":run", ':'),
            Parsed::CommandList {
                query: "run".to_string()
            }
        );
        assert_eq!(
            parse(&infos, ":", ':'),
            Parsed::CommandList {
                query: String::new()
            }
        );
        assert_eq!(
            parse(&infos, ":nope", ':'),
            Parsed::CommandList {
                query: "nope".to_string()
            }
        );

        // 前缀可以配置成别的字符
        assert_eq!(
            parse(&infos, "@app code", '@'),
            Parsed::Command {
                provider: 0,
                flags: vec![],
                args: "code".to_string()
            }
        );
        assert_eq!(parse(&infos, "@app code", ':'), Parsed::Default);
    }

    #[test]
    fn prefix_highlight() {
        assert_eq!(prefix_end(":app code", ':'), Some(4));
        assert_eq!(prefix_end(":app", ':'), Some(4));
        assert_eq!(prefix_end("code", ':'), None);
        assert_eq!(prefix_end("@app code", '@'), Some(4));
    }

    #[test]
    fn prefix_matching_prefers_prefix() {
        let hits = match_prefix(["app", "everything", "web"], "e");
        assert_eq!(hits.first().map(String::as_str), Some("everything"));
    }

    #[test]
    fn control_adjusts_values() {
        let mut toggle = Control::Toggle { value: false };
        assert!(toggle.adjust(1));
        assert_eq!(toggle.label(), "开");
        assert!(!toggle.adjust(0));

        let mut choice = Control::Choice {
            options: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            index: 0,
        };
        assert!(choice.adjust(-1));
        assert_eq!(choice.label(), "c");
        assert!(choice.adjust(1));
        assert_eq!(choice.label(), "a");

        let mut number = Control::Number {
            value: 95,
            min: 0,
            max: 100,
            step: 10,
            suffix: "%".to_string(),
        };
        assert!(number.adjust(1));
        assert_eq!(number.label(), "100%"); // 夹到 max
        assert!(number.adjust(-2));
        assert_eq!(number.label(), "80%");

        assert!(!Control::Unavailable.adjust(1));
        assert!(!Control::Run.adjust(1));
    }
}
