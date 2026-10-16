//! 常驻守护进程：全局热键 + 面板窗口管理
//!
//! ## 实现方式
//!
//! 用 Win32 的 `RegisterHotKey` 注册一个全局热键（默认 `Ctrl+Win+S`），
//! 在消息循环里等 `WM_HOTKEY`，然后显示 / 隐藏 / 拉起面板窗口。
//!
//! ## 为什么不去抢裸 `Win+S`
//!
//! 试过，是个死结：
//!
//! 1. `Win+S` 是系统保留键，`RegisterHotKey` 注册不了；
//! 2. 换成低级键盘钩子（`WH_KEYBOARD_LL`）能吞掉按键，但 shell 判断
//!    Win+S 看的是**异步按键状态**（`GetAsyncKeyState`），吞按键拦不住；
//! 3. 注入一个假的 Win 抬起把异步状态抹平，shell 又认为「只按了一下 Win 键」，
//!    于是弹出开始菜单；
//! 4. 要阻止开始菜单就得吞掉**真实的** Win 抬起 —— 而那样在系统看来
//!    Win 键一直按着：打字全大写（Shift 同理）、点任务栏变成 `Win+点击`。
//!
//! 而 `Ctrl+Win+S` 这类「额外修饰键 + Win」的组合**不在系统保留范围内**，
//! `RegisterHotKey` 直接就能注册 —— 于是完全不需要钩子，
//! 也就没有钩子超时、修饰键卡死这些坑。

use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN, RegisterHotKey, SendInput, UnregisterHotKey, VK_ESCAPE, VK_F1, VK_F23,
    VK_OEM_3, VK_RETURN, VK_SPACE, VK_TAB,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, EnumWindows, GetClassNameW, GetForegroundWindow, GetMessageW, GetWindowTextW,
    IsWindowVisible, KBDLLHOOKSTRUCT, MSG, PostThreadMessageW, SW_HIDE, SW_SHOW, SW_SHOWNORMAL,
    SetForegroundWindow, SetWindowsHookExW, ShowWindow, WH_KEYBOARD_LL, WM_APP, WM_HOTKEY,
    WM_KEYDOWN, WM_SYSKEYDOWN,
};

/// Windows Terminal 的窗口类名
const WT_CLASS: &str = "CASCADIA_HOSTING_WINDOW_CLASS";
/// 面板窗口标题（对应 WT profile 的 tabTitle）
const PANEL_TITLE: &str = "term-palette";
/// 面板使用的 WT profile 名
const PANEL_PROFILE: &str = "term-palette";
/// 热键 ID（同一个进程里唯一即可）
const HOTKEY_ID: i32 = 1;

/// Copilot 键发的是 `Win+Shift+F23`，这是微软给 OEM 定的标准信号
const VK_COPILOT: u16 = VK_F23;

/// 注入事件的自标识，用于区分“我们自己注入的”和“别的程序注入的”
const SELF_INJECT_MARK: usize = 0x7465_726d; // "term"

/// 自定义消息：钩子回调只投递消息，真正的窗口操作放到别的线程
const WM_PALETTE_TOGGLE: u32 = WM_APP + 1;

/// toggle 串行化，避免连按时并发操作窗口
static TOGGLE_LOCK: Mutex<()> = Mutex::new(());

/// 日志通过 channel 交给专用线程写盘，热键处理线程不做 IO
static LOG_TX: OnceLock<Sender<String>> = OnceLock::new();

/// 窗口枚举结果（单线程调用，用 static 传递）
static FOUND: AtomicUsize = AtomicUsize::new(0);

/// 安装 Copilot 钩子的线程 ID（钩子回调在这个线程上跑）
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);
/// Copilot 键防抖（按住会产生连续 keydown）
static LAST_COPILOT: Mutex<Option<Instant>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// 热键
// ---------------------------------------------------------------------------

/// 解析好的热键
#[derive(Debug, Clone)]
pub struct Hotkey {
    pub modifiers: u32,
    pub vk: u32,
    /// 原始写法，用于日志/报错
    pub spec: String,
}

/// 解析 `"ctrl+win+s"` 这样的热键写法
pub fn parse_hotkey(spec: &str) -> Result<Hotkey> {
    let mut modifiers = 0u32;
    let mut vk: Option<u32> = None;

    for part in spec.split('+') {
        let part = part.trim().to_ascii_lowercase();
        match part.as_str() {
            "" => continue,
            "ctrl" | "control" => modifiers |= MOD_CONTROL,
            "alt" => modifiers |= MOD_ALT,
            "shift" => modifiers |= MOD_SHIFT,
            "win" | "super" | "meta" => modifiers |= MOD_WIN,
            other => {
                if vk.is_some() {
                    anyhow::bail!("热键 {spec} 里只能有一个主键");
                }
                vk = Some(parse_key(other)?);
            }
        }
    }

    let vk = vk.with_context(|| format!("热键 {spec} 缺少主键（例如 ctrl+win+s 里的 s）"))?;
    Ok(Hotkey {
        modifiers,
        vk,
        spec: spec.to_string(),
    })
}

fn parse_key(name: &str) -> Result<u32> {
    // 单个字母或数字
    if name.len() == 1 {
        let c = name.chars().next().unwrap().to_ascii_uppercase();
        if c.is_ascii_alphanumeric() {
            return Ok(c as u32);
        }
    }

    // F1..F24
    if let Some(number) = name.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&number) {
            return Ok(VK_F1 as u32 + (number - 1));
        }
    }

    match name {
        "space" => Ok(VK_SPACE as u32),
        "tab" => Ok(VK_TAB as u32),
        "enter" | "return" => Ok(VK_RETURN as u32),
        "esc" | "escape" => Ok(VK_ESCAPE as u32),
        "`" | "backtick" | "grave" => Ok(VK_OEM_3 as u32),
        _ => anyhow::bail!("不认识的按键：{name}"),
    }
}

// ---------------------------------------------------------------------------
// 主流程
// ---------------------------------------------------------------------------

pub fn run(hotkey: &Hotkey, copilot_key: bool, debug: bool) -> Result<()> {
    if !is_single_instance() {
        log("已有守护进程在运行，本次退出");
        return Ok(());
    }

    // 脱离控制台：开机自启时不在任务栏留一个最小化的黑窗
    unsafe { windows_sys::Win32::System::Console::FreeConsole() };
    spawn_logger();
    log("守护进程启动");

    // MOD_NOREPEAT：按住热键不会连续触发
    let modifiers = hotkey.modifiers | MOD_NOREPEAT;
    let registered = unsafe { RegisterHotKey(std::ptr::null_mut(), HOTKEY_ID, modifiers, hotkey.vk) };
    if registered == 0 {
        let error = unsafe { GetLastError() };
        log(&format!("注册热键 {} 失败，错误码 {error}", hotkey.spec));
        anyhow::bail!(
            "注册热键 {} 失败（错误码 {error}）—— 可能被别的程序占用了",
            hotkey.spec
        );
    }
    log(&format!("已注册热键 {}", hotkey.spec));

    // Copilot 键：它的原样组合（Win+Shift+F23）被 shell 占着，RegisterHotKey
    // 注册不了，所以用低级钩子把 F23 这一个键拦下来。
    if copilot_key {
        HOOK_THREAD_ID.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
        let hook = unsafe {
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(copilot_hook),
                GetModuleHandleW(std::ptr::null()),
                0,
            )
        };
        if hook.is_null() {
            let error = unsafe { GetLastError() };
            log(&format!("安装 Copilot 键钩子失败（错误码 {error}，不影响主热键）"));
        } else {
            log("已接管 Copilot 键");
        }
    }

    // 消息循环：RegisterHotKey 的 WM_HOTKEY 和钩子投递的消息都到这里
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        let ret = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
        if ret <= 0 {
            break;
        }
        if msg.message == WM_HOTKEY || msg.message == WM_PALETTE_TOGGLE {
            if debug {
                log("收到唤起消息");
            }
            // 窗口操作丢给独立线程：消息循环要一直保持轻快
            std::thread::spawn(move || {
                let _guard = TOGGLE_LOCK.lock();
                toggle_panel(debug);
            });
        }
    }

    unsafe { UnregisterHotKey(std::ptr::null_mut(), HOTKEY_ID) };
    Ok(())
}

// ---------------------------------------------------------------------------
// Copilot 键的钩子
// ---------------------------------------------------------------------------

/// 只用于接管 Copilot 键的低级键盘钩子。
///
/// 刻意**只吞 `F23` 这一个键，绝不碰任何修饰键**。
/// 之前那版吞掉了修饰键的抬起事件，导致系统认为 Shift/Win 一直按着 ——
/// 打字全大写、点任务栏变成 `Win+点击`、Win 键失去响应。
/// `F23` 不是修饰键，吞它并补一个它的抬起是无害的。
unsafe extern "system" fn copilot_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: 对低级键盘钩子，lparam 一定指向 KBDLLHOOKSTRUCT
        let kb = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let wparam_value = wparam as u32;
        let is_keydown = wparam_value == WM_KEYDOWN || wparam_value == WM_SYSKEYDOWN;

        // 自己注入的事件不处理（否则会把注入的“抬起”当成真实按键）
        if kb.dwExtraInfo != SELF_INJECT_MARK && is_keydown && kb.vkCode as u16 == VK_COPILOT {
            // 抹平异步按键状态：shell 也可能靠它判定 Copilot 键
            inject_key_up(VK_COPILOT);
            if !copilot_debounced() {
                unsafe {
                    PostThreadMessageW(
                        HOOK_THREAD_ID.load(Ordering::SeqCst),
                        WM_PALETTE_TOGGLE,
                        0,
                        0,
                    );
                }
            }
            return 1; // 吞掉：Copilot 面板不会弹出来
        }
    }

    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

fn copilot_debounced() -> bool {
    let Ok(mut last) = LAST_COPILOT.lock() else {
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

/// 注入一个按键抬起事件（带自标识，避免被自己的钩子重复处理）
fn inject_key_up(vk: u16) {
    let mut input: INPUT = unsafe { std::mem::zeroed() };
    input.r#type = INPUT_KEYBOARD;
    input.Anonymous.ki = KEYBDINPUT {
        wVk: vk,
        wScan: 0,
        dwFlags: KEYEVENTF_KEYUP,
        time: 0,
        dwExtraInfo: SELF_INJECT_MARK,
    };
    unsafe {
        SendInput(1, &mut input, std::mem::size_of::<INPUT>() as i32);
    }
}

// ---------------------------------------------------------------------------
// 窗口
// ---------------------------------------------------------------------------

fn toggle_panel(debug: bool) {
    let hwnd = find_panel();
    if hwnd.is_null() {
        if debug {
            log("未找到面板窗口，尝试启动");
        }
        launch_panel(debug);
        return;
    }

    unsafe {
        let visible = IsWindowVisible(hwnd) != 0;
        let is_foreground = GetForegroundWindow() == hwnd;
        if debug {
            log(&format!(
                "找到面板窗口 visible={visible} foreground={is_foreground}"
            ));
        }

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

/// 启动面板窗口。
///
/// 用 `ShellExecuteW` 而不是 `CreateProcess`：守护进程调用过 `FreeConsole()`，
/// 自身已经没有控制台了，而 `wt.exe` 是控制台程序 —— 直接 spawn 会「启动成功」
/// 但根本不创建窗口（实测 spawn 返回 Ok，窗口却一个都没出现）。
/// ShellExecute 走 shell 的启动路径，不依赖调用者的控制台。
fn launch_panel(debug: bool) {
    let operation = wide("open");
    let file = wide("wt.exe");
    let parameters = wide(&format!("-p {PANEL_PROFILE}"));

    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;

    // ShellExecute 的返回码 <= 32 表示失败
    if result <= 32 {
        log(&format!("启动面板失败（ShellExecute 返回 {result}）"));
    } else if debug {
        log("已启动面板窗口");
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

/// 守护进程没有控制台，日志交给专用线程写文件
fn spawn_logger() {
    use std::io::Write;

    let (tx, rx) = mpsc::channel::<String>();
    if LOG_TX.set(tx).is_err() {
        return;
    }

    std::thread::spawn(move || {
        let path = std::env::temp_dir().join("term-palette-daemon.log");
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();
        while let Ok(message) = rx.recv() {
            if let Some(file) = file.as_mut() {
                let _ = writeln!(file, "{message}");
                let _ = file.flush();
            }
        }
    });
}

fn log(msg: &str) {
    if let Some(tx) = LOG_TX.get() {
        let _ = tx.send(msg.to_string());
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
