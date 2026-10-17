//! 配置加载：`~/.config/tupa-launcher/config.toml`
//!
//! 配置不存在时全部走内置默认值（不会自动往用户目录写文件）。

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::theme::ThemeOverrides;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub daemon: Daemon,
    pub theme: ThemeOverrides,
    pub apps: Vec<AppEntry>,
}

/// `[daemon]` —— 常驻守护进程的设置
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Daemon {
    /// 唤起/隐藏面板的全局热键，写法形如 `alt+``
    ///
    /// 支持 `ctrl` / `alt` / `shift` / `win` 四个修饰键，主键可以是字母、数字、
    /// `f1`..`f24`、`space`、`tab`、`enter`、`esc`、`` ` ``。
    ///
    /// 注意：**不要用裸 `Win+单个字母`**（如 `win+s`）—— 那是系统保留键，
    /// 注册不上（详细原因见 `daemon.rs` 开头的注释）。
    pub hotkey: String,

    /// 是否接管 **Copilot 键**（`Win+Shift+F23`）。
    ///
    /// 这个键的原样组合被 shell 占着，`RegisterHotKey` 注册不了，
    /// 所以要走低级键盘钩子拦下 `F23`；不想要就设 `false`。
    pub copilot_key: bool,
}

impl Default for Daemon {
    fn default() -> Self {
        Daemon {
            hotkey: "alt+`".to_string(),
            copilot_key: true,
        }
    }
}


#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct General {
    /// 输入不以任何 provider 关键字开头时，交给哪个 provider
    pub default_provider: String,
    /// 单个 provider 最多返回多少条结果
    pub max_results: usize,
    /// `es.exe` 路径；留空则自动探测
    pub everything_path: Option<String>,
}

impl Default for General {
    fn default() -> Self {
        General {
            default_provider: "app".to_string(),
            max_results: 30,
            everything_path: None,
        }
    }
}

/// `[[apps]]` —— 手工维护的应用列表（需求 R4）
#[derive(Debug, Clone, Deserialize)]
pub struct AppEntry {
    /// 列表里显示的名字
    pub name: String,
    /// 要执行的命令：可以是绝对路径、PATH 里的命令名、甚至 URL
    pub command: String,
    /// 额外参数
    #[serde(default)]
    pub args: Vec<String>,
    /// 额外别名（参与模糊匹配）
    #[serde(default)]
    pub keywords: Vec<String>,
}

impl AppEntry {
    /// 参与模糊匹配的所有字符串
    pub fn search_keys(&self) -> Vec<String> {
        let mut keys = vec![self.name.clone(), self.command.clone()];
        keys.extend(self.keywords.iter().cloned());
        keys
    }
}

impl Config {
    pub fn load() -> Result<Config> {
        let Some(path) = config_path() else {
            return Ok(Config::default());
        };
        if !path.is_file() {
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("读取配置失败：{}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("解析配置失败：{}", path.display()))
    }
}

/// `~/.config/tupa-launcher/`（刻意的 Linux 风格路径，便于和 dotfiles 一起管理）
pub fn config_dir() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".config").join("tupa-launcher"))
}

pub fn config_path() -> Option<PathBuf> {
    Some(config_dir()?.join("config.toml"))
}
