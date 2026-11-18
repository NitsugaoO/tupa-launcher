//! 窗口外观：系统圆角 + 置顶。
//!
//! 为什么要裁圆角：亚克力是 **DWM 在整个窗口矩形上**画的，而面板内容只在内部画
//! 圆角（`style::RADIUS`）。窗口本身是方的 → 四个角会漏出模糊，看起来像
//! 「亚克力/阴影溢出圆角」。这里用 `SetWindowRgn` 按内容半径把窗口裁成圆角，
//! 模糊跟着一起被裁掉；`DWMWA_WINDOW_CORNER_PREFERENCE` 再兜一层（Win11 22H2+）。
//!
//! 注意 `SetWindowRgn` 只在**尺寸变化时**要重调（region 不会自动跟着窗口缩放），
//! 所以 `Panel::render` 里比较缓存尺寸后调用 [`round`]。

use anyhow::{Result, bail};
use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::{
        Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute},
        Gdi::{ClientToScreen, CreateRoundRectRgn, SetWindowRgn},
    },
    UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetClientRect, GetWindowLongPtrW, GetWindowRect, HWND_TOPMOST,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW,
        SetWindowPos, WS_EX_NOREDIRECTIONBITMAP,
    },
};

use super::style;

/// 窗口创建后调一次：系统圆角 + 置顶。
pub fn apply(window: &Window, always_on_top: bool) -> Result<()> {
    let hwnd = hwnd(window)?;

    // DComp 窗口默认带 `WS_EX_NOREDIRECTIONBITMAP`，DWM 会忽略它的窗口形状
    // （`SetWindowRgn` / 系统圆角都静默失效，四角继续露出亚克力）。
    // 去掉这个 ex-style 后窗口重新有 redirection surface，region 才被 DWM 认。
    // GPUI 的 DComp 呈现不依赖它，实测渲染正常。
    let ex = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    if ex & (WS_EX_NOREDIRECTIONBITMAP as isize) != 0 {
        unsafe {
            SetWindowLongPtrW(
                hwnd,
                GWL_EXSTYLE,
                ex & !(WS_EX_NOREDIRECTIONBITMAP as isize),
            );
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
    }

    // Win11 的系统圆角：即使下面的 region 哪天失效，也不至于四个角全方。
    // 失败不致命（Win10 没有这个属性），只让调用方记日志。
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


    round_region(window)?;

    // 上面那些 DWM 属性要配一次 frame change 才真正重算（否则边框/阴影照旧）。
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

    // 试过但**无效**、别再折腾的三个（这台 Win11 26200 上返回值都是成功，视觉没变化）：
    // - `DWMWA_NCRENDERING_POLICY = DWMNCRP_DISABLED`
    // - `DWMWA_BORDER_COLOR = DWMWA_COLOR_NONE`
    // - `DWMWA_VISIBLE_FRAME_BORDER_THICKNESS = 0`
    // 圆角外侧剩下的那 1~2px 浅色描边是 DWM 在**窗口矩形外**画的装饰，
    // 窗口形状管不到；深色背景下基本看不见。真要彻底去掉就得拿掉
    // `WS_THICKFRAME`（GPUI 的拖边缩放热区），目前不值得。

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

/// 按当前尺寸把窗口裁成与内容一致的圆角。尺寸变化后要重新调。
pub fn round_region(window: &Window) -> Result<()> {
    let hwnd = hwnd(window)?;
    let scale = window.scale_factor();
    // 必须用**整个窗口**的尺寸：`viewport_size()` 是客户区，比窗口小一圈
    // （GPUI 在 `WM_NCCALCSIZE` 里减掉了 `WS_THICKFRAME` 的 insets），
    // 拿客户区当 region 会把右边和下边的边框裁掉一半。
    // 窗口比客户区大一圈（gpui 给 `WS_THICKFRAME` 留的 resize 热区，实测左右各 7px、
    // 上下各 4px）。亚克力画在**整个窗口**上，所以 region 只覆盖客户区才不会「大一圈」。
    let mut rect = RECT::default();
    let mut client = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        bail!("GetWindowRect 失败");
    }
    if unsafe { GetClientRect(hwnd, &mut client) } == 0 {
        bail!("GetClientRect 失败");
    }
    let mut origin = POINT { x: 0, y: 0 };
    if unsafe { ClientToScreen(hwnd, &mut origin) } == 0 {
        bail!("ClientToScreen 失败");
    }
    let left = origin.x - rect.left;
    let top = origin.y - rect.top;
    let (width, height) = (client.right - client.left, client.bottom - client.top);
    if width <= 0 || height <= 0 {
        return Ok(());
    }
    // `CreateRoundRectRgn` 的宽/高是**椭圆**的直径，且右/下边界是开区间 → +1。
    let diameter = (style::RADIUS * scale).round() as i32 * 2;
    let region = unsafe {
        CreateRoundRectRgn(
            left,
            top,
            left + width + 1,
            top + height + 1,
            diameter,
            diameter,
        )
    };
    if region.is_null() {
        bail!("CreateRoundRectRgn 失败");
    }
    // 成功后 region 归系统所有，**不要** DeleteObject。
    let ok = unsafe { SetWindowRgn(hwnd, region, 1) };
    if ok == 0 {
        bail!("SetWindowRgn 返回 0");
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
