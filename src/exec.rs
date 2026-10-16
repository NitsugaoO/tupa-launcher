//! 执行动作：打开文件、启动程序、定位路径

use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::providers::Action;
use crate::util::{detached_command, which};

pub fn run(action: &Action) -> Result<String> {
    match action {
        Action::Open(path) => {
            open::that_detached(path).with_context(|| format!("无法打开 {}", path.display()))?;
            Ok(format!("已打开 {}", short_name(path)))
        }
        Action::Reveal(path) => {
            detached_command("explorer.exe")
                .arg(format!("/select,{}", path.display()))
                .spawn()
                .context("无法启动文件资源管理器")?;
            Ok(format!("已定位 {}", short_name(path)))
        }
        Action::Launch { command, args } => {
            launch(command, args)?;
            Ok(format!("已启动 {command}"))
        }
    }
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
    open::that_detached(command).with_context(|| format!("找不到可执行的 {command}"))?;
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
                .with_context(|| format!("无法启动 {}", path.display()))?;
        }
        // .cmd / .bat：必须由 cmd.exe 解释
        "cmd" | "bat" => {
            detached_command("cmd.exe")
                .arg("/C")
                .arg(path)
                .args(args)
                .spawn()
                .with_context(|| format!("无法启动 {}", path.display()))?;
        }
        // .lnk 快捷方式等：交给 shell 解析
        _ => {
            if args.is_empty() {
                open::that_detached(path)
                    .with_context(|| format!("无法打开 {}", path.display()))?;
            } else {
                bail!("{} 是快捷方式，暂不支持附加参数", path.display());
            }
        }
    }
    Ok(())
}

fn short_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}
