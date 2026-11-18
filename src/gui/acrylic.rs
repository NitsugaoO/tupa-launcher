//! Windows 亚克力材质：给 GPUI 的窗口补一次 `SetWindowCompositionAttribute`。
//!
//! 背景（2026-11-18 实测，别改回去猜）：
//!
//! - GPUI 在 `WindowBackgroundAppearance::Blurred` 时自己会调
//!   `SetWindowCompositionAttribute(ACCENT_ENABLE_ACRYLICBLURBEHIND)`，但 0.2.2 传的
//!   gradient color 是 `(0, 0, 0, 0)` —— **alpha = 0 会退化成整窗黑玻璃**。
//!   上游后来补了 `if is_acrylic && color.3 == 0 { color.3 = 1 }`（见
//!   `zed/crates/gpui_windows/src/window.rs`），alpha = 1 才是正常亚克力。
//! - 所以这里在窗口创建后再调一次同参数、`alpha = 1` 的版本把 GPUI 那次盖掉，
//!   **不用改 gpui 源码**（也就没有 fork）。
//! - 材质本身由 DWM 画，**节能模式**或「透明效果」关闭时会被系统降级成不透明纯色。
//!   那时本模块照样成功返回，只是看起来没效果 —— 排查顺序见 `[gui] acrylic` 的注释。
//!
//! 参考实现：ELYTerminal 的 `src/blur/windows.rs`（同样照抄上游的 alpha = 1 修正）。

use anyhow::{Result, bail};
use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::core::BOOL;
use windows_sys::Win32::{
    Foundation::HWND,
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
};

/// `ACCENT_ENABLE_ACRYLICBLURBEHIND`：Acrylic。其余值见 `SetWindowCompositionAttribute`。
const ACCENT_ACRYLIC: u32 = 4;
/// `WCA_ACCENT_POLICY`
const WCA_ACCENT_POLICY: u32 = 0x13;

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    color: u32,
    animation: u32,
}

#[repr(C)]
struct CompositionData {
    attribute: u32,
    data: *mut AccentPolicy,
    size: usize,
}

/// 给窗口挂上亚克力。调用点：`gui::run` 里 view 创建好之后（窗口已上屏）。
///
/// 失败只返回 error 让调用方记日志：面板没有模糊也能用，不该因此退出。
pub fn enable(window: &Window) -> Result<()> {
    accent(window, true)
}

fn accent(window: &Window, acrylic: bool) -> Result<()> {
    let hwnd = native_handle(window)?;
    type SetAccent = unsafe extern "system" fn(HWND, *mut CompositionData) -> BOOL;
    let user32 = unsafe { GetModuleHandleW(windows_sys::w!("user32.dll")) };
    if user32.is_null() {
        bail!("user32.dll 没有加载");
    }
    let Some(found) = (unsafe { GetProcAddress(user32, windows_sys::s!("SetWindowCompositionAttribute")) })
    else {
        bail!("user32.dll 里没有 SetWindowCompositionAttribute");
    };
    let set: SetAccent = unsafe { std::mem::transmute(found) };

    let state = if acrylic { ACCENT_ACRYLIC } else { 0 };
    let mut policy = AccentPolicy {
        state,
        // 亚克力用 0；透明渐变（state = 2）用 2 —— 与 GPUI 保持一致
        flags: if acrylic { 0 } else { 2 },
        // 关键：alpha 必须是 1，0 会变成黑玻璃（见文件头）
        color: if acrylic { 1u32 << 24 } else { 0 },
        animation: 0,
    };
    let mut data = CompositionData {
        attribute: WCA_ACCENT_POLICY,
        data: &mut policy,
        size: std::mem::size_of::<AccentPolicy>(),
    };
    let ok = unsafe { set(hwnd, &mut data) };
    if ok == 0 {
        bail!("SetWindowCompositionAttribute(state = {state}) 返回 0");
    }
    Ok(())
}

fn native_handle(window: &Window) -> Result<HWND> {
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|error| anyhow::anyhow!("拿不到原生窗口句柄：{error:?}"))?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => {
            Ok(win32.hwnd.get() as HWND)
        }
        _ => bail!("不是 Win32 窗口"),
    }
}
