//! `cfg <关键词>` —— 列出配置项，回车在 VSCode 里跳到 `config.toml` 对应的行。
//!
//! 之所以不做「在 TUI 里编辑」：这个面板的输入框是一行纯文本，
//! 改 TOML 没有语法高亮、没有补全、也看不到上下文注释。
//! 直接跳进 VSCode 反而更快——那里本来就是你写配置的地方。

use std::collections::HashMap;
use std::path::Path;

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use super::{Action, Item, Kind, Provider};
use crate::config::{self, Config};

pub struct ConfigItems;

struct Field {
    path: &'static str,
    value: String,
    hint: &'static str,
}

impl Provider for ConfigItems {
    fn keyword(&self) -> &str {
        "cfg"
    }

    fn description(&self) -> &str {
        "查看配置（Enter 在编辑器里跳到对应行）"
    }

    fn query(&self, args: &str) -> Vec<Item> {
        let cfg = Config::load().unwrap_or_default();
        let lines = locate_fields();
        let file = config::config_path().unwrap_or_default();

        let query = args.trim();
        let matcher = SkimMatcherV2::default();

        let mut items: Vec<(i64, Item)> = Vec::new();
        for field in build_fields(&cfg) {
            let score = if query.is_empty() {
                0
            } else {
                match matcher
                    .fuzzy_match(field.path, query)
                    .or_else(|| matcher.fuzzy_match(field.hint, query))
                {
                    Some(score) => score,
                    None => continue,
                }
            };
            let line = lines.get(field.path).copied();
            items.push((score, field.into_item(&file, line)));
        }

        // 稳定排序：同分时保持定义顺序
        items.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        items.into_iter().map(|(_, item)| item).collect()
    }
}

fn build_fields(cfg: &Config) -> Vec<Field> {
    let opt = |value: &Option<String>| value.clone().unwrap_or_default();
    let color_hint = |name: &'static str| -> &'static str { name };

    vec![
        Field {
            path: "general.default_provider",
            value: cfg.general.default_provider.clone(),
            hint: "不带关键字时走哪个 provider：app / ev",
        },
        Field {
            path: "general.max_results",
            value: cfg.general.max_results.to_string(),
            hint: "每个 provider 最多返回多少条",
        },
        Field {
            path: "daemon.hotkey",
            value: cfg.daemon.hotkey.clone(),
            hint: "唤起热键，如 alt+`（改完需重启守护进程）",
        },
        Field {
            path: "daemon.copilot_key",
            value: cfg.daemon.copilot_key.to_string(),
            hint: "是否接管 Copilot 键：true / false",
        },
        Field {
            path: "theme.background",
            value: opt(&cfg.theme.background),
            hint: "背景色，如 #272e33（留空 = 内置默认）",
        },
        Field {
            path: "theme.accent",
            value: opt(&cfg.theme.accent),
            hint: color_hint("强调色，如 #a7c080（留空 = 内置默认）"),
        },
        Field {
            path: "theme.fg",
            value: opt(&cfg.theme.fg),
            hint: "正文色，如 #d3c6aa（留空 = 内置默认）",
        },
    ]
}

/// 扫描 `config.toml`，记下每个配置项的行号（1 开始）。
///
/// 只做最朴素的「段名 + `key =`」识别，够用就行 —— 这个文件是我们自己生成的
/// 结构，不会有 TOML 的多行数组之类花活。
fn locate_fields() -> HashMap<String, usize> {
    let Some(path) = config::config_path() else {
        return HashMap::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };

    let mut map = HashMap::new();
    let mut section = String::new();

    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed[1..trimmed.len() - 1].to_string();
            continue;
        }
        if let Some((lhs, _)) = line.split_once('=') {
            let key = lhs.trim();
            let full = if section.is_empty() {
                key.to_string()
            } else {
                format!("{section}.{key}")
            };
            map.entry(full).or_insert(index + 1);
        }
    }

    map
}

impl Field {
    fn into_item(self, file: &Path, line: Option<usize>) -> Item {
        let shown = if self.value.is_empty() {
            "(默认)".to_string()
        } else {
            self.value.clone()
        };
        let location = match line {
            Some(number) => format!("第 {number} 行"),
            None => "未写进配置文件".to_string(),
        };

        Item {
            kind: Kind::Setting,
            title: self.path.to_string(),
            subtitle: format!("{location}  ·  = {shown}  ·  {}", self.hint),
            action: Action::OpenInEditor {
                file: file.to_path_buf(),
                line: line.unwrap_or(1),
            },
        }
    }
}
