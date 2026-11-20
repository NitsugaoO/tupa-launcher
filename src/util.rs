//! 跨模块的小工具

use std::path::{Path, PathBuf};
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
        .or_else(|| {
            candidates
                .iter()
                .find(|p| matches!(ext(p).as_str(), "cmd" | "bat"))
        })
        .or_else(|| candidates.first())
        .cloned()
}

// ---------------------------------------------------------------------------
// 快捷方式解析 / 可执行文件定位
// ---------------------------------------------------------------------------

/// 解析 `.lnk` 快捷方式的目标路径（递归，最多 5 层）。
///
/// 不是 `.lnk` 的话原样返回。用 COM 的 `IShellLinkW` —— 这是微软官方的解析方式，
/// 比自己拆 `.lnk` 二进制格式可靠。
#[cfg(windows)]
pub fn resolve_shortcut(path: &Path) -> PathBuf {
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        IPersistFile, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{Interface, PCWSTR};

    let mut current = path.to_path_buf();

    for _ in 0..5 {
        let is_lnk = current
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"));
        if !is_lnk {
            break;
        }

        let wide: Vec<u16> = current
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        // SAFETY: 只做 COM 调用；任何一步失败就保留当前路径
        let next = unsafe {
            // CoInitializeEx 是幂等的（已初始化会返回 S_FALSE），这里忽略返回值
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let Ok(link) =
                CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER)
            else {
                break;
            };
            let Ok(file) = link.cast::<IPersistFile>() else {
                break;
            };
            if file.Load(PCWSTR(wide.as_ptr()), STGM_READ).is_err() {
                break;
            }

            let mut buffer = [0u16; 1024];
            if link.GetPath(&mut buffer, std::ptr::null_mut(), 0).is_err() {
                break;
            }

            let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
            if len == 0 {
                break;
            }
            PathBuf::from(String::from_utf16_lossy(&buffer[..len]))
        };

        if next.as_os_str().is_empty() {
            break;
        }
        current = next;
    }

    current
}

#[cfg(not(windows))]
pub fn resolve_shortcut(path: &Path) -> PathBuf {
    path.to_path_buf()
}

/// 把 `command`（裸命令 / 绝对路径 / `.lnk`）解析成真实的文件路径。
/// 用于「在资源管理器里定位」—— app 条目也要能定位。
pub fn resolve_executable(command: &str) -> Option<PathBuf> {
    let path = Path::new(command);
    let target = if path.is_file() {
        path.to_path_buf()
    } else {
        which(command)?
    };

    Some(resolve_shortcut(&target))
}

// ---------------------------------------------------------------------------
// 面板窗口（终端宿主 / 自绘窗口）
// ---------------------------------------------------------------------------

/// 面板窗口的外壳类型
///
/// - `Tui`：终端模拟器里的终端界面（窗口属于终端进程）
/// - `Gui`：`tupa-launcher --gui` 的自绘窗口（窗口属于 `tupa-launcher.exe`）
///
/// 两者的窗口归属完全不同，所以找窗口的方式也必须分开。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    Tui,
    Gui,
}

impl PanelKind {
    /// 解析配置里的 `[daemon] panel`（无法识别时退回 TUI）
    pub fn parse(value: &str) -> PanelKind {
        match value.trim().to_ascii_lowercase().as_str() {
            "gui" | "window" => PanelKind::Gui,
            _ => PanelKind::Tui,
        }
    }
}

/// TUI 面板跑在哪个终端模拟器里。
///
/// 两个宿主的差别只有「怎么启动」和「怎么找回窗口」：
///
/// - **WezTerm**：`wezterm-gui start --class tupa-launcher -- <自身>`。窗口类名是我们
///   自己指定的，天然唯一；但它**会跟着前台进程改标题**（实测变成 `TUPA-LAUNCHER.EXE`），
///   所以不能靠标题识别。
/// - **Windows Terminal**：`wt.exe -p tupa-launcher`。类名固定是 `CASCADIA_HOSTING_WINDOW_CLASS`，
///   只能靠 `suppressApplicationTitle` + 标题前缀区分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalKind {
    Wezterm,
    Wt,
}

impl TerminalKind {
    /// 解析配置里的 `[daemon] terminal`。
    ///
    /// `auto`（以及任何看不懂的值）= 装了 WezTerm 就用 WezTerm，否则退回 Windows Terminal。
    /// 显式写 `wezterm` 但本机没装时也会在启动阶段退回 WT（见 `daemon::launch_wezterm`）。
    pub fn resolve(value: &str) -> TerminalKind {
        match value.trim().to_ascii_lowercase().as_str() {
            "wezterm" => TerminalKind::Wezterm,
            "wt" | "windows-terminal" | "windowsterminal" => TerminalKind::Wt,
            // auto / 未知值 / 空 → 按本机装了什么决定
            _ if wezterm_gui().is_some() => TerminalKind::Wezterm,
            _ => TerminalKind::Wt,
        }
    }
}

/// 找 `wezterm-gui.exe`。
///
/// 优先 PATH；用户实际是**便携装**（不在 PATH 里），所以再按 WezTerm 的两种默认目录补探。
/// 返回 `None` 表示本机没装，调用方退回 Windows Terminal。
pub fn wezterm_gui() -> Option<PathBuf> {
    if let Some(path) = which("wezterm-gui") {
        return Some(path);
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let base = PathBuf::from(local).join("Programs").join("WezTerm");
        candidates.push(base.join("wezterm-nightly").join("wezterm-gui.exe"));
        candidates.push(base.join("wezterm-gui.exe"));
    }
    if let Ok(program_files) = std::env::var("ProgramFiles") {
        candidates.push(
            PathBuf::from(program_files)
                .join("WezTerm")
                .join("wezterm-gui.exe"),
        );
    }
    candidates.into_iter().find(|path| path.is_file())
}

/// 面板窗口标题
#[cfg(windows)]
pub const PANEL_TITLE: &str = "tupa-launcher";
/// 自绘面板的进程名
#[cfg(windows)]
pub const PANEL_EXE: &str = "tupa-gui.exe";
/// Windows Terminal 的窗口类名（TUI 面板藏在它里面）
#[cfg(windows)]
pub const WT_CLASS: &str = "CASCADIA_HOSTING_WINDOW_CLASS";
/// WezTerm 面板的窗口类名：启动时用 `--class` 指定，所以是**我们说了算**的唯一值。
#[cfg(windows)]
pub const WEZTERM_CLASS: &str = "tupa-launcher";

#[cfg(windows)]
static FOUND_PANEL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// 枚举窗口时用的是哪种面板（0 = Tui，1 = Gui）
#[cfg(windows)]
static PANEL_KIND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 面板窗口标题是否属于面板。
///
/// 只认 `tupa-launcher` 本身，或 `tupa-launcher - …` / `tupa-launcher: …` 这种后缀；
/// **不能用 `contains`**：pi 在项目目录里工作时窗口标题是 `π - tupa-launcher`，
/// 包含 `tupa-launcher` 但不是面板，按 contains 会把 pi 窗口当成面板。
#[cfg(windows)]
fn is_panel_title(title: &str) -> bool {
    let title = title.to_lowercase();
    let panel = PANEL_TITLE.to_lowercase();
    let Some(rest) = title.strip_prefix(panel.as_str()) else {
        return false;
    };
    rest.is_empty() || rest.starts_with(" - ") || rest.starts_with(": ")
}

/// 找指定类型的面板窗口。
///
/// - TUI + WezTerm：窗口类是 `tupa-launcher`（`--class` 指定），**类名即唯一标识**，
///   不再看标题（WezTerm 的标题会跟着前台进程跑，认不住）
/// - TUI + Windows Terminal：窗口类是 `CASCADIA_HOSTING_WINDOW_CLASS`，还要标题前缀匹配
/// - GUI（GPUI）：窗口类名是全框架共用的 `Zed::Window`（不可控），只能靠
///   **进程名 `tupa-launcher.exe` + 标题**认人
///
/// 后两种必须按标题锁定，否则 pi 的 `π - tupa-launcher` 会被误判。
#[cfg(windows)]
pub fn find_panel(kind: PanelKind) -> windows_sys::Win32::Foundation::HWND {
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumWindows;

    FOUND_PANEL.store(0, Ordering::SeqCst);
    PANEL_KIND.store(
        usize::from(matches!(kind, PanelKind::Gui)),
        Ordering::SeqCst,
    );
    unsafe { EnumWindows(Some(enum_panel_proc), 0) };
    FOUND_PANEL.load(Ordering::SeqCst) as windows_sys::Win32::Foundation::HWND
}

#[cfg(windows)]
unsafe extern "system" fn enum_panel_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    _lparam: windows_sys::Win32::Foundation::LPARAM,
) -> i32 {
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW;

    let class = window_class(hwnd);
    let want_gui = PANEL_KIND.load(Ordering::SeqCst) == 1;

    // GUI（GPUI）两条路都走不通：窗口类名是全框架共用的 `Zed::Window`，
    // 标题从别的进程读也不可靠（实测 `GetWindowTextW` 只回一个字符）。
    // 但面板进程只有一个可见顶层窗口 —— 按「进程名 + 可见 + 有尺寸」认人。
    if want_gui {
        return if is_gui_panel(hwnd) {
            FOUND_PANEL.store(hwnd as usize, Ordering::SeqCst);
            0
        } else {
            1
        };
    }

    let is_wezterm = class == WEZTERM_CLASS;
    if !is_wezterm && class != WT_CLASS {
        return 1;
    }

    // WezTerm 的类名是我们 `--class` 指定的，全机唯一 —— 到此就可以收工。
    // 它的窗口标题会随前台进程变（实测跑 ping 时标题是 `PING.EXE`），必须跳过标题检查。
    if is_wezterm {
        FOUND_PANEL.store(hwnd as usize, Ordering::SeqCst);
        return 0;
    }

    let mut title_buf = [0u16; 512];
    let title_len = unsafe { GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32) };
    if title_len <= 0 {
        return 1;
    }
    let title = String::from_utf16_lossy(&title_buf[..title_len as usize]);
    if !is_panel_title(&title) {
        return 1;
    }

    FOUND_PANEL.store(hwnd as usize, Ordering::SeqCst);
    0 // 找到就停
}

#[cfg(windows)]
fn window_class(hwnd: windows_sys::Win32::Foundation::HWND) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;

    let mut buffer = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..len as usize])
}

/// 这个窗口是我们的 GUI 面板吗（进程名 + 有尺寸）？
///
/// **不能要求 `IsWindowVisible`**：`Esc` 隐藏面板后还要能靠热键找到它。
/// GPUI 内部会建 0 尺寸的辅助窗口，用尺寸把它们滤掉。
#[cfg(windows)]
fn is_gui_panel(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

    let mut rect = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return false;
    }
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return false;
    }
    window_process_is_panel(hwnd)
}

/// 这个窗口属于 `tupa-launcher.exe` 吗？
#[cfg(windows)]
fn window_process_is_panel(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return false;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return false;
    }
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut len) };
    unsafe { CloseHandle(process) };
    if ok == 0 || len == 0 {
        return false;
    }

    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    path.rsplit(['\\', '/'])
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case(PANEL_EXE))
}

/// 面板窗口当前是否可见
#[cfg(windows)]
pub fn panel_visible(kind: PanelKind) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible;

    let hwnd = find_panel(kind);
    !hwnd.is_null() && unsafe { IsWindowVisible(hwnd) } != 0
}

/// 隐藏面板窗口（面板进程继续在后台运行）
#[cfg(windows)]
pub fn hide_panel(kind: PanelKind) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};

    let hwnd = find_panel(kind);
    if !hwnd.is_null() {
        unsafe { ShowWindow(hwnd, SW_HIDE) };
    }
}

/// 窗口停在别的**虚拟桌面**上时把它搬回当前桌面。
///
/// `ShowWindow` 不会带走跨虚拟桌面的窗口：`IsWindowVisible` 仍是 true，但 DWM 把窗口
/// cloaked 掉（`DwmGetWindowAttribute(DWMWA_CLOAKED)` = `DWM_CLOAKED_SHELL`），
/// 表现就是「按热键没反应，面板调不出来」（2026-11-20 用户遇到）。
///
/// 公开的 `IVirtualDesktopManager` 没有「拿当前桌面 id」的方法，所以拿**前台窗口**
/// 所在的桌面当目标（前台窗口必然在当前桌面；拿不到就算了，失败静默）。
#[cfg(windows)]
fn ensure_current_desktop(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};

    unsafe {
        // 幂等：托盘线程有消息泵，STA 没问题
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let Ok(manager) = CoCreateInstance::<_, IVirtualDesktopManager>(
            &VirtualDesktopManager,
            None,
            CLSCTX_ALL,
        ) else {
            return;
        };
        let panel = HWND(hwnd);
        let Ok(on_current) = manager.IsWindowOnCurrentVirtualDesktop(panel) else {
            return;
        };
        if on_current.as_bool() {
            return;
        }
        let foreground = HWND(windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow());
        if foreground == panel || foreground.is_invalid() {
            return;
        }
        if let Ok(desktop) = manager.GetWindowDesktopId(foreground) {
            let _ = manager.MoveWindowToDesktop(panel, &desktop);
        }
    }
}

/// 显示面板窗口（隐藏太久时 WM 会把它标记成 minimized，用 SW_RESTORE 处理）
#[cfg(windows)]
pub fn show_panel(kind: PanelKind) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{IsIconic, SW_RESTORE, SW_SHOW, ShowWindow};

    let hwnd = find_panel(kind);
    if hwnd.is_null() {
        return;
    }
    // 先搬回当前虚拟桌面，否则 ShowWindow 在用户眼前等于什么都没发生
    ensure_current_desktop(hwnd);
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
pub fn panel_visible(_kind: PanelKind) -> bool {
    true
}
#[cfg(not(windows))]
pub fn hide_panel(_kind: PanelKind) {}
#[cfg(not(windows))]
pub fn show_panel(_kind: PanelKind) {}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn panel_title_does_not_match_pi_window() {
        assert!(is_panel_title("tupa-launcher"));
        assert!(is_panel_title("TUPA-LAUNCHER"));
        assert!(is_panel_title("tupa-launcher - PowerShell"));
        assert!(is_panel_title("tupa-launcher: pi"));

        // pi 在项目目录里工作时是 `π - tupa-launcher`，不能误判
        assert!(!is_panel_title("π - tupa-launcher"));
        assert!(!is_panel_title("tupa-launcher-dev"));
        assert!(!is_panel_title("foo tupa-launcher"));
    }
}
