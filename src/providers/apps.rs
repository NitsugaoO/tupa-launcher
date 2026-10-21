//! `app <名字>` —— 从手工维护的列表里启动应用（需求 R4）

use std::path::Path;

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use super::{Action, Item, Kind, Provider};
use crate::config::{AppEntry, Config};

pub struct Apps {
    entries: Vec<AppEntry>,
    /// 与 entries 一一对应：参与模糊匹配的字符串
    keys: Vec<Vec<String>>,
    /// 与 entries 一一对应：副标题（`.lnk` 已解析成目标 exe 的文件名）
    display: Vec<String>,
    max: usize,
}

impl Apps {
    pub fn new(cfg: &Config) -> Self {
        let entries = cfg.apps.clone();
        let keys = entries.iter().map(|e| e.search_keys()).collect();
        // 构造时解析一次 .lnk，免得每次按键查询都跑一遍 COM
        let display = entries.iter().map(display_name).collect();
        Apps {
            entries,
            keys,
            display,
            max: cfg.general.max_results,
        }
    }
}

/// 副标题只显示可执行文件名，不把整条路径摊出来。
///
/// 如果 `command` 是真实存在的文件（典型是 `.lnk` 快捷方式），会递归解析到目标，
/// 所以 `微信.lnk` 后面显示的是 `WeChat.exe` 而不是 `.lnk`。
fn display_name(entry: &AppEntry) -> String {
    let path = Path::new(&entry.command);

    let name = if path.is_file() {
        crate::util::resolve_shortcut(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| entry.command.clone())
    } else {
        // 裸命令（wt / code / ms-settings:）保持原样
        entry.command.clone()
    };

    if entry.args.is_empty() {
        name
    } else {
        format!("{} {}", name, entry.args.join(" "))
    }
}

impl Provider for Apps {
    fn keyword(&self) -> &str {
        "app"
    }

    fn description(&self) -> &'static str {
        crate::i18n::desc_app()
    }

    fn query(&self, args: &str, flags: &[String]) -> Vec<Item> {
        let q = args.trim();
        let elevated = flags
            .iter()
            .any(|flag| matches!(flag.as_str(), "admin" | "elevated" | "sudo"));
        let matcher = SkimMatcherV2::default();

        let mut hits: Vec<(i64, usize)> = if q.is_empty() {
            // 空查询：按配置顺序原样列出（相当于"收藏"）
            (0..self.entries.len()).map(|i| (-(i as i64), i)).collect()
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
                Item::new(
                    Kind::App,
                    vec![e.name.clone(), self.display[i].clone()],
                    Action::Launch {
                        command: e.command.clone(),
                        args: e.args.clone(),
                        elevated,
                    },
                )
            })
            .collect()
    }
}
