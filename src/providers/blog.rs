//! `blog` —— 自己博客（Hexo）的三件事：站点入口、新建文章、管理页。
//!
//! ```text
//! blog              → 三个入口：本地预览 / GitHub Pages / Cloudflare
//! blog:new 周末折腾  → 新建 source/_posts/周末折腾.md（带 front matter）并在 VSCode 里打开
//! blog:new          → 列最近改过的文章（回车用 VSCode 打开）
//! blog:manage       → 后台跑 `pnpm admin`，稍后打开管理页（127.0.0.1:5173）
//! ```
//!
//! 仓库目录与三个网址都在 `config.toml` 的 `[blog]` 里（目录留空 = `~/blog`）。
//!
//! 新建文章**不覆盖**：同名文件已存在时直接打开它，列表里也会标出来。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{Action, Item, Kind, Provider};
use crate::config::Config;
use crate::i18n;

/// 认识的修饰符（`blog:new` / `blog:manage`）
const FLAGS: &[&str] = &["new", "manage"];

/// Windows 不允许进文件名的字符
const ILLEGAL: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

pub struct Blog {
    dir: PathBuf,
    local: String,
    github: String,
    cloudflare: String,
    admin: String,
    /// `blog:new` 空参数时列几篇最近改过的文章
    max: usize,
}

impl Blog {
    pub fn new(cfg: &Config) -> Self {
        let dir = cfg
            .blog
            .dir
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join("blog")))
            .unwrap_or_else(|| PathBuf::from("blog"));

        Blog {
            dir,
            local: cfg.blog.local_url.clone(),
            github: cfg.blog.github_url.clone(),
            cloudflare: cfg.blog.cloudflare_url.clone(),
            admin: cfg.blog.admin_url.clone(),
            max: cfg.general.max_results,
        }
    }

    /// 文章目录（`<博客>/source/_posts`）
    fn posts_dir(&self) -> PathBuf {
        self.dir.join("source").join("_posts")
    }

    /// 不带修饰符：三个站点入口
    fn entries(&self) -> Vec<Item> {
        [
            (i18n::blog_local_preview(), &self.local),
            (i18n::blog_github(), &self.github),
            (i18n::blog_cloudflare(), &self.cloudflare),
        ]
        .into_iter()
        .map(|(label, url)| {
            Item::new(
                Kind::Web,
                vec![label.to_string(), url.clone()],
                Action::OpenUrl(url.clone()),
            )
        })
        .collect()
    }

    fn manage_item(&self) -> Item {
        Item::new(
            Kind::Command,
            vec![
                i18n::blog_admin().to_string(),
                format!("pnpm admin · {}", self.dir.display()),
            ],
            Action::BlogAdmin {
                dir: self.dir.clone(),
                url: self.admin.clone(),
            },
        )
    }

    /// `blog:new [标题]`
    fn new_post(&self, title: &str) -> Vec<Item> {
        let posts = self.posts_dir();

        // 没给标题：列最近改过的文章，回车直接进编辑器改
        if title.is_empty() {
            return self.recent_posts(&posts);
        }

        let Some(name) = post_file_name(title) else {
            return Vec::new();
        };
        let path = posts.join(&name);

        let (action, subtitle) = if path.exists() {
            (
                Action::OpenInEditor {
                    file: path,
                    line: 1,
                },
                i18n::blog_post_exists().to_string(),
            )
        } else {
            (
                Action::CreateBlogPost {
                    path,
                    title: title.trim().to_string(),
                },
                i18n::blog_posts_dir().to_string(),
            )
        };

        vec![Item::new(Kind::File, vec![name, subtitle], action)]
    }

    /// 最近改过的文章（`source/_posts` 里的 `.md`，按修改时间倒序）
    fn recent_posts(&self, posts: &Path) -> Vec<Item> {
        let Ok(entries) = std::fs::read_dir(posts) else {
            return Vec::new();
        };

        let mut files: Vec<(SystemTime, PathBuf)> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
            })
            .map(|path| {
                let mtime = std::fs::metadata(&path)
                    .and_then(|meta| meta.modified())
                    .unwrap_or(SystemTime::UNIX_EPOCH);
                (mtime, path)
            })
            .collect();

        files.sort_by_key(|(mtime, _)| std::cmp::Reverse(*mtime));

        files
            .into_iter()
            .take(self.max)
            .map(|(mtime, path)| {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                Item::new(
                    Kind::File,
                    vec![
                        name,
                        format!("{} · {}", i18n::blog_posts_dir(), super::pi::age(mtime)),
                    ],
                    Action::OpenInEditor { file: path, line: 1 },
                )
            })
            .collect()
    }
}

impl Provider for Blog {
    fn keyword(&self) -> &str {
        "blog"
    }

    fn description(&self) -> &'static str {
        i18n::desc_blog()
    }

    fn flags(&self) -> &'static [&'static str] {
        FLAGS
    }

    fn query(&self, args: &str, flags: &[String]) -> Vec<Item> {
        if flags.iter().any(|flag| flag == "new") {
            return self.new_post(args.trim());
        }
        if flags.iter().any(|flag| flag == "manage") {
            return vec![self.manage_item()];
        }
        self.entries()
    }
}

/// 标题 → 文件名：Windows 不认的字符换成 `-`，补 `.md`（标题本身不动）
fn post_file_name(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if ILLEGAL.contains(&c) || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches('.').trim();
    if cleaned.is_empty() {
        return None;
    }
    Some(if cleaned.to_ascii_lowercase().ends_with(".md") {
        cleaned.to_string()
    } else {
        format!("{cleaned}.md")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_chinese_titles_as_file_names() {
        assert_eq!(post_file_name("周末折腾").as_deref(), Some("周末折腾.md"));
        assert_eq!(
            post_file_name("TSCTF2026-WriteUp").as_deref(),
            Some("TSCTF2026-WriteUp.md")
        );
        // 已经写了 `.md` 就不重复补
        assert_eq!(post_file_name("笔记.md").as_deref(), Some("笔记.md"));
    }

    #[test]
    fn replaces_illegal_characters() {
        assert_eq!(post_file_name("a/b:c").as_deref(), Some("a-b-c.md"));
        assert_eq!(post_file_name("q?").as_deref(), Some("q-.md"));
        assert_eq!(post_file_name("  空格  ").as_deref(), Some("空格.md"));
    }

    #[test]
    fn rejects_empty_titles() {
        assert_eq!(post_file_name(""), None);
        assert_eq!(post_file_name("   "), None);
        assert_eq!(post_file_name(".."), None);
        assert_eq!(post_file_name("..."), None);
    }
}
