//! 临时探针：验证「WinRT 合成层 + backdrop brush」能不能在普通 Win32 窗口上
//! 给出**窗口后面的桌面内容**，以及哪个 brush 自带模糊。
//!
//! 不属于启动器功能，结论抄进 STATE 后可以整个删掉。
//!
//! ```text
//! cargo run --example acrylic_probe               # CreateBackdropBrush
//! cargo run --example acrylic_probe -- host       # CreateHostBackdropBrush + DWMWA_USE_HOSTBACKDROPBRUSH
//! cargo run --example acrylic_probe -- host 30    # 第二个参数 = tint 不透明度百分比（默认 60）
//! ```
//!
//! 窗口是 `WS_EX_NOREDIRECTIONBITMAP` 的 `WS_POPUP`，**没有任何 swapchain**，
//! 画面完全由合成层构成 —— 窗口里看到的东西就是 backdrop brush 采到的内容。
//! 20 秒后自动退出（省得关不掉）。

use std::time::{Duration, Instant};

use windows::core::Interface;
use windows_numerics::Vector2;
use windows::Win32::Foundation::HWND as W_HWND;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_HOSTBACKDROPBRUSH};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
};
use windows::UI::Color;
use windows::UI::Composition::{Compositor, CompositionBrush, Visual};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WIDTH: i32 = 520;
const HEIGHT: i32 = 320;

fn main() -> windows::core::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let host = args.iter().any(|a| a == "host");
    let tint = args
        .iter()
        .find_map(|a| a.parse::<u8>().ok())
        .unwrap_or(60)
        .min(100);

    unsafe {
        // DispatcherQueue 用 DQTAT_COM_NONE，但 Compositor 自己仍要 COM 单元。
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let hwnd = create_window(host)?;
        println!(
            "[probe] hwnd={hwnd:?} brush={} tint={tint}%",
            if host { "host" } else { "backdrop" }
        );

        let options = DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_NONE,
        };
        let _queue = CreateDispatcherQueueController(options)?;

        let compositor = Compositor::new()?;
        let interop: ICompositorDesktopInterop = compositor.cast()?;
        let target = interop.CreateDesktopWindowTarget(W_HWND(hwnd), true)?;

        if host {
            let enable: i32 = 1;
            let hr = DwmSetWindowAttribute(
                W_HWND(hwnd),
                DWMWA_USE_HOSTBACKDROPBRUSH,
                &enable as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            );
            println!("[probe] DWMWA_USE_HOSTBACKDROPBRUSH -> {hr:?}");
        }

        let backdrop = if host {
            compositor.CreateHostBackdropBrush()?
        } else {
            compositor.CreateBackdropBrush()?
        };
        let backdrop_base: CompositionBrush = backdrop.cast()?;

        let root = compositor.CreateContainerVisual()?;
        let size = Vector2 {
            X: WIDTH as f32,
            Y: HEIGHT as f32,
        };

        // 底层：backdrop（窗口后面的桌面），它糊不糊就是本次结论
        let blur = compositor.CreateSpriteVisual()?;
        blur.SetSize(size)?;
        blur.SetBrush(&backdrop_base)?;
        root.Children()?.InsertAtTop(&blur.cast::<Visual>()?)?;

        // 上层：tint，模拟面板底色，同时验证叠层顺序
        let tint_visual = compositor.CreateSpriteVisual()?;
        tint_visual.SetSize(size)?;
        let tint_brush = compositor.CreateColorBrushWithColor(Color {
            A: (tint as f32 / 100.0 * 255.0) as u8,
            R: 12,
            G: 14,
            B: 16,
        })?;
        tint_visual.SetBrush(&tint_brush)?;
        root.Children()?.InsertAtTop(&tint_visual.cast::<Visual>()?)?;

        target.SetRoot(&root.cast::<Visual>()?)?;
        println!("[probe] 合成层已挂上，窗口显示 20 秒");

        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(20) {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        DestroyWindow(hwnd);
        println!("[probe] 结束");
    }
    Ok(())
}

unsafe fn create_window(host: bool) -> windows::core::Result<HWND> {
    unsafe {
        let class: Vec<u16> = "TupaAcrylicProbe\0".encode_utf16().collect();
        let hinstance = GetModuleHandleW(std::ptr::null());
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wndproc);
        wc.lpszClassName = class.as_ptr();
        wc.hInstance = hinstance;
        wc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
        RegisterClassW(&wc);

        let title: Vec<u16> = format!(
            "acrylic probe [{}]\0",
            if host { "host" } else { "backdrop" }
        )
        .encode_utf16()
        .collect();
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_VISIBLE,
            120,
            120,
            WIDTH,
            HEIGHT,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err(windows::core::Error::from_thread());
        }
        Ok(hwnd)
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
