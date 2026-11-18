//! `pi` —— 列出 pi 的所有会话，按关键词搜索，回车在新终端里打开。
//!
//! pi 的会话按项目目录存在 `~/.pi/agent/sessions/<编码路径>/*.jsonl`，
//! 归档在 `session-archive/`（用 `:pi:all` 才包含）。每个文件：
//!
//! - 第一行 `{"type":"session","id":…,"timestamp":…,"cwd":…}`
//! - 后面是 `message` 事件
//! - `pi-sessions` 的自动标题写成 `{"type":"session_info","name":…}`，
//!   **可以出现在任意位置**（改了标题就追加一条）
//!
//! 标题优先级（见 [`read_meta`]）：pi-sessions 索引里的 `session_name` >
//! 本文件前 80 行里最后一条 `session_info` > `session-keywords.json` 的 summary >
//! 第一条 `role=user` 的文本 > 文件名。索引优先是因为它由 pi 自己在每个 `turn_end`
//! 同步，能拿到本文件前 80 行看到的最新标题。
//!
//! 只读每个文件的前几十行（不会把几 MB 的会话全读进来），索引查询是 SQLite 只读的。

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use super::pi_index::key_of;
use super::{Action, Column, Item, Kind, Provider};
use crate::i18n;

pub struct PiSessions;

/// 标题 · 项目 · 时间
pub(crate) const COLUMNS: &[Column] = &[
    Column::new(5, 14, 0),
    Column::new(3, 0, 28),
    Column::new(1, 6, 0),
];

#[derive(Debug, Default, Deserialize)]
pub(super) struct KeywordInfo {
    #[serde(default)]
    pub(super) summary: String,
    #[serde(default)]
    pub(super) keywords: Vec<String>,
}

pub(super) struct Meta {
    pub(super) path: PathBuf,
    pub(super) cwd: Option<PathBuf>,
    pub(super) title: String,
    pub(super) keywords: Vec<String>,
    pub(super) favorite: bool,
    pub(super) mtime: SystemTime,
    /// 前 80 行里的 user + assistant 消息数（`:clean` 用它判定“多轮”）
    pub(super) messages: u32,
}

impl Provider for PiSessions {
    fn keyword(&self) -> &str {
        "pi"
    }

    fn description(&self) -> &'static str {
        i18n::desc_pi()
    }

    fn flags(&self) -> &'static [&'static str] {
        &["all", "clean", "cleanall", "week", "month", "restore"]
    }

    fn columns(&self) -> &'static [Column] {
        COLUMNS
    }

    fn query(&self, args: &str, flags: &[String]) -> Vec<Item> {
        // `:pi:clean` 走清理视图（同一个 provider，避免多一个命令）
        if flags
            .iter()
            .any(|flag| flag == "clean" || flag == "cleanall")
        {
            return super::clean::query(args, flags);
        }
        let Some(home) = dirs::home_dir() else {
            return Vec::new();
        };
        let root = home.join(".pi").join("agent");
        let keywords = load_keywords(&root);
        let favorites = load_favorites(&root);
        let names = super::pi_index::load_names();

        let mut metas = Vec::new();
        collect(
            &root.join("sessions"),
            &keywords,
            &favorites,
            &names,
            &mut metas,
        );
        if flags.iter().any(|flag| flag == "all") {
            collect(
                &root.join("session-archive"),
                &keywords,
                &favorites,
                &names,
                &mut metas,
            );
        }

        let query = args.trim().to_lowercase();
        let mut items: Vec<Item> = Vec::new();

        // 时间新的在前；收藏只加星，不改变排序
        metas.sort_by_key(|meta| std::cmp::Reverse(meta.mtime));

        for meta in metas {
            if !query.is_empty() {
                let cwd = meta
                    .cwd
                    .as_ref()
                    .map(|p| p.display().to_string().to_lowercase())
                    .unwrap_or_default();
                let hit = meta.title.to_lowercase().contains(&query)
                    || cwd.contains(&query)
                    || meta
                        .keywords
                        .iter()
                        .any(|keyword| keyword.to_lowercase().contains(&query));
                if !hit {
                    continue;
                }
            }

            let project = meta
                .cwd
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "~".to_string());
            let title = if meta.favorite {
                format!("★ {}", meta.title)
            } else {
                meta.title
            };
            let dir = meta.cwd.clone().filter(|p| p.is_dir());
            items.push(Item::new(
                Kind::Pi,
                vec![title, project, age(meta.mtime)],
                Action::LaunchInTerminal {
                    command: format!("pi --session \"{}\"", meta.path.display()),
                    dir,
                },
            ));
        }

        items
    }
}

pub(super) fn collect(
    dir: &Path,
    keywords: &HashMap<String, KeywordInfo>,
    favorites: &[String],
    names: &HashMap<String, String>,
    out: &mut Vec<Meta>,
) {
    let Ok(projects) = std::fs::read_dir(dir) else {
        return;
    };
    for project in projects.flatten() {
        let project_path = project.path();
        if !project_path.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&project_path) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(meta) = read_meta(&path, keywords, favorites, names) {
                out.push(meta);
            }
        }
    }
}

pub(super) fn read_meta(
    path: &Path,
    keywords: &HashMap<String, KeywordInfo>,
    favorites: &[String],
    names: &HashMap<String, String>,
) -> Option<Meta> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut cwd = None;
    // `session_info` 里的会话名（自动标题 / 手动 `/name`），后出现的覆盖前面的
    let mut session_name = None;
    // 第一条用户消息，作为没有名字时的兜底
    let mut first_user = None;
    let mut messages = 0u32;

    for line in reader.lines().take(80) {
        let Ok(line) = line else {
            break;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        match value.get("type").and_then(|v| v.as_str()) {
            Some("session") => {
                cwd = value.get("cwd").and_then(|v| v.as_str()).map(PathBuf::from);
            }
            Some("message") => {
                let Some(message) = value.get("message") else {
                    continue;
                };
                let role = message.get("role").and_then(|v| v.as_str());
                if matches!(role, Some("user") | Some("assistant")) {
                    messages += 1;
                }
                if role != Some("user") || first_user.is_some() {
                    continue;
                }
                let Some(text) = message.get("content").and_then(extract_text) else {
                    continue;
                };
                let first = text.lines().next().unwrap_or("").trim();
                if !first.is_empty() {
                    first_user = Some(truncate(first, 80));
                }
            }
            Some("session_info") => {
                let name = value
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|name| !name.is_empty());
                if let Some(name) = name {
                    session_name = Some(truncate(name, 80));
                }
            }
            _ => {}
        }
    }

    let key = path.to_string_lossy().into_owned();
    let keyword_info = keywords.get(&key);
    // 索引里的名字最可信：它由 pi 自己同步，能看到前 80 行之外的最新标题
    let title = names
        .get(&key_of(&key))
        .map(|name| truncate(name, 80))
        .or(session_name)
        .or_else(|| {
            keyword_info
                .and_then(|info| (!info.summary.is_empty()).then(|| info.summary.clone()))
        })
        .or(first_user)
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    let keywords = keyword_info
        .map(|info| info.keywords.clone())
        .unwrap_or_default();
    let favorite = favorites
        .iter()
        .any(|favorite| favorite.eq_ignore_ascii_case(&key));
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(UNIX_EPOCH);

    Some(Meta {
        path: path.to_path_buf(),
        cwd,
        title,
        keywords,
        favorite,
        mtime,
        messages,
    })
}

pub(super) fn load_keywords(root: &Path) -> HashMap<String, KeywordInfo> {
    std::fs::read_to_string(root.join("session-keywords.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub(super) fn load_favorites(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join("session-favorites.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// `content` 可能是字符串，也可能是 `[{type:"text",text:…}, …]`
fn extract_text(content: &serde_json::Value) -> Option<String> {
    match content {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Array(parts) => {
            let text: Vec<&str> = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
                .collect();
            (!text.is_empty()).then(|| text.join(" "))
        }
        _ => None,
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        out.push('…');
    }
    out
}

pub(super) fn age(mtime: SystemTime) -> String {
    let secs = SystemTime::now()
        .duration_since(mtime)
        .unwrap_or_default()
        .as_secs();
    match secs {
        0..=59 => i18n::age_now().to_string(),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_session(tag: &str, lines: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tupa-pi-meta-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{tag}.jsonl"));
        std::fs::write(&path, lines.join("\n")).unwrap();
        path
    }

    fn meta_of(path: &Path, names: &HashMap<String, String>) -> Meta {
        read_meta(path, &HashMap::new(), &[], names).unwrap()
    }

    const HEAD: &str = r#"{"type":"session","cwd":"C:\\Users\\x"}"#;

    #[test]
    fn session_info_name_beats_first_user_message() {
        let path = temp_session(
            "name",
            &[
                HEAD,
                r#"{"type":"message","message":{"role":"user","content":"第一条消息"}}"#,
                r#"{"type":"session_info","name":"自动标题"}"#,
            ],
        );
        assert_eq!(meta_of(&path, &HashMap::new()).title, "自动标题");
    }

    #[test]
    fn later_session_info_wins() {
        let path = temp_session(
            "retitle",
            &[
                HEAD,
                r#"{"type":"session_info","name":"旧标题"}"#,
                r#"{"type":"session_info","name":"新标题"}"#,
            ],
        );
        assert_eq!(meta_of(&path, &HashMap::new()).title, "新标题");
    }

    #[test]
    fn empty_or_missing_name_falls_back() {
        let path = temp_session(
            "fallback",
            &[
                HEAD,
                r#"{"type":"session_info","name":"   "}"#,
                r#"{"type":"message","message":{"role":"user","content":"第一条消息\n第二行"}}"#,
            ],
        );
        assert_eq!(meta_of(&path, &HashMap::new()).title, "第一条消息");
    }

    #[test]
    fn index_name_beats_everything_in_the_file() {
        let path = temp_session(
            "indexed",
            &[
                HEAD,
                r#"{"type":"session_info","name":"文件里的名字"}"#,
                r#"{"type":"message","message":{"role":"user","content":"第一条消息"}}"#,
            ],
        );
        let key = key_of(&path.to_string_lossy());
        let names = HashMap::from([(key, "索引里的名字".to_string())]);
        assert_eq!(meta_of(&path, &names).title, "索引里的名字");
    }

    #[test]
    fn file_stem_is_the_last_resort() {
        let path = temp_session("only-stem", &[HEAD]);
        assert_eq!(meta_of(&path, &HashMap::new()).title, "only-stem");
    }

    #[test]
    fn extracts_text_from_string_and_parts() {
        let s = serde_json::json!("hello");
        assert_eq!(extract_text(&s).as_deref(), Some("hello"));

        let a = serde_json::json!([
            {"type": "text", "text": "foo"},
            {"type": "text", "text": "bar"}
        ]);
        assert_eq!(extract_text(&a).as_deref(), Some("foo bar"));
    }

    #[test]
    fn truncates_by_chars() {
        assert_eq!(truncate("abcdef", 3), "abc…");
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("中文测试", 2), "中文…");
    }
}
