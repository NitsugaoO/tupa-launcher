//! `hist` —— 搜索命令历史（`~/.config/tupa-launcher/history.json`）。
//!
//! ```text
//! hist            → 最近执行过的条目（最多 max_results 条）
//! hist 报告        → 按关键词过滤，命中 `输入行 / 标题 / 副标题` 任一项
//! ```
//!
//! 和开屏（`home`）的区别：开屏只列最近 25 条、并且混着收藏；`hist` 是纯历史、
//! 能带关键词把 200 条全翻一遍。带关键词时按模糊匹配得分排序（最相关的在前），
//! 不带关键词时保持「最近在前」。
//!
//! 回车行为与开屏一致：有快照的直接再打开一次，老记录只把输入行填回输入框。

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use super::{Item, Provider};
use crate::config::Config;
use crate::history;

pub struct Hist {
    max: usize,
}

impl Hist {
    pub fn new(cfg: &Config) -> Self {
        Hist {
            max: cfg.general.max_results,
        }
    }
}

impl Provider for Hist {
    fn keyword(&self) -> &str {
        "hist"
    }

    fn description(&self) -> &'static str {
        crate::i18n::desc_hist()
    }

    fn query(&self, args: &str, _flags: &[String]) -> Vec<Item> {
        self.ranked(args)
            .into_iter()
            .take(self.max)
            .map(|(_, item)| item)
            .collect()
    }
}

impl Hist {
    /// 带分数的候选：带关键词按模糊得分，不带关键词按「最近在前」（分数 0）。
    ///
    /// 分数是 skim 的打分，`all`（应用 + 历史）用它跟应用条目混排。
    pub fn ranked(&self, keyword: &str) -> Vec<(i64, Item)> {
        let keyword = keyword.trim();
        let matcher = SkimMatcherV2::default();

        let mut rows: Vec<(i64, usize, Item)> = Vec::new();
        for (index, entry) in history::load().iter().enumerate() {
            let score = if keyword.is_empty() {
                0
            } else {
                match entry_score(entry, keyword, &matcher) {
                    Some(score) => score,
                    None => continue,
                }
            };
            rows.push((score, index, history::to_item(entry)));
        }

        // 带关键词时按得分排序（同分保持「最近在前」）
        if !keyword.is_empty() {
            rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        }

        // 同一个条目可能被不同的输入行记过多次（`@app code` / `@app vsc`），只留最新的一条
        let mut seen = Vec::new();
        rows.into_iter()
            .filter(|(_, _, item)| {
                if seen.contains(&item.action) {
                    return false;
                }
                seen.push(item.action.clone());
                true
            })
            .map(|(score, _, item)| (score, item))
            .collect()
    }
}

/// 一条历史的匹配得分：输入行 / 快照标题 / 快照副标题里取最高分
fn entry_score(entry: &history::Entry, keyword: &str, matcher: &SkimMatcherV2) -> Option<i64> {
    let mut best = matcher.fuzzy_match(&entry.line, keyword);
    if let Some(item) = &entry.item {
        for column in &item.columns {
            if let Some(score) = matcher.fuzzy_match(column, keyword) {
                best = Some(best.map_or(score, |b| b.max(score)));
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{Action, Kind};

    fn entry(line: &str, item: Option<Item>) -> history::Entry {
        history::Entry {
            line: line.to_string(),
            item,
            hits: 1,
            last_used: 0,
        }
    }

    #[test]
    fn scores_line_title_and_subtitle() {
        let matcher = SkimMatcherV2::default();

        let plain = entry("@app code", None);
        assert!(entry_score(&plain, "code", &matcher).is_some());
        assert!(entry_score(&plain, "zzz", &matcher).is_none());

        let with_item = entry(
            "@ev 报告",
            Some(Item::new(
                Kind::File,
                vec!["季度报告.pdf".to_string(), "C:\\Docs".to_string()],
                Action::Open("C:\\Docs\\季度报告.pdf".into()),
            )),
        );
        // 输入行不含「pdf」，但快照标题含 → 仍然命中
        assert!(entry_score(&with_item, "pdf", &matcher).is_some());
        // 副标题（路径）也能命中
        assert!(entry_score(&with_item, "docs", &matcher).is_some());
    }
}
