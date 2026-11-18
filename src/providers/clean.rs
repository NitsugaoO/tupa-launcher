//! `clean` —— 清理 pi 会话：按时间窗口列出可归档 / 可删除的会话，回车执行。
//!
//! 规则（时间窗口是**前置条件**）：
//!
//! - 收藏 / 未满窗口 → **不列出**（视为保护）
//! - 已总结（`session-keywords.json` 里有条目）或消息数 ≥ 8 → **归档**到 `session-archive/`
//! - 其余瘦会话 → **删除**到系统回收站
//!
//! `:pi:clean:restore` 列出归档里的会话，回车移回 `sessions/` 原目录。
//!
//! 时间窗口用修饰符切：`:pi:clean:week`（一周）/ `:pi:clean:month`（一个月），默认两周。
//! 会话扫描与标题解析复用 [`super::pi`]，这里只做分类与动作；
//! 入口是 [`super::pi::PiSessions`]（同一个 provider，`clean` 只是一个修饰符）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use anyhow::{bail, Context, Result};

use super::pi::{self, Meta};use super::{Action, CleanMode, CleanOp, Item, Kind};
use super::pi_index::load_names;
use crate::i18n;

/// 默认只看两周以前的会话
pub(super) const WINDOW_DEFAULT_DAYS: u64 = 14;
/// ≥ 8 条消息算「多轮」，归档而不是删除
pub(super) const MULTI_TURN: u32 = 8;
const DAY_SECS: u64 = 86_400;

/// `:pi:clean`（以及 `:pi:clean:restore`）的查询实现。
///
/// 列与 `:pi` 保持一致（标题 / 项目 / 时间），处置前缀放进标题、体积并入时间列，
/// 这样两个视图共用同一套列宽规格。
pub(super) fn query(args: &str, flags: &[String]) -> Vec<Item> {
    let Some(root) = agent_dir() else {
        return Vec::new();
    };
    let keywords = pi::load_keywords(&root);
    let favorites = pi::load_favorites(&root);
    let names = load_names();
    let restore = flags.iter().any(|flag| flag == "restore");
    // cleanall 只对清理有意义；恢复要一条条挑（恢复 131 个归档会瞬间撑大 sessions/）
    let batch = flags.iter().any(|flag| flag == "cleanall") && !restore;
    let search = args.trim().to_lowercase();
    let window_days = window_days(flags);
    let now = SystemTime::now();

    let mut metas = Vec::new();
    if restore {
        pi::collect(
            &root.join("session-archive"),
            &keywords,
            &favorites,
            &names,
            &mut metas,
        );
    } else {
        pi::collect(
            &root.join("sessions"),
            &keywords,
            &favorites,
            &names,
            &mut metas,
        );
    }

    let mut rows: Vec<(SystemTime, Item)> = Vec::new();
    let mut ops: Vec<CleanOp> = Vec::new();
    let mut archived = 0usize;
    let mut trashed = 0usize;
    let mut total_bytes = 0u64;

    for meta in metas {
        if !search.is_empty() && !matches(&meta, &search) {
            continue;
        }
        let (mode, label) = if restore {
            (CleanMode::Restore, i18n::clean_do_restore())
        } else {
            match classify(&meta, window_days, now) {
                // 收藏 / 未满窗口：不列出，避免误操作
                Disposition::Protect => continue,
                Disposition::Archive => (CleanMode::Archive, i18n::clean_do_archive()),
                Disposition::Trash => (CleanMode::Trash, i18n::clean_do_trash()),
            }
        };

        if batch {
            match mode {
                CleanMode::Archive => archived += 1,
                CleanMode::Trash => trashed += 1,
                CleanMode::Restore => {}
            }
            total_bytes += size_bytes(&meta.path);
            ops.push(CleanOp {
                path: meta.path.clone(),
                mode,
            });
            continue;
        }

        let item = Item::new(
            Kind::Clean,
            vec![
                format!("{label} {}", meta.title),
                project_name(&meta),
                format!(
                    "{} · {}",
                    pi::age(meta.mtime),
                    fmt_size(size_bytes(&meta.path))
                ),
            ],
            Action::CleanSession {
                path: meta.path.clone(),
                mode,
            },
        );
        rows.push((meta.mtime, item));
    }

    // 批量视图：只返回一条汇总项，回车才真正执行
    if batch {
        if ops.is_empty() {
            return Vec::new();
        }
        let window = i18n::clean_window_label(window_days);
        return vec![Item::new(
            Kind::Clean,
            vec![
                i18n::clean_all_label(archived, trashed),
                i18n::clean_all_scope(ops.len(), &fmt_size(total_bytes), window),
                window.to_string(),
            ],
            Action::CleanBatch { ops },
        )];
    }

    // 清理时最旧的在前（先看最久没动的）；恢复时最近归档的在前
    if restore {
        rows.sort_by_key(|(mtime, _)| std::cmp::Reverse(*mtime));
    } else {
        rows.sort_by_key(|(mtime, _)| *mtime);
    }
    rows.into_iter().map(|(_, item)| item).collect()
}

/// 处置：保护（收藏 / 未满窗口） / 归档 / 删除
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Disposition {
    Protect,
    Archive,
    Trash,
}

pub(super) fn window_days(flags: &[String]) -> u64 {
    if flags.iter().any(|flag| flag == "week") {
        7
    } else if flags.iter().any(|flag| flag == "month") {
        30
    } else {
        WINDOW_DEFAULT_DAYS
    }
}

/// 纯规则分类（不读盘，便于测试）
pub(super) fn classify(meta: &Meta, window_days: u64, now: SystemTime) -> Disposition {
    if meta.favorite {
        return Disposition::Protect;
    }
    let age = now.duration_since(meta.mtime).unwrap_or_default().as_secs();
    if age < window_days * DAY_SECS {
        return Disposition::Protect;
    }
    if !meta.keywords.is_empty() || meta.messages >= MULTI_TURN {
        Disposition::Archive
    } else {
        Disposition::Trash
    }
}

/// `~/.pi/agent`
pub(crate) fn agent_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".pi").join("agent"))
}

/// `sessions/<dir>/x.jsonl` → `session-archive/<dir>/x.jsonl`
pub(crate) fn archive_target(root: &Path, path: &Path) -> Option<PathBuf> {
    let rel = path.strip_prefix(root.join("sessions")).ok()?;
    Some(root.join("session-archive").join(rel))
}

/// `session-archive/<dir>/x.jsonl` → `sessions/<dir>/x.jsonl`
pub(crate) fn restore_target(root: &Path, path: &Path) -> Option<PathBuf> {
    let rel = path.strip_prefix(root.join("session-archive")).ok()?;
    Some(root.join("sessions").join(rel))
}

fn project_name(meta: &Meta) -> String {
    let Some(cwd) = &meta.cwd else {
        return "~".to_string();
    };
    // 家目录下的会话其实没有“项目”，显示 ~ 比显示用户名有用
    if dirs::home_dir().as_deref() == Some(cwd.as_path()) {
        return "~".to_string();
    }
    cwd.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "~".to_string())
}

fn matches(meta: &Meta, query: &str) -> bool {
    if meta.title.to_lowercase().contains(query) {
        return true;
    }
    if let Some(cwd) = &meta.cwd
        && cwd.display().to_string().to_lowercase().contains(query)
    {
        return true;
    }
    meta.keywords
        .iter()
        .any(|keyword| keyword.to_lowercase().contains(query))
}

fn size_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn fmt_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{}K", bytes / 1024)
    } else {
        format!("{:.1}M", bytes as f64 / (1024.0 * 1024.0))
    }
}

// ---------- 执行（从 exec.rs 挪来：清理的「列」和「办」聚合在一个文件里） ----------

/// 会话文件的显示名（提示文案用）
fn session_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// 处理一个 pi 会话文件（[`Action::CleanSession`] 的实现）。
///
/// - 归档 / 恢复都是**移动**（可逆），目标已存在时绝不覆盖（恢复会加 `-restored-HHmmss`）
/// - 删除优先 `trash` CLI（进系统回收站）；不可用时才真正删掉（与 pi 内置一致）
pub(crate) fn run_session(path: &Path, mode: CleanMode) -> Result<String> {
    let root = agent_dir().context(i18n::no_agent_dir())?;
    let name = session_name(path);

    match mode {
        CleanMode::Archive => {
            let target = archive_target(&root, path).context(i18n::clean_not_a_session())?;
            if target.exists() {
                bail!("{}", i18n::clean_target_exists(&target.display().to_string()));
            }
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir).with_context(|| {
                    i18n::cannot("创建目录", "create folder", &dir.display().to_string())
                })?;
            }
            std::fs::rename(path, &target)
                .with_context(|| i18n::cannot("归档", "archive", &path.display().to_string()))?;
            Ok(i18n::clean_archived(&name))
        }
        CleanMode::Trash => {
            let ok = Command::new("trash")
                .arg(path)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !ok {
                std::fs::remove_file(path).with_context(|| {
                    i18n::cannot("删除", "delete", &path.display().to_string())
                })?;
            }
            Ok(i18n::clean_trashed(&name))
        }
        CleanMode::Restore => {
            let target = restore_target(&root, path).context(i18n::clean_not_a_session())?;
            let target = if target.exists() { unique_target(&target) } else { target };
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir).with_context(|| {
                    i18n::cannot("创建目录", "create folder", &dir.display().to_string())
                })?;
            }
            std::fs::rename(path, &target)
                .with_context(|| i18n::cannot("恢复", "restore", &path.display().to_string()))?;
            Ok(i18n::clean_restored(&session_name(&target)))
        }
    }
}

/// 批量执行（[`Action::CleanBatch`]）：失败的项不中断，最后汇总
pub(crate) fn run_batch(ops: &[CleanOp]) -> Result<String> {
    let mut archived = 0usize;
    let mut trashed = 0usize;
    let mut failed = 0usize;
    for op in ops {
        match run_session(&op.path, op.mode) {
            Ok(_) => match op.mode {
                CleanMode::Archive => archived += 1,
                CleanMode::Trash => trashed += 1,
                CleanMode::Restore => {}
            },
            Err(_) => failed += 1,
        }
    }
    Ok(i18n::clean_batch_done(archived, trashed, failed))
}

/// 重名时加 `-restored-<HHMMSS>` 后缀
fn unique_target(target: &Path) -> PathBuf {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        % 86_400;
    let stamp = format!("{:02}{:02}{:02}", secs / 3600, (secs % 3600) / 60, secs % 60);
    let stem = target
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!("{stem}-restored-{stamp}.jsonl"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_target_keeps_stem_and_adds_suffix() {
        let target = Path::new("C:/agent/sessions/--proj--/2026-10-01_x.jsonl");
        let got = unique_target(target);
        let name = got.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("2026-10-01_x-restored-"), "实际：{name}");
        assert!(name.ends_with(".jsonl"));
    }

    #[test]
    fn session_name_falls_back_to_path() {
        assert_eq!(session_name(Path::new("C:/a/b.jsonl")), "b.jsonl");
    }

    fn meta(mtime: SystemTime, favorite: bool, messages: u32, keywords: &[&str]) -> Meta {
        Meta {
            path: PathBuf::from("C:/sessions/demo/x.jsonl"),
            cwd: Some(PathBuf::from("C:/Users/Agustin/Projects/tupa-launcher")),
            title: "修托盘菜单".to_string(),
            keywords: keywords.iter().map(|k| k.to_string()).collect(),
            favorite,
            mtime,
            messages,
        }
    }

    fn ago(days: u64) -> SystemTime {
        SystemTime::now() - std::time::Duration::from_secs(days * DAY_SECS)
    }

    #[test]
    fn window_from_flags() {
        let f = |s: &str| vec![s.to_string()];
        assert_eq!(window_days(&[]), 14);
        assert_eq!(window_days(&f("week")), 7);
        assert_eq!(window_days(&f("month")), 30);
        assert_eq!(window_days(&f("restore")), 14);
    }

    #[test]
    fn favorites_are_protected_regardless_of_age() {
        let m = meta(ago(100), true, 30, &["插件"]);
        assert_eq!(classify(&m, 14, SystemTime::now()), Disposition::Protect);
    }

    #[test]
    fn sessions_inside_window_are_protected() {
        let m = meta(ago(3), false, 2, &[]);
        assert_eq!(classify(&m, 14, SystemTime::now()), Disposition::Protect);
    }

    #[test]
    fn boundary_of_window() {
        let now = SystemTime::now();
        let m = meta(now - std::time::Duration::from_secs(14 * DAY_SECS), false, 2, &[]);
        assert_eq!(classify(&m, 14, now), Disposition::Trash);
    }

    #[test]
    fn multi_turn_is_archived() {
        let m = meta(ago(20), false, MULTI_TURN, &[]);
        assert_eq!(classify(&m, 14, SystemTime::now()), Disposition::Archive);
    }

    #[test]
    fn summarized_is_archived() {
        let m = meta(ago(20), false, 2, &["插件构建"]);
        assert_eq!(classify(&m, 14, SystemTime::now()), Disposition::Archive);
    }

    #[test]
    fn thin_session_is_trashed() {
        let m = meta(ago(20), false, 2, &[]);
        assert_eq!(classify(&m, 14, SystemTime::now()), Disposition::Trash);
    }

    #[test]
    fn narrower_window_exposes_more_sessions() {
        let m = meta(ago(10), false, 2, &[]);
        let now = SystemTime::now();
        assert_eq!(classify(&m, 14, now), Disposition::Protect);
        assert_eq!(classify(&m, 7, now), Disposition::Trash);
    }

    #[test]
    fn archive_and_restore_targets_round_trip() {
        let root = Path::new("C:/Users/Agustin/.pi/agent");
        let active = root.join("sessions").join("--C--Users-Agustin--").join("a.jsonl");
        let archived = archive_target(root, &active).expect("archive target");
        assert_eq!(
            archived,
            root.join("session-archive")
                .join("--C--Users-Agustin--")
                .join("a.jsonl")
        );
        assert_eq!(restore_target(root, &archived).expect("restore target"), active);
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(fmt_size(0), "0B");
        assert_eq!(fmt_size(2048), "2K");
        assert_eq!(fmt_size(5 * 1024 * 1024), "5.0M");
    }

    #[test]
    fn home_directory_shows_as_tilde() {
        let Some(home) = dirs::home_dir() else { return };
        let mut m = meta(ago(1), false, 1, &[]);
        m.cwd = Some(home);
        assert_eq!(project_name(&m), "~");
    }

    #[test]
    fn targets_require_matching_root() {
        let root = Path::new("C:/Users/Agustin/.pi/agent");
        assert!(archive_target(root, Path::new("D:/elsewhere/a.jsonl")).is_none());
        assert!(restore_target(root, Path::new("D:/elsewhere/a.jsonl")).is_none());
    }
}
