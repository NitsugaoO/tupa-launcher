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
        Some(item) => item.clone(),
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
