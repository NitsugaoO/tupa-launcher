//! `go <路径片段>` —— 跳到目录 / 定位文件。
//!
//! 输入可以是这些写法（都能识别）：
//!
//! ```text
//! go ~/Projects/tupa-launcher
//! go "C:\Users\<你的用户名>\Projects"
//! go file:///C:/Users/<你的用户名>/Projects
//! go projects\term          ← 不完整的路径片段
//! go                        ← 空输入时自动读剪贴板
//! ```
//!
//! 匹配顺序：
//! 1. 输入本身就是一个存在的路径 → 直接用
//! 2. **zoxide** 的历史记录（和你 `z` 命令共用同一份数据）
//! 3. Everything 的路径匹配结果
//!
//! 打开的行为：目录 → 直接打开；文件 → 在资源管理器里选中它。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{Action, Item, Kind, Provider};
use crate::config::Config;
use crate::util::{decode_output, hide_window};

/// 「路径以输入结尾」这次额外查询取多少条
const TAIL_LIMIT: usize = 20;

pub struct Go {
    es: Option<PathBuf>,
    max: usize,
    /// 小写的用户主目录（rank 里给家目录下的候选加分）
    home: Option<String>,
}

impl Go {
    pub fn new(cfg: &Config) -> Self {
        let es = cfg
            .general
            .everything_path
            .as_ref()
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .or_else(super::everything::find_es);

        Go {
            es,
            max: cfg.general.max_results,
            home: dirs::home_dir().map(|dir| dir.to_string_lossy().to_lowercase()),
        }
    }
}

impl Provider for Go {
    fn keyword(&self) -> &str {
        "go"
    }

    fn description(&self) -> &'static str {
        crate::i18n::desc_go()
    }

    fn query(&self, args: &str, flags: &[String]) -> Vec<Item> {
        let opener = Opener::from_flags(flags);

        // 空输入时尝试用剪贴板里的内容（常用来“粘贴一个路径然后跳过去”）
        let raw = if args.trim().is_empty() {
            clipboard_text().unwrap_or_default()
        } else {
            args.to_string()
        };

        let input = normalize(&raw);
        if input.is_empty() {
            return Vec::new();
        }

        // 1) 输入本身就是存在路径
        let path = Path::new(&input);
        if path.exists() {
            return vec![item_for(path.to_path_buf(), opener)];
        }

        // 1b) `/Themes` 这类写法：在 Windows 上会被规整成 `\Themes`
        //（当前盘的根目录），基本不会是用户想要的 —— 当成 home 下的路径再试一次
        if let Some(candidate) = home_relative(&input)
            && candidate.exists()
        {
            return vec![item_for(candidate, opener)];
        }

        let mut found: Vec<PathBuf> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();

        // 2) zoxide 历史（最常见的目录优先）
        for candidate in zoxide_query(&input) {
            if seen.insert(candidate.clone()) {
                found.push(candidate);
            }
        }

        // 3) Everything 补充路径片段匹配
        if let Some(es) = &self.es {
            // 3a) 先捣「路径正好以输入结尾」的：Everything 默认按名称排序，
            //     目标目录本身常常沉在一堆子目录后面（`go theme` → `...\Themes`）
            for candidate in everything_tail(es, &input, TAIL_LIMIT) {
                if seen.insert(candidate.clone()) {
                    found.push(candidate);
                }
            }

            // 3b) 普通的路径包含匹配
            for candidate in everything_paths(es, &input, self.max) {
                if seen.insert(candidate.clone()) {
                    found.push(candidate);
                }
            }
        }

        // 4) 重排：和输入「对得上」的排前面
        //    （否则 `go themes` 可能先给出 `...\Themes	hemes\everforest` 这种子目录）
        found.sort_by_key(|path| std::cmp::Reverse(rank(&input, path, self.home.as_deref())));
        found.truncate(self.max);

        found
            .into_iter()
            .map(|path| item_for(path, opener))
            .collect()
    }
}

/// 用哪个程序打开
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opener {
    /// 目录直接打开，文件在资源管理器里定位
    Default,
    /// 在新终端窗口里用 yazi 打开所在目录
    Yazi,
    /// 在新终端窗口里、在目标目录运行 pi
    Pi,
    /// 用 VSCode 打开
    Code,
}

impl Opener {
    fn from_flags(flags: &[String]) -> Opener {
        if flags.iter().any(|flag| flag == "yazi") {
            Opener::Yazi
        } else if flags.iter().any(|flag| flag == "pi") {
            Opener::Pi
        } else if flags.iter().any(|flag| flag == "code") {
            Opener::Code
        } else {
            Opener::Default
        }
    }

    fn action(self, path: &Path) -> Action {
        let parent = || {
            Some(
                path.parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or(path)
                    .to_path_buf(),
            )
        };

        match self {
            Opener::Default => {
                if path.is_dir() {
                    Action::Open(path.to_path_buf())
                } else {
                    Action::Reveal(path.to_path_buf())
                }
            }
            Opener::Code => Action::Launch {
                command: "code".to_string(),
                args: vec![path.display().to_string()],
                elevated: false,
            },
            Opener::Yazi | Opener::Pi => {
                let command = if self == Opener::Pi { "pi" } else { "yazi" };
                Action::LaunchInTerminal {
                    command: command.to_string(),
                    dir: if path.is_dir() {
                        Some(path.to_path_buf())
                    } else {
                        parent()
                    },
                }
            }
        }
    }
}

/// 输入以单个 `\` 开头（不是 UNC）时，把它当成 home 下的相对路径
fn home_relative(input: &str) -> Option<PathBuf> {
    if !input.starts_with('\\') || input.starts_with("\\\\") {
        return None;
    }
    let rest = input.trim_start_matches('\\');
    if rest.is_empty() {
        return None;
    }
    Some(dirs::home_dir()?.join(rest))
}

/// 候选与输入的匹配质量（分数越高越应该排前面）
fn rank(input: &str, path: &Path, home: Option<&str>) -> i64 {
    let full = path.to_string_lossy().to_lowercase();
    let needle = input.to_lowercase();
    let name = full.rsplit('\\').next().unwrap_or("");

    let mut score = if full == needle {
        2000
    } else if full.ends_with(&needle) {
        1000
    } else if !needle.is_empty() && name.starts_with(&needle) {
        // 目录名本身就是输入的那个（`go theme` → `...\Themes`）
        800
    } else if full.contains(&needle) {
        200
    } else {
        0
    };

    // 同样匹配质量下：目录优先、自己家目录下的优先、路径短的优先
    if path.is_dir() {
        score += 30;
    }
    if let Some(home) = home
        && full.starts_with(home)
    {
        score += 150;
    }
    score - full.chars().count() as i64
}

fn item_for(path: PathBuf, opener: Opener) -> Item {
    let is_dir = path.is_dir();
    let title = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let subtitle = path
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();
    let action = opener.action(&path);

    Item::new(
        if is_dir { Kind::Dir } else { Kind::File },
        vec![title, subtitle],
        action,
    )
}

// ---------------------------------------------------------------------------
// 路径规整
// ---------------------------------------------------------------------------

/// 把各种写法统一成 Windows 路径。
fn normalize(input: &str) -> String {
    let mut s = input.trim().to_string();

    // 去掉成对的引号（`"C:\a b"` / `'C:\a b'`，也处理多层嵌套）
    loop {
        let bytes = s.as_bytes();
        let paired = bytes.len() >= 2
            && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
                || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''));
        if paired {
            s = s[1..s.len() - 1].to_string();
        } else {
            break;
        }
    }

    // file:///C:/foo%20bar → C:\foo bar
    if let Some(rest) = s.strip_prefix("file://") {
        let decoded = percent_decode(rest);
        // `file:///C:/...` 解码后是 `/C:/...`，要去掉前面那个斜杠；
        // UNC 形式 `file://server/share` 没有前导斜杠，保持原样。
        s = if decoded.len() > 2
            && decoded.starts_with('/')
            && decoded.as_bytes().get(2) == Some(&b':')
        {
            decoded[1..].to_string()
        } else {
            decoded
        };
    }

    // 正斜杠 → 反斜杠（`//server/share` 这种 UNC 先放过）
    if !s.starts_with("//") {
        s = s.replace('/', "\\");
    }

    // ~ 展开
    if s == "~" {
        if let Some(home) = dirs::home_dir() {
            s = home.display().to_string();
        }
    } else if let Some(rest) = s.strip_prefix("~\\")
        && let Some(home) = dirs::home_dir()
    {
        s = format!("{}\\{rest}", home.display());
    }

    // 去掉末尾多余的路径分隔符（`C:\` 这种根路径保留）
    while s.len() > 3 && s.ends_with('\\') {
        s.pop();
    }

    s
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 3 <= bytes.len()
            && let Ok(byte) = u8::from_str_radix(&input[index + 1..index + 3], 16)
        {
            out.push(byte);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------
// 数据源
// ---------------------------------------------------------------------------

/// 查 zoxide 的历史记录（`zoxide query --list`）。
///
/// 和用户在终端里用的 `z` 命令是**同一份数据库**，所以在终端里跳过的目录，
/// 这里也能直接搜到；反过来这里打开过的目录也会被记进去（见 `exec::run`）。
fn zoxide_query(keyword: &str) -> Vec<PathBuf> {
    let mut cmd = Command::new("zoxide");
    cmd.args(["query", "--list", "--", keyword]);
    hide_window(&mut cmd);

    let Ok(output) = cmd.output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    decode_output(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .collect()
}

/// 构建路径时通常不想要的目录（构建产物、依赖）
const NOISE: &[&str] = &[
    r"\target\",
    r"\node_modules\",
    r"\.git",
    r"\__pycache__\",
    r"\.cargo\registry\",
    r"\winsxs\",
    r"\windows\system32\",
];

fn is_noise(path: &Path) -> bool {
    let lower = path.to_string_lossy().to_lowercase();
    NOISE.iter().any(|needle| lower.contains(needle))
}

/// 用 Everything 按路径匹配（`-p` = match path）。
fn everything_paths(es: &Path, keyword: &str, max: usize) -> Vec<PathBuf> {
    // 多取一些候选：Everything 按文件名排序，`Themes` 这种名字可能排在
    // 一堆 `_xxx` 目录后面，取少了会漏掉"就是目标本身"的那个路径
    let fetch = (max * 4).clamp(max, 200);

    let mut cmd = Command::new(es);
    cmd.arg("-n").arg(fetch.to_string()).arg("-p");
    for token in keyword.split_whitespace() {
        cmd.arg(token);
    }
    hide_window(&mut cmd);

    let Ok(output) = cmd.output() else {
        return Vec::new();
    };

    decode_output(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| !is_noise(path))
        .collect()
}

/// 「路径正好以输入的最后一段结尾」的目录用正则单独捣一遍。
///
/// Everything 默认按名称排序，`C:\Users\me\Themes` 会沉在
/// `...\Themes\themes\everforest`、`...\Themes\.git` 这些子项后面，
/// 取几百条都轮不到它。正则结尾匹配能把「就是目标本身」的先拿出来。
///
/// 注意 Everything 的 `-r` 是**区分大小写**的（`(?i)` 可以打开不敏感）。
fn everything_tail(es: &Path, keyword: &str, limit: usize) -> Vec<PathBuf> {
    let tail = keyword.rsplit(['\\', '/']).next().unwrap_or(keyword).trim();
    if tail.is_empty() {
        return Vec::new();
    }
    let needle = tail.to_lowercase();

    // 按**名称**搜（而不是 `-p` 路径匹配），再在本地只留「名字以输入开头」的。
    // Everything 的 `-p` 结果按名称排序，目标目录本身会沉到几百条之后。
    // 名称搜索的结果按文件名排序，目标目录可能排在几百条之后（实测 `theme`
    // → `...\Themes` 在第 400+ 条），所以多捞一些，本地过滤后再截断
    let fetch = (limit * 25).clamp(limit, 800);

    let mut cmd = Command::new(es);
    cmd.arg("-n").arg(fetch.to_string()).arg(tail);
    hide_window(&mut cmd);

    let Ok(output) = cmd.output() else {
        return Vec::new();
    };

    decode_output(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|path| !is_noise(path))
        .filter(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_lowercase().starts_with(&needle))
                .unwrap_or(false)
        })
        // 不在这里截断：交给上层的 rank 排序来决定谁留下
        .collect()
}

// ---------------------------------------------------------------------------
// 剪贴板
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn clipboard_text() -> Option<String> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    const CF_UNICODETEXT: u32 = 13;

    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            return None;
        }
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None; // 剪贴板被别的程序占着，放弃
        }

        let handle = GetClipboardData(CF_UNICODETEXT);
        if handle.is_null() {
            CloseClipboard();
            return None;
        }

        let ptr = GlobalLock(handle) as *const u16;
        if ptr.is_null() {
            CloseClipboard();
            return None;
        }

        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));

        GlobalUnlock(handle);
        CloseClipboard();
        Some(text)
    }
}

#[cfg(not(windows))]
fn clipboard_text() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_relative_only_for_single_leading_slash() {
        assert_eq!(
            home_relative(r"\Themes"),
            dirs::home_dir().map(|home| home.join("Themes"))
        );
        assert_eq!(home_relative(r"\\server\share"), None); // UNC 不动
        assert_eq!(home_relative(r"C:\Tools"), None);
        assert_eq!(home_relative(""), None);
        assert_eq!(home_relative(r"\"), None);
    }

    #[test]
    fn rank_prefers_exact_and_suffix_matches() {
        let exact = rank(r"\Themes", Path::new(r"C:\Users\me\Themes"), None);
        let child = rank(
            r"\Themes",
            Path::new(r"C:\Users\me\Themes\themes\everforest"),
            None,
        );
        let unrelated = rank(r"\Themes", Path::new(r"C:\Windows"), None);

        assert!(exact > child, "exact={exact} child={child}");
        assert!(child > unrelated, "child={child} unrelated={unrelated}");
    }

    #[test]
    fn rank_likes_directory_name_matches() {
        // `go theme` 时，目录名以 theme 开头的 `...\Themes` 要排在子目录前面
        let named = rank("theme", Path::new(r"C:\Users\me\Themes"), None);
        let nested = rank(
            "theme",
            Path::new(r"C:\Users\me\Themes\themes\everforest"),
            None,
        );
        assert!(named > nested, "named={named} nested={nested}");
    }

    #[test]
    fn openers_map_to_actions() {
        // 默认行为按「是否目录」决定，用一个真实存在的目录
        let real_dir = std::env::temp_dir();
        let dir = Path::new(r"C:\Users\me\Projects");

        assert!(matches!(
            Opener::from_flags(&[]).action(&real_dir),
            Action::Open(_)
        ));
        assert!(matches!(
            Opener::from_flags(&["code".to_string()]).action(dir),
            Action::Launch { .. }
        ));

        match Opener::from_flags(&["yazi".to_string()]).action(&real_dir) {
            Action::LaunchInTerminal {
                command,
                dir: Some(path),
            } => {
                assert_eq!(command, "yazi");
                assert_eq!(path, real_dir);
            }
            other => panic!("unexpected action: {other:?}"),
        }
    }
}
