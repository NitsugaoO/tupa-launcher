//! 命令历史：`~/.config/tupa-launcher/history.json`
//!
//! 每条记录 = 当时的输入行 + 实际执行的条目快照。
//! 例如 `@ev 报告` 回车打开了某个 PDF，历史里记的是**那个 PDF**，
//! 下次从 `rec` 列表直接回车就能再打开一次。
//!
//! 只在动作**执行成功**后记录（失败的不记，免得污染历史）。
//! 最新的在最前，重复的移到最前，最多 [`LIMIT`] 条。
//!
//! 每条还带执行次数（`hits`）与最后执行时间（`last_used`），用来算 frecency
//! （[`score`]）：开屏和 `@app` 空查询按它排序，常用的自然浮到前面。

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::providers::{Action, Item, Kind};

pub const LIMIT: usize = 200;

/// 一条历史记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// 当时的输入行（`Ctrl+↑` / `Ctrl+↓` 浏览的就是它）
    pub line: String,
    /// 实际执行的条目；老版本的历史没有这一项
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Item>,
    /// 被执行的次数（老文件没有这个字段 → 算 1 次）
    #[serde(default = "one_hit")]
    pub hits: u32,
    /// 最后一次执行的时间（unix 秒）；老文件没有 → 0（按很久以前算）
    #[serde(default)]
    pub last_used: u64,
}

fn one_hit() -> u32 {
    1
}

/// 当前时间（unix 秒）
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn path() -> Option<PathBuf> {
    Some(crate::config::config_dir()?.join("history.json"))
}

pub fn load() -> Vec<Entry> {
    let Some(path) = path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };

    // 新格式优先；读不动再按老格式（纯字符串数组）转
    let mut items: Vec<Entry> = serde_json::from_str(&text)
        .or_else(|_| {
            serde_json::from_str::<Vec<String>>(&text).map(|lines| {
                lines
                    .into_iter()
                    .map(|line| Entry {
                        line,
                        item: None,
                        hits: 1,
                        last_used: 0,
                    })
                    .collect()
            })
        })
        .unwrap_or_default();
    items.truncate(LIMIT);
    items
}

pub fn save(items: &[Entry]) -> Result<()> {
    let Some(path) = path() else {
        return Ok(()); // 没有配置目录就只在内存里留一份
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("无法创建目录 {}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(items).context("序列化历史失败")?;

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("写入失败：{}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("写入失败：{}", path.display()))?;
    Ok(())
}

/// 把一条历史记录转成可执行的条目。
///
/// - 有快照（打开过的文件 / 网页 / 应用）：直接用快照，回车能再打开一次
/// - 老记录只有输入行：把行填回输入框，不直接执行
pub fn to_item(entry: &Entry) -> Item {
    match &entry.item {
        Some(item) => {
            // 老快照的 web 列可能还是「域名」，这里顺手修成完整 URL
            let mut item = item.clone();
            item.normalize_web();
            item
        }
        None => Item::new(
            Kind::History,
            vec![entry.line.clone(), String::new()],
            Action::FillInput(entry.line.clone()),
        ),
    }
}

/// 把一行输入记进历史（去重前移 + 次数累加 + 截断）
pub fn push(items: &mut Vec<Entry>, line: &str, item: Option<Item>) {
    let now = now_secs();
    let hits = items
        .iter()
        .find(|entry| entry.line == line)
        .map(|entry| entry.hits.saturating_add(1))
        .unwrap_or(1);

    items.retain(|entry| entry.line != line);
    items.insert(
        0,
        Entry {
            line: line.to_string(),
            item,
            hits,
            last_used: now,
        },
    );
    items.truncate(LIMIT);
}

/// 执行成功后按「被执行的条目本身」给历史次数 +1（开屏点击走这条路）。
///
/// 开屏时输入框是空的，没有输入行可当去重键，[`push`] 会被空行挡掉；
/// 所以这里按 [`Action`] 找回它对应的那条记录：次数累加、刷新时间、
/// 顺手更新快照（列布局改过之后开屏显示也跟着变），再移到最前。
///
/// 找不到就什么都不做 —— 只有收藏、从没在输入框里执行过的东西不新建记录，
/// 否则没有合理的 `line` 可写，还会挤占开屏那几条历史。
/// 返回是否命中了已有记录。
pub fn bump(items: &mut Vec<Entry>, item: &Item) -> bool {
    let Some(index) = items.iter().position(|entry| {
        entry
            .item
            .as_ref()
            .is_some_and(|old| old.action == item.action)
    }) else {
        return false;
    };

    let mut entry = items.remove(index);
    entry.hits = entry.hits.saturating_add(1);
    entry.last_used = now_secs();
    entry.item = Some(item.clone());
    items.insert(0, entry);
    items.truncate(LIMIT);
    true
}

/// 开屏里执行成功后的记账：先按 `action` 找旧记录累加，找不到就补一条。
///
/// 补的那条需要一行 `line` 当去重键，用 `<前缀><provider> <完整标识>` 拼出来
/// （见 [`synthetic_line`]）：既是唯一的，`Ctrl+↑` 回放时也能重跑。
/// 返回是否记上了（重放不了的动作不记）。
pub fn record_open(items: &mut Vec<Entry>, item: &Item, prefix: char) -> bool {
    if bump(items, item) {
        return true;
    }
    let Some(line) = synthetic_line(item, prefix) else {
        return false;
    };
    push(items, &line, Some(item.clone()));
    true
}

/// 给一条还没有历史的条目编一行输入（写文件的 / 改配置的 / 清理会话的不编）。
fn synthetic_line(item: &Item, prefix: char) -> Option<String> {
    let (keyword, key) = match &item.action {
        Action::OpenUrl(url) => ("web", url.clone()),
        Action::Launch { command, args, .. } => (
            "app",
            if args.is_empty() {
                command.clone()
            } else {
                format!("{command} {}", args.join(" "))
            },
        ),
        Action::Open(path) | Action::Reveal(path) => ("ev", path.display().to_string()),
        Action::OpenInEditor { file, .. } => ("ev", file.display().to_string()),
        Action::RunCommand { command, .. } | Action::LaunchInTerminal { command, .. } => {
            ("run", command.clone())
        }
        _ => return None,
    };
    if key.trim().is_empty() {
        return None;
    }
    Some(format!("{prefix}{keyword} {key}"))
}

// ---------------------------------------------------------------------------
// frecency（频率 × 新鲜度）
// ---------------------------------------------------------------------------

/// 新鲜度权重（×100，避开浮点）：一小时 / 一天 / 一周 / 一个月内 / 更早
fn freshness_weight(last_used: u64, now: u64) -> u64 {
    match now.saturating_sub(last_used) {
        0..=3_600 => 400,
        3_601..=86_400 => 200,
        86_401..=604_800 => 100,
        604_801..=2_592_000 => 50,
        _ => 25,
    }
}

/// frecency 得分：执行次数 × 新鲜度（越大越应该排在前面）。
///
/// 老记录（`last_used = 0`）只拿到最低权重，但重新执行一次就会刷新时间。
pub fn score(entry: &Entry, now: u64) -> u64 {
    u64::from(entry.hits.max(1)) * freshness_weight(entry.last_used, now)
}

/// 就地按 frecency 排序（得分相同则「最近用过的在前」）
pub fn sort_frecency(items: &mut [Entry], now: u64) {
    items.sort_by(|a, b| {
        score(b, now)
            .cmp(&score(a, now))
            .then(b.last_used.cmp(&a.last_used))
    });
}

/// 某个动作在启动器里被执行过几次（同 `action` 的多条记录相加）。
///
/// 开屏最后一列显示的就是它；与 [`score`] 用的 `hits` 同源，只是那里取的是
/// 「最近那条的分数」，这里要的是「一共用过几次」。
pub fn hits_of(items: &[Entry], action: &Action) -> u32 {
    items
        .iter()
        .filter(|entry| {
            entry
                .item
                .as_ref()
                .is_some_and(|item| &item.action == action)
        })
        .map(|entry| entry.hits)
        .sum()
}

/// 某个动作在历史里的 frecency 得分（没记录就是 0）。
///
/// 收藏没有自己的使用计数：开屏里点收藏会补一条历史（`App::bump_item`），
/// 所以收藏的排序直接借历史里同 `action` 那条的分数。
pub fn score_of(items: &[Entry], action: &Action, now: u64) -> u64 {
    items
        .iter()
        .filter(|entry| {
            entry
                .item
                .as_ref()
                .is_some_and(|item| &item.action == action)
        })
        .map(|entry| score(entry, now))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupes_and_moves_to_front() {
        let mut items = Vec::new();
        push(&mut items, "a", None);
        push(&mut items, "b", None);
        push(&mut items, "a", None);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].line, "a");
        assert_eq!(items[0].hits, 2, "重复执行要累加次数");
        assert_eq!(items[1].hits, 1);
        assert!(items[0].last_used > 0);
    }

    #[test]
    fn bump_counts_open_at_home() {
        let code = Item::new(
            Kind::App,
            vec!["Visual Studio Code".to_string(), "code".to_string()],
            Action::Launch {
                command: "code".to_string(),
                args: Vec::new(),
                elevated: false,
            },
        );
        let mut items = Vec::new();
        push(&mut items, "code", Some(code.clone()));
        push(&mut items, "other", None);

        assert!(bump(&mut items, &code), "按 action 应该能找到那条记录");
        assert_eq!(items[0].line, "code", "bump 之后要移到最前");
        assert_eq!(items[0].hits, 2, "开屏点击也要累加次数");
        assert!(items[0].last_used > 0);
        assert_eq!(items[1].line, "other");
    }

    #[test]
    fn bump_ignores_items_never_run_here() {
        let favorite = Item::new(
            Kind::Web,
            vec!["Example".to_string(), "https://example.com".to_string()],
            Action::OpenUrl("https://example.com".to_string()),
        );
        let mut items = Vec::new();
        push(&mut items, "code", None);

        assert!(!bump(&mut items, &favorite), "没执行过的不新建记录");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].hits, 1);
    }

    #[test]
    fn record_open_adds_a_history_line_for_favorites() {
        let url = "https://a.example/x".to_string();
        let item = Item::new(
            Kind::Web,
            vec!["示例".to_string(), url.clone()],
            Action::OpenUrl(url.clone()),
        );
        let mut items = Vec::new();

        assert!(record_open(&mut items, &item, '@'));
        assert_eq!(items.len(), 1, "收藏里第一次点击要补一条历史");
        assert_eq!(items[0].line, "@web https://a.example/x");
        assert_eq!(items[0].hits, 1);

        // 第二次就命中这条，累加而不新增
        assert!(record_open(&mut items, &item, '@'));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].hits, 2);
    }

    #[test]
    fn record_open_skips_actions_that_cannot_be_replayed() {
        let item = Item::new(
            Kind::Web,
            vec!["x".to_string()],
            Action::FillInput("@cfg".to_string()),
        );
        let mut items = Vec::new();
        assert!(!record_open(&mut items, &item, '@'));
        assert!(items.is_empty());
    }

    #[test]
    fn hits_of_sums_every_line_of_the_same_action() {
        let url = "https://a.example/x".to_string();
        let item = Item::new(
            Kind::Web,
            vec!["A".to_string(), url.clone()],
            Action::OpenUrl(url.clone()),
        );
        let mut items = Vec::new();
        push(&mut items, "code", Some(item.clone()));
        push(&mut items, "code", Some(item.clone())); // hits = 2
        push(&mut items, "@web a", Some(item.clone())); // 同一动作的另一行，hits = 1

        assert_eq!(hits_of(&items, &Action::OpenUrl(url)), 3);
        assert_eq!(hits_of(&items, &Action::OpenUrl("nope".into())), 0);
    }

    #[test]
    fn score_of_reads_the_matching_action() {
        let now = 1_000_000_000u64;
        let url = "https://example.com".to_string();
        let item = Item::new(
            Kind::Web,
            vec!["Example".to_string(), url.clone()],
            Action::OpenUrl(url.clone()),
        );
        let mut items = Vec::new();
        push(&mut items, "@web example", Some(item.clone()));
        push(&mut items, "@web example", Some(item.clone()));

        assert_eq!(score_of(&items, &Action::OpenUrl(url), now), 2 * 400);
        assert_eq!(
            score_of(&items, &Action::OpenUrl("https://other".into()), now),
            0,
            "没记录的动作得 0 分"
        );
    }

    #[test]
    fn legacy_entries_get_hits_and_time_defaults() {
        let legacy = r#"[{"line":"@app code"}]"#;
        let entries: Vec<Entry> = serde_json::from_str(legacy).unwrap();
        assert_eq!(entries[0].hits, 1);
        assert_eq!(entries[0].last_used, 0);
    }

    #[test]
    fn frecency_prefers_frequent_and_recent() {
        let now = 1_000_000_000u64;
        let mut items = vec![
            Entry {
                line: "old-but-frequent".to_string(),
                item: None,
                hits: 10,
                last_used: now - 90 * 86_400, // 三个月前：权重 25
            },
            Entry {
                line: "just-once".to_string(),
                item: None,
                hits: 1,
                last_used: now - 60, // 一分钟前：权重 400
            },
            Entry {
                line: "frequent-and-recent".to_string(),
                item: None,
                hits: 5,
                last_used: now - 3_600, // 一小时前：权重 400
            },
        ];
        sort_frecency(&mut items, now);
        let order: Vec<&str> = items.iter().map(|e| e.line.as_str()).collect();
        assert_eq!(order, vec!["frequent-and-recent", "just-once", "old-but-frequent"]);
    }

    #[test]
    fn same_score_falls_back_to_recent() {
        let now = 1_000_000_000u64;
        let mut items = vec![
            Entry {
                line: "older".to_string(),
                item: None,
                hits: 2,
                last_used: now - 10_000,
            },
            Entry {
                line: "newer".to_string(),
                item: None,
                hits: 2,
                last_used: now - 100,
            },
        ];
        sort_frecency(&mut items, now);
        assert_eq!(items[0].line, "newer");
    }

    #[test]
    fn entry_without_snapshot_fills_input() {
        let entry = Entry {
            line: "@app code".to_string(),
            item: None,
            hits: 1,
            last_used: 0,
        };
        let item = to_item(&entry);
        assert_eq!(item.kind, Kind::History);
        assert_eq!(item.columns[0], "@app code");
        assert_eq!(item.action, Action::FillInput("@app code".to_string()));
    }

    #[test]
    fn entry_with_snapshot_is_reused() {
        let snapshot = Item::new(
            Kind::App,
            vec!["Visual Studio Code".to_string(), "code".to_string()],
            Action::Launch {
                command: "code".to_string(),
                args: Vec::new(),
                elevated: false,
            },
        );
        let entry = Entry {
            line: "@app code".to_string(),
            item: Some(snapshot.clone()),
            hits: 1,
            last_used: 0,
        };
        assert_eq!(to_item(&entry), snapshot);
    }

    #[test]
    fn reads_legacy_string_array() {
        let legacy = r#"["@app code", "@cfg language"]"#;
        let entries: Vec<Entry> = serde_json::from_str(legacy).unwrap_or_else(|_| {
            serde_json::from_str::<Vec<String>>(legacy)
                .unwrap()
                .into_iter()
                .map(|line| Entry {
                    line,
                    item: None,
                    hits: 1,
                    last_used: 0,
                })
                .collect()
        });
        assert_eq!(entries.len(), 2);
        assert!(entries[0].item.is_none());
    }
}
