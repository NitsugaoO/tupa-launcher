//! `blog` —— 自己博客（Hexo）的日常操作：新建文章、本地预览、管理页、站点入口。
//!
//! ```text
//! blog              → 五个入口：新建文章 / 本地预览 / 管理页 / GitHub Pages / Cloudflare
//! blog:new 周末折腾  → 新建 source/_posts/周末折腾.md（带 front matter）并在 VSCode 里打开
//! blog:new          → 列最近改过的文章（回车用 VSCode 打开）
//! blog:manage       → 后台跑 `pnpm admin`，端口就绪后打开管理页（127.0.0.1:5173）
//! ```
//!
//! 仓库目录与三个网址都在 `config.toml` 的 `[blog]` 里（目录留空 = `~/blog`）。
//!
//! 本地预览 / 管理页都走「先探测端口」的路子（见 [`crate::serve`]）：
//!
//! - 服务在跑 → 只开浏览器，不重复起进程
//! - 没在跑 → 后台起 `pnpm server` / `pnpm admin`，等端口监听上再打开
//!
//! 列表里这两行的状态列是**查询时现探的**（`query` 在后台线程，150ms × 2 的探测不会卡界面）。
//!
//! 新建文章**不覆盖**：同名文件已存在时直接打开它，列表里也会标出来。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::{Action, Column, Item, Kind, Provider};
use crate::config::Config;
use crate::i18n;
use crate::serve;

/// 认识的修饰符（`blog:new` / `blog:manage`）
const FLAGS: &[&str] = &["new", "manage"];

/// Windows 不允许进文件名的字符
const ILLEGAL: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// 标题 · 网址 · 本地服务状态。
///
/// 第三列只在本地预览 / 管理页两行有内容（「已在运行」/「未启动，回车开」），
/// 三个网址入口留空 —— 宽度给得比较宽，因为中文状态带提示语。
pub(crate) const COLUMNS: &[Column] =
    &[Column::new(4, 10, 0), Column::new(5, 12, 0), Column::new(2, 6, 18)];

pub struct Blog {
    dir: PathBuf,
    local: String,
    github: String,
    cloudflare: String,
    admin: String,
    /// `blog:new` 空参数时列几篇最近改过的文章
    max: usize,
    /// 命令前缀（「新建文章」条目要把 `blog:new ` 补进输入框）
    prefix: char,
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
            prefix: cfg.general.command_prefix.chars().next().unwrap_or(':'),
        }
    }

    /// 文章目录（`<博客>/source/_posts`）
    fn posts_dir(&self) -> PathBuf {
        self.dir.join("source").join("_posts")
    }

    /// 「新建文章」条目：回车只是把 `blog:new ` 补进输入框，标题接着打
    fn new_post_item(&self) -> Item {
        Item::new(
            Kind::Help,
            vec![
                format!("{prefix}blog:new", prefix = self.prefix),
                i18n::blog_new_post_hint().to_string(),
            ],
            Action::FillInput(format!("{prefix}blog:new ", prefix = self.prefix)),
        )
    }

    /// 一个「本地服务」条目：状态列现探（端口在跑 / 没跑）
    fn local_item(&self, label: &str, url: &str, action: Action) -> Item {
        let state = match serve::port_of(url) {
            Some(port) if serve::is_listening(port) => i18n::serve_running(),
            _ => i18n::serve_stopped(),
        };
        Item::new(
            Kind::Web,
            vec![label.to_string(), url.to_string(), state.to_string()],
            action,
        )
    }

    /// 不带修饰符：五个入口
    fn entries(&self) -> Vec<Item> {
        let mut items = vec![
            self.new_post_item(),
            self.local_item(
                i18n::blog_local_preview(),
                &self.local,
                Action::BlogPreview {
                    dir: self.dir.clone(),
                    url: self.local.clone(),
                },
            ),
            self.local_item(i18n::blog_admin(), &self.admin, self.manage_item_action()),
        ];

        // 远端站点：没有本地服务，第三列空着（只占宽度，什么都不显示）
        for (label, url) in [
            (i18n::blog_github(), &self.github),
            (i18n::blog_cloudflare(), &self.cloudflare),
        ] {
            items.push(Item::new(
                Kind::Web,
                vec![label.to_string(), url.to_string(), String::new()],
                Action::OpenUrl(url.clone()),
            ));
        }

        items
    }

    fn manage_item_action(&self) -> Action {
        Action::BlogAdmin {
            dir: self.dir.clone(),
            url: self.admin.clone(),
        }
    }

    fn manage_item(&self) -> Item {
        self.local_item(i18n::blog_admin(), &self.admin, self.manage_item_action())
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

    fn columns(&self) -> &'static [Column] {
        COLUMNS
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
