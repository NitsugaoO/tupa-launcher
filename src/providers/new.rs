//! `new <文件名>` —— 在 `~/Projects/New Files` 里新建文件并打开。
//!
//! ```text
//! new 报告          → 新建 New Files\报告.md 并打开
//! new 笔记.md       → 新建 New Files\笔记.md
//! new todo.txt      → 新建 New Files\todo.txt
//! ```
//!
//! - 没写扩展名时默认补 `.md`（最常见的场景是记点东西）
//! - 同名文件已存在时**不覆盖**，列表里会提示
//! - 不支持放子目录（名字里带 `\` / `/` 等会被拒绝），避免拼出目录外的路径
//! - 目录可以在 `config.toml` 的 `general.new_files_dir` 里改

use std::path::{Path, PathBuf};

use super::{Action, Column, Item, Kind, Provider};
use crate::config::Config;

const COLUMNS: &[Column] = &[Column::new(5, 12, 0), Column::new(3, 0, 0)];

pub struct NewFile {
    dir: PathBuf,
}

impl NewFile {
    pub fn new(cfg: &Config) -> Self {
        let dir = cfg
            .general
            .new_files_dir
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join("Projects").join("New Files")))
            .unwrap_or_else(|| PathBuf::from("New Files"));

        NewFile { dir }
    }
}

impl Provider for NewFile {
    fn keyword(&self) -> &str {
        "new"
    }

    fn description(&self) -> &'static str {
        crate::i18n::desc_new()
    }

    fn columns(&self) -> &'static [Column] {
        COLUMNS
    }

    fn query(&self, args: &str, _flags: &[String]) -> Vec<Item> {
        let Some(name) = file_name(args) else {
            return Vec::new();
        };
        let path = self.dir.join(&name);

        let subtitle = if path.exists() {
            format!(
                "{} · {}",
                self.dir.display(),
                crate::i18n::new_file_exists()
            )
        } else {
            self.dir.display().to_string()
        };

        vec![Item::new(
            Kind::File,
            vec![name, subtitle],
            Action::CreateFile(path),
        )]
    }
}

/// 输入 → 安全的文件名（拒绝空名、`.`、`..` 和 Windows 不允许的字符）
fn file_name(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "." || raw == ".." {
        return None;
    }
    if raw
        .chars()
        .any(|c| matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        // 不允许路径分隔符：避免拼出 New Files 之外的路径
        return None;
    }

    // 没写扩展名就当 Markdown 笔记
    if Path::new(raw).extension().is_none() {
        Some(format!("{raw}.md"))
    } else {
        Some(raw.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_markdown_extension_by_default() {
        assert_eq!(file_name("报告").as_deref(), Some("报告.md"));
        assert_eq!(file_name("todo.txt").as_deref(), Some("todo.txt"));
        assert_eq!(file_name("  spaced.md  ").as_deref(), Some("spaced.md"));
    }

    #[test]
    fn rejects_paths_and_empty_names() {
        assert_eq!(file_name(""), None);
        assert_eq!(file_name("   "), None);
        assert_eq!(file_name(".."), None);
        assert_eq!(file_name("sub/name.md"), None);
        assert_eq!(file_name(r"sub\name.md"), None);
        assert_eq!(file_name("C:name.md"), None);
        assert_eq!(file_name("a*b.md"), None);
    }
}
