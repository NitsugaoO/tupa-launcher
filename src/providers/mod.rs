//! Provider：一个关键字对应一类功能。
//!
//! **加功能 = 加一个 provider 文件**，不需要打包、不需要重启框架
//! （需求 N2）。实现 [`Provider`] trait 后在 [`build`] 里注册即可。

pub mod apps;
pub mod config_items;
pub mod everything;
pub mod go;
pub mod run;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use crate::config::Config;

/// 结果条目的大类，决定徽标显示
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    App,
    File,
    Dir,
    Setting,
    Command,
}

impl Kind {
    pub fn badge(self) -> &'static str {
        match self {
            Kind::App => "app",
            Kind::File => "file",
            Kind::Dir => "dir",
            Kind::Setting => "cfg",
            Kind::Command => "run",
        }
    }
}

/// 选中一条结果后要执行的动作
#[derive(Debug, Clone)]
pub enum Action {
    /// 用系统默认程序打开（文件 / 文件夹）
    Open(PathBuf),
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
    },
    /// 在文件资源管理器里定位
    Reveal(PathBuf),
    /// 在编辑器里打开某个文件的指定行
    OpenInEditor { file: PathBuf, line: usize },
}

#[derive(Debug, Clone)]
pub struct Item {
    pub kind: Kind,
    pub title: String,
    pub subtitle: String,
    pub action: Action,
}

impl Item {
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
        Item {
            kind: if is_dir { Kind::Dir } else { Kind::File },
            title,
            subtitle,
            action: Action::Open(path),
        }
    }
}

/// 一个功能模块
pub trait Provider: Send {
    /// 触发关键字（输入的第一个词）
    fn keyword(&self) -> &str;
    /// 一句话描述，显示在界面上
    fn description(&self) -> &str;
    /// 查询。**在后台线程里调用**，可以放心做阻塞 IO。
    ///
    /// - `args`：关键字后面（空格之后）的内容
    /// - `flags`：写在关键字上的修饰符，如 `run:admin ipconfig` 里的 `admin`
    fn query(&self, args: &str, flags: &[String]) -> Vec<Item>;
}

/// 主线程只需要知道 provider 的元信息（真正的 provider 在 worker 线程里）
pub struct ProviderInfo {
    pub keyword: String,
    pub description: String,
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
    list.push(Box::new(go::Go::new(cfg)));
    list.push(Box::new(config_items::ConfigItems));
    list
}

pub fn infos(providers: &[Box<dyn Provider>]) -> Vec<ProviderInfo> {
    providers
        .iter()
        .map(|p| ProviderInfo {
            keyword: p.keyword().to_string(),
            description: p.description().to_string(),
        })
        .collect()
}

/// 认得的修饰符。不在这个列表里的 `:` 后面的内容会被当成**参数**，
/// 所以 `ev:123` 也能正常工作（而不是静默把 123 丢掉）。
const KNOWN_FLAGS: &[&str] = &["admin", "elevated", "sudo"];

/// 把输入行解析成 `(provider 下标, flags, 参数)`
///
/// **命令只有在“后面跟了空格”或“带冒号”时才生效**：
///
/// - `ev 123` / `ev: 123` / `ev:` → 走 ev provider
/// - `ev`（单独一个词）         → 不触发，当普通搜索词交给默认 provider
///   （所以打 `ev` 能搜到 Everything 这个应用）
/// - `run:admin ipconfig`       → （run, ["admin"], "ipconfig"）
///
/// 关键字部分用 `:` 切分：第一段是关键字，其余是修饰符。
pub fn resolve(
    infos: &[ProviderInfo],
    default_idx: usize,
    line: &str,
) -> (usize, Vec<String>, String) {
    let (head, rest) = match line.split_once(' ') {
        Some((h, r)) => (h, r),
        None => (line, ""),
    };
    if head.is_empty() {
        return (default_idx, Vec::new(), String::new());
    }

    let has_space = line.contains(' ');
    let has_colon = head.contains(':');

    let mut pieces = head.split(':');
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

    // 单独一个词：当普通搜索词，而不是命令
    if !has_space && !has_colon {
        return (default_idx, Vec::new(), line.to_string());
    }

    match infos.iter().position(|info| info.matches(keyword)) {
        Some(index) => {
            let mut args = inline_args.join(" ");
            if !rest.is_empty() {
                if !args.is_empty() {
                    args.push(' ');
                }
                args.push_str(rest);
            }
            (index, flags, args)
        }
        // 没命中关键字：整行当参数交给默认 provider
        None => (default_idx, Vec::new(), line.to_string()),
    }
}

/// 输入的“命令前缀”在哪个位置结束（用于高亮）。
///
/// 只有真正触发了某个 provider 时才返回 —— 与 [`resolve`] 的判定保持一致。
pub fn command_prefix_end(infos: &[ProviderInfo], line: &str) -> Option<usize> {
    let head_end = line.find(' ').unwrap_or(line.len());
    let head = &line[..head_end];
    if head.is_empty() {
        return None;
    }

    let has_space = line.contains(' ');
    let has_colon = head.contains(':');
    if !has_space && !has_colon {
        return None;
    }

    let keyword = head.split(':').next().unwrap_or("");
    if infos.iter().any(|info| info.matches(keyword)) {
        Some(head_end)
    } else {
        None
    }
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
}

pub enum Evt {
    Results {
        generation: u64,
        items: Vec<Item>,
    },
}

/// 单线程 worker：串行执行查询，并且**只执行排到最前面的那一条**，
/// 这样快速打字时不会堆积一堆 `es.exe` 进程。
pub fn spawn_worker(providers: Vec<Box<dyn Provider>>) -> (Sender<Cmd>, Receiver<Evt>) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
    let (evt_tx, evt_rx) = mpsc::channel::<Evt>();

    thread::spawn(move || {
        while let Ok(first) = cmd_rx.recv() {
            // 丢弃排队中的旧请求
            let mut latest = first;
            while let Ok(next) = cmd_rx.try_recv() {
                latest = next;
            }

            let Cmd::Query {
                generation,
                provider,
                args,
                flags,
            } = latest;

            let items = match providers.get(provider) {
                Some(p) => p.query(&args, &flags),
                None => Vec::new(),
            };

            if evt_tx
                .send(Evt::Results { generation, items })
                .is_err()
            {
                break; // 主线程已退出
            }
        }
    });

    (cmd_tx, evt_rx)
}
