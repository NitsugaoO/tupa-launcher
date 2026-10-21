//! `switch [关键词]` —— 列出 `actions.toml` 里的开关 / 选项 / 数值。
//!
//! 开屏时并行读取每一项的当前状态（脚本可能几百毫秒），读不到的条目标成
//! 「不可用」（默认隐藏，`:switch:all` 才显示）。

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use super::{Action, Column, Item, Kind, Provider};
use crate::actions::{self, Spec};
use crate::i18n;
use crate::providers::Control;

pub struct Switch;

/// 标题 · 来源/分组（状态列由界面按 `Control` 追加）
const COLUMNS: &[Column] = &[Column::new(5, 14, 0), Column::new(3, 0, 0)];

impl Provider for Switch {
    fn keyword(&self) -> &str {
        "switch"
    }

    fn description(&self) -> &'static str {
        i18n::desc_switch()
    }

    fn flags(&self) -> &'static [&'static str] {
        &["all"]
    }

    fn columns(&self) -> &'static [Column] {
        COLUMNS
    }

    fn query(&self, args: &str, flags: &[String]) -> Vec<Item> {
        let show_all = flags.iter().any(|flag| flag == "all");
        let query = args.trim();
        let matcher = SkimMatcherV2::default();

        let mut hits: Vec<Spec> = Vec::new();
        for spec in actions::load() {
            if !query.is_empty() {
                let matched = [
                    spec.title.as_str(),
                    spec.group.as_str(),
                    spec.subtitle.as_str(),
                ]
                .iter()
                .filter_map(|text| matcher.fuzzy_match(text, query))
                .max();
                if matched.is_none() {
                    continue;
                }
            }
            hits.push(spec);
        }

        // 每项的 get 相互独立，并行跑（PowerShell 冷启动就要几百毫秒）
        let controls: Vec<Control> = std::thread::scope(|scope| {
            let handles: Vec<_> = hits
                .iter()
                .map(|spec| scope.spawn(|| actions::get_control(spec)))
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap_or(Control::Unavailable))
                .collect()
        });

        hits.into_iter()
            .zip(controls)
            .filter(|(_, control)| show_all || control != &Control::Unavailable)
            .map(|(spec, control)| {
                let id = spec.id();
                let source = source(&spec);
                Item::with_control(
                    Kind::Switch,
                    vec![spec.title, source],
                    Action::SetControl {
                        id,
                        // 真正的目标值在按下回车时由界面根据 `control` 计算
                        value: String::new(),
                    },
                    control,
                )
            })
            .collect()
    }
}

/// 第二列：优先显示 group，没有再显示 subtitle
fn source(spec: &Spec) -> String {
    if !spec.group.is_empty() {
        spec.group.clone()
    } else {
        spec.subtitle.clone()
    }
}
