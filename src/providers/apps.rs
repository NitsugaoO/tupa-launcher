//! `app <名字>` —— 从手工维护的列表里启动应用（需求 R4）

use std::path::Path;

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use super::{Action, Item, Kind, Provider};
use crate::config::{AppEntry, Config};
use crate::history;

pub struct Apps {
    entries: Vec<AppEntry>,
    /// 与 entries 一一对应：参与模糊匹配的字符串
    keys: Vec<Vec<String>>,
    /// 与 entries 一一对应：副标题（`.lnk` 已解析成目标 exe 的文件名）
    display: Vec<String>,
    max: usize,
    /// 空查询是否按 frecency（使用频率 × 新鲜度）排序
    frecency: bool,
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
            frecency: cfg.general.frecency,
        }
    }

    /// 空查询的顺序：用过的按 frecency 排在前面，没用过的保持配置顺序。
    ///
    /// 历史里记的是完整的 `Action::Launch`，按 `command + args` 精确对回配置项
    /// （`elevated` 不参与——当初是用 `:app:admin` 启动的也算同一个应用）。
    fn ranked_by_usage(&self) -> Vec<(i64, usize)> {
        let usage = if self.frecency {
            usage_scores()
        } else {
            Vec::new()
        };

        (0..self.entries.len())
            .map(|i| {
                let entry = &self.entries[i];
                let score = usage
                    .iter()
                    .filter(|(command, args, _)| command == &entry.command && args == &entry.args)
                    .map(|(_, _, score)| *score)
                    .sum::<u64>();
                (score as i64, i)
            })
            .collect()
    }
}

/// 历史里每个应用（按 `command + args` 分组）的 frecency 总分
fn usage_scores() -> Vec<(String, Vec<String>, u64)> {
    let now = history::now_secs();
    let mut scores: Vec<(String, Vec<String>, u64)> = Vec::new();

    for entry in history::load().iter() {
        let Some(item) = &entry.item else { continue };
        if item.kind != Kind::App {
            continue;
        }
        let Action::Launch { command, args, .. } = &item.action else {
            continue;
        };
        let score = history::score(entry, now);
        // 同一个应用可能被不同的输入行记过（`@app code` / `@app vsc`），分数累加
        match scores
            .iter_mut()
            .find(|(known, known_args, _)| known == command && known_args == args)
        {
            Some(slot) => slot.2 += score,
            None => scores.push((command.clone(), args.clone(), score)),
        }
    }
    scores
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

/// flags 里有没有提权修饰符
pub fn is_elevated(flags: &[String]) -> bool {
    flags
        .iter()
        .any(|flag| matches!(flag.as_str(), "admin" | "elevated" | "sudo"))
}

impl Provider for Apps {
    fn keyword(&self) -> &str {
        "app"
    }

    fn description(&self) -> &'static str {
        crate::i18n::desc_app()
    }

    fn flags(&self) -> &'static [&'static str] {
        &["admin"]
    }

    fn query(&self, args: &str, flags: &[String]) -> Vec<Item> {
        self.ranked(args, is_elevated(flags))
            .into_iter()
            .take(self.max)
            .map(|(_, item)| item)
            .collect()
    }
}

impl Apps {
    /// 带分数的候选（空的查询 = 按 frecency + 配置顺序，分数为 0）。
    ///
    /// 分数是 skim 的打分，`all`（应用 + 历史）用它跟历史条目混排。
    pub fn ranked(&self, query: &str, elevated: bool) -> Vec<(i64, Item)> {
        let q = query.trim();
        let matcher = SkimMatcherV2::default();

        let mut hits: Vec<(i64, usize)> = if q.is_empty() {
            // 空查询：常用的按 frecency 靠前，其余保持配置顺序（相当于"收藏"）
            self.ranked_by_usage()
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
            .map(|(score, i)| (score, self.item_at(i, elevated)))
            .collect()
    }

    fn item_at(&self, i: usize, elevated: bool) -> Item {
        let entry = &self.entries[i];
        Item::new(
            Kind::App,
            vec![entry.name.clone(), self.display[i].clone()],
            Action::Launch {
                command: entry.command.clone(),
                args: entry.args.clone(),
                elevated,
            },
        )
    }
}

/// 让 `all`（默认 provider）和 `app` 共用同一份应用列表，
/// 不用二次解析 55 个 `.lnk`
impl Provider for std::sync::Arc<Apps> {
    fn keyword(&self) -> &str {
        (**self).keyword()
    }

    fn description(&self) -> &'static str {
        (**self).description()
    }

    fn columns(&self) -> &'static [super::Column] {
        (**self).columns()
    }

    fn flags(&self) -> &'static [&'static str] {
        (**self).flags()
    }

    fn query(&self, args: &str, flags: &[String]) -> Vec<Item> {
        (**self).query(args, flags)
    }
}
