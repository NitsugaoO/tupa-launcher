//! 读系统剪贴板里的文本。
//!
//! 两处用到：
//! - `:go` 空参数（「复制一个路径 → 唤起 → 回车跳过去」）
//! - 开屏第一行的「剪贴板」条目（见 `providers::home`）
//!
//! 只读文本，不写、不清空剪贴板；剪贴板被别的程序占着时返回 `None`
//! （调用方按「没有剪贴板内容」处理即可，不要报错）。

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
