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
    pub gui: Gui,
    pub glazewm: Glazewm,
    pub blog: Blog,
    pub theme: ThemeOverrides,
    pub apps: Vec<AppEntry>,
}

/// `[gui]` —— GPUI 面板（`tupa-gui.exe`）自己的设置。
///
/// 与 `[daemon]` 完全独立：`tupa-gui.exe` 自带托盘和热键，不经过守护进程。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Gui {
    /// 唤起 / 隐藏 GPUI 面板的全局热键（和 `[daemon] hotkey` 互不干扰）
    pub hotkey: String,

    /// 是否显示托盘图标（关掉就只剩热键 + 窗口）
    pub tray: bool,

    /// 是否开动画：列表平滑滚动、选框滑动、光标滑动、展开淡入。
    ///
    /// 关掉后所有位移都是瞬时跳变（省电 / 不想要动效时用）。
    pub animations: bool,

    /// 面板背景不透明度（1.0 = 完全不透明；0.8 能透出后面的壁纸/窗口）
    ///
    /// 开了 [`Gui::acrylic`] 之后，它就是「亚克力上的 tint 浓度」：越小越能看清背后的模糊。
    pub opacity: f32,

    /// 是否给面板挂 Windows 的亚克力材质（DWM 的 Acrylic：背景实时模糊）
    ///
    /// **系统可能自己关掉它**：节能模式（`GetSystemPowerStatus` 的 `SystemStatusFlag`）、
    /// 「设置 → 个性化 → 颜色 → 透明效果」关闭时，Acrylic/Mica 会被 DWM 降级成不透明纯色
    /// —— 这不是 bug，排查「开了没效果」先看这两处。
    ///
    /// 实现见 `src/gui/acrylic.rs`：GPUI 只负责把窗口设成 `Blurred`，真正生效靠
    /// 我们再补一次 `SetWindowCompositionAttribute`（gradient alpha 必须是 1，0 会变黑玻璃）。
    pub acrylic: bool,

    /// 面板外框是否用圆角（默认开）
    ///
    /// **关掉它才能让亚克力边缘干净**：DWM 的 `SetWindowCompositionAttribute` 是给
    /// 整个窗口矩形铺亚克力的，`DWMWA_WINDOW_CORNER_PREFERENCE` 只裁内容不裁它，
    /// 所以圆角外侧会露出一块方形的亚克力。直角时两者重合，看不出接缝。
    pub rounded: bool,

    /// 面板是否一直置顶（默认开）
    ///
    /// 关掉后窗口会被普通窗口压住 —— GlazeWM 里 `tupa-gui` 是 `ignore`，不会替你抬到前面。
    pub always_on_top: bool,
}

impl Default for Gui {
    fn default() -> Self {
        Gui {
            hotkey: "alt+0".to_string(),
            tray: true,
            animations: true,
            opacity: 1.0,
            acrylic: false,
            rounded: true,
            always_on_top: true,
        }
    }
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

    /// 热键唤起的面板用哪一版：`tui`（终端里的界面，默认）还是 `gui`（自绘窗口）。
    ///
    /// 两套界面共用同一份代码和状态机，只是外壳不同。
    pub panel: String,

    /// TUI 面板跑在哪个终端里：`auto`（默认）、`wezterm`、`wt`。
    ///
    /// `auto` = 装了 WezTerm 就用 WezTerm（窗口类名可控，不会误判），
    /// 否则退回 Windows Terminal（需要 `wt` 里有个叫 `tupa-launcher` 的 profile）。
    pub terminal: String,
}

impl Default for Daemon {
    fn default() -> Self {
        Daemon {
            hotkey: "alt+`".to_string(),
            copilot_key: true,
            panel: "tui".to_string(),
            terminal: "auto".to_string(),
        }
    }
}

/// `[glazewm]` —— GlazeWM 窗口管理器的集成
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Glazewm {
    /// 打开网页后，如果浏览器窗口在**另一个 workspace** 里（或被 WM 隐藏），
    /// 让 GlazeWM 自己把那个 workspace 切到前台。没装 GlazeWM 时自动忽略。
    ///
    /// 关掉之后 `:web` / `:blog` 打开的网页只是在后台新建了一个标签，
    /// 浏览器不在当前 workspace 时就看不到任何变化。
    pub focus_browser: bool,
}

impl Default for Glazewm {
    fn default() -> Self {
        Glazewm {
            focus_browser: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct General {
    /// 输入不以任何 provider 关键字开头时，交给哪个 provider（默认 `all` = 应用 + 历史）
    pub default_provider: String,
    /// 单个 provider 最多返回多少条结果
    pub max_results: usize,
    /// `es.exe` 路径；留空则自动探测
    pub everything_path: Option<String>,
    /// 开屏时输入框是否预填命令前缀（直接进入命令模式）。
    ///
    /// `false` 时开屏是空输入，但仍然显示命令列表。
    pub prefill_colon: bool,
    /// 命令前缀，默认 `:`，也可以改成 `@` 之类不常用的符号。
    ///
    /// 修饰符分隔符也用这个字符：`@run@admin`。
    pub command_prefix: String,
    /// 界面语言：`zh` 或 `en`（未知值按 `zh` 处理）
    pub language: String,
    /// 是否在右下角显示快捷键提示
    pub show_hints: bool,
    /// 开屏与 `@app` 空查询是否按 frecency（使用频率 × 新鲜度）排序。
    ///
    /// 关掉之后：开屏历史按时间、`@app` 按配置顺序。
    pub frecency: bool,
    /// 剪贴板里是**存在的路径**或 **http(s) 网址**时，开屏第一行多一条
    /// 「跳过去」的条目（默认不选中，按 `↑` 才选它）。
    pub clipboard: bool,
}

impl Default for General {
    fn default() -> Self {
        General {
            default_provider: "all".to_string(),
            max_results: 30,
            everything_path: None,
            prefill_colon: false,
            command_prefix: ":".to_string(),
            language: "zh".to_string(),
            show_hints: true,
            frecency: true,
            clipboard: true,
        }
    }
}

/// `[blog]` —— `:blog` 用到的目录与网址
///
/// 默认值就是本机这套 Hexo 博客（`~/blog`，GitHub Pages + Cloudflare 双部署）。
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Blog {
    /// 博客仓库根目录（`package.json` 和 `source/_posts` 在这里）；留空 = `~/blog`
    pub dir: Option<String>,
    /// 本地开发服务器（`pnpm server`，Hexo 默认 4000）
    pub local_url: String,
    /// GitHub Pages 地址
    pub github_url: String,
    /// Cloudflare Workers 地址
    pub cloudflare_url: String,
    /// `pnpm admin` 起来的管理页
    pub admin_url: String,
}

impl Default for Blog {
    fn default() -> Self {
        Blog {
            dir: None,
            local_url: "http://localhost:4000".to_string(),
            github_url: "https://nitsugaoo.github.io".to_string(),
            cloudflare_url: "https://blog.wooyun.workers.dev".to_string(),
            admin_url: "http://127.0.0.1:5173".to_string(),
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
