//! term-palette —— 命令式启动器（TUI）
//!
//! 用法：
//! ```text
//! term-palette                    启动 TUI
//! term-palette --query "ev 报告"   不进界面，直接把结果打到 stdout（调试/脚本用）
//! term-palette --help
//! ```

mod app;
mod config;
#[cfg(windows)]
mod daemon;
mod exec;
mod providers;
mod theme;
mod ui;
mod util;

use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};

use app::App;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return Ok(());
    }

    // 常驻守护进程：全局热键 + 面板窗口管理
    if args.iter().any(|a| a == "--daemon") {
        #[cfg(windows)]
        return daemon::run(args.iter().any(|a| a == "--debug"));
        #[cfg(not(windows))]
        anyhow::bail!("--daemon 目前只在 Windows 上实现");
    }

    let cfg = config::Config::load()?;

    // 自检：把配置里每个 app 能否解析出来列一遍
    if args.iter().any(|a| a == "--check") {
        return check_config(&cfg);
    }

    // 无界面模式：便于测试 provider，也方便以后被别的脚本调用
    if let Some(pos) = args.iter().position(|a| a == "--query") {
        let line = args.get(pos + 1).cloned().unwrap_or_default();
        return cli_query(&cfg, &line);
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

    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        // 最多等 30ms：输入跟手，后台结果也能及时刷出来
        if event::poll(Duration::from_millis(30))?
            && let Event::Key(key) = event::read()? {
                // 忽略 key release 事件，否则 Windows 上会重复触发
                if key.kind == KeyEventKind::Press {
                    app.on_key(key);
                }
            }

        app.drain_events();
    }

    Ok(())
}

fn check_config(cfg: &config::Config) -> Result<()> {
    if let Some(path) = config::config_path() {
        println!("配置文件：{}", path.display());
    }
    println!();

    let mut missing = 0;
    for app in &cfg.apps {
        let command = app.command.as_str();
        let path = Path::new(command);

        let (mark, detail) = if let Some(found) = util::which(command) {
            ("✓", found.display().to_string())
        } else if path.is_file() {
            ("✓", format!("文件 {}", path.display()))
        } else if command.contains(':') {
            ("?", format!("交给 shell 处理（协议/URI）：{command}"))
        } else {
            missing += 1;
            ("✗", format!("找不到：{command}"))
        };

        println!("  {mark} {:<28} {detail}", app.name);
    }

    println!();
    if missing == 0 {
        println!("{} 项全部可解析。", cfg.apps.len());
    } else {
        println!("{missing} 项无法解析，改一下 config.toml 里的 command。");
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

    let (idx, args) = providers::resolve(&infos, default_idx, line);
    let items = providers[idx].query(&args);

    println!("provider={} args={:?} hits={}", infos[idx].keyword, args, items.len());
    for item in items {
        println!("{:<5} {}\t{}", item.kind.badge(), item.title, item.subtitle);
    }
    Ok(())
}

fn print_help() {
    println!(
        "\
term-palette —— 命令式启动器

用法:
  term-palette                     启动 TUI
  term-palette --query \"ev 报告\"   不进界面，把结果打到 stdout
  term-palette --check             检查 config.toml 里每个 app 能否解析
  term-palette --daemon            常驻：监听 Win+S，唤起/隐藏面板
  term-palette --help              显示本帮助

输入语法:
  app <名字>      启动应用（列表在 config.toml 的 [[apps]] 里自定义）
  ev <关键词>     用 Everything 搜文件，支持 ext:pdf 等原生语法
  <名字>          不带关键字时走默认 provider（见 general.default_provider）

按键:
  ↑/↓ 或 ^P/^N    移动选择
  Enter           打开 / 启动
  ^R / Alt+Enter  在资源管理器里定位
  Tab             补全关键字
  ^W / ^U         删词 / 清空输入
  Esc             关闭面板

配置文件:
  {cfg}
  {theme}
",
        cfg = config::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        theme = config::config_dir()
            .map(|d| d.join("theme.toml").display().to_string())
            .unwrap_or_default(),
    );
}
