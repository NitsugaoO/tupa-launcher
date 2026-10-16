//! 跨模块的小工具

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// 构造一个"完全独立"的子进程命令：
///
/// - 不继承标准句柄（守护进程 `FreeConsole()` 之后句柄是无效的，
///   继承过去会让 `CreateProcess` 直接报 "句柄无效"）
/// - Windows 上不弹黑窗
pub fn detached_command<S: AsRef<std::ffi::OsStr>>(program: S) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_window(&mut cmd);
    cmd
}

/// Windows 上以不弹黑窗的方式启动子进程
pub fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

/// 子进程把输出重定向到管道时，输出的是**系统 ANSI 代码页**
/// （中文系统 = GBK/936），而 TUI 需要 UTF-8。
///
/// 先按 UTF-8 严格解码，失败再回退 GBK。
pub fn decode_output(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => encoding_rs::GBK.decode(bytes).0.into_owned(),
    }
}

/// 在 PATH 里查找命令。
///
/// 用 `where.exe` 而不是自己拼 PATH：它会按 `PATHEXT` 展开，
/// 所以 `code` 能找到 `code.cmd`，`wt` 能找到 WindowsApps 里的 `wt.exe`。
///
/// 注意 `where code` 会同时返回 `...\bin\code`（无扩展名的 shell 脚本）
/// 和 `...\bin\code.cmd`，必须按可执行性挑，不能直接取第一行。
pub fn which(command: &str) -> Option<PathBuf> {
    if command.is_empty() {
        return None;
    }
    let mut cmd = Command::new("where.exe");
    cmd.arg(command);
    hide_window(&mut cmd);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }

    let candidates: Vec<PathBuf> = decode_output(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .collect();

    let ext = |p: &PathBuf| {
        p.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
    };

    candidates
        .iter()
        .find(|p| matches!(ext(p).as_str(), "exe" | "com"))
        .or_else(|| candidates.iter().find(|p| matches!(ext(p).as_str(), "cmd" | "bat")))
        .or_else(|| candidates.first())
        .cloned()
}
