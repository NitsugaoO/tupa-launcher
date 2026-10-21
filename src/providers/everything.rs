//! `ev <关键词>` —— 走 Everything 的命令行 `es.exe` 搜文件。
//!
//! 直接用 es.exe 而不是自己建索引：它是这台机器上最快的文件索引，
//! 我们只负责"把结果拿过来重排、渲染、回车打开"。
//!
//! 注意：Everything 只按名字/路径排序，不区分"用户文件"和"系统文件"，
//! 所以这里会多拉一批候选，用 [`score`] 在本地重排后再截断。

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{Item, Provider};
use crate::config::Config;
use crate::util::{decode_output, hide_window};

/// 一次向后端索取多少条候选（本地重排前）
const FETCH_FACTOR: usize = 8;
const FETCH_CAP: usize = 400;

pub struct Everything {
    exe: PathBuf,
    max: usize,
    /// 小写的用户主目录，用于给"自己的文件"加权
    home: Option<String>,
}

impl Everything {
    /// 探测失败就返回 None，整个 provider 不注册（界面里就不会出现 ev）
    pub fn new(cfg: &Config) -> Option<Self> {
        let exe = if let Some(p) = cfg.general.everything_path.as_deref() {
            let path = PathBuf::from(p);
            if path.is_file() { path } else { find_es()? }
        } else {
            find_es()?
        };

        Some(Everything {
            exe,
            max: cfg.general.max_results,
            home: dirs::home_dir().map(|h| h.to_string_lossy().to_lowercase()),
        })
    }
}

/// 依次尝试：PATH → `%LOCALAPPDATA%\Everything` → Program Files
pub(crate) fn find_es() -> Option<PathBuf> {
    if let Ok(out) = Command::new("where.exe").arg("es.exe").output()
        && out.status.success()
        && let Some(line) = String::from_utf8_lossy(&out.stdout).lines().next()
    {
        let p = PathBuf::from(line.trim());
        if p.is_file() {
            return Some(p);
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = dirs::data_local_dir() {
        candidates.push(dir.join("Everything").join("es.exe"));
    }
    for base in [
        r"C:\Program Files\Everything",
        r"C:\Program Files (x86)\Everything",
    ] {
        candidates.push(PathBuf::from(base).join("es.exe"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

// ---------------------------------------------------------------------------
// 相关性打分
// ---------------------------------------------------------------------------

/// 噪音目录：命中就扣分（这些地方的命中有用率极低）
const NOISE: &[(&str, i64)] = &[
    (r"\winsxs\", 140),
    (r"\appdata\local\assembly\", 140),
    (r"\$recycle.bin\", 150),
    (r"\.vscode\extensions\", 90),
    (r"\node_modules\", 80),
    (r"\appdata\local\temp\", 70),
    (r"\site-packages\", 60),
    (r"\windows\system32\", 60),
    (r"\.git\", 50),
    (r"\windows\", 40),
    (r"\appdata\local\packages\", 40),
    (r"\programdata\", 30),
];

/// 可执行/可直接打开的文件，比同名目录更可能是想要的结果
const BOOST_EXTS: &[&str] = &["exe", "lnk", "cmd", "bat", "ps1"];

/// 从查询里挑出"普通词"——`ext:pdf`、`folder:` 这类语法不参与打分
fn plain_terms(args: &str) -> Vec<String> {
    args.split_whitespace()
        .filter(|t| !t.contains(':'))
        .filter(|t| !t.starts_with(['-', '!']))
        .map(|t| t.to_lowercase())
        .collect()
}

fn score(path: &Path, terms: &[String], home: Option<&str>) -> i64 {
    let full = path.to_string_lossy().to_lowercase();
    let mut s = 0i64;

    // 自己目录下的文件最可能是想要的那个
    if let Some(home) = home
        && full.starts_with(home)
    {
        s += 120;
    }

    for (needle, penalty) in NOISE {
        if full.contains(needle) {
            s -= penalty;
        }
    }

    // 路径越浅越好
    s -= full.matches('\\').count() as i64 * 4;

    // 文件名匹配质量
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let name = name.to_lowercase();
        for term in terms {
            if name == *term || name.starts_with(&format!("{term}.")) {
                s += 260;
            } else if name.starts_with(term.as_str()) {
                s += 120;
            } else if name.contains(term.as_str()) {
                s += 60;
            }
        }
    }

    // 可执行文件优先于同名目录
    if let Some(ext) = path.extension().and_then(|e| e.to_str())
        && BOOST_EXTS.contains(&ext.to_lowercase().as_str())
    {
        s += 60;
    }

    // 整体路径短一点更好
    s -= (full.len() / 10) as i64;

    s
}

// ---------------------------------------------------------------------------

impl Provider for Everything {
    fn keyword(&self) -> &str {
        "ev"
    }

    fn description(&self) -> &'static str {
        crate::i18n::desc_ev()
    }

    fn query(&self, args: &str, _flags: &[String]) -> Vec<Item> {
        let args = args.trim();
        if args.is_empty() {
            return Vec::new();
        }

        let fetch = (self.max * FETCH_FACTOR).clamp(self.max, FETCH_CAP);

        let mut cmd = Command::new(&self.exe);
        cmd.arg("-n").arg(fetch.to_string());
        // 关键词按空格拆开透传给 es.exe，Everything 原生的
        // `ext:pdf`、`folder:` 等语法因此可以直接用
        for token in args.split_whitespace() {
            cmd.arg(token);
        }
        hide_window(&mut cmd);

        let Ok(out) = cmd.output() else {
            return Vec::new();
        };

        let mut scored: Vec<(i64, PathBuf)> = decode_output(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(PathBuf::from)
            .map(|p| (0, p))
            .collect();

        // 本地重排（stable sort：同分时保留 es.exe 原本的顺序）
        let terms = plain_terms(args);
        let home = self.home.as_deref();
        for entry in scored.iter_mut() {
            entry.0 = score(&entry.1, &terms, home);
        }
        // stable sort：同分时保留 es.exe 原本的顺序
        scored.sort_by_key(|e| std::cmp::Reverse(e.0));
        scored.truncate(self.max);

        scored
            .into_iter()
            .map(|(_, p)| Item::from_path(p))
            .collect()
    }
}
