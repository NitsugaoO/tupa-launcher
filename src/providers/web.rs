//! `web <网址 / 关键词>` —— 打开网页，或搜浏览器历史。
//!
//! ```text
//! web github.com              → 像 URL，直接打开
//! web https://example.com/a   → 同上
//! web rust async              → 不像 URL，搜 Edge 历史记录
//! web                         → 列最近访问过的页面
//! ```
//!
//! 判断规则（简单够用）：
//!
//! - 以 `http://` / `https://` 开头 → URL
//! - 无空格，且形如 `a.b` / `a.b/c` → 当作 URL，补 `https://` 前缀
//! - 其他 → 搜历史
//!
//! **坑 1**：Edge 运行时会把 `History` 文件独占锁住，所以必须先复制到临时目录
//! 再用 SQLite 打开。**坑 2**：`last_visit_time` 是 Chromium 时间戳
//! （1601-01-01 起、单位**微秒**），不是 Unix 时间；换算时除以 `1_000_000`
//! 再减 `11_644_473_600`（网上不少资料写的是 100ns，那是 `webkit_time`，会差 10 倍）。
//!
//! SQL 只负责「取最近 N 条」，关键词过滤放在 Rust 侧（能复用 fuzzy-matcher，
//! 也不用拼 SQL）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use rusqlite::{Connection, OpenFlags};

use super::{Action, Column, Item, Kind, Provider};
use crate::config::Config;

/// 一次从数据库里捞多少条（过滤前的候选量）
const SCAN_LIMIT: usize = 3000;

/// 标题里完整包含关键词的加分
const EXACT_BONUS: i64 = 200;
/// URL（host + 路径）里完整包含关键词的基准分
const URL_BONUS: i64 = 60;

/// 标题（宽）· 域名 · 访问次数
const COLUMNS: &[Column] = &[
    Column::new(6, 12, 0),
    Column::new(2, 8, 40),
    Column::new(1, 0, 12),
];

pub struct Web {
    max: usize,
    /// 预先找好的 History 文件（构造时扫一次，之后每次查询直接用）
    databases: Vec<PathBuf>,
    /// 上次扫描的结果 + 当时的文件指纹。
    ///
    /// 每敲一个字都会 query 一次，而一次扫描要把几十 MB 的 History 复制出来，
    /// 所以文件没变就直接复用上一次的结果。
    cache: Mutex<Option<(Stamp, Arc<Vec<Row>>)>>,
}

type Stamp = Vec<(PathBuf, Option<SystemTime>, u64)>;

impl Web {
    pub fn new(cfg: &Config) -> Self {
        Web {
            max: cfg.general.max_results,
            databases: history_files(),
            cache: Mutex::new(None),
        }
    }

    /// History 文件的指纹（mtime + 大小）
    fn stamp(&self) -> Stamp {
        self.databases
            .iter()
            .map(|path| {
                let meta = std::fs::metadata(path).ok();
                let modified = meta.as_ref().and_then(|m| m.modified().ok());
                let len = meta.map(|m| m.len()).unwrap_or(0);
                (path.clone(), modified, len)
            })
            .collect()
    }
}

impl Provider for Web {
    fn keyword(&self) -> &str {
        "web"
    }

    fn description(&self) -> &'static str {
        crate::i18n::desc_web()
    }

    fn flags(&self) -> &'static [&'static str] {
        &["url", "title", "week", "month", "all"]
    }

    fn columns(&self) -> &'static [Column] {
        COLUMNS
    }

    fn query(&self, args: &str, flags: &[String]) -> Vec<Item> {
        let input = args.trim();
        let scope = Scope::from_flags(flags);
        let field = Field::from_flags(flags);
        let direct = as_url(input);

        let cutoff = scope.cutoff();
        let rows = self.load_recent();
        let visible: Vec<&Row> = rows
            .iter()
            .filter(|row| cutoff.is_none_or(|cut| row.last_visit >= cut))
            .collect();

        // 空输入：列这个时间范围里最近访问的页面
        if input.is_empty() {
            return visible.into_iter().take(self.max).map(to_item).collect();
        }

        let items = search_rows(&visible, input, self.max, field);

        // 历史里搜不到、但输入本身像个网址 → 直接打开
        if items.is_empty()
            && let Some(url) = direct
        {
            return vec![open_item(url)];
        }
        items
    }
}

/// 搜索哪一栏：`:web:url` 只搜网址，`:web:title` 只搜标题，默认两边都搜
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Both,
    Title,
    Url,
}

impl Field {
    fn from_flags(flags: &[String]) -> Field {
        let title = flags.iter().any(|flag| flag == "title");
        let url = flags.iter().any(|flag| flag == "url");
        match (title, url) {
            (true, false) => Field::Title,
            (false, true) => Field::Url,
            _ => Field::Both,
        }
    }
}

/// `:web` 的搜索范围（默认只看一周内，避免被很老的历史和超大候选集拖慢）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Week,
    Month,
    All,
}

impl Scope {
    fn from_flags(flags: &[String]) -> Scope {
        if flags.iter().any(|flag| flag == "all") {
            Scope::All
        } else if flags.iter().any(|flag| flag == "month") {
            Scope::Month
        } else {
            Scope::Week
        }
    }

    /// 起始时间（Chromium 时间戳：1601 起、单位**微秒**）；`None` = 不限
    fn cutoff(self) -> Option<i64> {
        let days = match self {
            Scope::Week => 7,
            Scope::Month => 30,
            Scope::All => return None,
        };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
        Some((now - days * 86_400 + 11_644_473_600) * 1_000_000)
    }
}

fn search_rows(rows: &[&Row], input: &str, max: usize, field: Field) -> Vec<Item> {
    let matcher = SkimMatcherV2::default();
    let mut hits: Vec<(i64, usize)> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, row)| Some((score_row(&matcher, row, input, field)?, i)))
        .collect();

    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

    hits.into_iter()
        .take(max)
        .map(|(_, i)| to_item(rows[i]))
        .collect()
}

fn open_item(url: String) -> Item {
    let host = host_of(&url).unwrap_or_default().to_string();
    Item::new(
        Kind::Web,
        vec![url.clone(), host, crate::i18n::web_direct().to_string()],
        Action::OpenUrl(url),
    )
}

impl Web {
    /// 把所有 profile 最近访问的页面合并、按时间倒序（带缓存）
    fn load_recent(&self) -> Arc<Vec<Row>> {
        let stamp = self.stamp();
        if let Ok(guard) = self.cache.lock()
            && let Some((cached_stamp, rows)) = guard.as_ref()
            && *cached_stamp == stamp
        {
            return Arc::clone(rows);
        }

        let rows = Arc::new(self.scan());

        if let Ok(mut guard) = self.cache.lock() {
            *guard = Some((stamp, Arc::clone(&rows)));
        }
        rows
    }

    fn scan(&self) -> Vec<Row> {
        let mut all: Vec<Row> = Vec::new();
        for path in &self.databases {
            all.extend(read_history(path, SCAN_LIMIT));
        }
        all.sort_by_key(|row| std::cmp::Reverse(row.last_visit));

        // 多个 profile 可能访问过同一个网址，只留最近的那条
        let mut seen = std::collections::HashSet::new();
        all.retain(|row| seen.insert(row.url.clone()));
        all
    }
}

#[derive(Clone)]
struct Row {
    url: String,
    title: String,
    visit_count: i64,
    last_visit: i64,
}

fn to_item(row: &Row) -> Item {
    let title = if row.title.trim().is_empty() {
        row.url.clone()
    } else {
        row.title.clone()
    };
    let host = host_of(&row.url).unwrap_or_default().to_string();
    Item::new(
        Kind::Web,
        vec![title, host, crate::i18n::web_visits(row.visit_count)],
        Action::OpenUrl(row.url.clone()),
    )
}

/// 给一条历史打分：标题用模糊匹配（短文本，靠谱），URL 只做**子串**匹配
/// （去掉 `?` / `#` 后面的参数 —— 那里的 base64 随机串会让模糊匹配乱命中）。
fn score_row(matcher: &SkimMatcherV2, row: &Row, pattern: &str, field: Field) -> Option<i64> {
    let needle = pattern.to_lowercase();
    let mut score: Option<i64> = None;

    if field != Field::Url
        && let Some(base) = matcher.fuzzy_match(&row.title, pattern)
    {
        // 标题里完整出现关键词的排最前
        let bonus = if row.title.to_lowercase().contains(&needle) {
            EXACT_BONUS
        } else {
            0
        };
        score = Some(base + bonus);
    }

    if field != Field::Title && url_key(&row.url).to_lowercase().contains(&needle) {
        score = Some(score.unwrap_or(0).max(URL_BONUS));
    }

    score
}

/// URL 里参与匹配的部分：协议之后的 host + 路径，去掉查询串和锚点
fn url_key(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    rest.split(['?', '#']).next().unwrap_or(rest)
}

// ---------------------------------------------------------------------------
// URL 判断
// ---------------------------------------------------------------------------

/// 输入像网址就返回规整后的 URL
fn as_url(input: &str) -> Option<String> {
    if input.is_empty() || input.contains(char::is_whitespace) {
        return None;
    }
    let lowered = input.to_ascii_lowercase();
    if lowered.starts_with("http://") || lowered.starts_with("https://") {
        return Some(input.to_string());
    }

    // `github.com` / `github.com/foo` / `localhost:3000` 之类（至少要有一个点）
    let host_and_path = input;
    let (host, path) = match host_and_path.split_once('/') {
        Some((h, p)) => (h, Some(p)),
        None => (host_and_path, None),
    };
    let host_ok = !host.is_empty()
        && host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '.' | '_' | ':'));
    if !host_ok {
        return None;
    }

    let mut url = format!("https://{host}");
    if let Some(path) = path {
        url.push('/');
        url.push_str(path);
    }
    Some(url)
}

/// 取出网址的域名（`https://a.b/c` → `a.b`），失败就返回 `None`
fn host_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next()?;
    if host.is_empty() { None } else { Some(host) }
}

// ---------------------------------------------------------------------------
// Edge 历史
// ---------------------------------------------------------------------------

/// `%LOCALAPPDATA%\Microsoft\Edge\User Data\{Default,Profile N}\History`
fn history_files() -> Vec<PathBuf> {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else {
        return Vec::new();
    };
    let user_data = PathBuf::from(local)
        .join("Microsoft")
        .join("Edge")
        .join("User Data");

    let Ok(entries) = std::fs::read_dir(&user_data) else {
        return Vec::new();
    };

    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name == "Default" || name.starts_with("Profile ")
        })
        .map(|entry| entry.path().join("History"))
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    files
}

/// 读一个 History 文件（先复制到临时目录，避开 Edge 的独占锁）
fn read_history(path: &Path, limit: usize) -> Vec<Row> {
    // 临时文件带上 profile 名和进程号，避免并发查询 / 多个实例互相覆盖
    let profile = path
        .parent()
        .and_then(|dir| dir.file_name())
        .map(|name| name.to_string_lossy().replace(' ', "_"))
        .unwrap_or_else(|| "Default".to_string());
    let tmp = std::env::temp_dir().join(format!(
        "tupa-edge-history-{profile}-{}.sqlite",
        std::process::id()
    ));

    if std::fs::copy(path, &tmp).is_err() {
        return Vec::new();
    }

    let result = query_rows(&tmp, limit);
    let _ = std::fs::remove_file(&tmp);
    result
}

fn query_rows(file: &Path, limit: usize) -> Vec<Row> {
    let Ok(conn) = Connection::open_with_flags(file, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return Vec::new();
    };

    let sql = "SELECT url, title, visit_count, last_visit_time \
               FROM urls ORDER BY last_visit_time DESC LIMIT ?1";
    let Ok(mut stmt) = conn.prepare(sql) else {
        return Vec::new();
    };

    let rows = stmt.query_map([limit as i64], |row| {
        Ok(Row {
            url: row.get::<_, String>(0)?,
            title: row.get::<_, String>(1).unwrap_or_default(),
            visit_count: row.get::<_, i64>(2).unwrap_or(0),
            last_visit: row.get::<_, i64>(3).unwrap_or(0),
        })
    });

    match rows {
        Ok(rows) => rows.filter_map(Result::ok).collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_detection() {
        assert_eq!(
            as_url("https://example.com/a").as_deref(),
            Some("https://example.com/a")
        );
        assert_eq!(as_url("github.com").as_deref(), Some("https://github.com"));
        assert_eq!(
            as_url("github.com/foo").as_deref(),
            Some("https://github.com/foo")
        );
        assert_eq!(as_url("rust async"), None);
        assert_eq!(as_url("README"), None);
        assert_eq!(as_url("").as_deref(), None);
    }

    #[test]
    fn url_key_ignores_query() {
        assert_eq!(url_key("https://a.b/c?q=1#x"), "a.b/c");
        assert_eq!(url_key("a.b/c"), "a.b/c");
    }

    #[test]
    fn long_query_strings_do_not_match() {
        let m = SkimMatcherV2::default();
        let row = Row {
            url: "https://cn.bing.com/search?q=%E4%B8%AD%E5%9B%BD&cvid=rustxx".to_string(),
            title: "中国银行".to_string(),
            visit_count: 1,
            last_visit: 0,
        };
        assert_eq!(score_row(&m, &row, "rust", Field::Both), None);
    }

    /// `:web:url` 只搜网址，`:web:title` 只搜标题
    #[test]
    fn fields_limit_which_column_matches() {
        let m = SkimMatcherV2::default();
        let row = Row {
            url: "https://github.com/rust-lang/rust".to_string(),
            title: "The Rust Programming Language".to_string(),
            visit_count: 1,
            last_visit: 0,
        };

        assert!(score_row(&m, &row, "rust-lang", Field::Url).is_some());
        assert!(score_row(&m, &row, "rust-lang", Field::Title).is_none());
        assert!(score_row(&m, &row, "Programming", Field::Title).is_some());
        assert!(score_row(&m, &row, "Programming", Field::Url).is_none());
        assert!(score_row(&m, &row, "Programming", Field::Both).is_some());

        let flags = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(Field::from_flags(&flags(&["url"])), Field::Url);
        assert_eq!(Field::from_flags(&flags(&["title"])), Field::Title);
        assert_eq!(Field::from_flags(&flags(&[])), Field::Both);
        // 两个都给：退回两边都搜
        assert_eq!(Field::from_flags(&flags(&["url", "title"])), Field::Both);
    }

    #[test]
    fn scopes() {
        let flags = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        assert_eq!(Scope::from_flags(&flags(&[])), Scope::Week);
        assert_eq!(Scope::from_flags(&flags(&["month"])), Scope::Month);
        assert_eq!(Scope::from_flags(&flags(&["all"])), Scope::All);
        // all 优先于 month
        assert_eq!(Scope::from_flags(&flags(&["month", "all"])), Scope::All);

        // 一周的起点比一个月晚；all 不限
        let week = Scope::Week.cutoff().expect("week");
        let month = Scope::Month.cutoff().expect("month");
        assert!(week > month);
        assert!(Scope::All.cutoff().is_none());
    }

    #[test]
    fn host_extraction() {
        assert_eq!(host_of("https://a.b/c?d=1"), Some("a.b"));
        assert_eq!(host_of("a.b"), Some("a.b"));
    }
}
