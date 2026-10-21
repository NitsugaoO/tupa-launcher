//! `home` —— 开屏内容：收藏 + 最近执行的命令。
//!
//! 输入为空时（刚唤起面板、或按 Esc 回到开屏）显示这个 provider。
//! 一旦开始打字就回到正常的默认 provider，所以它不需要关键字补全，
//! 也不会出现在命令列表里。

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use super::{Action, Column, Item, Kind, Provider};
use crate::config::Config;
use crate::{favorites, history};

/// 这个 provider 的关键字（只用于反查下标，不参与输入解析）
pub const KEYWORD: &str = "home";

/// 开屏最多列几条历史（收藏不算在内）
const HISTORY_LIMIT: usize = 5;

/// 收藏可能是各种来源的混装，和 `fav` 用同一套三列布局
const COLUMNS: &[Column] = &[
    Column::new(5, 12, 0),
    Column::new(3, 0, 0),
    Column::new(2, 0, 20),
];

pub struct Home {
    max: usize,
}

impl Home {
    pub fn new(cfg: &Config) -> Self {
        Home {
            max: cfg.general.max_results,
        }
    }
}

impl Provider for Home {
    fn keyword(&self) -> &str {
        KEYWORD
    }

    fn description(&self) -> &'static str {
        crate::i18n::desc_home()
    }

    fn columns(&self) -> &'static [Column] {
        COLUMNS
    }

    fn query(&self, args: &str, _flags: &[String]) -> Vec<Item> {
        let keyword = args.trim();
        let matcher = SkimMatcherV2::default();

        // 1) 收藏：有关键词就顺手过滤
        let mut items = favorites::load();
        if !keyword.is_empty() {
            items.retain(|item| {
                matcher.fuzzy_match(item.title(), keyword).is_some()
                    || matcher.fuzzy_match(item.subtitle(), keyword).is_some()
            });
        }

        // 2) 最近执行的命令：回车只是把输入补回去（再回车才执行，避免误触）
        let recent: Vec<Item> = history::load()
            .into_iter()
            .filter(|line| {
                keyword.is_empty() || matcher.fuzzy_match(line, keyword).is_some()
            })
            .take(HISTORY_LIMIT)
            .map(|line| recent_item(&line))
            .collect();
        items.extend(recent);

        items.truncate(self.max);
        items
    }
}

/// 历史条目：只把命令文本填回输入框，不直接执行
fn recent_item(line: &str) -> Item {
    Item::new(
        Kind::History,
        vec![line.to_string(), String::new(), String::new()],
        Action::FillInput(line.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_item_fills_input() {
        let item = recent_item("@app code");
        assert_eq!(item.kind, Kind::History);
        assert_eq!(item.columns[0], "@app code");
        assert_eq!(item.action, Action::FillInput("@app code".to_string()));
    }
}
