//! 应用图标：`.exe` / `.lnk` → 一张 BGRA 位图 → GPUI 的 [`RenderImage`]。
//!
//! 取图走 `IShellItemImageFactory`（Win7+）而不是 `SHGetFileInfoW`：
//! - `SHGetFileInfoW` 只能给 32×32（`SHGFI_LARGEICON`）；本机 200% DPI 下 20 逻辑像素
//!   要 40 物理像素，32 会糊
//! - 要 256 得走 `SHGetImageList(SHIL_JUMBO)` + `IImageList::GetIcon`，
//!   而 `IShellItemImageFactory::GetImage` 直接要**指定尺寸**，一次调用到位
//! - 返回的 HBITMAP 是 32bpp DIB，`GetDIBits` 出来就是 **BGRA（非预乘）**，
//!   正好是 GPUI `RenderImage` 要的格式（它内部把 RGBA 预乘换成 BGRA，
//!   见 `vendor/gpui/src/color.rs::swap_rgba_pa_to_bgra`）
//!
//! 缓存不在这里做：调用方（`Panel::icons`）按配置里的 `command` 缓存，
//! **失败也缓存**（裸命令 `wt` / `ms-settings:` 没有文件可提取，每次重试都是白花 COM 时间）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use gpui::RenderImage;
use image::{Frame, RgbaImage};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDIBits, GetObjectW, HBITMAP, SelectObject,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::core::HSTRING;

/// 请求的位图边长（物理像素）。
///
/// 显示尺寸是 20 逻辑像素；本机 200% DPI 要 40 物理，取 96 留 2.4 倍余量
/// （换 150% / 250% 缩放也不用改）。96² × 4 ≈ 37KB/个，几十个应用不到 3MB，
/// 没必要为了「更大」去拿 256 jumbo。
pub const ICON_PX: i32 = 96;

/// 给一个**存在的文件**（`.exe` / `.lnk` / `.msc` …）取图标。
///
/// 出错全部静默（返回 `None`）：调用方回退到类型 chip，不能因为一个图标让整行画不出来。
pub fn load(path: &Path) -> Option<Arc<RenderImage>> {    ensure_com();

    // 安全：下面全是 Win32 调用，句柄在 `read_bgra` 前后各自检查、用完立刻释放。
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(
            &HSTRING::from(path.as_os_str()),
            None::<&windows::Win32::System::Com::IBindCtx>,
        )
        .ok()?;
        let bitmap = factory
            .GetImage(
                SIZE {
                    cx: ICON_PX,
                    cy: ICON_PX,
                },
                SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
            )
            .ok()?;

        let pixels = read_bgra(bitmap);
        let _ = DeleteObject(bitmap.into());

        let (width, height, bytes) = pixels?;
        let buffer = RgbaImage::from_raw(width, height, bytes)?;
        Some(Arc::new(RenderImage::new([Frame::new(buffer)])))
    }
}

/// `command` → 真正带应用图标的 `.exe`。
///
/// 三层兑底（命中就停）：
/// 1. `command` 直接解析成 `.exe`（`.lnk` 会先解到目标）
/// 2. 解析到的是 `.cmd` / `.bat` / **没扩展名**的 shim（npm 装的 CLI 全是这种，
///    VSCode 的 `bin\code` 也是）→ 同级和上一级目录里找同名 `.exe`
/// 3. 按**条目标题**、再按命令名，去开始菜单索引里找 `.lnk` → 解析到 exe
///
/// 第 2 步就是「VSCode 图标变成白纸 + 齿轮」的修法：SHELL 对无扩展名脚本只给
/// “未知文件类型”图标，而那个 shim 上一级的 `Code.exe` 才是真 logo。
pub fn exe_for(command: &str, title: &str) -> Option<PathBuf> {
    if let Some(path) = crate::util::resolve_executable(command) {
        if is_exe(&path) {
            return Some(path);
        }
        if let Some(exe) = sibling_exe(&path) {
            return Some(exe);
        }
    }

    start_menu_exe(title).or_else(|| start_menu_exe(command))
}

fn is_exe(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

/// 同一程序目录里按名字找 `.exe`（先看同级，再看上一级）。
fn sibling_exe(path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let parent = path.parent()?;
    let mut dirs = vec![parent];
    if let Some(up) = parent.parent() {
        dirs.push(up);
    }
    dirs.into_iter().find_map(|dir| find_named(dir, &stem, "exe"))
}

/// 目录里按 `名字.扩展名` 找。Windows 文件系统大小写不敏感，但 `read_dir`
/// 返回的是磁盘上的真实大小写，所以比较前统一转小写。
fn find_named(dir: &Path, stem: &str, ext: &str) -> Option<PathBuf> {
    let want = format!("{}.{}", stem.to_lowercase(), ext.to_lowercase());
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let Some(name) = path
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
        else {
            continue;
        };
        if name == want {
            return Some(path);
        }
    }
    None
}

/// 开始菜单里按名字找 `.lnk`，解到目标 exe
/// （`Visual Studio Code` → `…\Visual Studio Code.lnk` → `Code.exe`）。
fn start_menu_exe(name: &str) -> Option<PathBuf> {
    let key = name.trim().to_lowercase();
    if key.is_empty() {
        return None;
    }
    let lnk = start_menu_index().get(&key)?;
    let target = crate::util::resolve_shortcut(lnk);
    target.is_file().then_some(target)
}

/// `lnk 文件名（小写、去扩展名）` → 路径。整个进程只建一次。
fn start_menu_index() -> &'static HashMap<String, PathBuf> {
    static INDEX: OnceLock<HashMap<String, PathBuf>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut map = HashMap::new();
        for dir in start_menu_dirs() {
            collect_shortcuts(&dir, &mut map, 0);
        }
        map
    })
}

fn start_menu_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(programdata) = std::env::var_os("ProgramData") {
        dirs.push(PathBuf::from(programdata).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    dirs.retain(|dir| dir.is_dir());
    dirs
}

fn collect_shortcuts(dir: &Path, map: &mut HashMap<String, PathBuf>, depth: usize) {
    // 开始菜单一般两三层；限个深度，避免 junction / 符号链接绕成环
    if depth > 5 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_shortcuts(&path, map, depth + 1);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
            && let Some(stem) = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_lowercase())
        {
            map.entry(stem).or_insert(path);
        }
    }
}

/// COM 已经初始化过就别再初始化（`RPC_E_CHANGED_MODE` 说明当前线程是别的 apartment，
/// 直接用也行 —— SHELL 的解析接口在 MTA 下同样可用）。
fn ensure_com() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    });
}

/// HBITMAP → `(宽, 高, BGRA 自顶向下)`。
///
/// 用负的 `biHeight` 让 `GetDIBits` 直接输出自顶向下的行序，省一次翻转。
unsafe fn read_bgra(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    let mut info = BITMAP::default();
    let got = unsafe {
        GetObjectW(
            bitmap.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut info as *mut _ as *mut _),
        )
    };
    if got == 0 || info.bmWidth <= 0 || info.bmHeight <= 0 {
        return None;
    }
    let width = info.bmWidth as u32;
    let height = info.bmHeight as u32;

    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.is_invalid() {
        return None;
    }
    let old = unsafe { SelectObject(dc, bitmap.into()) };

    let mut header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            // 负高度 = 自顶向下
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let lines = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr().cast()),
            &mut header,
            DIB_RGB_COLORS,
        )
    };
    unsafe {
        SelectObject(dc, old);
        let _ = DeleteDC(dc);
    }

    if lines == 0 {
        return None;
    }
    // 老式图标（16 色 / 单色）的 DIB 里 alpha 全是 0，当不透明处理，
    // 否则整块画出来是「什么都没有」。
    if pixels.as_chunks::<4>().0.iter().all(|px| px[3] == 0) {
        for px in pixels.as_chunks_mut::<4>().0.iter_mut() {
            px[3] = 255;
        }
    }
    Some((width, height, pixels))
}
