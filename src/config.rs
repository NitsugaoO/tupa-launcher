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

    /// 热键唤起的面板用哪一版：`tui`（Windows Terminal，默认）还是 `gui`（自绘窗口）。
    ///
    /// 两套界面共用同一份代码和状态机，只是外壳不同。
    /// GUI 版还在打磨（字体/圆角/鼠标等），所以默认仍走终端版。
    pub panel: String,
}

impl Default for Daemon {
    fn default() -> Self {
        Daemon {
            hotkey: "alt+`".to_string(),
            copilot_key: true,
            panel: "tui".to_string(),
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
    /// 开屏时输入框是否预填命令前缀（直接进入命令模式）。
    ///
    /// `false` 时开屏是空输入，但仍然显示命令列表。
    pub prefill_colon: bool,
    /// `:new` 新建文件放在哪个目录；留空 = `~/Projects/New Files`
    pub new_files_dir: Option<String>,
    /// 命令前缀，默认 `:`，也可以改成 `@` 之类不常用的符号。
    ///
    /// 修饰符分隔符也用这个字符：`@run@admin`。
    pub command_prefix: String,
    /// 界面语言：`zh` 或 `en`（未知值按 `zh` 处理）
    pub language: String,
    /// 是否在右下角显示快捷键提示
    pub show_hints: bool,
}

impl Default for General {
    fn default() -> Self {
        General {
            default_provider: "app".to_string(),
            max_results: 30,
            everything_path: None,
            prefill_colon: false,
            new_files_dir: None,
            command_prefix: ":".to_string(),
            language: "zh".to_string(),
            show_hints: true,
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
