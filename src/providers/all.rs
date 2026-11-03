//! `all` —— 默认搜索：**应用 + 命令历史**混排。
//!
//! ```text
//! code            → VS Code / 历史里的 `:app code` / 以前打开过的 Code 相关文件
//! all code        → 同上（显式写出来）
//! all             → 只列应用（等同 `app` 空查询，开屏由 `home` 负责）
//! ```
//!
//! 两边都走 `SkimMatcherV2`，分数同一尺度，所以直接混排：分数高的在前，
//! **同分时应用优先**（能启动的东西比"以前做过的事"更该排前面）。
//!
//! 去重按 `Action`：历史里已经记为 `Launch code` 的条目不会在应用结果之后再冒一遍。
//!
//! 为什么单独做成一个 provider（而不是塞进 `app`）：`app` 是"我的应用列表"，
//! `@app code` 应该只给应用；只有不带前缀的默认搜索才要混。
//! 要在两者间切换，改 `[general] default_provider`（`all` / `app`）。

use super::{Action, Item, Provider};
use super::apps::{is_elevated, Apps};
use super::hist::Hist;
use crate::config::Config;

pub struct All {
    apps: std::sync::Arc<Apps>,
    hist: Hist,
    max: usize,
}

impl All {
    /// `apps` 与 `app` provider 共用同一份（省掉第二次解析 55 个 `.lnk`）
    pub fn new(apps: std::sync::Arc<Apps>, cfg: &Config) -> Self {
        All {
            apps,
            hist: Hist::new(cfg),
            max: cfg.general.max_results,
        }
    }
}

impl Provider for All {
    fn keyword(&self) -> &str {
        "all"
    }

    fn description(&self) -> &'static str {
        crate::i18n::desc_all()
    }

    fn flags(&self) -> &'static [&'static str] {
        &["admin"]
    }

    fn query(&self, args: &str, flags: &[String]) -> Vec<Item> {
        let keyword = args.trim();
        // 空查询不混历史：应用列表本身就是"收藏"，历史由开屏负责
        if keyword.is_empty() {
            return self.apps.query(args, flags);
        }

        let mut rows: Vec<(i64, bool, Item)> = Vec::new();
        let mut seen: Vec<Action> = Vec::new();

        for (score, item) in self.apps.ranked(keyword, is_elevated(flags)) {
            seen.push(item.action.clone());
            rows.push((score, true, item));
        }
        for (score, item) in self.hist.ranked(keyword) {
            if seen.contains(&item.action) {
                continue;
            }
            seen.push(item.action.clone());
            rows.push((score, false, item));
        }

        // 分数降序；同分时应用优先（true 排在 false 前面）
        rows.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        rows.into_iter()
            .take(self.max)
            .map(|(_, _, item)| item)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Kind;

    /// 直接测排序规则：同分时应用优先、高分的在前
    #[test]
    fn same_score_prefers_apps() {
        let mut rows = vec![
            (100i64, false, "hist"),
            (100, true, "app"),
            (50, true, "app-low"),
        ];
        rows.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        let order: Vec<&str> = rows.iter().map(|(_, _, name)| *name).collect();
        assert_eq!(order, vec!["app", "hist", "app-low"]);
    }

    #[test]
    fn keyword_is_respected() {
        let cfg = Config::default();
        let apps = std::sync::Arc::new(Apps::new(&cfg));
        let all = All::new(apps, &cfg);
        // 空查询 = 只列应用
        let items = all.query("", &[]);
        assert!(items.iter().all(|item| item.kind == Kind::App) || items.is_empty());
        // 随便一个不可能命中的词：两边都空，不能 panic
        assert!(all.query("zzz-不存在的关键词", &[]).is_empty());
    }
}
