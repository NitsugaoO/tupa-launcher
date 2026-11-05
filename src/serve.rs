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
use std::process::Command;
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

/// 一个在监听的进程
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listener {
    pub pid: u32,
    /// 可执行文件名（`node.exe`），读不到就是空串
    pub name: String,
}

/// 监听某个端口的进程（`0.0.0.0:4000` 与 `[::]:4000` 是同一个进程，已去重）。
///
/// 用 `netstat -ano -p tcp` + `tasklist` 而不是 `Get-NetTCPConnection`：后者要走
/// PowerShell 模块加载，冷启动几百毫秒，而 `:blog` 刷新时会探两个端口。
pub fn listeners(port: u16) -> Vec<Listener> {
    let pids = listen_pids(port);
    pids.into_iter()
        .map(|pid| Listener {
            pid,
            name: process_name(pid).unwrap_or_default(),
        })
        .collect()
}

/// `netstat` 里监听该端口的 PID（去重、去 0）。
pub fn listen_pids(port: u16) -> Vec<u32> {
    #[cfg(windows)]
    {
        let output = Command::new("netstat")
            .args(["-ano", "-p", "tcp"])
            .output();
        let Ok(output) = output else {
            return Vec::new();
        };
        parse_netstat_pids(&crate::util::decode_output(&output.stdout), port)
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        Vec::new()
    }
}

/// 从 `netstat -ano` 的输出里找监听 `port` 的 PID。
///
/// 行格式（列宽不定，按空白切）：
/// ```text
///   TCP    0.0.0.0:4000           0.0.0.0:0              LISTENING       28004
///   TCP    [::]:4000              [::]:0                 LISTENING       28004
/// ```
/// `LISTENING` 这列在中文系统上也是英文，可以直接比。
pub fn parse_netstat_pids(text: &str, port: u16) -> Vec<u32> {
    let suffix = format!(":{port}");
    let mut pids: Vec<u32> = Vec::new();

    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 {
            continue;
        }
        if !fields[0].eq_ignore_ascii_case("TCP") && !fields[0].eq_ignore_ascii_case("TCPv6") {
            continue;
        }
        // 本地地址必须是 :port 结尾（`ends_with` 不会把 :14000 当成 :4000）
        if !fields[1].ends_with(&suffix) {
            continue;
        }
        if !fields[3].eq_ignore_ascii_case("LISTENING") {
            continue;
        }
        if let Ok(pid) = fields[4].parse::<u32>()
            && pid != 0
            && !pids.contains(&pid)
        {
            pids.push(pid);
        }
    }

    pids
}

/// PID → 可执行文件名（`node.exe`）
pub fn process_name(pid: u32) -> Option<String> {
    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .ok()?;
        let text = crate::util::decode_output(&output.stdout);
        // "node.exe","28004","Console","1","44,000 K"
        let first = text.lines().next()?.trim();
        let name = first.split(',').next()?.trim().trim_matches('"');
        (!name.is_empty()).then(|| name.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}

/// 杀一个进程**和它的子进程**（`pnpm` 会拉起 node，只杀父进程会留下孤儿）。
pub fn kill_pid(pid: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill");
        command.args(["/PID", &pid.to_string(), "/T", "/F"]);
        crate::util::hide_window(&mut command);

        match command.output() {
            Ok(output) if output.status.success() => Ok(()),
            Ok(_) => Err(format!("taskkill {pid} failed")),
            Err(error) => Err(error.to_string()),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        Err("taskkill is Windows-only".to_string())
    }
}

/// 这个进程名算不算「我们自己起的本地服务」
///
/// 只认得 node / pnpm / cmd：端口可能被别的程序占着（4000 上有别的东西也一样会
/// 让 `is_listening` 为真），那种情况**不能顺手杀**。
pub fn is_our_service(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    matches!(lowered.as_str(), "node.exe" | "pnpm.exe" | "cmd.exe")
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

    #[test]
    fn parses_netstat_listeners() {
        let text = "\n\
  TCP    0.0.0.0:4000           0.0.0.0:0              LISTENING       28004\n\
  TCP    127.0.0.1:4000         127.0.0.1:5555         ESTABLISHED     28004\n\
  TCP    [::]:4000              [::]:0                 LISTENING       28004\n\
  TCP    0.0.0.0:14000          0.0.0.0:0              LISTENING       999\n\
  TCP    0.0.0.0:5173           0.0.0.0:0              LISTENING       1234\n";

        // 只留 LISTENING 的 4000，0.0.0.0 与 [::] 去重；ESTABLISHED / :14000 / 5173 都不算
        assert_eq!(parse_netstat_pids(text, 4000), vec![28004]);
        assert_eq!(parse_netstat_pids(text, 5173), vec![1234]);
        assert_eq!(parse_netstat_pids(text, 4001), Vec::<u32>::new());
    }

    #[test]
    fn rejects_garbage_netstat_output() {
        assert!(parse_netstat_pids("", 4000).is_empty());
        assert!(parse_netstat_pids("活动连接\n  协议  本地地址\n", 4000).is_empty());
        // PID 列不是数字（被别的本地化文本占据）
        assert!(parse_netstat_pids(
            "  TCP    0.0.0.0:4000   0.0.0.0:0   LISTENING   unknown",
            4000
        )
        .is_empty());
    }

    #[test]
    fn only_node_like_processes_are_ours() {
        assert!(is_our_service("node.exe"));
        assert!(is_our_service("NODE.EXE"));
        assert!(is_our_service("pnpm.exe"));
        assert!(!is_our_service("chrome.exe"));
        assert!(!is_our_service(""));
    }
}
