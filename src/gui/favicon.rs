//! 网页图标（favicon）：抓取 → 缓存 → 解码成 GPUI `RenderImage`。
//!
//! 只在 `[gui] web_icons = "favicon"`（默认）时启用；设 `globe` 就永远用 Nerd Font
//! 的地球字形，完全不联网。
//!
//! 取图顺序（命中就停）：
//! 1. `GET <站点>/favicon.ico` —— 大部分站点都有
//! 2. 没有（404 / 不是图片）→ 抓首页 HTML，从 `<link rel="...icon..." href>` 里找
//!    （**很多现代站点干脆没有 `/favicon.ico`**，实测 `pwn.college` 就是 404）
//!
//! 三个约定：
//! - **一定在后台线程调用**（`Panel::prefetch_icons` 里 spawn）：WinHTTP 是阻塞的，
//!   放渲染线程上会把面板卡住，慢站点能卡好几秒
//! - 缓存存**原始字节**（`icons/<站点>.bin`），不转码 —— ICO / PNG 都能被 `image`
//!   自动识别，省一次编码；下次启动只解码一次
//! - 拉不到就写一个空文件当「别重试」的标记，并回落成地球；否则每次呼出面板都要重试
//!
//! 用 WinHTTP 而不是引一个新 HTTP 库：它自带系统代理设置、证书校验，而 `windows`
//! 依赖本来就在（只是多开一个 `Win32_Networking_WinHttp` feature）。

use std::path::PathBuf;
use std::sync::Arc;

use gpui::RenderImage;
use image::{Frame, RgbaImage};

/// 一个网页的「站点坐标」：协议 + 主机 + 端口。
///
/// [`Site::key`] 是缓存文件名和查表的键：默认端口不带端口号，**非默认端口要带上** ——
/// 本机 `127.0.0.1:5173`（博客）和 `127.0.0.1:4000`（Hexo）是两个不同的服务。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub key: String,
    pub host: String,
    pub port: u16,
    pub secure: bool,
}

impl Site {
    /// 从 URL 解析。非 http(s)（`file:` / `steam:` / 裸路径）返回 `None`。
    pub fn from_url(url: &str) -> Option<Self> {
        let (scheme, rest) = url.split_once("://")?;
        let secure = match scheme.to_ascii_lowercase().as_str() {
            "https" => true,
            "http" => false,
            _ => return None,
        };
        let authority = rest.split(['/', '?', '#']).next()?;
        let authority = authority.rsplit('@').next()?; // 去掉 user:pass@
        let default_port = if secure { 443 } else { 80 };
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) if !port.is_empty() && port.chars().all(|ch| ch.is_ascii_digit()) => {
                (host.to_string(), port.parse().ok()?)
            }
            _ => (authority.to_string(), default_port),
        };
        if host.is_empty() || host.contains(' ') {
            return None;
        }
        let host = host.to_ascii_lowercase();
        let key = if port == default_port {
            host.clone()
        } else {
            format!("{host}:{port}")
        };
        Some(Site {
            key,
            host,
            port,
            secure,
        })
    }
}

/// 取某个站点的图标：先看磁盘缓存，没有就抓一次（同时写缓存）。**阻塞**。
pub fn icon_for(site: &Site) -> Option<Arc<RenderImage>> {
    let path = cache_path(&site.key)?;
    if let Ok(bytes) = std::fs::read(&path) {
        // 空文件 = 上次就失败了，别重试
        return if bytes.is_empty() {
            None
        } else {
            decode(&bytes)
        };
    }

    let bytes = fetch_icon(site);
    let _ = std::fs::write(&path, bytes.as_deref().unwrap_or_default());
    bytes.as_deref().and_then(decode)
}

/// `~/.config/tupa-launcher/icons/<站点>.bin`。站点名里的非法字符换掉。
fn cache_path(key: &str) -> Option<PathBuf> {
    let dir = crate::config::config_dir()?.join("icons");
    let _ = std::fs::create_dir_all(&dir);
    let name: String = key
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    Some(dir.join(format!("{name}.bin")))
}

/// 先试 `/favicon.ico`，404 就翻首页的 `<link rel="icon">`。
fn fetch_icon(site: &Site) -> Option<Vec<u8>> {
    if let Some(bytes) = http_get_path(site, "/favicon.ico")
        && decode(&bytes).is_some()
    {
        return Some(bytes);
    }

    let html = http_get_path(site, "/")?;
    let href = link_icon(&String::from_utf8_lossy(&html))?;
    let (target, path) = resolve_href(&href, site)?;
    let bytes = http_get_path(&target, &path)?;
    decode(&bytes).is_some().then_some(bytes)
}

/// 从 `<link ... rel="...icon..." ... href="...">` 里取 href。大小写不敏感。
fn link_icon(html: &str) -> Option<String> {
    for tag in html.split('<') {
        let lower = tag.to_ascii_lowercase();
        if !lower.starts_with("link") || !lower.contains("icon") {
            continue;
        }
        if let Some(href) = attr_value(tag, "href")
            && !href.trim().is_empty()
            && !href.trim_start().starts_with("data:")
        {
            return Some(href.trim().to_string());
        }
    }
    None
}

/// 取某个属性的值（`name="value"` 或 `name='value'`）。
fn attr_value(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let index = lower.find(name)?;
    let rest = tag[index + name.len()..].trim_start_matches(['=', ' ', '\t', '\n', '\r']);
    let rest = rest.strip_prefix('"').or_else(|| rest.strip_prefix('\''))?;
    let end = rest.find(['"', '\''])?;
    Some(rest[..end].to_string())
}

/// `href` 可能是 `/a.png`、`a.png`、`//cdn/a.png`、完整 URL —— 统一成「站点 + 路径」。
fn resolve_href(href: &str, site: &Site) -> Option<(Site, String)> {
    let href = href.trim();
    if let Some(rest) = href.strip_prefix("//") {
        let url = format!("{}://{rest}", if site.secure { "https" } else { "http" });
        return absolute(&url);
    }
    if href.starts_with("http://") || href.starts_with("https://") {
        return absolute(href);
    }
    let path = if href.starts_with('/') {
        href.to_string()
    } else {
        format!("/{href}")
    };
    Some((site.clone(), path))
}

fn absolute(url: &str) -> Option<(Site, String)> {
    let site = Site::from_url(url)?;
    let rest = url.split_once("://")?.1;
    let path = match rest.find('/') {
        Some(index) => rest[index..].to_string(),
        None => "/".to_string(),
    };
    Some((site, path.split(['?', '#']).next()?.to_string()))
}

/// ICO / PNG / JPEG → BGRA（非预乘）。GPUI 的 `RenderImage` 要的就是这个顺序。
fn decode(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let rgba = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let mut data = rgba.into_raw();
    for pixel in data.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2); // RGBA → BGRA（和 appicon 保持一致，不预乘）
    }
    let buffer = RgbaImage::from_raw(width, height, data)?;
    Some(Arc::new(RenderImage::new([Frame::new(buffer)])))
}

/// 带路径的 GET：只要 2xx 且非空。**阻塞**。
fn http_get_path(site: &Site, path: &str) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        winhttp_get(site, path)
    }
    #[cfg(not(windows))]
    {
        let _ = (site, path);
        None
    }
}

#[cfg(windows)]
fn winhttp_get(site: &Site, path: &str) -> Option<Vec<u8>> {
    use windows::Win32::Networking::WinHttp::*;
    use windows::core::PCWSTR;

    unsafe {
        let agent: Vec<u16> = "tupa-launcher".encode_utf16().chain(Some(0)).collect();
        let session = WinHttpOpen(
            PCWSTR(agent.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return None;
        }
        // 连不上就别拖：resolve / connect / send / receive 各 2 秒
        let _ = WinHttpSetTimeouts(session, 2000, 2000, 2000, 2000);

        let result = (|| {
            let host_w: Vec<u16> = site.host.encode_utf16().chain(Some(0)).collect();
            let connect = WinHttpConnect(session, PCWSTR(host_w.as_ptr()), site.port, 0);
            if connect.is_null() {
                return None;
            }
            let connection = Handle(connect);

            let verb: Vec<u16> = "GET".encode_utf16().chain(Some(0)).collect();
            let object: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            let flags = if site.secure {
                WINHTTP_FLAG_SECURE
            } else {
                WINHTTP_OPEN_REQUEST_FLAGS(0)
            };
            let request = WinHttpOpenRequest(
                *connection,
                PCWSTR(verb.as_ptr()),
                PCWSTR(object.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                flags,
            );
            if request.is_null() {
                return None;
            }
            let request = Handle(request);

            // 不少站点对空 UA 直接 403；带一个常见的浏览器 UA 最省事
            let headers: Vec<u16> = "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64)\r\n"
                .encode_utf16()
                .collect();
            if WinHttpSendRequest(*request, Some(&headers), None, 0, 0, 0).is_err() {
                return None;
            }
            if WinHttpReceiveResponse(*request, std::ptr::null_mut()).is_err() {
                return None;
            }

            // 状态码必须是 2xx（404 的 HTML 送进解码器只会白忙）
            let mut status: u32 = 0;
            let mut size = std::mem::size_of::<u32>() as u32;
            if WinHttpQueryHeaders(
                *request,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some(&mut status as *mut _ as *mut _),
                &mut size,
                std::ptr::null_mut(),
            )
            .is_err()
            {
                return None;
            }
            if !(200..300).contains(&status) {
                return None;
            }

            let mut body = Vec::new();
            loop {
                let mut chunk = [0u8; 8192];
                let mut read: u32 = 0;
                if WinHttpReadData(
                    *request,
                    chunk.as_mut_ptr() as *mut _,
                    chunk.len() as u32,
                    &mut read,
                )
                .is_err()
                {
                    return None;
                }
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..read as usize]);
                // 图标 / 首页都不该有 1MB；明显不对劲就放弃
                if body.len() > 1024 * 1024 {
                    return None;
                }
            }
            (!body.is_empty()).then_some(body)
        })();

        let _ = WinHttpCloseHandle(session);
        result
    }
}

/// `HINTERNET` 的 RAII 包装：提前 return 时也要关句柄（WinHTTP 的 handle 在
/// `windows` crate 里就是裸 `*mut c_void`，没有专门的类型）。
#[cfg(windows)]
struct Handle(*mut std::ffi::c_void);

#[cfg(windows)]
impl std::ops::Deref for Handle {
    type Target = *mut std::ffi::c_void;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(windows)]
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Networking::WinHttp::WinHttpCloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_is_taken_from_the_url() {
        let site = Site::from_url("https://pwn.college/program-security/x").unwrap();
        assert_eq!(site.key, "pwn.college");
        assert_eq!(site.port, 443);
        assert!(site.secure);

        // 本机服务：端口必须进 key，否则 5173 和 4000 会共用一个图标
        let blog = Site::from_url("http://127.0.0.1:5173/").unwrap();
        assert_eq!(blog.key, "127.0.0.1:5173");
        assert_eq!(blog.port, 5173);
        assert!(!blog.secure);

        let other = Site::from_url("http://127.0.0.1:4000/").unwrap();
        assert_ne!(blog.key, other.key);

        assert_eq!(
            Site::from_url("https://user:pass@example.com:8443/a")
                .unwrap()
                .key,
            "example.com:8443"
        );
        assert_eq!(Site::from_url("https://Example.COM").unwrap().key, "example.com");
        assert!(Site::from_url("steam://rungameid/1").is_none());
        assert!(Site::from_url("not a url").is_none());
    }

    #[test]
    fn link_icon_is_found_in_html() {
        let html = r#"<html><head><link rel="shortcut icon" href="/static/fav.png">
            <link rel="stylesheet" href="/x.css"></head></html>"#;
        assert_eq!(link_icon(html).as_deref(), Some("/static/fav.png"));

        let quoted = r#"<LINK REL='icon' HREF='/i.ico'>"#;
        assert_eq!(link_icon(quoted).as_deref(), Some("/i.ico"));

        // data: URI 不是能抓的东西
        let inline = r#"<link rel="icon" href="data:image/png;base64,AAAA">"#;
        assert_eq!(link_icon(inline), None);
    }

    #[test]
    fn hrefs_resolve_to_site_and_path() {
        let site = Site::from_url("https://example.com/a/b").unwrap();
        assert_eq!(
            resolve_href("/fav.png", &site).unwrap().1,
            "/fav.png".to_string()
        );
        assert_eq!(
            resolve_href("fav.png", &site).unwrap().1,
            "/fav.png".to_string()
        );
        // 协议相对 → 继承 scheme
        let (cdn, path) = resolve_href("//cdn.example.net/i.png", &site).unwrap();
        assert_eq!(cdn.key, "cdn.example.net");
        assert!(cdn.secure);
        assert_eq!(path, "/i.png");
        // 完整 URL
        let (other, path) = resolve_href("http://x.test/f.ico?v=2", &site).unwrap();
        assert_eq!(other.key, "x.test");
        assert_eq!(path, "/f.ico");
    }

    #[test]
    fn cache_names_are_filesystem_safe() {
        let path = cache_path("pwn.college").expect("缓存目录应该可用");
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("bin"));
        assert!(path.to_string_lossy().ends_with("pwn.college.bin"));

        // 端口里的冒号不能留在文件名里
        let path = cache_path("127.0.0.1:5173").expect("缓存目录应该可用");
        assert!(path.to_string_lossy().ends_with("127.0.0.1_5173.bin"));
    }

    /// 会联网，默认不跑：`cargo test --release --features gui -- --ignored`
    #[test]
    #[ignore = "需要联网"]
    fn winhttp_can_fetch_a_real_favicon() {
        let site = Site::from_url("https://github.com").unwrap();
        let bytes = http_get_path(&site, "/favicon.ico").expect("github 的 favicon.ico 应该能拉到");
        let icon = decode(&bytes).expect("ICO 应该能解码");
        assert!(icon.frame_count() > 0);
    }
}
