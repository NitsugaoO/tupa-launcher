//! 窗口外观：系统圆角 + 置顶。
//!
//! 亚克力是 DWM 按**整个窗口矩形**画的，而面板内容只在内部画 `.rounded(12)` ——
//! 窗口本身方角的话，四个角会漏出模糊。让 DWM 把窗口裁成圆角
//! （`DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND`），亚克力会跟着一起被裁。
//!
//! **不要**自己 `SetWindowRgn`（2026-11-18 试过，两条路都更差）：
//! - 保留 gpui 默认的 `WS_EX_NOREDIRECTIONBITMAP` 时，DWM 会静默忽略 region
//!   （`SetWindowRgn` 返回成功，`GetWindowRgn` 也查得到，视觉毫无变化）；
//! - 去掉那个 ex-style 后 region 才生效，但 region 外的区域会被 DWM 填成浅灰/白，
//!   而 gpui 还给 `WS_THICKFRAME` 留了 6/8px 的 insets（`WM_NCCALCSIZE` 里减掉的，
//!   跟 `is_resizable` 无关）—— 结果就是「面板外多一圈」。
//!
//! 和 ElyTerminal（同样是 GPUI + `WS_EX_NOREDIRECTIONBITMAP` + 亚克力）的实测对比：
//! 它的 insets 也是 L7 T0 R7 B7，圆角同样是 DWM 画的，没有额外处理。
//!
//! 顺序很重要：`SWP_FRAMECHANGED` 会把 `SetWindowCompositionAttribute` 重置回 gpui 的
//! alpha = 0（黑玻璃），所以亚克力的 alpha = 1 补丁必须在 [`apply`] **之后**再打
//! （调用点在 `gui::Panel::render` 的首帧）。

use anyhow::{Result, bail};
use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute},
    UI::WindowsAndMessaging::{
        HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
        SetWindowPos,
    },
};

/// 窗口创建后调一次：系统圆角（+ 让 DWM 重算一遍 frame）+ 置顶。
pub fn apply(window: &Window, always_on_top: bool) -> Result<()> {
    let hwnd = hwnd(window)?;

    // Win11 的系统圆角。Win10 没有这个属性，返回失败也照常跑。
    let round = DWMWCP_ROUND;
    let hr = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &round as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&round) as u32,
        )
    };
    if hr < 0 {
        crate::tray::log(&format!(
            "[tupa-gui] DwmSetWindowAttribute(CORNER_PREFERENCE) = {hr:#x}（Win10 或更早正常）"
        ));
    }

    // 让 DWM 重算 frame（圆角 / 边框 / 阴影）。
    // 注意这会**重置窗口的 composition attribute**，亚克力的 alpha 补丁必须在这之后打。
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
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
