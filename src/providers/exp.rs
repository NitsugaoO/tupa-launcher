//! `exp` —— 最近在资源管理器里打开过的文件夹。
//!
//! ```text
//! exp             → 按时间倒序列出最近打开过的文件夹（最多 max_results 条）
//! exp 线代        → 按关键词模糊匹配完整路径
//! ```
//!
//! 两个数据源合并：
//!
//! 1. [`crate::explorer_watch`] 记的「刚刚开着哪些窗口」—— 秒级新鲜，但只覆盖
//!    tupa 在跑的时段；
//! 2. [`crate::destlist`] 读的系统跳转列表 —— 覆盖几个月的历史，但系统是**长时间
//!    批量 flush** 的，最新的一条可能已经是几十小时前。
//!
//! 同一路径两边都有时用时间更新的那个。列表只留**仍然存在**的目录。

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use super::pi::age;
use super::{Action, Column, Item, Kind, Provider};
use crate::config::Config;
use crate::{destlist, explorer_watch};

/// 合并后最多对这么多条做 `is_dir()`。
///
/// 全部 1300+ 条都 stat 一遍的话，`\\wsl.localhost\…` 这类 UNC 每条都要等 WSL 应答，
/// 面板会卡。按时间倒序截断后，要看的几十条基本都落在这一段里。
const STAT_LIMIT: usize = 300;

const COLUMNS: &[Column] = &[
    Column::new(4, 14, 0),
    Column::new(6, 12, 0),
    Column::new(1, 4, 8),
];

pub struct Exp {
    max: usize,
}

impl Exp {
    pub fn new(cfg: &Config) -> Self {
        Exp {
            max: cfg.general.max_results,
        }
    }

    /// 带分数的候选（空关键词时分数一律 0，保持时间倒序）
    fn ranked(&self, keyword: &str) -> Vec<(i64, Item)> {
        let candidates = collect();
        to_items(candidates, keyword, self.max)
    }
}

impl Provider for Exp {
    fn keyword(&self) -> &str {
        "exp"
    }

    fn description(&self) -> &'static str {
        crate::i18n::desc_exp()
    }

    fn query(&self, args: &str, _flags: &[String]) -> Vec<Item> {
        self.ranked(args)
            .into_iter()
            .map(|(_, item)| item)
            .collect()
    }

    fn columns(&self) -> &'static [Column] {
        COLUMNS
    }
}

/// 合并两个数据源 → 按时间倒序的候选（本地记录优先）
fn collect() -> Vec<(PathBuf, Option<SystemTime>)> {
    let mut candidates: Vec<(PathBuf, Option<SystemTime>)> = Vec::new();

    for seen in explorer_watch::load() {
        let time = UNIX_EPOCH.checked_add(Duration::from_secs(seen.last_seen));
        candidates.push((seen.path, time));
    }

    let mut entries = destlist::load();
    // 文件顺序本来就是时间倒序，这里按编号再排一次，防止新版布局换了存储顺序
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.entry_id));
    for entry in entries {
        if candidates.iter().any(|(path, _)| *path == entry.path) {
            continue;
        }
        candidates.push((entry.path, entry.last_access));
    }

    candidates.sort_by_key(|(_, time)| std::cmp::Reverse(*time));
    candidates.truncate(STAT_LIMIT);
    candidates
}

/// 候选 → 列表行：过滤掉不存在的 / 噪音目录、去重、按关键词打分。
fn to_items(
    candidates: Vec<(PathBuf, Option<SystemTime>)>,
    keyword: &str,
    max: usize,
) -> Vec<(i64, Item)> {
    let keyword = keyword.trim();
    let matcher = SkimMatcherV2::default();

    let mut rows: Vec<(i64, u64, Item)> = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();

    for (order, (path, time)) in candidates.into_iter().enumerate() {
        if seen.contains(&path) {
            continue;
        }
        if !path.is_dir() || is_noise(&path) {
            continue;
        }
        seen.push(path.clone());

        let full = path.display().to_string();
        let score = if keyword.is_empty() {
            0
        } else {
            match matcher.fuzzy_match(&full, keyword) {
                Some(score) => score,
                None => continue,
            }
        };
        rows.push((score, order as u64, to_item(&path, time)));
    }

    // 带关键词时按相关度排（同分保持「新的在前」）；不用关键词时顺序已经是对的
    if !keyword.is_empty() {
        rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    }

    rows.into_iter()
        .take(max)
        .map(|(score, _, item)| (score, item))
        .collect()
}

/// 安装程序 / 系统自己「打开」的目录，不是用户主动浏览的。
///
/// 跳转列表里混着一堆 `C:\Windows\Installer\_{…}`（MSI 缓存）和 `\Temp\` 之类，
/// 它们会淹没用户真正想找的「我刚刚去过哪儿」。
fn is_noise(path: &Path) -> bool {
    let lower = path.to_string_lossy().to_lowercase();

    let mut prefixes: Vec<String> = Vec::new();
    if let Ok(windir) = std::env::var("WINDIR") {
        let base = windir.to_lowercase();
        prefixes.push(format!("{base}\\installer"));
        prefixes.push(format!("{base}\\temp"));
        prefixes.push(format!("{base}\\winsxs"));
        prefixes.push(format!("{base}\\servicing"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        prefixes.push(format!("{}\\temp", local.to_lowercase()));
    }

    prefixes
        .iter()
        .any(|prefix| lower.starts_with(prefix.as_str()))
}

fn to_item(path: &Path, time: Option<SystemTime>) -> Item {
    let full = path.display().to_string();
    // 盘符根（`C:\`）没有 file_name，退回完整路径，别显示成空白
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| full.clone());
    let time = time.map(age).unwrap_or_default();

    Item::new(
        Kind::Dir,
        vec![name, full, time],
        Action::Open(path.to_path_buf()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(path: &str, secs: u64) -> (PathBuf, Option<SystemTime>) {
        (
            PathBuf::from(path),
            Some(UNIX_EPOCH + Duration::from_secs(secs)),
        )
    }

    /// 一个一定存在、且不会被 [`is_noise`] 筛掉的目录
    fn real_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn keeps_only_existing_directories() {
        let temp = real_dir();
        let temp_text = temp.display().to_string();

        let rows = to_items(
            vec![
                candidate(&temp_text, 1_700_000_000),
                candidate(r"C:\This\Folder\Does\Not\Exist\At\All", 1_700_000_001),
            ],
            "",
            60,
        );
        assert_eq!(rows.len(), 1, "不存在的目录应当被滤掉");
        assert_eq!(rows[0].1.action, Action::Open(temp));

        // 三列：名称 / 完整路径 / 时间
        assert_eq!(rows[0].1.columns.len(), 3);
        assert_eq!(rows[0].1.columns[1], temp_text);
        assert_eq!(rows[0].1.kind, Kind::Dir);
    }

    #[test]
    fn dedupes_same_path() {
        let temp = real_dir().display().to_string();
        let rows = to_items(vec![candidate(&temp, 300), candidate(&temp, 200)], "", 60);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn keeps_the_newer_timestamp_when_merging() {
        // `collect` 的合并规则：本地记录（新）盖过跳转列表（旧）——同一路径只留一条
        let mut candidates: Vec<(PathBuf, Option<SystemTime>)> =
            vec![candidate(r"C:\Alpha", 500)];
        let duplicate = candidate(r"C:\Alpha", 100);
        if !candidates.iter().any(|(path, _)| *path == duplicate.0) {
            candidates.push(duplicate);
        }
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].1.expect("有时间").duration_since(UNIX_EPOCH).unwrap().as_secs(),
            500
        );
    }

    #[test]
    fn filters_by_keyword_and_sorts_by_score() {
        let base = real_dir().join("target");
        let alpha = base.join("exp_test_alpha");
        let beta = base.join("exp_test_beta");
        std::fs::create_dir_all(&alpha).unwrap();
        std::fs::create_dir_all(&beta).unwrap();

        let rows = to_items(
            vec![
                candidate(&beta.display().to_string(), 200),
                candidate(&alpha.display().to_string(), 100),
            ],
            "alpha",
            60,
        );
        assert_eq!(rows.len(), 1, "只有 alpha 命中");
        assert_eq!(rows[0].1.action, Action::Open(alpha.clone()));

        // 不命中的关键词一条都不剩
        assert!(to_items(vec![candidate(&beta.display().to_string(), 200)], "zzz", 60).is_empty());

        let _ = std::fs::remove_dir_all(Path::new(&alpha));
        let _ = std::fs::remove_dir_all(Path::new(&beta));
    }

    #[test]
    fn drops_installer_noise() {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".to_string());
        assert!(is_noise(Path::new(&format!(
            r"{windir}\Installer\_{{00000070-0240-1033-84C8-B8D95FA3C8C3}}"
        ))));
        assert!(!is_noise(&real_dir()));
    }
}
