//! tupa-launcher —— 命令式启动器（TUI）
//!
//! 用法：
//! ```text
//! tupa-launcher                    启动 TUI
//! tupa-launcher --query "ev 报告"   不进界面，直接把结果打到 stdout（调试/脚本用）
//! tupa-launcher --help
//! ```

use tupa_launcher::{actions, app, config, exec, i18n, providers, theme, ui, util};
#[cfg(windows)]
use tupa_launcher::daemon;

use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};

use app::App;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // 语言要在任何输出之前确定（包括 --help）
    let loaded = config::Config::load();
    if let Ok(cfg) = &loaded {
        i18n::apply(cfg);
    }

    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return Ok(());
    }

    let cfg = loaded?;

    // 常驻守护进程：全局热键 + 面板窗口管理
    if args.iter().any(|a| a == "--daemon") {
        #[cfg(windows)]
        {
            let hotkey = daemon::parse_hotkey(&cfg.daemon.hotkey)?;
            return daemon::run(
                &hotkey,
                cfg.daemon.copilot_key,
                daemon::Shell {
                    panel: util::PanelKind::parse(&cfg.daemon.panel),
                    terminal: util::TerminalKind::resolve(&cfg.daemon.terminal),
                },
                args.iter().any(|a| a == "--debug"),
            );
        }
        #[cfg(not(windows))]
        anyhow::bail!("--daemon 目前只在 Windows 上实现");
    }

    // 自检：把配置里每个 app 能否解析出来列一遍
    if args.iter().any(|a| a == "--check") {
        return check_config(&cfg);
    }

    // 体检：配置里每个 app 的图标能不能拿到（需要 --features gui）
    if args.iter().any(|a| a == "--icons") {
        #[cfg(feature = "gui")]
        {
            if let Some(path) = config::config_path() {
                println!("{}", path.display());
            }
            println!();
            let report = tupa_launcher::gui::icon_report(&cfg.apps);
            let mut missing = 0;
            for (name, detail) in &report {
                if !detail.starts_with('✓') {
                    missing += 1;
                }
                println!("  {name:<28} {detail}");
            }
            println!();
            println!("{} / {} 有图标", report.len() - missing, report.len());
            return Ok(());
        }
        #[cfg(not(feature = "gui"))]
        anyhow::bail!("--icons 需要 `--features gui` 构建");
    }

    // 调试：看 .lnk / 命令会解析成什么（排查快捷键或路径问题）
    if let Some(pos) = args.iter().position(|a| a == "--resolve") {
        let target = args.get(pos + 1).cloned().unwrap_or_default();
        println!("输入 : {target}");
        println!("存在 : {}", std::path::Path::new(&target).is_file());
        match util::resolve_executable(&target) {
            Some(path) => println!("解析 : {}", path.display()),
            None => println!("解析 : 失败"),
        }
        return Ok(());
    }

    // 调试：直接执行一次“在资源管理器里定位”
    if let Some(pos) = args.iter().position(|a| a == "--reveal") {
        let target = args.get(pos + 1).cloned().unwrap_or_default();
        let path = std::path::PathBuf::from(&target);
        let message = exec::run(&providers::Action::Reveal(path))?;
        println!("{message}");
        return Ok(());
    }

    // 调试：直接执行一次开关写入（不启动界面）
    if let Some(pos) = args.iter().position(|a| a == "--apply") {
        let id = args.get(pos + 1).cloned().unwrap_or_default();
        let value = args.get(pos + 2).cloned().unwrap_or_default();
        let message = actions::apply(&id, &value)?;
        println!("{message}");
        return Ok(());
    }

    // 无界面模式：便于测试 provider，也方便以后被别的脚本调用
    if let Some(pos) = args.iter().position(|a| a == "--query") {
        let line = args.get(pos + 1).cloned().unwrap_or_default();
        return cli_query(&cfg, &line);
    }

    // 面板已经拆成独立程序（tupa-gui.exe），这里只提示一句
    if args.iter().any(|a| a == "--gui") {
        anyhow::bail!(i18n::gui_moved());
    }

    let theme = theme::Theme::load(&cfg);
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, cfg, theme);
    ratatui::restore();
    result
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    cfg: config::Config,
    theme: theme::Theme,
) -> Result<()> {
    let mut app = App::new(cfg, theme);
    app.refresh();

    // 面板隐藏时进程继续跑（不销毁 WT 窗口），下次热键显示就是瞬时的
    let mut hidden = false;

    while !app.should_quit {
        // 隐藏期间：不渲染、不轮询输入，只定期看看窗口是否又被唤起
        if hidden {
            std::thread::sleep(Duration::from_millis(200));
            if util::panel_visible(util::PanelKind::Tui) {
                hidden = false;
                terminal.clear()?;
            }
            continue;
        }

        app.tick = app.tick.wrapping_add(1);
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        // 需要滚动 / 还在等后台结果时 30ms 一帧；否则降到 150ms 省电
        let frame = if app.animate || app.pending { 30 } else { 150 };
        if event::poll(Duration::from_millis(frame))?
            && let Event::Key(key) = event::read()?
        {
            // 忽略 key release 事件，否则 Windows 上会重复触发
            if key.kind == KeyEventKind::Press {
                app.on_key(key);
            }
        }

        app.drain_events();

        if app.should_hide {
            app.should_hide = false;
            app.reset();
            util::hide_panel(util::PanelKind::Tui);
            hidden = true;
        }
    }

    Ok(())
}

fn check_config(cfg: &config::Config) -> Result<()> {
    if let Some(path) = config::config_path() {
        println!("{}", path.display());
    }
    println!();

    let mut missing = 0;
    for app in &cfg.apps {
        let command = app.command.as_str();
        let path = Path::new(command);

        let (mark, detail) = if let Some(found) = util::which(command) {
            ("✓", found.display().to_string())
        } else if path.is_file() {
            ("✓", path.display().to_string())
        } else if command.contains(':') {
            ("?", format!("shell: {command}"))
        } else {
            missing += 1;
            ("✗", command.to_string())
        };

        println!("  {mark} {:<28} {detail}", app.name);
    }

    println!();
    if missing == 0 {
        println!("{}", i18n::check_ok(cfg.apps.len()));
    } else {
        println!("{}", i18n::check_missing(missing));
    }
    Ok(())
}

fn cli_query(cfg: &config::Config, line: &str) -> Result<()> {
    let providers = providers::build(cfg);
    let infos = providers::infos(&providers);
    let default_idx = infos
        .iter()
        .position(|i| i.keyword == cfg.general.default_provider)
        .unwrap_or(0);

    let prefix = cfg.general.command_prefix.chars().next().unwrap_or(':');
    let (idx, flags, args) = match providers::parse(&infos, line, prefix) {
        providers::Parsed::Default => (default_idx, Vec::new(), line.to_string()),
        providers::Parsed::Command {
            provider,
            flags,
            args,
        } => (provider, flags, args),
        providers::Parsed::CommandList { query } => (
            providers::commands_index(&infos).unwrap_or(default_idx),
            Vec::new(),
            query,
        ),
    };
    let items = providers[idx].query(&args, &flags);

    println!(
        "provider={} args={:?} hits={}",
        infos[idx].keyword,
        args,
        items.len()
    );
    for item in items {
        let state = item.control.label();
        if state.is_empty() {
            println!("{:<5} {}", item.kind.badge(), item.columns.join("  |  "));
        } else {
            println!(
                "{:<5} {}  → {}",
                item.kind.badge(),
                item.columns.join("  |  "),
                state
            );
        }
    }
    Ok(())
}

fn print_help() {
    print!("{}", i18n::help());
}
