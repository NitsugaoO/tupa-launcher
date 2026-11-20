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
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
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
pub fn load(path: &Path) -> Option<Arc<RenderImage>> {
    ensure_thread_com();

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

/// 从图片文件（png / jpg / ico）直接解码，**不走 SHELL**。
///
/// 配置里的 `icon = "..."` 用这个：`SIIGBF_ICONONLY` 对图片文件返回的是
/// 「图片文件」的通用图标（白纸），不是图本身；这里用 `image` 解出原图，
/// 由界面的 `img()` 自己缩放到 20 逻辑像素。
pub fn load_image(path: &Path) -> Option<Arc<RenderImage>> {
    let rgba = image::open(path).ok()?.into_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let mut data = rgba.into_raw();
    for pixel in data.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2); // RGBA → BGRA（和 load 一致，不预乘）
    }
    let buffer = RgbaImage::from_raw(width, height, data)?;
    Some(Arc::new(RenderImage::new([Frame::new(buffer)])))
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
    if let Some(resolved) = crate::util::resolve_executable(command) {
        // `AppData\Local\Microsoft\WindowsApps\wt.exe` 这类别名是个 symlink，
        // 跟到包里真正的 exe 才能推出 AUMID（直接拿别名文件名只会得到 `wt.exe!App`）
        let path = resolve_alias(&resolved).unwrap_or(resolved);
        // Scoop 的 shim（`<scoop>\shims\windhawk.exe`）是个通用代理程序，
        // SHELL 只能给出 shim 自己的图标，得找到 apps 目录里的真程序
        if let Some(real) = scoop_target(&path) {
            return Some(real);
        }
        // MSIX 应用（`wt.exe` 转发到 `...\WindowsApps\<包名>\wt.exe`）的目录受保护，
        // `IShellItemImageFactory` 只能给出通用的「应用」图标（白纸 + 一块蓝），
        // 所以跳过它，直接用 `shell:AppsFolder` 让 shell 去包资源里取。
        if !is_msix(&path) {
            if is_exe(&path) {
                return Some(path);
            }
            if let Some(exe) = sibling_exe(&path) {
                return Some(exe);
            }
        } else if let Some(item) =
            msix_shell_item(&path).or_else(|| alias_family_item(&path))
        {
            return Some(PathBuf::from(item));
        }
    }

    start_menu_exe(title).or_else(|| start_menu_exe(command))
}

/// App Execution Alias（`WindowsApps\wt.exe`）→ 同名 exe 所在包目录的 AUMID。
///
/// `WindowsApps\wt.exe` 是 **AppExecLink reparse point**，不是 symlink：`std::fs::read_link`
/// 直接失败（PowerShell 的 `LinkType` 也是空的，只有 MSYS 的 `ls` 会把它显示成 `->`），
/// 所以 [`resolve_alias`] 拿不到包目录，图标就断在这一步（2026-11-20 的 wt）。
/// 不过 alias 的**目录形态**也同时存在（`WindowsApps\<PackageFamilyName>\wt.exe`），
/// 在那里能拿到包名 —— 枚举一层子目录找同名 exe 即可，比解 reparse 二进制可靠。
fn alias_family_item(path: &Path) -> Option<String> {
    let dir = path.parent()?;
    // 只处理 `…\WindowsApps\<name>.exe` 这一层；包目录里的 exe 已由 `msix_shell_item` 处理
    if !dir
        .file_name()?
        .to_string_lossy()
        .eq_ignore_ascii_case("WindowsApps")
    {
        return None;
    }
    let file = path.file_name()?;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let candidate = entry.path();
        if candidate.is_dir() && candidate.join(file).is_file() {
            return msix_shell_item(&candidate.join(file));
        }
    }
    None
}

/// symlink 解析（`WindowsApps\wt.exe` → 包里的真 exe）。失败就当作普通文件。
fn resolve_alias(path: &Path) -> Option<PathBuf> {
    let link = std::fs::read_link(path).ok()?;
    let target = if link.is_absolute() {
        link
    } else {
        path.parent()?.join(link)
    };
    target.is_file().then_some(target)
}

/// MSIX 包目录 → `shell:AppsFolder\<PackageFamilyName>!App`。
///
/// 包目录名形如 `Microsoft.WindowsTerminal_1.24.12741.0_x64__8wekyb3d8bbwe`：
/// PackageFamilyName = `<Name>_<PublisherId>`（去掉版本和架构）。
/// 多数 UWP 的入口 AppId 就是 `App`（Terminal / PowerShell 都是）；猜错了取图会失败，
/// 自然回落到 chip，不会把行画坏。
fn msix_shell_item(path: &Path) -> Option<String> {
    let mut after = false;
    let mut package = None;
    for part in path.components() {
        let text = part.as_os_str().to_string_lossy();
        if after {
            package = Some(text.into_owned());
            break;
        }
        if text.eq_ignore_ascii_case("WindowsApps") {
            after = true;
        }
    }
    let package = package?;
    // 包目录名必须含 `_`（包全名或 PackageFamilyName）；
    // `WindowsApps\wt.exe` 这种别名文件不是包目录
    if !package.contains('_') {
        return None;
    }
    // 两种目录形态：
    // - `Microsoft.WindowsTerminal_1.24…_x64__8wekyb3d8bbwe`（Program Files\WindowsApps 下的包全名）
    // - `Microsoft.PowerShell_8wekyb3d8bbwe`（AppData\Local\Microsoft\WindowsApps 下的别名目录，
    //   已经是 PackageFamilyName）
    let family = match package.rsplit_once("__") {
        Some((head, publisher)) => format!("{}_{publisher}", head.split('_').next()?),
        None => package,
    };
    Some(format!(r"shell:AppsFolder\{family}!App"))
}

/// 已知的协议 / shell 命令 → `shell:AppsFolder` 里的项。
///
/// UWP（Microsoft Store、设置面板）没有可执行的 exe，图标只能通过
/// `shell:AppsFolder\<AUMID>` 让 shell 到包资源里取。
pub fn shell_item_for(command: &str) -> Option<String> {
    let command = command.trim().to_ascii_lowercase();
    let aumid = if command.starts_with("ms-windows-store:") {
        "Microsoft.WindowsStore_8wekyb3d8bbwe!App"
    } else if command.starts_with("ms-settings:") {
        "windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel"
    } else {
        return None;
    };
    Some(format!(r"shell:AppsFolder\{aumid}"))
}

/// 是否在 MSIX 包目录里（`C:\Program Files\WindowsApps\<包名>\…`）。
fn is_msix(path: &Path) -> bool {
    path.components().any(|part| {
        part.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("WindowsApps")
    })
}

/// Scoop shim → `apps\<名字>\current\<名字>.exe`（没有 current 就取版本目录里最新的）。
fn scoop_target(path: &Path) -> Option<PathBuf> {
    let shims = path.parent()?;
    if !shims
        .file_name()?
        .to_string_lossy()
        .eq_ignore_ascii_case("shims")
    {
        return None;
    }
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let apps = shims.parent()?.join("apps").join(&stem);

    if let Some(exe) = find_named(&apps.join("current"), &stem, "exe") {
        return Some(exe);
    }
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&apps)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    versions.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    versions
        .into_iter()
        .find_map(|dir| find_named(&dir, &stem, "exe"))
}

/// `.url`（Internet 快捷方式）→ INI 里 `IconFile=` 指向的 `.ico`。
///
/// SHELL 对 `.url` 只会给「Internet 快捷方式」的白纸图标，不会读 INI 里的 IconFile，
/// 所以得自己解（Steam 给每个游戏生成的 `.url` 都带一个游戏 ico）。
/// **只认 `.ico`**：`%SystemRoot%\system32\url.dll,0` 这种要靠 `IconIndex` 选图，
/// 我们拿不到 index，索性回落 `.url` 本身（与旧行为一致）。
fn url_icon_file(path: &Path) -> Option<PathBuf> {
    let text = String::from_utf8_lossy(&std::fs::read(path).ok()?).into_owned();
    let value = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("IconFile="))?
        .trim();
    // 去掉可选的 `,<IconIndex>` 尾巴，再展开 `%SystemRoot%` 这类环境变量
    let raw = value.split(',').next()?.trim();
    let candidate = PathBuf::from(expand_env(raw));
    let is_ico = candidate
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ico"));
    (is_ico && candidate.is_file()).then_some(candidate)
}

/// 展开 `%VAR%`（不认识的变量原样保留）。
fn expand_env(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        match std::env::var(name) {
            Ok(value) => out.push_str(&value),
            Err(_) => {
                out.push('%');
                out.push_str(name);
                out.push('%');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn is_exe(path: &Path) -> bool {    path.extension()
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
    let exact = name.trim().to_lowercase();
    if exact.is_empty() {
        return None;
    }
    let index = start_menu_index();
    let mut candidates = vec![exact];
    candidates.extend(normalize_menu_keys(name));

    let lnk = candidates
        .iter()
        .find_map(|key| index.get(key))
        .or_else(|| {
            // 标题往往比快捷方式名长（「Ubuntu 24.04 (WSL)」vs 「Ubuntu-24.04.lnk」），
            // 再按「去括号 / 取括号内 + 分隔符归一」两种写法各试一次
            index
                .iter()
                .find(|(key, _)| {
                    let normalized = normalize_menu_keys(key);
                    candidates.iter().any(|want| normalized.contains(want))
                })
                .map(|(_, path)| path)
        })?;
    // `.url` 没有可解析的目标（它是 INI + IconFile），但**不能**直接拿 `.url` 去取图标：
    // SHELL 对 `.url` 只给「Internet 快捷方式」白纸（`SIIGBF_ICONONLY` 不读 INI 里的
    // IconFile），Steam 的六个游戏就是这么变白纸的（2026-11-20）。先解出 `IconFile=.ico`。
    if lnk
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("url"))
    {
        return Some(url_icon_file(lnk).unwrap_or_else(|| lnk.to_path_buf()));
    }
    let target = crate::util::resolve_shortcut(lnk);
    target.is_file().then_some(target)
}

/// 开始菜单名字归一，返回多个候选：
///
/// - 去掉括号（含中文括号）里的内容：「Ubuntu 24.04 (WSL)」→ `ubuntu 24 04`
/// - 只取括号里的：「终端 (Windows Terminal)」→ `windows terminal`
///
/// `-` `.` `_` 一律当空格（快捷方式名「Ubuntu-24.04」与标题「Ubuntu 24.04」对齐）。
fn normalize_menu_keys(text: &str) -> Vec<String> {
    let mut outside = String::with_capacity(text.len());
    let mut inside = String::with_capacity(text.len());
    let mut depth = 0usize;
    for ch in text.chars() {
        match ch {
            '(' | '（' => depth += 1,
            ')' | '）' => depth = depth.saturating_sub(1),
            _ if depth > 0 => inside.push(ch),
            _ => outside.push(ch),
        }
    }

    let tidy = |raw: &str| -> String {
        raw.chars()
            .map(|ch| match ch {
                '-' | '_' | '.' | '·' => ' ',
                other => other,
            })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };

    let mut keys = vec![tidy(&outside)];
    let inner = tidy(&inside);
    if !inner.is_empty() {
        keys.push(inner);
    }
    keys.retain(|key| !key.is_empty());
    keys
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
    // 取 `Start Menu` 根目录（递归会覆盖 `Programs`），因为用户把快捷方式拖到
    // 开始菜单顶层的情况不少（本机 `Ubuntu-24.04.lnk` 就在那一层）
    let mut dirs = Vec::new();
    for base in ["APPDATA", "ProgramData"] {
        if let Some(root) = std::env::var_os(base) {
            dirs.push(PathBuf::from(root).join(r"Microsoft\Windows\Start Menu"));
        }
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
        } else if path.extension().is_some_and(|ext| {
            // `.lnk` 是普通快捷方式；`.url` 是 Steam 给每个游戏生成的 Internet
            // Shortcut（`IconFile=` 指向 Steam 缓存的游戏 ico），同样能取图
            ext.eq_ignore_ascii_case("lnk") || ext.eq_ignore_ascii_case("url")
        }) && let Some(stem) = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_lowercase())
        {
            map.entry(stem).or_insert(path);
        }
    }
}

/// 在当前**线程**初始化 COM（幂等）。
///
/// 不能改成进程级 `Once`：谁先调谁把 apartment 定死，后到的线程看到 Once 已完成
/// 就直接跳过，于是那个线程上的 SHELL 调用全静默失败（`GetImage` 返回空 → 图标变 chip）。
/// 图标提取现在跑在 [`super::Panel`] 的常驻 worker 线程上（2026-11-20），
/// 所以每个会碰 COM 的线程都得自己初始化一次。
///
/// 用 MTA：短命工作线程没有消息泵，STA 的跨 apartment 调用要靠消息循环才返回。
pub fn ensure_thread_com() {
    use std::cell::Cell;

    thread_local! {
        static DONE: Cell<bool> = const { Cell::new(false) };
    }

    DONE.with(|done| {
        if done.get() {
            return;
        }
        // 失败（已经初始化过别的模式）不算错：SHELL 接口在 MTA 下同样可用
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        done.set(true);
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

