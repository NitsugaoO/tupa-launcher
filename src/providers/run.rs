//! `run <命令>` —— 执行一条命令。
//!
//! - `run ipconfig`        → 后台执行（不弹窗，输出丢弃）
//! - `run:admin ipconfig`  → 提权执行（会弹 UAC）
//!
//! 想在看得见的终端里跑，可以自己套一层：`run wt cmd /k ipconfig`。

use super::{Action, Item, Kind, Provider};

pub struct Run;

impl Provider for Run {
    fn keyword(&self) -> &str {
        "run"
    }

    fn description(&self) -> &str {
        "执行命令（run:admin 提权执行）"
    }

    fn query(&self, args: &str, flags: &[String]) -> Vec<Item> {
        let command = args.trim();
        if command.is_empty() {
            return Vec::new();
        }

        let elevated = flags
            .iter()
            .any(|flag| matches!(flag.as_str(), "admin" | "elevated" | "sudo"));

        vec![Item {
            kind: Kind::Command,
            title: command.to_string(),
            subtitle: if elevated {
                "以管理员身份执行（会弹 UAC）".to_string()
            } else {
                "在后台执行".to_string()
            },
            action: Action::RunCommand {
                command: command.to_string(),
                elevated,
            },
        }]
    }
}
