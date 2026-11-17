//! 系统托盘 + 全局热键（GUI 面板自带，不依赖守护进程）。
//!
//! GPUI 的 `Application::run` 会占住主线程，所以这里开一个后台线程：
//! 建一个 message-only 窗口，`Shell_NotifyIconW` 挂托盘图标、`RegisterHotKey`
//! 注册热键，然后跑自己的 `GetMessageW` 循环。面板的显隐直接用 `ShowWindow`
//! 操作 HWND（[`crate::util::find_panel`]），不碰 GPUI 内部状态。
//!
//! 主线程退出（关窗口 / 托盘「退出」）时整个进程结束，托盘图标先删掉。

use std::thread;

use anyhow::Result;

use crate::daemon::Hotkey;
use crate::i18n;
use crate::util::{self, PanelKind};

/// 托盘回调消息（`lparam` 低字是鼠标事件）
const WM_TRAY: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 1;
const TRAY_ID: u32 = 1;
const HOTKEY_ID: i32 = 1;
const MENU_SHOW: usize = 1;
const MENU_QUIT: usize = 2;

/// 启动托盘（后台线程，立即返回）。热键注册失败只写日志，不影响面板本身。
///
/// `icon` 由 [`crate::gui::icon::draw`] 画好（背景色方块 + 草绿 ❯）。
pub fn start(hotkey: Hotkey, icon: isize) {
    let spawned = thread::Builder::new()
        .name("tupa-tray".into())
        .spawn(move || {
            if let Err(error) = run(hotkey, icon) {
                log(&format!("托盘线程退出：{error:#}"));
            }
        });
    if let Err(error) = spawned {
        log(&format!("托盘线程起不来：{error}"));
    }
}

/// 进程退出前删掉托盘图标（`std::process::exit` 不会走 Drop）
pub fn remove_icon() {
    use windows_sys::Win32::UI::Shell::{NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW};

    let hwnd = tray_window();
    if hwnd.is_null() {
        return;
    }
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_ID;
    unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
}

fn run(hotkey: Hotkey, icon: isize) -> Result<()> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_NOREPEAT, RegisterHotKey};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG};

    let hwnd = create_window()?;

    let registered = unsafe {
        RegisterHotKey(hwnd, HOTKEY_ID, hotkey.modifiers | MOD_NOREPEAT, hotkey.vk)
    };
    if registered == 0 {
        log(&format!("注册热键 {} 失败（可能被占用）", hotkey.spec));
    } else {
        log(&format!("已注册热键 {}", hotkey.spec));
    }

    add_icon(hwnd, icon);

    let mut msg: MSG = unsafe { std::mem::zeroed() };
    loop {
        let ret = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
        if ret <= 0 {
            break;
        }
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::TranslateMessage(&msg);
            windows_sys::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
        }
    }

    remove_icon();
    Ok(())
}

/// 建一个 message-only 窗口（收发托盘回调 / 热键 / 菜单命令，不上屏）
fn create_window() -> Result<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, HWND_MESSAGE, RegisterClassW, WNDCLASSW,
    };

    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let class_name = wide("TupaGuiTray");

    let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
    class.lpfnWndProc = Some(wndproc);
    class.hInstance = instance;
    class.lpszClassName = class_name.as_ptr();
    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        log("注册托盘窗口类失败（可能已经注册过）");
    }

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            class_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        anyhow::bail!("创建托盘消息窗口失败");
    }
    TRAY_HWND.store(hwnd as isize, std::sync::atomic::Ordering::SeqCst);
    Ok(hwnd)
}

fn add_icon(hwnd: windows_sys::Win32::Foundation::HWND, icon: isize) {
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NOTIFYICONDATAW, Shell_NotifyIconW,
    };

    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_ID;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WM_TRAY;
    // `HICON` 是裸指针，跨线程只能传整数
    data.hIcon = icon as windows_sys::Win32::UI::WindowsAndMessaging::HICON;
    let tip: Vec<u16> = "tupa-gui"
        .encode_utf16()
        .take(data.szTip.len() - 1)
        .collect();
    data.szTip[..tip.len()].copy_from_slice(&tip);
    unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
}

unsafe extern "system" fn wndproc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, WM_COMMAND, WM_DESTROY, WM_HOTKEY, WM_LBUTTONUP, WM_RBUTTONUP,
    };

    match message {
        WM_TRAY => {
            let event = (lparam as u32) & 0xFFFF;
            match event {
                WM_LBUTTONUP => toggle_panel(),
                WM_RBUTTONUP => show_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            match wparam & 0xFFFF {
                MENU_SHOW => toggle_panel(),
                MENU_QUIT => quit(),
                _ => {}
            }
            0
        }
        WM_HOTKEY => {
            toggle_panel();
            0
        }
        WM_DESTROY => 0,
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// 显示 / 隐藏面板（按当前可见性切换）
fn toggle_panel() {
    if util::panel_visible(PanelKind::Gui) {
        util::hide_panel(PanelKind::Gui);
    } else {
        util::show_panel(PanelKind::Gui);
        focus_panel();
    }
}

fn focus_panel() {
    use windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

    let hwnd = util::find_panel(PanelKind::Gui);
    if hwnd.is_null() {
        return;
    }
    unsafe { SetForegroundWindow(hwnd) };
}

fn show_menu(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, MF_SEPARATOR, MF_STRING,
        PostMessageW, SW_SHOWNA, SetForegroundWindow, ShowWindow, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
        TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_NULL,
    };

    let menu = unsafe { CreatePopupMenu() };
    if menu.is_null() {
        return;
    }
    let show = wide(i18n::t("显示面板", "Show panel"));
    let quit_item = wide(i18n::t("退出", "Quit"));
    unsafe {
        AppendMenuW(menu, MF_STRING, MENU_SHOW, show.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, MENU_QUIT, quit_item.as_ptr());
    }

    let mut point: POINT = unsafe { std::mem::zeroed() };
    unsafe { GetCursorPos(&mut point) };

    // message-only 窗口当菜单 owner：先把「虚拟」前景交给它，菜单才响应外部点击消失
    unsafe {
        ShowWindow(hwnd, SW_SHOWNA);
        SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
            point.x,
            point.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        if chosen != 0 {
            match chosen as usize {
                MENU_SHOW => toggle_panel(),
                MENU_QUIT => quit(),
                _ => {}
            }
        }
    }
}

fn quit() {
    remove_icon();
    log("托盘菜单退出");
    std::process::exit(0);
}

/// 托盘窗口句柄（首次创建后缓存）
static TRAY_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

fn tray_window() -> windows_sys::Win32::Foundation::HWND {
    TRAY_HWND.load(std::sync::atomic::Ordering::SeqCst) as windows_sys::Win32::Foundation::HWND
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 写 `%TEMP%\tupa-gui.log`（GUI 进程没有控制台）
pub fn log(message: &str) {
    use std::io::Write;

    let path = std::env::temp_dir().join("tupa-gui.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(file, "{message}");
    }
}
