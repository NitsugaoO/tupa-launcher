//! 命令列表：输入 `:` 后显示所有命令（也是命令补全的数据源）。
//!
//! 回车只是把输入补成 `:<命令> `，不执行任何动作。

use super::{Action, Column, Item, Kind, Provider, match_prefix};
use crate::config::Config;

/// 这个 provider 的「关键字」。用它反查下标，不参与正常的命令匹配。
pub const KEYWORD: &str = ":";

pub struct Meta {
    pub keyword: String,
    pub description: String,
}

pub struct Commands {
    metas: Vec<Meta>,
    max: usize,
    prefix: char,
}

const COLUMNS: &[Column] = &[Column::new(1, 6, 16), Column::new(6, 0, 0)];

impl Commands {
    pub fn new(metas: Vec<Meta>, cfg: &Config) -> Self {
        Commands {
            metas,
            max: cfg.general.max_results,
            prefix: cfg.general.command_prefix.chars().next().unwrap_or(':'),
        }
    }
}

impl Provider for Commands {
    fn keyword(&self) -> &str {
        KEYWORD
    }

    fn description(&self) -> &'static str {
        crate::i18n::desc_commands()
    }

    fn columns(&self) -> &'static [Column] {
        COLUMNS
    }

    fn query(&self, args: &str, _flags: &[String]) -> Vec<Item> {
        // 已经进入了某个命令的参数区（`:app code`），这里不再出命令
        if args.contains(' ') {
            return Vec::new();
        }

        let keywords: Vec<&str> = self.metas.iter().map(|m| m.keyword.as_str()).collect();
        let hits = match_prefix(keywords, args);

        hits.into_iter()
            .take(self.max)
            .filter_map(|keyword| {
                let meta = self.metas.iter().find(|m| m.keyword == keyword)?;
                Some(Item::new(
                    Kind::Help,
                    vec![
                        format!("{}{keyword}", self.prefix),
                        meta.description.clone(),
                    ],
                    Action::FillInput(format!("{}{keyword} ", self.prefix)),
                ))
            })
            .collect()
    }
}
