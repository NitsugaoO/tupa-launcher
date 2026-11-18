//! 窗口外观：圆角 + 置顶。
//!
//! **现状（2026-11-18 结案）**：亚克力是 DWM 的 `SetWindowCompositionAttribute`
//! （`ACCENT_ENABLE_ACRYLICBLURBEHIND`，实时、能采到别的窗口），它按**整个窗口矩形**
//! 铺；DWM 的系统圆角只裁内容，不裁它 —— 所以圆角外侧永远会剩一点点方形亚克力。
//! 这是 DWM 的机制，不是参数没调对。真要彻底干净只能换应用层方案（呼出前截一帧 +
//! 模糊 + `paint_image`，静态帧），WinUI 应用能做圆角 Acrylic 是因为它们用
//! WinRT compositor 自己的 visual clip，不走 accent。
//!
//! 已经试过、**别再试**的（都实测过，返回成功但视觉无效或更差）：
//! - `SetWindowRgn`：保留 `WS_EX_NOREDIRECTIONBITMAP` 时 DWM 静默忽略；
//!   去掉那个 flag 后 region 生效，但 GPUI 的无边框依赖它（标题栏冒出来 + insets 变灰）。
//! - `DwmExtendFrameIntoClientArea(-1)`、`DWMWA_NCRENDERING_POLICY`、
//!   `DWMWA_BORDER_COLOR = COLOR_NONE`、`DWMWA_VISIBLE_FRAME_BORDER_THICKNESS = 0`。
//! - 运行时子类化 `WM_NCCALCSIZE` 返回 0：`WindowBorderOffset` 与客户区失配，
//!   窗口每轮 resize 再大一圈（966 → 1551）。
//!
//! **最关键的一个坑**：`src/gui/mod.rs` 原来在 `make_overlay_window` 里调
//! `tune_dwm()`，显式设了 `DWMWCP_DONOTROUND` —— 那一行会让 DWM 彻底不管这个窗口的
//! 圆角，于是圆角只裁内容、亚克力按矩形溢出，而且之后所有 DWM 侧的调整都被它盖掉。
//! 排查这类问题**先确认窗口上没有残留的 `DONOTROUND`**。
//!
//! insets 与圆角半径是绑定的：`vendor/gpui` 把 `get_client_area_insets` 改成 0，
//! 客户区铺满窗口，内容可以直接盖到窗口边缘（此时圆角完全由内容画）。
//!
//! 顺序：`SWP_FRAMECHANGED`（如果有）会把 composition attribute 重置回 alpha = 0
//! （黑玻璃），所以亚克力的 alpha = 1 补丁必须在 [`apply`] **之后**打。

use anyhow::{Result, bail};
use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::{
        Dwm::{
            DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmSetWindowAttribute,
        },
    },
    UI::WindowsAndMessaging::{
        HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    },
};

/// 窗口创建后调一次：系统圆角（+ 让 DWM 重算一遍 frame）+ 置顶。
pub fn apply(window: &Window, always_on_top: bool, rounded: bool) -> Result<()> {
    let hwnd = hwnd(window)?;

    // 不做任何窗口形状处理：Win11 会给带系统 frame 的窗口自动加圆角，
    // 并把整个窗口（含亚克力）一起裁 —— 这正是 ElyTerminal 的表现。
    // `rounded = false` 时才显式要求直角。
    if !rounded {
        let square = DWMWCP_DONOTROUND;
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                &square as *const _ as *const core::ffi::c_void,
                std::mem::size_of_val(&square) as u32,
            );
        }
    }

    // 想在圆角外去掉那 1~2px 描边的三个属性，这台 Win11 26200 上**都无效**
    // （返回值成功，视觉零变化），别再试：`DWMWA_NCRENDERING_POLICY = DWMNCRP_DISABLED`、
    // `DWMWA_BORDER_COLOR = DWMWA_COLOR_NONE`、`DWMWA_VISIBLE_FRAME_BORDER_THICKNESS = 0`。

    if always_on_top {
        let ok = unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        };
        if ok == 0 {
            bail!("SetWindowPos(HWND_TOPMOST) 返回 0");
        }
        crate::tray::log("[tupa-gui] 置顶已设置");
    }
    Ok(())
}

fn hwnd(window: &Window) -> Result<HWND> {
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|error| anyhow::anyhow!("拿不到原生窗口句柄：{error:?}"))?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => Ok(win32.hwnd.get() as HWND),
        _ => bail!("不是 Win32 窗口"),
    }
}
