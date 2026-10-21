//! `run <命令>` —— 执行一条命令。
//!
//! - `run ipconfig`       → 开一个新终端窗口执行（能看到输出）
//! - `run:bg ipconfig`    → 后台执行（不弹窗，输出丢弃）
//! - `run:admin ipconfig` → 提权执行（会弹 UAC）

use super::{Action, Item, Kind, Provider};

pub struct Run;

impl Provider for Run {
    fn keyword(&self) -> &str {
        "run"
    }

    fn description(&self) -> &'static str {
        crate::i18n::desc_run()
    }

    fn query(&self, args: &str, flags: &[String]) -> Vec<Item> {
        let command = args.trim();
        if command.is_empty() {
            return Vec::new();
        }

        let elevated = flags.iter().any(|flag| flag == "admin");
        let background = flags.iter().any(|flag| flag == "bg");

        let note = crate::i18n::run_note(elevated, background);

        vec![Item::new(
            Kind::Command,
            vec![command.to_string(), note.to_string()],
            Action::RunCommand {
                command: command.to_string(),
                elevated,
                background,
            },
        )]
    }
}
