//! 执行动作：打开文件、启动程序、定位路径
//!
//! 只放「进程 / 窗口 / shell」这一层的通用能力，加本地服务编排（serve_and_open /
//! serve_stop）和开关分发（SetControl）；各动作自己的业务实现在各自的模块里：
//! 文章创建在 [`crate::providers::blog`]。

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::i18n;
use crate::providers::Action;
use crate::serve;
use crate::util::{detached_command, which};

pub fn run(action: &Action) -> Result<String> {
    match action {
        Action::Open(path) => {
            // 目录：顺手记进 zoxide，这样终端的 `z` 命令也能用得上（共用同一份数据）
            if path.is_dir() {
                record_zoxide(path);
            }
            open::that_detached(path)
                .with_context(|| i18n::cannot("打开", "open", &path.display().to_string()))?;
            Ok(i18n::opened(&short_name(path)))
        }
        Action::FillInput(text) => Ok(i18n::filled(text)),
        Action::OpenUrl(url) => {
            open::that_detached(url).with_context(|| i18n::cannot("打开", "open", url))?;
            // 浏览器可能把标签开在别的 workspace 里，让 GlazeWM 把它切到前台
            // （后台线程，不卡面板隐藏）
            crate::glazewm::focus_browser_async();
            Ok(i18n::opened(url))
        }
        Action::Reveal(path) => {
            let mut cmd = detached_command("explorer.exe");

            // explorer 的 /select 要求**引号只包住路径**：
            //   explorer.exe /select,"C:\Program Files\Clash Party\Clash Party.exe"
            // 用 `arg()` 会生成 "/select,C:\..."（引号包了整个参数），explorer 解析不了，
            // 于是会随手打开一个默认文件夹（比如「文档」）。所以这里用 raw_arg 自己拼。
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.raw_arg(format!("/select,\"{}\"", path.display()));
            }
            #[cfg(not(windows))]
            cmd.arg(format!("/select,{}", path.display()));

            cmd.spawn().context(i18n::explorer_failed())?;
            Ok(i18n::revealed(&short_name(path)))
        }
        Action::Launch {
            command,
            args,
            elevated,
        } => {
            if *elevated {
                elevate(command, args)?;
            } else {
                launch(command, args)?;
            }
            Ok(i18n::launched(command))
        }
        Action::RunCommand {
            command,
            elevated,
            background,
        } => {
            // 后台用 `/C`（跑完就走）；可见窗口用 `/K`（跑完留在窗口里）
            let shell = shell();

            if *elevated {
                elevate(shell, &shell_args(command, *background))?;
            } else if *background {
                detach_script(shell, command)?;
            } else {
                spawn_console(command, None)?;
            }
            Ok(i18n::ran(command))
        }
        Action::LaunchInTerminal { command, dir } => {
            spawn_console(command, dir.as_deref())?;
            Ok(i18n::launched(command))
        }
        // 新建博客文章：写 front matter，然后用 VSCode 打开（实现在 providers/blog.rs）
        Action::CreateBlogPost { path, title } => {
            crate::providers::blog::create_post(path, title)?;
            Ok(i18n::blog_post_created(&short_name(path)))
        }
        // 博客管理页：已在跑就只开浏览器，没跑就后台启动再等就绪
        Action::BlogAdmin { dir, url } => serve_and_open(dir, "pnpm admin", url),
        // 博客本地预览：同上，命令换成 `pnpm server`
        Action::BlogPreview { dir, url } => serve_and_open(dir, "pnpm server", url),
        // 停掉静默后台的本地服务（没有窗口可关，只能从这里停）
        Action::ServeStop { port, name } => serve_stop(*port, name),
        Action::ServeStopAll { targets } => serve_stop_all(targets),
        Action::OpenInEditor { file, line } => {
            open_in_editor(file, *line)?;
            Ok(i18n::opened_in_editor(&short_name(file), *line))
        }
        // 开关 / 选项 / 数值：交给 actions.toml 里的命令
        Action::SetControl { id, value } => crate::actions::apply(id, value),
    }
}

/// 本地服务的统一处理：**端口在跑就只开浏览器，没跑先起进程、等就绪再开**。
///
/// 旧版 `blog_admin` 是无条件 `spawn pnpm admin` + 固定 `sleep 1.5s`：
/// 连点两次会起第二个 node（端口冲突后管理页白屏），起得慢时浏览器先到、页面还没好。
/// 现在换成「探测 → 按需启动 → 轮询端口」，两种情况都对：
///
/// - 已经在跑：立刻打开，不重复起进程
/// - 没在跑：`spawn` 后最多等 [`serve::READY_TIMEOUT`] 让端口监听上；超时返回错误
///   （面板留在原地显示红字，比「浏览器空白页 + 以为启动好了」好排查）
fn serve_and_open(dir: &Path, command: &str, url: &str) -> Result<String> {
    if !dir.is_dir() {
        bail!("{}", i18n::blog_dir_missing(&dir.display().to_string()));
    }

    // 解析不出端口（配置里写了个怪网址）就没法探测，退回「直接打开」
    let Some(port) = serve::port_of(url) else {
        open_and_focus(url);
        return Ok(i18n::opened(url));
    };

    if serve::is_listening(port) {
        open_and_focus(url);
        return Ok(i18n::serve_already_running(url));
    }

    detached_command(shell())
        .args(shell_args(command, true))
        .current_dir(dir)
        .spawn()
        .with_context(|| i18n::cannot("启动", "start", command))?;

    if !serve::wait_until_ready(port, serve::READY_TIMEOUT) {
        bail!("{}", i18n::serve_timeout(port, command, serve::READY_TIMEOUT.as_secs()));
    }

    open_and_focus(url);
    Ok(i18n::serve_started(url))
}

/// 打开网页，并让 GlazeWM 把浏览器窗口切前台（后台线程，不卡面板）
fn open_and_focus(url: &str) {
    let _ = open::that_detached(url);
    crate::glazewm::focus_browser_async();
}

/// 停掉占着某个端口的本地服务（`pnpm server` / `pnpm admin` 都是无窗口的后台进程）。
///
/// 两道保险：
///
/// 1. 只杀 node / pnpm / cmd —— 端口可能被别的程序占着（那就不是我们起的），
///    顺手杀掉别人的窗口/浏览器是灾难
/// 2. `taskkill /T` 带子进程：`pnpm` 会拉起 node，只杀父进程会留下孤儿占着端口
fn serve_stop(port: u16, name: &str) -> Result<String> {
    let listeners = serve::listeners(port);
    if listeners.is_empty() {
        return Ok(i18n::serve_stop_nothing(name));
    }

    let mut killed: Vec<u32> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    for listener in listeners {
        if !serve::is_our_service(&listener.name) {
            notes.push(i18n::serve_not_ours(&listener.name, listener.pid));
            continue;
        }
        match serve::kill_pid(listener.pid) {
            Ok(()) => killed.push(listener.pid),
            Err(error) => notes.push(i18n::serve_stop_failed(listener.pid, &error)),
        }
    }

    if killed.is_empty() {
        bail!("{}", notes.join(i18n::list_separator()));
    }

    let message = i18n::serve_stopped(name, &port_pids(&killed));
    if notes.is_empty() {
        Ok(message)
    } else {
        Ok(format!("{message}{}{}", i18n::list_separator(), notes.join(i18n::list_separator())))
    }
}

/// `:blog:stop` —— 一次停掉所有在跑的本地服务（失败不中断，最后汇总）
fn serve_stop_all(targets: &[crate::providers::ServeTarget]) -> Result<String> {
    if targets.is_empty() {
        return Ok(i18n::serve_stop_none().to_string());
    }

    let mut done = 0usize;
    let mut failed: Vec<String> = Vec::new();

    for target in targets {
        match serve_stop(target.port, &target.name) {
            Ok(_) => done += 1,
            Err(error) => failed.push(format!("{error:#}")),
        }
    }

    if failed.is_empty() {
        return Ok(i18n::serve_stopped_all(done));
    }

    let message = i18n::serve_stop_batch(done, &failed.join(i18n::list_separator()));
    if done == 0 {
        bail!("{message}");
    }
    Ok(message)
}

/// `[1234, 5678]` → `PID 1234, 5678`
fn port_pids(pids: &[u32]) -> String {
    let list: Vec<String> = pids.iter().map(u32::to_string).collect();
    i18n::serve_pids(&list.join(", "))
}

/// 优先 PowerShell 7（pwsh），退回 Windows PowerShell，再退回 cmd
fn shell() -> &'static str {
    if which("pwsh").is_some() {
        "pwsh"
    } else if which("powershell").is_some() {
        "powershell"
    } else {
        "cmd.exe"
    }
}

/// 给 shell 的参数：后台用 `-Command` / `/C`，可见窗口用 `-NoExit -Command` / `/K`
fn shell_args(command: &str, background: bool) -> Vec<String> {
    if shell() == "cmd.exe" {
        let switch = if background { "/C" } else { "/K" };
        vec![switch.to_string(), command.to_string()]
    } else if background {
        vec![
            "-NoProfile".to_string(),
            "-Command".to_string(),
            command.to_string(),
        ]
    } else {
        vec![
            "-NoExit".to_string(),
            "-Command".to_string(),
            command.to_string(),
        ]
    }
}

/// 后台执行一段命令（输出丢弃）
fn detach_script(target_shell: &str, command: &str) -> Result<()> {
    detached_command(target_shell)
        .args(shell_args(command, true))
        .spawn()
        .with_context(|| i18n::cannot("执行", "run", command))?;
    Ok(())
}

/// 开一个看得见的终端窗口执行命令（优先 Windows Terminal，没有就新开控制台）
///
/// ⚠ **PowerShell 脚本必须用 `-EncodedCommand` 传**：`wt` 把 `;` 当成它自己的命令
/// 分隔符（**在引号里也照切**），于是 `-Command "Set-Location x; pi --session y"` 会被
/// 切成两条，第二段 `" pi --session y"`（还带着前导空格）被当成可执行文件 →
/// `[出现错误 2147942402 (0x80070002) ... 系统找不到指定的文件]`。
/// base64 里没有 `;` 也没有空格，wt 就无从下刀。
///
/// 起始目录走 `wt -d <dir>`（不经 wt 时用 `current_dir`），命令里不再拼 `Set-Location`。
fn spawn_console(command: &str, dir: Option<&Path>) -> Result<()> {
    let target_shell = shell();

    // 非 Windows 只用来编译 / 跑测试，命令直接交给 sh
    #[cfg(not(windows))]
    {
        let line = match dir {
            Some(dir) => format!("cd '{}' && {command}", dir.display()),
            None => command.to_string(),
        };
        Command::new("sh")
            .arg("-c")
            .arg(&line)
            .spawn()
            .with_context(|| i18n::cannot("执行", "run", command))?;
        return Ok(());
    }

    // cmd.exe 没有 `-EncodedCommand`（`;` 在 cmd 里不是分隔符，但也躲不过 wt 的刀）
    let args: Vec<String> = if target_shell == "cmd.exe" {
        vec!["/K".to_string(), command.to_string()]
    } else {
        vec![
            "-NoExit".to_string(),
            "-EncodedCommand".to_string(),
            encoded_command(command),
        ]
    };

    if let Some(wt) = which("wt") {
        let mut cmd = Command::new(wt);
        if let Some(dir) = dir {
            cmd.arg("-d").arg(dir);
        }
        cmd.arg(target_shell)
            .args(&args)
            .spawn()
            .with_context(|| i18n::cannot("在新窗口执行", "run in a new window", command))?;
        return Ok(());
    }

    let mut cmd = Command::new(target_shell);
    cmd.args(&args);
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        cmd.creation_flags(CREATE_NEW_CONSOLE);
    }
    cmd.spawn()
        .with_context(|| i18n::cannot("在新窗口执行", "run in a new window", command))?;

    Ok(())
}

/// PowerShell 的 `-EncodedCommand`：**UTF-16LE** 字节的 base64
///
/// 5.1 和 7.x 都支持，而且编码后的参数里不含 `;` / 空格 / 引号，
/// 能安全地作为 `wt` 的 commandline 参数传下去。
fn encoded_command(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

/// 标准 base64（带 `=` 填充）。只为 `-EncodedCommand` 服务，不引额外依赖。
fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);

        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// 启动一个程序。分三种情况：
///
/// 1. 本身就是一个存在的文件 → 按扩展名决定怎么执行
/// 2. 是 PATH 里的命令（`code` / `wt` / `explorer`）→ 先 `where` 解析出真实路径，
///    再按扩展名执行
/// 3. 其他（`ms-settings:`、`steam://`、App Paths 里注册的程序）→ 交给 ShellExecute
fn launch(command: &str, args: &[String]) -> Result<()> {
    let path = Path::new(command);

    if path.is_file() {
        return spawn_executable(path, args);
    }

    if let Some(found) = which(command) {
        return spawn_executable(&found, args);
    }

    // 兜底：ShellExecute 能处理 URL 协议、ms-settings:、App Paths
    open::that_detached(command).with_context(|| i18n::not_found(command))?;
    Ok(())
}

/// 按扩展名选择执行方式
fn spawn_executable(path: &Path, args: &[String]) -> Result<()> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        // .exe / .com：直接 CreateProcess，不经过 shell
        "exe" | "com" => {
            detached_command(path)
                .args(args)
                .spawn()
                .with_context(|| i18n::cannot("启动", "start", &path.display().to_string()))?;
        }
        // .cmd / .bat：必须由 cmd.exe 解释
        "cmd" | "bat" => {
            detached_command("cmd.exe")
                .arg("/C")
                .arg(path)
                .args(args)
                .spawn()
                .with_context(|| i18n::cannot("启动", "start", &path.display().to_string()))?;
        }
        // .lnk 快捷方式等：交给 shell 解析
        _ => {
            if args.is_empty() {
                open::that_detached(path)
                    .with_context(|| i18n::cannot("打开", "open", &path.display().to_string()))?;
            } else {
                bail!("{}", i18n::shortcut_no_args(&path.display().to_string()));
            }
        }
    }
    Ok(())
}

/// 在编辑器里打开指定文件的某一行。
///
/// 优先用 VSCode 的 `code --goto file:line`；没有就退回系统默认程序。
pub(crate) fn open_in_editor(file: &Path, line: usize) -> Result<()> {
    let spec = format!("{}:{line}", file.display());

    if let Some(code) = which("code") {
        detached_command("cmd.exe")
            .arg("/C")
            .arg(&code)
            .arg("--goto")
            .arg(&spec)
            .spawn()
            .context(i18n::vscode_failed())?;
    } else {
        open::that_detached(file)
            .with_context(|| i18n::cannot("打开", "open", &file.display().to_string()))?;
    }
    Ok(())
}

/// 把目录记进 zoxide 的数据库（失败就算了，不影响打开）
fn record_zoxide(path: &Path) {
    let _ = detached_command("zoxide")
        .args(["add", &path.to_string_lossy()])
        .spawn();
}

/// 以管理员身份运行（ShellExecute 的 `runas`，会弹 UAC）
fn elevate(file: &str, args: &[String]) -> Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };

        let verb = wide("runas");
        let file_w = wide(file);
        let params = wide(&args.join(" "));

        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file_w.as_ptr(),
                params.as_ptr(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        } as isize;

        if result <= 32 {
            bail!("{}", i18n::elevate_failed(result));
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        bail!("{}", i18n::elevate_windows_only())
    }
}

fn short_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn encoded_command_is_utf16le() {
        // "echo" 的 UTF-16LE 字节是 65 00 63 00 68 00 6f 00
        assert_eq!(encoded_command("echo"), "ZQBjAGgAbwA=");
    }

    /// 回归：wt 把 `;` 当自己的分隔符，所以传给它的脚本里不能有 `;`
    #[test]
    fn encoded_command_has_no_semicolons() {
        let encoded = encoded_command("Set-Location 'C:\\a b'; pi --session 'x;y'");
        assert!(!encoded.contains(';'));
        assert!(!encoded.contains(' '));
        assert!(encoded
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')));
    }
}
