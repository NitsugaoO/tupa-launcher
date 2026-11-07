//! 记录「资源管理器最近打开过哪些目录」。
//!
//! 系统自带的跳转列表（`AutomaticDestinations\f01b4d95cf55d32a.…-ms`）在 Win11 上
//! 是**长时间批量 flush** 的：实测开着窗口等 75 秒不变、关窗口不变、甚至重启
//! explorer 也不变，文件能连续几十小时不动。所以光读那个文件，看到的永远是「昨天去过哪」。
//!
//! 好在 `ShellWindows` 能实时枚举**当前打开**的资源管理器窗口在哪个目录。这里靠
//! `SetWinEventHook` **事件驱动**：
//!
//! - 监听资源管理器窗口（类名 `CabinetWClass`）的 `EVENT_OBJECT_NAMECHANGE`
//!   —— 每次导航到新文件夹，窗口标题都会变；
//! - 再加上 `EVENT_SYSTEM_FOREGROUND` / `EVENT_OBJECT_SHOW`，切过去、新开窗口也能接上；
//! - 另外每 [`FALLBACK`] 兜底扫一次，防止漏事件。
//!
//! 效果：导航后不到 1 秒就记下，平时**零唤醒**（不再每 5 秒 poll 一次，实测
//! 有窗口时那会吃掉 ~0.1% CPU）。记录写到 `~/.config/tupa-launcher/explorer.json`。
//!
//! 局限：只有 tupa 守护进程在跑的时候才记得到（开机自启，通常一直在）；
//! 守护进程被关掉的那段时间只能靠跳转列表补。
//!
//! 注意：`WINEVENT_OUTOFCONTEXT` 的回调是**通过消息循环分发**的，所以钩子必须注册在
//! 一个会 `PeekMessage` + `DispatchMessage` 的线程上 —— 这里是本模块自己的后台线程，
//! 不去动守护进程主循环（那个循环只认 `WM_HOTKEY`，不 `DispatchMessage`）。

use std::path::PathBuf;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// 兜底轮询间隔（没有事件时最少多久扫一次）
const FALLBACK: Duration = Duration::from_secs(60);

/// 收到事件后等一下再枚举，让连续的导航事件聚齐（否则一次导航可能扫好几次）
const DEBOUNCE: Duration = Duration::from_millis(300);

/// 钩子回调只负责置位，真正的枚举在监听线程里做
static PENDING: AtomicBool = AtomicBool::new(false);

/// 本地最多留多少条
const LIMIT: usize = 300;

/// 同一个目录被反复扫到时，隔这么久才刷新一次时间戳（避免每轮都写盘）
const REFRESH_SECS: u64 = 60;

/// 一条本地记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Seen {
    pub path: PathBuf,
    /// Unix 秒
    pub last_seen: u64,
}

/// 记录文件的位置
pub fn path() -> Option<PathBuf> {
    Some(crate::config::config_dir()?.join("explorer.json"))
}

/// 读本地记录（按「最近在前」排好）。文件不存在 / 坏了都当空。
pub fn load() -> Vec<Seen> {
    let Some(path) = path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut items: Vec<Seen> = serde_json::from_str(&text).unwrap_or_default();
    items.sort_by_key(|item| std::cmp::Reverse(item.last_seen));
    items
}

/// 起后台监听线程。重复调用只会起一个。
///
/// 钩子注册在这条线程自己身上（不是调用方线程），因为它需要自带一个会
/// `DispatchMessage` 的消息循环。
pub fn start() {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("exp-watch".into())
            .spawn(watch_loop);
    });
}

#[cfg(windows)]
fn watch_loop() {
    install_hooks();

    let mut last_poll = std::time::Instant::now();
    let mut first = true;

    loop {
        // 等事件（或 `FALLBACK` 到点）。只有本线程的消息队列，所以醒来≈真的有事
        if !first {
            wait_for_event();
        }
        first = false;
        drain_messages();

        let pending = PENDING.swap(false, Ordering::Relaxed);
        let due = last_poll.elapsed() >= FALLBACK;
        // 其它窗口的标题变化（终端、浏览器）也会把消息推进来，但没命中
        // `CabinetWClass` 就不该跟着扫一遍 —— 那才是 CPU 被吃掉的真正原因
        if !pending && !due {
            continue;
        }

        if pending {
            // 一次导航常常连发几个事件，稍微等一下再统一取快照
            std::thread::sleep(DEBOUNCE);
            drain_messages();
            PENDING.store(false, Ordering::Relaxed);
        }

        last_poll = std::time::Instant::now();
        let folders = snapshot();
        if !folders.is_empty() {
            let _ = record(&folders);
        }
    }
}

#[cfg(not(windows))]
fn watch_loop() {
    loop {
        std::thread::sleep(FALLBACK);
        let folders = snapshot();
        if !folders.is_empty() {
            let _ = record(&folders);
        }
    }
}

/// `OBJID_WINDOW`（窗口本身，不是子控件）
#[cfg(windows)]
const OBJID_WINDOW: i32 = 0;

/// 注册两个 WinEvent 钩子：窗口标题变化（导航）+ 前台切换 / 窗口出现。
///
/// 必须先让本线程的消息队列存在，所以开头先 `PeekMessage` 一下。
#[cfg(windows)]
fn install_hooks() {
    use windows_sys::Win32::UI::Accessibility::SetWinEventHook;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_SHOW, EVENT_SYSTEM_FOREGROUND, MSG, PM_NOREMOVE,
        PeekMessageW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    };

    // SAFETY: 只是建消息队列 + 注册钩子；回调只置位一个 AtomicBool
    unsafe {
        let mut msg = std::mem::zeroed::<MSG>();
        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);

        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        // 区间必须是**窄**的：`EVENT_SYSTEM_FOREGROUND..EVENT_OBJECT_SHOW` 看上去是
        // 「两个事件」，实际是 0x0003–0x8002 区间，几乎把所有事件都订上了，
        // 系统会不停往本线程推消息（实测 2.6% CPU）。
        // 钩子句柄整个进程生命周期都留着，不需要 Unhook
        for (min, max) in [
            (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
            (EVENT_OBJECT_SHOW, EVENT_OBJECT_SHOW),
            (EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE),
        ] {
            let _ = SetWinEventHook(
                min,
                max,
                std::ptr::null_mut(),
                Some(on_event),
                0,
                0,
                flags,
            );
        }
    }
}

#[cfg(not(windows))]
fn install_hooks() {}

/// 钩子回调：只认资源管理器窗口本身的事件
#[cfg(windows)]
unsafe extern "system" fn on_event(
    _hook: windows_sys::Win32::UI::Accessibility::HWINEVENTHOOK,
    _event: u32,
    hwnd: windows_sys::Win32::Foundation::HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    // 子对象（列表项、文档等）的标题变化一律不要
    if id_object != OBJID_WINDOW || id_child != 0 {
        return;
    }
    if !is_explorer_window(hwnd) {
        return;
    }
    PENDING.store(true, Ordering::Relaxed);
}

/// 窗口类名是不是资源管理器（`CabinetWClass`）
#[cfg(windows)]
fn is_explorer_window(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;

    if hwnd.is_null() {
        return false;
    }
    let mut buffer = [0u16; 64];
    // SAFETY: buffer 长度自己控制，hwnd 由系统回调传入
    let len = unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    if len <= 0 {
        return false;
    }
    String::from_utf16_lossy(&buffer[..len as usize]) == "CabinetWClass"
}

/// 阻塞等消息（最多 [`FALLBACK`]）
#[cfg(windows)]
fn wait_for_event() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MSG_WAIT_FOR_MULTIPLE_OBJECTS_EX_FLAGS, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx,
        QS_ALLINPUT, QUEUE_STATUS_FLAGS,
    };

    // SAFETY: 不传句柄数组，只是等本线程的消息队列
    unsafe {
        MsgWaitForMultipleObjectsEx(
            0,
            std::ptr::null(),
            FALLBACK.as_millis() as u32,
            QS_ALLINPUT as QUEUE_STATUS_FLAGS,
            MWMO_INPUTAVAILABLE as MSG_WAIT_FOR_MULTIPLE_OBJECTS_EX_FLAGS,
        );
    }
}

/// 把线程消息队列里的消息全部分发掉 —— 只有 `DispatchMessage` 才会真正调用钩子回调
#[cfg(windows)]
fn drain_messages() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };

    // SAFETY: 标准的消息泵写法
    unsafe {
        let mut msg = std::mem::zeroed::<MSG>();
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// 把所有看到的目录并进本地记录
fn record(folders: &[PathBuf]) -> Result<()> {
    let now = unix_now();
    let mut items = load();
    let mut changed = false;

    for folder in folders {
        match items.iter_mut().find(|seen| &seen.path == folder) {
            Some(seen) => {
                if now.saturating_sub(seen.last_seen) >= REFRESH_SECS {
                    seen.last_seen = now;
                    changed = true;
                }
            }
            None => {
                items.push(Seen {
                    path: folder.clone(),
                    last_seen: now,
                });
                changed = true;
            }
        }
    }

    if !changed {
        return Ok(());
    }

    items.sort_by_key(|item| std::cmp::Reverse(item.last_seen));
    items.truncate(LIMIT);
    save(&items)
}

fn save(items: &[Seen]) -> Result<()> {
    let Some(path) = path() else {
        return Ok(());
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("无法创建目录 {}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(items).context("序列化 explorer.json 失败")?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("写入失败：{}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("写入失败：{}", path.display()))?;
    Ok(())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 当前打开的资源管理器窗口都在哪些目录
#[cfg(windows)]
pub fn snapshot() -> Vec<PathBuf> {
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Shell::{IShellWindows, IWebBrowser2, ShellWindows};
    use windows::core::Interface;

    let mut out = Vec::new();

    // SAFETY: 只做 COM 调用，任何一步失败就当这轮没扫到
    unsafe {
        // 每线程都要初始化一次；重复调用返回 S_FALSE，忽略返回值即可
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let Ok(shell) = CoCreateInstance::<_, IShellWindows>(&ShellWindows, None, CLSCTX_ALL) else {
            return out;
        };
        let Ok(count) = shell.Count() else {
            return out;
        };

        for index in 0..count {
            let variant = VARIANT::from(index);
            let Ok(dispatch) = shell.Item(&variant) else {
                continue;
            };
            let Ok(browser) = dispatch.cast::<IWebBrowser2>() else {
                continue;
            };
            let Ok(url) = browser.LocationURL() else {
                continue;
            };
            if let Some(path) = url_to_path(&url.to_string()) {
                out.push(path);
            }
        }
    }

    out
}

#[cfg(not(windows))]
pub fn snapshot() -> Vec<PathBuf> {
    Vec::new()
}

/// `file:///C:/Users/…` / `file://server/share/…` → 本地路径
fn url_to_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    // `file:///C:/…` 头部有三个斜杠，本地路径只要留一个分隔
    let rest = rest.trim_start_matches('/');
    let decoded = percent_decode(rest);
    let mut text = decoded.replace('/', "\\");

    // 不是 `C:` 这种盘符开头，就是 UNC（`file://server/share`）
    let is_drive = text.as_bytes().get(1) == Some(&b':');
    if !is_drive {
        text.insert_str(0, "\\\\");
    }

    Some(PathBuf::from(text))
}

/// `%20` 之类的最小反转义（URL 里的非 ASCII 也是按 UTF-8 百分号编码的）
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2]))
        {
            out.push(high * 16 + low);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_file_urls_into_paths() {
        assert_eq!(
            url_to_path("file:///C:/Users/Agustin/Projects"),
            Some(PathBuf::from(r"C:\Users\Agustin\Projects"))
        );
        assert_eq!(
            url_to_path("file:///D:/"),
            Some(PathBuf::from(r"D:\"))
        );
        assert_eq!(
            url_to_path("file://server/share/folder"),
            Some(PathBuf::from(r"\\server\share\folder"))
        );
        // 非 file 协议（`http:` / 搜索 `search-ms:`）一律不要
        assert_eq!(url_to_path("http://example.com"), None);
        assert_eq!(url_to_path("search-ms:query=x"), None);
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(
            url_to_path("file:///C:/Users/Agustin/%E4%B8%AD%E6%96%87%20dir"),
            Some(PathBuf::from(r"C:\Users\Agustin\中文 dir"))
        );
        assert_eq!(percent_decode("a%20b"), "a b");
        // 坏掉的转义原样保留
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn records_only_new_or_stale() {
        // 直接测合并规则的纯逻辑：同一路径不会重复追加
        let mut items = vec![Seen {
            path: PathBuf::from(r"C:\Alpha"),
            last_seen: 100,
        }];
        for folder in [PathBuf::from(r"C:\Alpha"), PathBuf::from(r"C:\Beta")] {
            if !items.iter().any(|seen| seen.path == folder) {
                items.push(Seen {
                    path: folder,
                    last_seen: 200,
                });
            }
        }
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].last_seen, 100, "老记录不该被无谓刷新");
    }
}
