//! 收藏：`~/.config/tupa-launcher/favorites.json`
//!
//! 存的是「Item 快照」（kind + 各列文本 + 动作），不追踪原条目的变化 ——
//! 对收藏来说够用了。

use std::path::PathBuf;

use anyhow::{Context, Result};

use serde::Deserialize;

use crate::providers::{Action, Item, Kind};

/// 收藏条数上限（防止文件无限膨胀）
pub const LIMIT: usize = 500;

pub fn path() -> Option<PathBuf> {
    Some(crate::config::config_dir()?.join("favorites.json"))
}

/// v0.3 的旧格式（title / subtitle 两个字段），读到时自动转成多列
#[derive(Deserialize)]
struct LegacyItem {
    kind: Kind,
    title: String,
    #[serde(default)]
    subtitle: String,
    action: Action,
}

/// 从磁盘读收藏；文件不存在或格式坏了都当作空列表
pub fn load() -> Vec<Item> {
    let Some(path) = path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    parse(&text)
}

/// 解析收藏文件内容：新格式（`columns`）优先，读不动再按旧格式（`title` / `subtitle`）转
fn parse(text: &str) -> Vec<Item> {
    if let Ok(items) = serde_json::from_str::<Vec<Item>>(text) {
        return items;
    }
    // 旧格式兼容：转换后下次保存就会写成新格式
    serde_json::from_str::<Vec<LegacyItem>>(text)
        .map(|list| {
            list.into_iter()
                .map(|old| Item::new(old.kind, vec![old.title, old.subtitle], old.action))
                .collect()
        })
        .unwrap_or_default()
}

pub fn save(items: &[Item]) -> Result<()> {
    let Some(path) = path() else {
        anyhow::bail!("找不到配置目录，无法保存收藏");
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("无法创建目录 {}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(items).context("序列化收藏失败")?;

    // 先写临时文件再改名，避免写到一半崩溃留下坏文件
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("写入失败：{}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("写入失败：{}", path.display()))?;
    Ok(())
}

/// 两条条目是不是「同一个东西」。
///
/// 只比 `kind + 标题 + 动作`，**不比整行的列**：像 web 的「访问 N 次」这种
/// 会变的列，一比就会让收藏「消失」或者重复收藏同一个页面。
pub fn same(entry: &Item, item: &Item) -> bool {
    entry.kind == item.kind && entry.title() == item.title() && entry.action == item.action
}

pub fn contains(items: &[Item], item: &Item) -> bool {
    items.iter().any(|entry| same(entry, item))
}

/// 收藏 / 取消收藏。返回 `true` 表示现在在收藏里，`false` 表示已移除。
pub fn toggle(item: &Item) -> Result<bool> {
    let mut items = load();

    if let Some(pos) = items.iter().position(|entry| same(entry, item)) {
        items.remove(pos);
        save(&items)?;
        return Ok(false);
    }

    // 新收藏放最前，`fav` 里就是「最近收藏的在上面」
    items.insert(0, item.clone());
    items.truncate(LIMIT);
    save(&items)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_current_format() {
        let text = r#"[
            {"kind":"Web","columns":["GitHub","github.com"],
             "action":{"OpenUrl":"https://github.com"}}
        ]"#;
        let items = parse(text);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title(), "GitHub");
        assert_eq!(items[0].subtitle(), "github.com");
    }

    #[test]
    fn reads_legacy_title_subtitle_format() {
        let text = r#"[
            {"kind":"App","title":"Visual Studio Code","subtitle":"code",
             "action":{"Launch":{"command":"code","args":[],"elevated":false}}}
        ]"#;
        let items = parse(text);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].columns, vec!["Visual Studio Code", "code"]);
    }

    #[test]
    fn same_ignores_changing_columns() {
        let saved = Item::new(
            Kind::Web,
            vec![
                "Rust".to_string(),
                "rust-lang.org".to_string(),
                "3 次".to_string(),
            ],
            Action::OpenUrl("https://rust-lang.org".to_string()),
        );
        // 同样的页面，访问次数变成了 4 次
        let current = Item::new(
            Kind::Web,
            vec![
                "Rust".to_string(),
                "rust-lang.org".to_string(),
                "4 次".to_string(),
            ],
            Action::OpenUrl("https://rust-lang.org".to_string()),
        );
        assert!(same(&saved, &current));

        // 不同页面就不算同一条
        let other = Item::new(
            Kind::Web,
            vec![
                "Docs".to_string(),
                "rust-lang.org".to_string(),
                "3 次".to_string(),
            ],
            Action::OpenUrl("https://rust-lang.org/docs".to_string()),
        );
        assert!(!same(&saved, &other));
    }

    #[test]
    fn broken_file_is_empty() {
        assert!(parse("not json").is_empty());
    }
}
