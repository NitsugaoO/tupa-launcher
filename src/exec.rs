//! 执行动作：打开文件、启动程序、定位路径

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::i18n;
use crate::providers::Action;
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
        Action::CreateFile(path) => {
            create_file(path)?;
            open::that_detached(path)
                .with_context(|| i18n::created_but_cannot_open(&path.display().to_string()))?;
            Ok(i18n::created(&short_name(path)))
        }
        Action::OpenInEditor { file, line } => {
            open_in_editor(file, *line)?;
            Ok(i18n::opened_in_editor(&short_name(file), *line))
        }
        // 开关 / 选项 / 数值：交给 actions.toml 里的命令
        Action::SetControl { id, value } => crate::actions::apply(id, value),
    }
}

/// 新建一个空文件（父目录不存在就建，已存在则报错、不覆盖）
fn create_file(path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| {
            i18n::cannot("创建目录", "create folder", &dir.display().to_string())
        })?;
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| i18n::cannot("新建", "create", &path.display().to_string()))?;
    Ok(())
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
fn spawn_console(command: &str, dir: Option<&Path>) -> Result<()> {
    let target_shell = shell();

    // 要先切目录的话，把 cd 拼进命令里（两种 shell 语法不同）
    let line = match dir {
        Some(dir) if target_shell == "cmd.exe" => {
            format!("cd /d \"{}\" && {command}", dir.display())
        }
        Some(dir) => format!("Set-Location -LiteralPath '{}'; {command}", dir.display()),
        None => command.to_string(),
    };

    if let Some(wt) = which("wt") {
        Command::new(wt)
            .arg(target_shell)
            .args(shell_args(&line, false))
            .spawn()
            .with_context(|| i18n::cannot("在新窗口执行", "run in a new window", command))?;
        return Ok(());
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

        Command::new(target_shell)
            .args(shell_args(&line, false))
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .with_context(|| i18n::cannot("在新窗口执行", "run in a new window", command))?;
    }
    #[cfg(not(windows))]
    {
        Command::new("sh")
            .arg("-c")
            .arg(&line)
            .spawn()
            .with_context(|| i18n::cannot("执行", "run", command))?;
    }

    Ok(())
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
fn open_in_editor(file: &Path, line: usize) -> Result<()> {
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
    fn create_file_does_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("tupa-new-test-{}", std::process::id()));
        let path = dir.join("hello.md");

        create_file(&path).expect("第一次应该能创建");
        assert!(path.is_file());
        assert!(create_file(&path).is_err(), "已存在时不应该覆盖");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
