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

/// 开屏最多列几条历史（收藏不算在内）。
///
/// 面板是 55% 屏高，本机 1536×960 下列表区约 28 行；5 条收藏 + 本值 ≈ 填满一屏，
/// 再多就得滚动。收藏数量变化时会有几行出入，滚动始终可用。
const HISTORY_LIMIT: usize = 25;

/// 收藏可能是各种来源的混装，和 `fav` 用同一套三列布局
pub(crate) const COLUMNS: &[Column] = &[
    Column::new(5, 12, 0),
    Column::new(3, 0, 0),
    Column::new(2, 0, 20),
];

pub struct Home {
    max: usize,
    frecency: bool,
}

impl Home {
    pub fn new(cfg: &Config) -> Self {
        Home {
            max: cfg.general.max_results,
            frecency: cfg.general.frecency,
        }
    }
}

/// 开屏最后一列 = 这个条目在启动器里被执行过几次。
///
/// web 行的第三列本来是 Edge 的浏览次数（快照值、会过期，而且跟“我常用什么”
/// 无关），先摘掉再换成自己的计数；本来就有 3 列以上内容的条目（`cfg` 的
/// 位置 / 说明、`pi` 的目录 / 时间）不再追加，否则一行挤成五段没法看。
fn apply_uses(item: &mut Item, uses: u32) {
    if item.kind == Kind::Web && item.columns.len() > 2 {
        item.columns.truncate(2);
    }
    if uses == 0 || item.columns.len() > 2 {
        return;
    }
    item.columns.push(crate::i18n::uses(uses));
}

/// 历史里同 `action` 的最新快照（`entries` 已按 frecency / 最近排好）
fn latest_for<'a>(entries: &'a [history::Entry], action: &Action) -> Option<&'a Item> {
    entries.iter().find_map(|entry| match &entry.item {
        Some(item) if &item.action == action => Some(item),
        _ => None,
    })
}

/// 收藏的显示列：名字用你收藏时的第一列，其余列取历史里最新那份。
///
/// 收藏存的是 `Item` 快照，不会自己变（`:web` 的列布局改过之后，老收藏会
/// 一直显示旧列）；跟着历史走之后「完整 URL」「访问次数」都会更新。
fn refreshed(favorite: &Item, latest: &Item) -> Item {
    let mut columns = latest.columns.clone();
    if let Some(name) = favorite
        .columns
        .first()
        .filter(|name| !name.trim().is_empty())
        && let Some(slot) = columns.first_mut()
    {
        *slot = name.clone();
    }
    Item {
        kind: favorite.kind,
        columns,
        action: favorite.action.clone(),
        control: favorite.control.clone(),
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

    /// 开屏是混装列表（收藏 / 历史 / 设置 / 会话什么都有），排成表反而浪费：
    /// 每段按自己的长度排，段间只留固定间隔。
    fn layout(&self) -> super::RowLayout {
        super::RowLayout::Natural
    }

    fn query(&self, args: &str, _flags: &[String]) -> Vec<Item> {
        let keyword = args.trim();
        let matcher = SkimMatcherV2::default();

        // 历史只读一次：收藏的列和排序都要从里面取
        let now = history::now_secs();
        let mut entries = history::load();
        if self.frecency {
            history::sort_frecency(&mut entries, now);
        }

        // 1) 收藏：列跟历史走（名字保留你收藏时那一列），
        //    排序也借历史里同 action 的 frecency —— 点一次就浮上来
        let mut items: Vec<Item> = favorites::load()
            .into_iter()
            .map(|favorite| match latest_for(&entries, &favorite.action) {
                Some(latest) => refreshed(&favorite, latest),
                None => favorite,
            })
            .collect();
        if self.frecency {
            // sort_by_key 是稳定排序：从没点过的（0 分）保持文件里的顺序
            items.sort_by_key(|item| {
                std::cmp::Reverse(history::score_of(&entries, &item.action, now))
            });
        }
        if !keyword.is_empty() {
            items.retain(|item| {
                matcher.fuzzy_match(item.title(), keyword).is_some()
                    || matcher.fuzzy_match(item.subtitle(), keyword).is_some()
            });
        }

        // 2) 最近执行的条目：有快照的（打开过的文件 / 网页）直接执行，
        //    只有输入行的（老记录）就补回输入框。
        //    顺序按 frecency（执行次数 × 新鲜度），常用的自然靠前
        let mut seen: Vec<Action> = items.iter().map(|item| item.action.clone()).collect();
        let recent: Vec<Item> = entries
            .iter()
            .filter(|entry| keyword.is_empty() || matcher.fuzzy_match(&entry.line, keyword).is_some())
            .map(history::to_item)
            .filter(|item| {
                // 收藏里已经有的就别在历史里再列一遍
                if seen.contains(&item.action) {
                    return false;
                }
                seen.push(item.action.clone());
                true
            })
            .take(HISTORY_LIMIT)
            .collect();
        items.extend(recent);

        for item in &mut items {
            // 老快照的 web 列可能还是「域名」（收藏那条直接用历史快照，绕过了
            // `history::to_item` 里的修正），显示前统一修成完整 URL
            item.normalize_web();
            // 最后一列：你在启动器里用过几次（不是浏览器的访问次数）
            let uses = history::hits_of(&entries, &item.action);
            apply_uses(item, uses);
        }

        items.truncate(self.max);
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn web(url: &str, title: &str, extra: &str) -> Item {
        Item::new(
            Kind::Web,
            vec![title.to_string(), url.to_string(), extra.to_string()],
            Action::OpenUrl(url.to_string()),
        )
    }

    #[test]
    fn columns_match_the_favorites_layout() {
        // 收藏是 3 列混装（app / file / web 都往里放），历史快照可能只有 2 列；
        // 布局按最多的那一行算，短行尾部留空
        assert_eq!(COLUMNS.len(), 3);
    }

    #[test]
    fn history_limit_fills_the_panel() {
        // 面板可视约 28 行，历史太少就会大片留白（曾被 fmt 误回退成 5 过一次）
        assert!(HISTORY_LIMIT >= 20, "开屏历史太少，面板会空一大片");
    }

    #[test]
    fn favorite_takes_columns_from_history_but_keeps_its_name() {
        // 收藏时的旧两列（域名列布局）
        let favorite = Item::new(
            Kind::Web,
            vec!["呜云 · 博客管理".to_string(), "127.0.0.1:5173".to_string()],
            Action::OpenUrl("http://127.0.0.1:5173/".to_string()),
        );
        // 历史里同一个动作，已经是新布局（完整 URL + 次数）
        let latest = Item::new(
            Kind::Web,
            vec![
                "Vite + React".to_string(),
                "http://127.0.0.1:5173/".to_string(),
                "20 次".to_string(),
            ],
            Action::OpenUrl("http://127.0.0.1:5173/".to_string()),
        );

        let shown = refreshed(&favorite, &latest);
        assert_eq!(shown.columns[0], "呜云 · 博客管理", "收藏的名字不能被改掉");
        assert_eq!(shown.columns[1], "http://127.0.0.1:5173/", "第二列跟新布局");
        assert_eq!(shown.columns[2], "20 次");
        assert_eq!(shown.action, favorite.action);
    }

    #[test]
    fn uses_column_replaces_edge_visits() {
        // web 行：第三列换成启动器里的使用次数
        let mut web_item = web("https://a.example/x", "A", "9 次");
        apply_uses(&mut web_item, 3);
        assert_eq!(web_item.columns, vec!["A", "https://a.example/x", "3 次"]);
    }

    #[test]
    fn uses_column_skips_rich_rows_and_unknown_items() {
        // cfg / pi 本来就有 3 列以上，不再追加
        let mut setting = Item::new(
            Kind::Setting,
            vec!["id".into(), "= on".into(), "第 8 行".into(), "说明".into()],
            Action::FillInput("@cfg".into()),
        );
        apply_uses(&mut setting, 7);
        assert_eq!(setting.columns.len(), 4);

        // 没有历史记录（0 次）就不显示这一列
        let mut app = Item::new(
            Kind::App,
            vec!["Code".into(), "code".into()],
            Action::Launch {
                command: "code".into(),
                args: Vec::new(),
                elevated: false,
            },
        );
        apply_uses(&mut app, 0);
        assert_eq!(app.columns.len(), 2);

        // 有记录的两列条目补上次数
        apply_uses(&mut app, 12);
        assert_eq!(app.columns, vec!["Code", "code", "12 次"]);
    }

    #[test]
    fn latest_lookup_is_by_action_only() {
        let hit = web("https://a.example/x", "A", "3 次");
        let other = web("https://b.example/y", "B", "1 次");
        let entries = vec![
            history::Entry {
                line: "@web b".to_string(),
                item: Some(other.clone()),
                hits: 9,
                last_used: 0,
            },
            history::Entry {
                line: "@web a".to_string(),
                item: Some(hit.clone()),
                hits: 1,
                last_used: 0,
            },
        ];

        let found = latest_for(&entries, &hit.action).expect("应该按 action 找到");
        assert_eq!(found.columns[0], "A");
        assert!(latest_for(&entries, &Action::OpenUrl("nope".into())).is_none());
    }
}
