//! `cfg <关键词>` —— 列出配置项，回车在 VSCode 里跳到 `config.toml` 对应的行。
//!
//! 之所以不做「在 TUI 里编辑」：这个面板的输入框是一行纯文本，
//! 改 TOML 没有语法高亮、没有补全、也看不到上下文注释。
//! 直接跳进 VSCode 反而更快——那里本来就是你写配置的地方。

use std::collections::HashMap;
use std::path::Path;

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use super::{Action, Column, Item, Kind, Provider};
use crate::config::{self, Config};

pub struct ConfigItems;

/// 配置项 · 当前值 · 位置 · 说明
const COLUMNS: &[Column] = &[
    Column::new(5, 14, 0),
    Column::new(2, 0, 26),
    Column::new(1, 0, 16),
    Column::new(4, 0, 0),
];

struct Field {
    path: &'static str,
    value: String,
    hint: &'static str,
}

impl Provider for ConfigItems {
    fn keyword(&self) -> &str {
        "cfg"
    }

    fn description(&self) -> &'static str {
        crate::i18n::desc_cfg()
    }

    fn columns(&self) -> &'static [Column] {
        COLUMNS
    }

    fn query(&self, args: &str, _flags: &[String]) -> Vec<Item> {
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

    vec![
        Field {
            path: "general.default_provider",
            value: cfg.general.default_provider.clone(),
            hint: crate::i18n::hint_cfg_default_provider(),
        },
        Field {
            path: "general.everything_path",
            value: opt(&cfg.general.everything_path),
            hint: crate::i18n::hint_cfg_everything_path(),
        },
        Field {
            path: "general.max_results",
            value: cfg.general.max_results.to_string(),
            hint: crate::i18n::hint_cfg_max_results(),
        },
        Field {
            path: "general.new_files_dir",
            value: opt(&cfg.general.new_files_dir),
            hint: crate::i18n::hint_cfg_new_files_dir(),
        },
        Field {
            path: "general.command_prefix",
            value: cfg.general.command_prefix.clone(),
            hint: crate::i18n::hint_cfg_command_prefix(),
        },
        Field {
            path: "general.show_hints",
            value: cfg.general.show_hints.to_string(),
            hint: crate::i18n::hint_cfg_show_hints(),
        },
        Field {
            path: "general.language",
            value: cfg.general.language.clone(),
            hint: crate::i18n::hint_cfg_language(),
        },
        Field {
            path: "general.prefill_colon",
            value: cfg.general.prefill_colon.to_string(),
            hint: crate::i18n::hint_cfg_prefill_colon(),
        },
        Field {
            path: "daemon.hotkey",
            value: cfg.daemon.hotkey.clone(),
            hint: crate::i18n::hint_cfg_hotkey(),
        },
        Field {
            path: "daemon.copilot_key",
            value: cfg.daemon.copilot_key.to_string(),
            hint: crate::i18n::hint_cfg_copilot_key(),
        },
        Field {
            path: "theme.background",
            value: opt(&cfg.theme.background),
            hint: crate::i18n::hint_cfg_theme_background(),
        },
        Field {
            path: "theme.accent",
            value: opt(&cfg.theme.accent),
            hint: crate::i18n::hint_cfg_theme_accent(),
        },
        Field {
            path: "theme.fg",
            value: opt(&cfg.theme.fg),
            hint: crate::i18n::hint_cfg_theme_fg(),
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
            crate::i18n::config_default_value().to_string()
        } else {
            self.value.clone()
        };
        let location = crate::i18n::config_line(line);

        Item::new(
            Kind::Setting,
            vec![
                self.path.to_string(),
                format!("= {shown}"),
                location,
                self.hint.to_string(),
            ],
            Action::OpenInEditor {
                file: file.to_path_buf(),
                line: line.unwrap_or(1),
            },
        )
    }
}
