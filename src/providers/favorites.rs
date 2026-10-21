//! `fav` —— 列出收藏（`Ctrl+D` 收藏 / 取消收藏）
//!
//! ```text
//! fav          → 全部收藏
//! fav:app      → 只看 app 类
//! fav app 微信 → 只看 app 类，并且按关键词过滤
//! fav 报告     → 全部收藏里模糊搜索
//! ```
//!
//! 每次查询都重新读文件，所以 `Ctrl+D` 改完立刻能看到最新结果。

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use super::{Column, Item, Kind, Provider};
use crate::config::Config;
use crate::favorites;

pub struct Favorites {
    max: usize,
}

/// 收藏可能是各种来源的混装，统一按三列展示
const COLUMNS: &[Column] = &[
    Column::new(5, 12, 0),
    Column::new(3, 0, 0),
    Column::new(2, 0, 20),
];

impl Favorites {
    pub fn new(cfg: &Config) -> Self {
        Favorites {
            max: cfg.general.max_results,
        }
    }
}

impl Provider for Favorites {
    fn keyword(&self) -> &str {
        "fav"
    }

    fn description(&self) -> &'static str {
        crate::i18n::desc_fav()
    }

    fn columns(&self) -> &'static [Column] {
        COLUMNS
    }

    fn query(&self, args: &str, _flags: &[String]) -> Vec<Item> {
        let mut items = favorites::load();

        // 第一个词如果是类型徽标（app / file / dir / cfg / run / web）就当过滤器
        let mut words = args.split_whitespace();
        let mut kind_filter: Option<Kind> = None;
        let mut keyword = "";
        if let Some(first) = words.next() {
            match Kind::from_badge(&first.to_ascii_lowercase()) {
                Some(kind) => kind_filter = Some(kind),
                None => keyword = first,
            }
        }
        let rest = words.collect::<Vec<_>>().join(" ");
        let keyword = if rest.is_empty() {
            keyword.to_string()
        } else if keyword.is_empty() {
            rest
        } else {
            format!("{keyword} {rest}")
        };

        if let Some(kind) = kind_filter {
            items.retain(|item| item.kind == kind);
        }

        if !keyword.trim().is_empty() {
            let matcher = SkimMatcherV2::default();
            items.retain(|item| {
                matcher.fuzzy_match(item.title(), &keyword).is_some()
                    || matcher.fuzzy_match(item.subtitle(), &keyword).is_some()
            });
        }

        items.truncate(self.max);
        items
    }
}
