//! 本地开发服务器的「端口探测 + 就绪等待」。
//!
//! `:blog` 的本地预览（`pnpm server` → 4000）和管理页（`pnpm admin` → 5173）
//! 都要先看服务在不在跑：在跑就只开浏览器，不在跑才起进程，并等端口真的监听上
//! 再开 —— 否则浏览器拿到的是「无法访问此网站」（或管理页白屏）。
//!
//! 这里只放纯逻辑（解析端口 / 探测 / 轮询），不负责起进程，也不碰浏览器：
//! 起进程要用 `exec` 里的 shell 选择与 `detached_command`，打开网页要配合
//! GlazeWM 切 workspace，都留在 [`crate::exec`]。

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// 单次连接探测的超时。本地回环，正常都是「立刻连上 / 立刻被拒」；
/// 给的短一点，因为 `:blog` 列表每次刷新都要探两个端口。
const PROBE_TIMEOUT: Duration = Duration::from_millis(150);

/// 等端口监听的默认上限。
///
/// 2026-11-05 实测：`pnpm server` 从 spawn 到 4000 监听 **5.2 秒**
/// （含 pnpm 自身启动开销与 Hexo 首次生成）。给 4 倍余量：正常情况 5 秒左右就返回，
/// 只有真的起不来时才会等满这个时间。
pub const READY_TIMEOUT: Duration = Duration::from_secs(20);

/// 轮询间隔
const POLL_INTERVAL: Duration = Duration::from_millis(150);

/// 从网址里取出端口：`http://localhost:4000` → `4000`。
///
/// 没写端口时按 scheme 猜（`https` → 443，其余 → 80）；拿不到 scheme 也当 http。
pub fn port_of(url: &str) -> Option<u16> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    // authority 到第一个 `/` `?` `#` 为止
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .rsplit('@') // 去掉可能的 user:pass@
        .next()
        .unwrap_or("");

    if authority.is_empty() {
        return None;
    }

    // IPv6 字面量：[::1]:4000
    if let Some(rest) = authority.strip_prefix('[') {
        let (_, tail) = rest.split_once(']')?;
        return match tail.strip_prefix(':') {
            Some(port) => port.parse().ok(),
            None => Some(default_port(url)),
        };
    }

    match authority.rsplit_once(':') {
        Some((_, port)) if !port.is_empty() => port.parse().ok(),
        _ => Some(default_port(url)),
    }
}

/// 网址没写端口时的默认端口
fn default_port(url: &str) -> u16 {
    if url.get(..6).is_some_and(|head| head.eq_ignore_ascii_case("https:")) {
        443
    } else {
        80
    }
}

/// 这个端口上有没有人在监听（先试 IPv4 回环，再试 IPv6）。
///
/// 两个都试是有原因的：Node 的 `server.listen(PORT)` 不指定 host 时可能只绑
/// `::1`，只探 `127.0.0.1` 会误判成「没在跑」，于是又起一个进程 → 端口占用。
pub fn is_listening(port: u16) -> bool {
    let v4 = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&v4, PROBE_TIMEOUT).is_ok()
        || TcpStream::connect_timeout(&v6, PROBE_TIMEOUT).is_ok()
}

/// 最多等 `timeout` 让端口监听上。已经在跑就立刻返回 `true`。
pub fn wait_until_ready(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if is_listening(port) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_explicit_ports() {
        assert_eq!(port_of("http://localhost:4000"), Some(4000));
        assert_eq!(port_of("http://127.0.0.1:5173/"), Some(5173));
        assert_eq!(port_of("https://example.com:8443/a/b?x=1"), Some(8443));
        assert_eq!(port_of("http://user:pass@localhost:6000"), Some(6000));
    }

    #[test]
    fn falls_back_to_scheme_default() {
        assert_eq!(port_of("http://example.com"), Some(80));
        assert_eq!(port_of("http://example.com/path"), Some(80));
        assert_eq!(port_of("https://example.com"), Some(443));
        // 没写 scheme 也当 http
        assert_eq!(port_of("example.com"), Some(80));
        assert_eq!(port_of("localhost:4000"), Some(4000));
    }

    #[test]
    fn reads_ipv6_literals() {
        assert_eq!(port_of("http://[::1]:4000"), Some(4000));
        assert_eq!(port_of("http://[::1]/"), Some(80));
    }

    #[test]
    fn rejects_unusable_input() {
        assert_eq!(port_of(""), None);
        assert_eq!(port_of("http://"), None);
        assert_eq!(port_of("http://localhost:port"), None);
    }

    #[test]
    fn closed_port_is_not_listening() {
        // 9 是 discard 端口，本机正常不会有人监听
        assert!(!is_listening(9), "9 号端口不该有人监听");
    }
}
