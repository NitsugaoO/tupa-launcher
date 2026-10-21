//! `pi` —— 列出 pi 的所有会话，按关键词搜索，回车在新终端里打开。
//!
//! pi 的会话按项目目录存在 `~/.pi/agent/sessions/<编码路径>/*.jsonl`，
//! 归档在 `session-archive/`（用 `:pi:all` 才包含）。每个文件：
//!
//! - 第一行 `{"type":"session","id":…,"timestamp":…,"cwd":…}`
//! - 后面是 `message` 事件，第一条 `role=user` 的文本拿来当标题
//!
//! 只读每个文件的前几十行（找到第一条用户消息就停），不会把几 MB 的会话全读进来。

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use super::{Action, Column, Item, Kind, Provider};
use crate::i18n;

pub struct PiSessions;

/// 标题 · 项目 · 时间
const COLUMNS: &[Column] = &[
    Column::new(5, 14, 0),
    Column::new(3, 0, 28),
    Column::new(1, 6, 0),
];

#[derive(Debug, Default, Deserialize)]
struct KeywordInfo {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    keywords: Vec<String>,
}

struct Meta {
    path: PathBuf,
    cwd: Option<PathBuf>,
    title: String,
    keywords: Vec<String>,
    favorite: bool,
    mtime: SystemTime,
}

impl Provider for PiSessions {
    fn keyword(&self) -> &str {
        "pi"
    }

    fn description(&self) -> &'static str {
        i18n::desc_pi()
    }

    fn flags(&self) -> &'static [&'static str] {
        &["all"]
    }

    fn columns(&self) -> &'static [Column] {
        COLUMNS
    }

    fn query(&self, args: &str, flags: &[String]) -> Vec<Item> {
        let Some(home) = dirs::home_dir() else {
            return Vec::new();
        };
        let root = home.join(".pi").join("agent");
        let keywords = load_keywords(&root);
        let favorites = load_favorites(&root);

        let mut metas = Vec::new();
        collect(&root.join("sessions"), &keywords, &favorites, &mut metas);
        if flags.iter().any(|flag| flag == "all") {
            collect(
                &root.join("session-archive"),
                &keywords,
                &favorites,
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

fn collect(
    dir: &Path,
    keywords: &HashMap<String, KeywordInfo>,
    favorites: &[String],
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
            if let Some(meta) = read_meta(&path, keywords, favorites) {
                out.push(meta);
            }
        }
    }
}

fn read_meta(
    path: &Path,
    keywords: &HashMap<String, KeywordInfo>,
    favorites: &[String],
) -> Option<Meta> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut cwd = None;
    let mut title = None;

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
                if message.get("role").and_then(|v| v.as_str()) != Some("user") {
                    continue;
                }
                let Some(text) = message.get("content").and_then(extract_text) else {
                    continue;
                };
                let first = text.lines().next().unwrap_or("").trim();
                if !first.is_empty() {
                    title = Some(truncate(first, 80));
                    break;
                }
            }
            _ => {}
        }
    }

    let key = path.to_string_lossy().into_owned();
    let keyword_info = keywords.get(&key);
    let title = keyword_info
        .and_then(|info| (!info.summary.is_empty()).then(|| info.summary.clone()))
        .or(title)
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
    })
}

fn load_keywords(root: &Path) -> HashMap<String, KeywordInfo> {
    std::fs::read_to_string(root.join("session-keywords.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn load_favorites(root: &Path) -> Vec<String> {
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

fn age(mtime: SystemTime) -> String {
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
