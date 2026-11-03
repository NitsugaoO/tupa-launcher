//! GlazeWM 集成：打开网页之后把浏览器窗口切到前台。
//!
//! ## 为什么需要
//!
//! `Action::OpenUrl` 只是 `open::that_detached`：已有浏览器实例会在**别的 workspace**
//! 里新建标签，而窗口本身仍是 `displayState=hidden`、当前 workspace 也不切，
//! 于是面板一隐藏，用户**什么都看不到**（`:web` 选中条目、`:blog` 三个入口、
//! 收藏里的网址全都走这条路，修一处全受益）。
//!
//! ## 做法
//!
//! `glazewm query workspaces`（一次查询就带 workspace 名 + children 的
//! id / handle / processName / hasFocus）→ 找到浏览器窗口 → 让 GlazeWM 自己切。
//!
//! 两条实测结论（照抄 [`pi-notify`](../../../../.pi/my-plugins/plugins/pi-notify) 的
//! `focus.runtime.mjs`，那边踩过同样的坑）：
//!
//! 1. **顺序不能反**：跨 workspace 直接 `focus --container-id` 会先把 WM 内部焦点
//!    改掉，之后 `focus --workspace` 成了 no-op，窗口还是不显示。所以先
//!    `--workspace <名字>`，等 WM 落焦点（约 300ms）复查，确实没落到目标窗口才补
//!    `--container-id`。
//! 2. **要复查**：切完系统可能把前台判回原窗口，GlazeWM 跟着 sync 回来，
//!    表现为「跳过去又弹回来」。所以隔 400ms 再查一次，跑偏就再拉一次。
//!
//! 没用 Win32 的 `SetForegroundWindow`：裸调用只改显示不改前台，GlazeWM 的
//! `isDisplayed` 不同步，之后 layout 会乱（要用也得像 `force-fg.ps1` 那样先
//! `AttachThreadInput`，而这里走 WM 自己的命令就够了）。
//!
//! 没有 GlazeWM、查询超时、找不到浏览器窗口 —— 一律静默放弃，保持原来的行为。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde::Deserialize;

use crate::util;

/// 认成「浏览器」的进程名（小写比较；`msedgewebview2`、`crashpad_handler` 不会命中）
const BROWSERS: &[&str] = &[
    "msedge",
    "chrome",
    "firefox",
    "brave",
    "vivaldi",
    "opera",
    "librewolf",
    "thorium",
    "zen",
    "arc",
];

/// 单条 `glazewm` 命令的超时（正常 30–80ms，卡住就别等）
const TIMEOUT: Duration = Duration::from_secs(4);
/// 刚 open 完让浏览器先处理一下，再开始找窗口
const FIRST_DELAY: Duration = Duration::from_millis(150);
/// 浏览器可能还在启动：没查到就隔一会儿再看
const RETRY_DELAY: Duration = Duration::from_millis(250);
const ATTEMPTS: usize = 3;
/// 切 workspace 之后等 WM 把焦点落到窗口上（pi-notify 实测约 250–300ms）
const SETTLE: Duration = Duration::from_millis(300);
/// 补刀之后再看一次，覆盖「跳过去又弹回来」的漂移
const VERIFY: Duration = Duration::from_millis(400);

// ---------------------------------------------------------------------------
// 数据结构（只取用得到的字段，其余忽略）
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Response {
    data: Data,
}

#[derive(Debug, Deserialize)]
struct Data {
    workspaces: Vec<Workspace>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub name: String,
    #[serde(rename = "hasFocus", default)]
    pub has_focus: bool,
    #[serde(default)]
    pub children: Vec<Window>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Window {
    pub id: String,
    #[serde(rename = "processName", default)]
    pub process_name: String,
    #[serde(rename = "hasFocus", default)]
    pub has_focus: bool,
}

/// 跳转目标
struct Target {
    workspace: String,
    id: String,
}

// ---------------------------------------------------------------------------
// 对外入口
// ---------------------------------------------------------------------------

/// `Action::OpenUrl` 之后的收尾：**立刻返回**，真正的跳转放到后台线程里做，
/// 这样面板能马上隐藏，不会卡在「切 workspace 之后面板还赖在屏幕上」。
///
/// 开关在 `config.toml` 的 `[glazewm] focus_browser`（默认开）；没装 GlazeWM、
/// 不是 Windows、查询失败都当无事发生。
pub fn focus_browser_async() {
    let enabled = crate::config::Config::load()
        .map(|cfg| cfg.glazewm.focus_browser)
        .unwrap_or(true);
    if !enabled {
        return;
    }
    std::thread::spawn(focus_browser);
}

/// 找到浏览器窗口并让它在前台。返回是否真的动过（`false` = 本来就在前台 / 没找到）。
pub fn focus_browser() -> bool {
    let Some(exe) = bin() else {
        return false;
    };

    std::thread::sleep(FIRST_DELAY);
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(RETRY_DELAY);
        }
        // 查不到状态就彻底放弃（WM 没跑 / IPC 坏了，重试也没用）
        let Some(workspaces) = query_workspaces(&exe) else {
            return false;
        };
        let Some((ws, window)) = pick_browser(&workspaces) else {
            continue; // 浏览器可能刚启动，再等等
        };
        if window.has_focus {
            return false; // 已经在前台，不用动
        }

        let target = Target {
            workspace: ws.name.clone(),
            id: window.id.clone(),
        };
        if is_current(&workspaces, &target) {
            return false;
        }
        activate(&exe, &target, &workspaces);
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// 挑选 / 判定（纯函数，方便测）
// ---------------------------------------------------------------------------

fn is_browser(process_name: &str) -> bool {
    let name = process_name.to_ascii_lowercase();
    BROWSERS.contains(&name.as_str())
}

/// 找浏览器窗口。多个时优先**有焦点**的那个（说明它已经在前台，调用方据此提前返回）。
pub fn pick_browser(workspaces: &[Workspace]) -> Option<(&Workspace, &Window)> {
    let mut fallback = None;
    for ws in workspaces {
        for window in &ws.children {
            if !is_browser(&window.process_name) {
                continue;
            }
            if window.has_focus {
                return Some((ws, window));
            }
            fallback.get_or_insert((ws, window));
        }
    }
    fallback
}

/// 当前工作区名 + 当前焦点窗口 id
fn current(workspaces: &[Workspace]) -> Option<(&str, &str)> {
    let ws = workspaces.iter().find(|ws| ws.has_focus)?;
    let window = ws.children.iter().find(|window| window.has_focus)?;
    Some((ws.name.as_str(), window.id.as_str()))
}

fn is_current(workspaces: &[Workspace], target: &Target) -> bool {
    matches!(
        current(workspaces),
        Some((ws, id)) if ws == target.workspace && id == target.id
    )
}

// ---------------------------------------------------------------------------
// 执行
// ---------------------------------------------------------------------------

fn activate(exe: &Path, target: &Target, before: &[Workspace]) {
    // 1) 先在目标 workspace 的另一侧：切 workspace（**不要**先切 container）
    let here = before
        .iter()
        .find(|ws| ws.has_focus)
        .map(|ws| ws.name.as_str());
    if here != Some(target.workspace.as_str()) {
        if !run(exe, &["command", "focus", "--workspace", &target.workspace]) {
            return;
        }
        std::thread::sleep(SETTLE);
        // WM 切过去通常已经把焦点落到原来的窗口上，这时不用再补刀（少一次重排闪烁）
        if query_workspaces(exe).is_some_and(|after| is_current(&after, target)) {
            return;
        }
    }

    // 2) 还在原地（或本来就同 workspace 但焦点在别的窗口）：按 container 切
    let _ = run(exe, &["command", "focus", "--container-id", &target.id]);

    // 3) 复查一次：系统可能把前台判回原窗口，跑偏就再拉一次
    std::thread::sleep(VERIFY);
    let drifted = query_workspaces(exe).is_some_and(|after| !is_current(&after, target));
    if drifted {
        let _ = run(exe, &["command", "focus", "--container-id", &target.id]);
    }}

/// `glazewm.exe` 的位置。
///
/// 守护进程是开机自启的快捷方式拉起来的，PATH 可能没有 scoop 的 shims
/// （pi-notify 那边也专门处理了这件事），所以 `where` 找不到时再探一次常见位置。
fn bin() -> Option<PathBuf> {
    if let Some(found) = util::which("glazewm") {
        return Some(found);
    }
    let shim = dirs::home_dir()?
        .join("scoop")
        .join("shims")
        .join("glazewm.exe");
    shim.is_file().then_some(shim)
}

fn query_workspaces(exe: &Path) -> Option<Vec<Workspace>> {
    let raw = run_output(exe, &["query", "workspaces"])?;
    parse_workspaces(&raw)
}

/// 解析 `glazewm query workspaces` 的输出
pub fn parse_workspaces(raw: &str) -> Option<Vec<Workspace>> {
    let response: Response = serde_json::from_str(raw).ok()?;
    Some(response.data.workspaces)
}

fn run(exe: &Path, args: &[&str]) -> bool {
    run_output(exe, args).is_some()
}

/// 跑一条命令拿 stdout（带超时；超时的子进程留在后台自生自灭，不影响面板）
fn run_output(exe: &Path, args: &[&str]) -> Option<String> {
    let exe = exe.to_path_buf();
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let mut cmd = Command::new(&exe);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        util::hide_window(&mut cmd);
        let _ = tx.send(cmd.output());
    });

    match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(out)) if out.status.success() => Some(util::decode_output(&out.stdout)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "clientMessage": "query workspaces",
      "data": {
        "workspaces": [
          {
            "name": "1",
            "hasFocus": false,
            "children": [
              {
                "type": "window",
                "id": "aaa",
                "handle": 721788,
                "title": "Changelog · Pi - Microsoft Edge",
                "processName": "msedge",
                "hasFocus": false,
                "displayState": "hidden"
              }
            ]
          },
          {
            "name": "3",
            "hasFocus": true,
            "children": [
              {
                "type": "window",
                "id": "bbb",
                "handle": 328490,
                "title": "π - scratch",
                "processName": "WindowsTerminal",
                "hasFocus": true,
                "displayState": "shown"
              }
            ]
          }
        ]
      }
    }"#;

    #[test]
    fn parses_workspaces() {
        let workspaces = parse_workspaces(SAMPLE).expect("应该能解析");
        assert_eq!(workspaces.len(), 2);
        assert_eq!(workspaces[0].name, "1");
        assert_eq!(workspaces[0].children[0].process_name, "msedge");
        assert!(workspaces[1].has_focus);
        assert_eq!(current(&workspaces), Some(("3", "bbb")));
    }

    #[test]
    fn picks_browser_over_terminal() {
        let workspaces = parse_workspaces(SAMPLE).unwrap();
        let (ws, window) = pick_browser(&workspaces).expect("应该找到 Edge");
        assert_eq!(ws.name, "1");
        assert_eq!(window.id, "aaa");
        assert!(!window.has_focus);
    }

    #[test]
    fn focused_browser_wins_over_other_candidates() {
        let raw = r#"{"data":{"workspaces":[
          {"name":"1","hasFocus":false,"children":[
            {"id":"first","processName":"chrome"},
            {"id":"second","processName":"msedge","hasFocus":true}]}
        ]}}"#;
        let workspaces = parse_workspaces(raw).unwrap();
        let (_, window) = pick_browser(&workspaces).unwrap();
        assert_eq!(window.id, "second");
    }

    #[test]
    fn ignores_non_browser_processes() {
        let raw = r#"{"data":{"workspaces":[
          {"name":"1","children":[
            {"id":"x","processName":"WindowsTerminal"},
            {"id":"y","processName":"msedgewebview2"},
            {"id":"z","processName":"Code"}]}
        ]}}"#;
        let workspaces = parse_workspaces(raw).unwrap();
        assert!(pick_browser(&workspaces).is_none());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_workspaces("not json").is_none());
        assert!(parse_workspaces(r#"{"data":{}}"#).is_none());
    }
}
