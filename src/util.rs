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

// ---------------------------------------------------------------------------
// 面板窗口（Windows Terminal）
// ---------------------------------------------------------------------------

/// Windows Terminal 的窗口类名
#[cfg(windows)]
pub const WT_CLASS: &str = "CASCADIA_HOSTING_WINDOW_CLASS";
/// 面板窗口标题（对应 WT profile 的 `tabTitle`）
#[cfg(windows)]
pub const PANEL_TITLE: &str = "term-palette";

#[cfg(windows)]
static FOUND_PANEL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 找面板窗口。
///
/// 不直接用 `FindWindowW(类名, 标题)` 精确匹配：WT 的窗口标题可能是
/// `term-palette`、`term-palette - PowerShell` 之类的形式，
/// 所以枚举出所有 Windows Terminal 窗口，按标题是否**包含** `term-palette` 判定。
#[cfg(windows)]
pub fn find_panel() -> windows_sys::Win32::Foundation::HWND {
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumWindows;

    FOUND_PANEL.store(0, Ordering::SeqCst);
    unsafe { EnumWindows(Some(enum_panel_proc), 0) };
    FOUND_PANEL.load(Ordering::SeqCst) as windows_sys::Win32::Foundation::HWND
}

#[cfg(windows)]
unsafe extern "system" fn enum_panel_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    _lparam: windows_sys::Win32::Foundation::LPARAM,
) -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowTextW};

    let mut class_buf = [0u16; 128];
    let class_len = unsafe { GetClassNameW(hwnd, class_buf.as_mut_ptr(), class_buf.len() as i32) };
    if class_len <= 0 {
        return 1; // 继续枚举
    }
    let class = String::from_utf16_lossy(&class_buf[..class_len as usize]);
    if class != WT_CLASS {
        return 1;
    }

    let mut title_buf = [0u16; 512];
    let title_len = unsafe { GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32) };
    if title_len <= 0 {
        return 1;
    }
    let title = String::from_utf16_lossy(&title_buf[..title_len as usize]);
    if title.to_lowercase().contains(PANEL_TITLE) {
        FOUND_PANEL.store(hwnd as usize, std::sync::atomic::Ordering::SeqCst);
        return 0; // 找到就停
    }
    1
}

/// 面板窗口当前是否可见
#[cfg(windows)]
pub fn panel_visible() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;

    let hwnd = find_panel();
    !hwnd.is_null() && unsafe { IsWindowVisible(hwnd) } != 0
}

/// 隐藏面板窗口（面板进程继续在后台运行）
#[cfg(windows)]
pub fn hide_panel() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};

    let hwnd = find_panel();
    if !hwnd.is_null() {
        unsafe { ShowWindow(hwnd, SW_HIDE) };
    }
}

/// 显示面板窗口（隐藏太久时 WM 会把它标记成 minimized，用 SW_RESTORE 处理）
#[cfg(windows)]
pub fn show_panel() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsIconic, SW_RESTORE, SW_SHOW, ShowWindow,
    };

    let hwnd = find_panel();
    if hwnd.is_null() {
        return;
    }
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else {
            ShowWindow(hwnd, SW_SHOW);
        }
    }
}

// 非 Windows 平台：这些函数不存在，给个空实现让调用方不用到处写 cfg
#[cfg(not(windows))]
pub fn panel_visible() -> bool {
    true
}
#[cfg(not(windows))]
pub fn hide_panel() {}
