//! 托盘图标：运行时用 GDI 画一个「背景色方块 + 草绿 ❯」，不依赖 .ico 资源文件。
//!
//! 为什么不用资源：加 `.ico` 得引入 `winres` / `build.rs`，而两个 bin 里只有 `tupa-gui`
//! 需要它；在运行时画 32×32 的位图标量很小（两条线段），也方便跟随主题色。

use std::ffi::c_void;

use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, HICON, ICONINFO,
};

/// 图标边长（托盘一般是 16，给到 32 让高 DPI 也清楚）
const SIZE: i32 = 32;

/// 画图标并返回 `HICON`（用完要 [`destroy_icon`]）
pub fn draw(background: (u8, u8, u8), accent: (u8, u8, u8)) -> HICON {
    unsafe {
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            crate::tray::log("icon: CreateCompatibleDC 失败");
            return std::ptr::null_mut();
        }

        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = SIZE;
        info.bmiHeader.biHeight = -SIZE; // 负值 = 自上而下
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;

        let mut bits: *mut c_void = std::ptr::null_mut();
        let color = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        if color.is_null() || bits.is_null() {
            crate::tray::log("icon: CreateDIBSection 失败");
            DeleteDC(dc);
            return std::ptr::null_mut();
        }

        let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, (SIZE * SIZE) as usize);
        let bg = argb(background, 255);
        let fg = argb(accent, 255);
        for pixel in pixels.iter_mut() {
            *pixel = bg;
        }

        // 「❯」= 两条线段：(10,8)→(21,16) 与 (21,16)→(10,24)，笔刷半径 1.6px
        draw_line(pixels, (10.0, 8.0), (21.0, 16.0), 1.6, fg);
        draw_line(pixels, (21.0, 16.0), (10.0, 24.0), 1.6, fg);

        // mask 全 0：32bpp 图标按 alpha 绘制，mask 只需形状正确
        let mask = CreateBitmap(SIZE, SIZE, 1, 1, std::ptr::null());

        let icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&icon_info);
        if icon.is_null() {
            crate::tray::log("icon: CreateIconIndirect 失败");
        }

        DeleteObject(color);
        DeleteObject(mask);
        DeleteDC(dc);
        icon
    }
}

/// 释放 [`draw`] 生成的图标
pub fn destroy_icon(icon: HICON) {
    if icon.is_null() {
        return;
    }
    unsafe { DestroyIcon(icon) };
}

/// `(r,g,b)` + alpha → DIB 里的 BGRA 小端 u32
fn argb((r, g, b): (u8, u8, u8), a: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

/// 画一条带粗细的线段（逐像素算到线段的距离）
fn draw_line(pixels: &mut [u32], from: (f32, f32), to: (f32, f32), radius: f32, color: u32) {
    let (x0, y0) = from;
    let (x1, y1) = to;
    let dx = x1 - x0;
    let dy = y1 - y0;
    let length_sq = dx * dx + dy * dy;

    for y in 0..SIZE {
        for x in 0..SIZE {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            // 点到线段的最近距离
            let t = if length_sq == 0.0 {
                0.0
            } else {
                (((px - x0) * dx + (py - y0) * dy) / length_sq).clamp(0.0, 1.0)
            };
            let nearest_x = x0 + t * dx;
            let nearest_y = y0 + t * dy;
            let distance = ((px - nearest_x).powi(2) + (py - nearest_y).powi(2)).sqrt();
            if distance <= radius {
                pixels[(y * SIZE + x) as usize] = color;
            }
        }
    }
}
