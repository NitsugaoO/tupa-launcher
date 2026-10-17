//! `go <路径片段>` —— 跳到目录 / 定位文件。
//!
//! 输入可以是这些写法（都能识别）：
//!
//! ```text
//! go ~/Projects/term-palette
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

pub struct Go {
    es: Option<PathBuf>,
    max: usize,
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
        }
    }
}

impl Provider for Go {
    fn keyword(&self) -> &str {
        "go"
    }

    fn description(&self) -> &str {
        "跳到目录 / 定位文件（与 zoxide 共用数据）"
    }

    fn query(&self, args: &str, _flags: &[String]) -> Vec<Item> {
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
            return vec![item_for(path.to_path_buf())];
        }

        let mut items: Vec<Item> = Vec::new();
        let mut seen: HashSet<PathBuf> = HashSet::new();

        // 2) zoxide 历史（最常见的目录优先）
        for candidate in zoxide_query(&input) {
            if seen.insert(candidate.clone()) {
                items.push(item_for(candidate));
            }
        }

        // 3) Everything 补充路径片段匹配
        if let Some(es) = &self.es {
            for candidate in everything_paths(es, &input, self.max) {
                if seen.insert(candidate.clone()) {
                    items.push(item_for(candidate));
                }
            }
        }

        items.truncate(self.max);
        items
    }
}

fn item_for(path: PathBuf) -> Item {
    let is_dir = path.is_dir();
    let title = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let subtitle = path
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();

    Item {
        kind: if is_dir { Kind::Dir } else { Kind::File },
        title,
        subtitle,
        // 目录直接打开；文件在资源管理器里定位（用户要的“打开所在位置”）
        action: if is_dir {
            Action::Open(path)
        } else {
            Action::Reveal(path)
        },
    }
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
    r"\.git\",
    r"\__pycache__\",
    r"\.cargo\registry\",
];

/// 用 Everything 按路径匹配（`-p` = match path）。
fn everything_paths(es: &Path, keyword: &str, max: usize) -> Vec<PathBuf> {
    let mut cmd = Command::new(es);
    cmd.arg("-n").arg(max.to_string()).arg("-p");
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
        .filter(|path| {
            let lower = path.to_string_lossy().to_lowercase();
            !NOISE.iter().any(|needle| lower.contains(needle))
        })
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
