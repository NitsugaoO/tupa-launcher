//! 常驻守护进程：全局热键 + 面板窗口管理
//!
//! ## 为什么需要它
//!
//! 1. `Win+S` 是系统保留键，`RegisterHotKey` 抢不到（Shell 已经注册了），
//!    只有**低级键盘钩子**（`WH_KEYBOARD_LL`）能拦下来 —— 而钩子要求
//!    有一个常驻进程 + 消息循环。
//! 2. 面板窗口的"显示 / 隐藏 / 居中"也需要一个窗口管理层。
//!
//! ## 工作方式
//!
//! ```text
//! 钩子回调（必须在 ~300ms 内返回）     主线程消息循环
//!   Win+S 按下                           │
//!   ├─ PostThreadMessageW ──────────────►├─ toggle_panel()
//!   └─ return 1（吞掉按键）              │   ├─ 窗口存在 → 显示/隐藏
//!                                        │   └─ 不存在   → 启动 wt -p term-palette
//! ```
//!
//! 面板窗口靠 **类名 + 标题** 识别：类名是 Windows Terminal 的
//! `CASCADIA_HOSTING_WINDOW_CLASS`，标题由 WT profile 的 `tabTitle` 固定为
//! `term-palette`。面板退出后窗口消失，下次热键就重新拉起 —— 正好是
//! "呼之即来、用完即走"的效果。

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;

use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_LWIN, VK_RWIN, VK_S,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowTextW,
    IsWindowVisible, KBDLLHOOKSTRUCT, MSG, PostThreadMessageW, SW_HIDE, SW_SHOW, SWP_NOACTIVATE,
    SWP_NOZORDER, SetForegroundWindow, SetWindowPos, SetWindowsHookExW, ShowWindow,
    SystemParametersInfoW, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_SYSKEYDOWN,
};

use crate::util;

/// Windows Terminal 的窗口类名
const WT_CLASS: &str = "CASCADIA_HOSTING_WINDOW_CLASS";
/// 面板窗口标题（对应 WT profile 的 tabTitle）
const PANEL_TITLE: &str = "term-palette";
/// 面板使用的 WT profile 名
const PANEL_PROFILE: &str = "term-palette";

/// 钩子回调只投递这个消息，真正的窗口操作放到主线程做
const WM_PALETTE_TOGGLE: u32 = WM_APP + 1;

/// 面板占工作区的比例
const WIDTH_RATIO: f32 = 0.62;
const HEIGHT_RATIO: f32 = 0.55;
/// 垂直位置：偏上一些更像"从顶部呼出"的命令面板
const TOP_RATIO: f32 = 3.0;

/// 安装钩子的线程 ID（钩子回调在这个线程上跑）
static THREAD_ID: AtomicU32 = AtomicU32::new(0);
/// 防抖：按住 Win+S 会连续产生 keydown
static LAST_TRIGGER: Mutex<Option<Instant>> = Mutex::new(None);
/// 诊断开关：设 `TERM_PALETTE_DEBUG_HOOK=1` 时记录钩子看到的事件
static DEBUG_HOOK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn run(debug: bool) -> Result<()> {
    if !is_single_instance() {
        log("已有守护进程在运行，本次退出");
        return Ok(());
    }

    // 脱离控制台：开机自启时不在任务栏留一个最小化的黑窗
    unsafe { windows_sys::Win32::System::Console::FreeConsole() };
    DEBUG_HOOK.store(debug, Ordering::SeqCst);
    log("守护进程启动");

    THREAD_ID.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);

    let hook = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(hook_proc),
            GetModuleHandleW(std::ptr::null()),
            0,
        )
    };
    if hook.is_null() {
        let err = unsafe { GetLastError() };
        log(&format!("安装键盘钩子失败，错误码 {err}"));
        anyhow::bail!("安装键盘钩子失败（错误码 {err}）");
    }
    log(&format!(
        "键盘钩子已安装（线程 {}）",
        THREAD_ID.load(Ordering::SeqCst)
    ));

    // 消息循环：低级钩子的回调依赖它
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        let ret = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetMessageW(
            &mut msg,
            std::ptr::null_mut(),
            0,
            0,
        ) };
        if ret <= 0 {
            break;
        }
        if msg.message == WM_PALETTE_TOGGLE {
            log("收到热键消息");
            toggle_panel();
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// 钩子
// ---------------------------------------------------------------------------

/// 低级键盘钩子。**必须尽快返回**，所以这里只做判断 + 投递消息。
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let wparam = wparam as u32;
        let is_keydown = wparam == WM_KEYDOWN || wparam == WM_SYSKEYDOWN;
        let debug = DEBUG_HOOK.load(Ordering::Relaxed);

        // SAFETY: 对低级键盘钩子，lparam 一定指向 KBDLLHOOKSTRUCT
        let kb = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };

        // 诊断：Win 键的按下/抬起（不记录普通按键，避免日志爆掉）
        if debug && (kb.vkCode == VK_LWIN as u32 || kb.vkCode == VK_RWIN as u32) {
            log(&format!(
                "hook: win key vk=0x{:X} keydown={is_keydown} async_held={}",
                kb.vkCode,
                win_key_held()
            ));
        }

        if is_keydown && kb.vkCode == VK_S as u32 && win_key_held() {
            if debug {
                log("hook: 命中 Win+S，吞掉按键");
            }
            // 不管是不是重复触发，都要吞掉，否则系统会弹出搜索面板
            if !recently_triggered() {
                unsafe {
                    PostThreadMessageW(
                        THREAD_ID.load(Ordering::SeqCst),
                        WM_PALETTE_TOGGLE,
                        0,
                        0,
                    );
                }
            }
            return 1;
        }
    }

    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

fn win_key_held() -> bool {
    unsafe { GetAsyncKeyState(VK_LWIN as i32) < 0 || GetAsyncKeyState(VK_RWIN as i32) < 0 }
}

fn recently_triggered() -> bool {
    let Ok(mut last) = LAST_TRIGGER.lock() else {
        return true;
    };
    let now = Instant::now();
    if let Some(prev) = *last
        && now.duration_since(prev) < Duration::from_millis(300)
    {
        return true;
    }
    *last = Some(now);
    false
}

// ---------------------------------------------------------------------------
// 窗口
// ---------------------------------------------------------------------------

fn toggle_panel() {
    let hwnd = find_panel();
    if hwnd.is_null() {
        log("未找到面板窗口，尝试启动");
        launch_panel();
        return;
    }

    unsafe {
        let visible = IsWindowVisible(hwnd) != 0;
        let is_foreground = GetForegroundWindow() == hwnd;
        log(&format!(
            "找到面板窗口 visible={visible} foreground={is_foreground}"
        ));

        if visible && is_foreground {
            ShowWindow(hwnd, SW_HIDE);
        } else {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
    }
}

/// 找面板窗口。
///
/// 不直接用 `FindWindowW(类名, 标题)` 精确匹配：WT 的窗口标题可能是
/// `term-palette`、`term-palette - PowerShell` 之类的形式，
/// 所以枚举出所有 Windows Terminal 窗口，按标题是否**包含** `term-palette` 判定。
fn find_panel() -> HWND {
    FOUND.store(0, Ordering::SeqCst);
    unsafe { EnumWindows(Some(enum_panel_proc), 0) };
    FOUND.load(Ordering::SeqCst) as HWND
}

/// 枚举结果（单线程调用，用 static 传递即可）
static FOUND: AtomicUsize = AtomicUsize::new(0);

unsafe extern "system" fn enum_panel_proc(hwnd: HWND, _lparam: LPARAM) -> i32 {
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
        FOUND.store(hwnd as usize, Ordering::SeqCst);
        return 0; // 找到就停
    }
    1
}

/// 启动面板窗口。窗口是异步出现的，所以另起一个线程等它出现后把它摆到屏幕中央。
fn launch_panel() {
    match util::detached_command("wt.exe")
        .args(["-p", PANEL_PROFILE])
        .spawn()
    {
        Ok(child) => log(&format!("已启动 wt.exe (pid {})", child.id())),
        Err(e) => {
            log(&format!("启动 wt.exe 失败: {e}"));
            return;
        }
    }

    std::thread::spawn(|| {
        for _ in 0..80 {
            std::thread::sleep(Duration::from_millis(50));
            let hwnd = find_panel();
            if hwnd.is_null() {
                continue;
            }
            // 再等一拍，避开 WT 自己的初始定位/动画
            std::thread::sleep(Duration::from_millis(200));
            let hwnd = find_panel();
            if hwnd.is_null() {
                return;
            }
            center_on_work_area(hwnd);
            unsafe { SetForegroundWindow(hwnd) };
            return;
        }
    });
}

/// 把窗口摆到工作区中央（排除任务栏 / 状态栏占用的区域）
fn center_on_work_area(hwnd: HWND) {
    let mut work = RECT::default();
    let ok = unsafe {
        SystemParametersInfoW(
            0x0030, // SPI_GETWORKAREA
            0,
            &mut work as *mut RECT as *mut core::ffi::c_void,
            0,
        )
    };
    if ok == 0 {
        return;
    }

    let area_w = work.right - work.left;
    let area_h = work.bottom - work.top;
    if area_w <= 0 || area_h <= 0 {
        return;
    }

    let w = (area_w as f32 * WIDTH_RATIO) as i32;
    let h = (area_h as f32 * HEIGHT_RATIO) as i32;
    let x = work.left + (area_w - w) / 2;
    let y = work.top + (area_h - h) / TOP_RATIO as i32;

    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            w,
            h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

// ---------------------------------------------------------------------------
// 杂项
// ---------------------------------------------------------------------------

/// 单实例：对一个锁文件加独占锁，拿到锁的才是唯一的守护进程。
/// （文件句柄故意泄漏，锁会一直持有到进程退出。）
fn is_single_instance() -> bool {
    let path = std::env::temp_dir().join("term-palette-daemon.lock");
    let Ok(file) = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
    else {
        return true; // 打不开锁文件就不拦
    };
    if file.try_lock().is_ok() {
        std::mem::forget(file);
        true
    } else {
        false
    }
}

/// 守护进程没有控制台，所以把排查信息写到临时文件
fn log(msg: &str) {
    use std::io::Write;
    let path = std::env::temp_dir().join("term-palette-daemon.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{msg}");
    }
}
