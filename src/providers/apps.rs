//! `app <名字>` —— 从手工维护的列表里启动应用（需求 R4）

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use super::{Action, Item, Kind, Provider};
use crate::config::{AppEntry, Config};

pub struct Apps {
    entries: Vec<AppEntry>,
    /// 与 entries 一一对应：参与模糊匹配的字符串
    keys: Vec<Vec<String>>,
    max: usize,
}

impl Apps {
    pub fn new(cfg: &Config) -> Self {
        let entries = cfg.apps.clone();
        let keys = entries.iter().map(|e| e.search_keys()).collect();
        Apps {
            entries,
            keys,
            max: cfg.general.max_results,
        }
    }
}

impl Provider for Apps {
    fn keyword(&self) -> &str {
        "app"
    }

    fn description(&self) -> &str {
        "启动应用（列表在 config.toml 里自定义）"
    }

    fn query(&self, args: &str, flags: &[String]) -> Vec<Item> {
        let q = args.trim();
        let elevated = flags
            .iter()
            .any(|flag| matches!(flag.as_str(), "admin" | "elevated" | "sudo"));
        let matcher = SkimMatcherV2::default();

        let mut hits: Vec<(i64, usize)> = if q.is_empty() {
            // 空查询：按配置顺序原样列出（相当于"收藏"）
            (0..self.entries.len())
                .map(|i| (-(i as i64), i))
                .collect()
        } else {
            self.keys
                .iter()
                .enumerate()
                .filter_map(|(i, keys)| {
                    let best = keys
                        .iter()
                        .filter_map(|k| matcher.fuzzy_match(k, q))
                        .max()?;
                    Some((best, i))
                })
                .collect()
        };

        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));

        hits.into_iter()
            .take(self.max)
            .map(|(_, i)| {
                let e = &self.entries[i];
                Item {
                    kind: Kind::App,
                    title: e.name.clone(),
                    subtitle: if e.args.is_empty() {
                        e.command.clone()
                    } else {
                        format!("{} {}", e.command, e.args.join(" "))
                    },
                    action: Action::Launch {
                        command: e.command.clone(),
                        args: e.args.clone(),
                        elevated,
                    },
                }
            })
            .collect()
    }
}
