//! 命令历史：`~/.config/tupa-launcher/history.json`
//!
//! 每条记录 = 当时的输入行 + 实际执行的条目快照。
//! 例如 `@ev 报告` 回车打开了某个 PDF，历史里记的是**那个 PDF**，
//! 下次从 `rec` 列表直接回车就能再打开一次。
//!
//! 只在动作**执行成功**后记录（失败的不记，免得污染历史）。
//! 最新的在最前，重复的移到最前，最多 [`LIMIT`] 条。

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::providers::Item;

pub const LIMIT: usize = 200;

/// 一条历史记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// 当时的输入行（`Ctrl+↑` / `Ctrl+↓` 浏览的就是它）
    pub line: String,
    /// 实际执行的条目；老版本的历史没有这一项
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Item>,
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
                    .map(|line| Entry { line, item: None })
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

/// 把一行输入记进历史（去重前移 + 截断）
pub fn push(items: &mut Vec<Entry>, line: &str, item: Option<Item>) {
    items.retain(|entry| entry.line != line);
    items.insert(
        0,
        Entry {
            line: line.to_string(),
            item,
        },
    );
    items.truncate(LIMIT);
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
    }

    #[test]
    fn reads_legacy_string_array() {
        let legacy = r#"["@app code", "@cfg language"]"#;
        let entries: Vec<Entry> = serde_json::from_str(legacy).unwrap_or_else(|_| {
            serde_json::from_str::<Vec<String>>(legacy)
                .unwrap()
                .into_iter()
                .map(|line| Entry { line, item: None })
                .collect()
        });
        assert_eq!(entries.len(), 2);
        assert!(entries[0].item.is_none());
    }
}
