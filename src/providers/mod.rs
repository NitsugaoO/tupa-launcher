//! Provider：一个关键字对应一类功能。
//!
//! **加功能 = 加一个 provider 文件**，不需要打包、不需要重启框架
//! （需求 N2）。实现 [`Provider`] trait 后在 [`build`] 里注册即可。

pub mod apps;
pub mod everything;

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
}

impl Kind {
    pub fn badge(self) -> &'static str {
        match self {
            Kind::App => "app",
            Kind::File => "file",
            Kind::Dir => "dir",
        }
    }
}

/// 选中一条结果后要执行的动作
#[derive(Debug, Clone)]
pub enum Action {
    /// 用系统默认程序打开（文件 / 文件夹）
    Open(PathBuf),
    /// 启动一个程序
    Launch { command: String, args: Vec<String> },
    /// 在文件资源管理器里定位
    Reveal(PathBuf),
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
    /// `args` 是关键字之后的内容（可能为空）。
    fn query(&self, args: &str) -> Vec<Item>;
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

/// 把输入行解析成 `(provider 下标, 参数)`
///
/// 规则：
/// - `ev 报告.pdf`  → （ev 的下标, "报告.pdf"）
/// - `报告.pdf`     → （默认 provider, "报告.pdf"）
pub fn resolve(infos: &[ProviderInfo], default_idx: usize, line: &str) -> (usize, String) {
    let (head, rest) = match line.split_once(' ') {
        Some((h, r)) => (h, r),
        None => (line, ""),
    };
    if head.is_empty() {
        return (default_idx, String::new());
    }
    match infos.iter().position(|i| i.matches(head)) {
        Some(i) => (i, rest.to_string()),
        None => (default_idx, line.to_string()),
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
            } = latest;

            let items = match providers.get(provider) {
                Some(p) => p.query(&args),
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
