//! 读写系统剪贴板里的文本。
//!
//! 读：`:go` 空参数（「复制一个路径 → 唤起 → 回车跳过去」）、开屏第一行的
//! 「剪贴板」条目（见 `providers::home`）。
//! 写：GUI 面板输入框的 `Ctrl+C` / `Ctrl+X`。
//!
//! 剪贴板被别的程序占着时返回 `None` / `false`（调用方按「没有内容」处理或
//! 提示一句，不要 panic）。

/// 剪贴板里的文本（原样，含前后空白 / 换行）
#[cfg(windows)]
pub fn text() -> Option<String> {
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
pub fn text() -> Option<String> {
    None
}

/// 把文本写进系统剪贴板（覆盖原内容）。成功返回 `true`。
///
/// 句柄交接规则：`SetClipboardData` 成功后内存归系统管，**不能**再释放；
/// 失败时自己回收（`windows-sys` 没导出 `GlobalFree`，用 `GlobalReAlloc(h, 0, 0)`
/// 等价释放）。
#[cfg(windows)]
pub fn set_text(text: &str) -> bool {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalReAlloc, GlobalUnlock,
    };

    const CF_UNICODETEXT: u32 = 13;

    // UTF-16 + 结尾的 NUL
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * std::mem::size_of::<u16>();

    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if handle.is_null() {
            return false;
        }
        let ptr = GlobalLock(handle) as *mut u16;
        if ptr.is_null() {
            GlobalReAlloc(handle, 0, GMEM_MOVEABLE);
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
        GlobalUnlock(handle);

        if OpenClipboard(std::ptr::null_mut()) == 0 {
            GlobalReAlloc(handle, 0, GMEM_MOVEABLE);
            return false;
        }
        EmptyClipboard();
        if SetClipboardData(CF_UNICODETEXT, handle).is_null() {
            CloseClipboard();
            GlobalReAlloc(handle, 0, GMEM_MOVEABLE);
            return false;
        }
        // 成功：内存已交给系统，这里绝不能释放
        CloseClipboard();
        true
    }
}

#[cfg(not(windows))]
pub fn set_text(_text: &str) -> bool {
    false
}
