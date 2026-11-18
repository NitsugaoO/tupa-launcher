//! Steam 游戏图标：`steam://rungameid/<appid>` → 本地已经缓存好的封面。
//!
//! **不联网**：Steam 客户端自己会把封面写进磁盘，我们只是去捡。两个来源：
//!
//! 1. `<Steam>\userdata\<id>\config\grid\<appid>p.png` —— 用户自己设的竖版封面
//! 2. `<Steam>\appcache\librarycache\<appid>\` —— 客户端缓存，`library_600x900.jpg`
//!    是竖版海报，`header.jpg` / `library_header.jpg` 是横版（老游戏可能只有横版）
//!
//! `logo.png`（横向的文字 logo）**不用**：缩到 20 逻辑像素后只剩 5px 高，认不出。
//!
//! 裁切规则和 favicon 一致：居中取正方形再缩放 —— 20×20 的格子显示竖海报，
//! 取中间那块正好是画面的主体。

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use gpui::RenderImage;
use image::{Frame, RgbaImage};

use super::appicon::ICON_PX;

/// 从 `steam://rungameid/413150` 里取 appid。认不出的协议 / 非数字一律 `None`。
///
/// 允许尾部多余的分隔符：Steam 自己生成过 `.../413150//` 这种写法。
pub fn appid(command: &str) -> Option<u32> {
    let rest = command.strip_prefix("steam://rungameid/")?;
    rest.split(['/', '?', '#']).next()?.trim().parse().ok()
}

/// Steam 安装目录。
///
/// 只查 `HKCU\Software\Valve\Steam\SteamPath`（客户端自己在装的时候写的），
/// 一次 `OnceLock` 缓存：失败也缓存，别每行都去碰注册表。
pub fn root() -> Option<&'static Path> {
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ROOT.get_or_init(|| {
        let path = read_steam_path()?;
        path.is_dir().then_some(path)
    })
    .as_deref()
}

fn read_steam_path() -> Option<PathBuf> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::w;

    let mut buf = [0u16; 512];
    let mut size = std::mem::size_of_val(&buf) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Valve\\Steam"),
            w!("SteamPath"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status.is_err() {
        return None;
    }
    // size 含结尾的 NUL
    let len = (size as usize / 2).saturating_sub(1);
    let raw = String::from_utf16_lossy(&buf[..len]);
    let trimmed = raw.trim().trim_start_matches(r"\\?\");
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

/// 用户自定义的竖版封面（`config/grid/<appid>p.png`）。
fn user_grid_cover(appid: u32) -> Option<PathBuf> {
    let dir = root()?.join("userdata");
    // userdata 下每个用户一个纯数字目录；一般只有一个，多了就按目录顺序取第一个有的
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .chars()
            .all(|c| c.is_ascii_digit())
        {
            continue;
        }
        let path = entry
            .path()
            .join("config")
            .join("grid")
            .join(format!("{appid}p.png"));
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// 客户端缓存里的封面，竖版优先。
fn cache_cover(appid: u32) -> Option<PathBuf> {
    let dir = root()?
        .join("appcache")
        .join("librarycache")
        .join(appid.to_string());
    // 按前缀匹配：Steam 会按语言存成 `library_600x900_schinese.jpg` 这种
    ["library_600x900", "library_header", "header", "library_hero"]
        .into_iter()
        .find_map(|prefix| find_prefix(&dir, prefix))
}

/// 目录里第一个文件名以 `prefix` 开头（忽略大小写）的条目。
fn find_prefix(dir: &Path, prefix: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().to_lowercase().starts_with(prefix))
                .unwrap_or(false)
        })
}

/// 给一条 `steam://rungameid/...` 命令取封面。
///
/// 出错全部静默（`None`）：没有图就回落 mini chip，不能因为一个图标坏了整行。
pub fn load(command: &str) -> Option<Arc<RenderImage>> {
    let appid = appid(command)?;
    let path = user_grid_cover(appid).or_else(|| cache_cover(appid))?;
    render(&path)
}

fn render(path: &Path) -> Option<Arc<RenderImage>> {
    let image = image::open(path).ok()?;
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 {
        return None;
    }

    // 居中取正方形（竖海报 ≈ 600×900 → 600×600 的中段）
    let side = width.min(height);
    let left = (width - side) / 2;
    let top = (height - side) / 2;
    let square = image.crop_imm(left, top, side, side).resize_exact(
        ICON_PX as u32,
        ICON_PX as u32,
        image::imageops::FilterType::Triangle,
    );

    let mut data = square.to_rgba8().into_raw();
    for pixel in data.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2); // RGBA → BGRA，和 appicon / favicon 保持一致（不预乘）
    }
    let buffer = RgbaImage::from_raw(ICON_PX as u32, ICON_PX as u32, data)?;
    Some(Arc::new(RenderImage::new([Frame::new(buffer)])))
}

#[cfg(test)]
mod tests {
    use super::appid;

    #[test]
    fn parses_appid_from_rungameid() {
        assert_eq!(appid("steam://rungameid/413150"), Some(413150));
        // Steam 自己生成过带尾部斜杠的写法
        assert_eq!(appid("steam://rungameid/413150//"), Some(413150));
        assert_eq!(appid("steam://rungameid/504230/?foo=1"), Some(504230));
    }

    #[test]
    fn rejects_non_game_urls() {
        assert_eq!(appid("steam://store/123"), None);
        assert_eq!(appid("steam://rungameid/abc"), None);
        assert_eq!(appid("https://store.steampowered.com/app/413150"), None);
        assert_eq!(appid("C:\\Games\\Stardew Valley.exe"), None);
    }
}
