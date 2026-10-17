//! 界面渲染

use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::theme::Theme;

pub fn draw(frame: &mut Frame, app: &mut App) {
    // Theme 是 Copy，先取一份，后面要可变借用 app
    let t = app.theme;

    // 铺满背景
    frame.render_widget(
        Block::new().style(Style::new().bg(t.background)),
        frame.area(),
    );

    let chunks = Layout::vertical([
        Constraint::Length(1), // 输入行
        Constraint::Length(1), // 分隔线
        Constraint::Min(1),    // 结果列表
        Constraint::Length(1), // 状态栏
    ])
    .split(frame.area());

    draw_input(frame, app, chunks[0], t);
    draw_separator(frame, chunks[1], t);
    draw_list(frame, app, chunks[2], t);
    draw_status(frame, app, chunks[3], t);
}

fn draw_input(frame: &mut Frame, app: &App, area: Rect, t: Theme) {
    let mut spans = vec![Span::styled(" ❯ ", Style::new().fg(t.accent))];

    // 命中了 provider 时，把命令前缀（ev / run:admin）高亮成黄色，其余保持正文色
    match app.matched {
        Some(_) => {
            let end = app.input.find(' ').unwrap_or(app.input.len());
            spans.push(Span::styled(
                &app.input[..end],
                Style::new().fg(t.warn).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(&app.input[end..], Style::new().fg(t.fg)));
        }
        None => spans.push(Span::styled(app.input.as_str(), Style::new().fg(t.fg))),
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(t.panel)),
        area,
    );

    // 把终端光标画到输入位置（" ❯ " 占 3 列）
    let x = area.x + 3 + app.cursor as u16;
    let max_x = area.right().saturating_sub(1);
    frame.set_cursor_position(Position::new(x.min(max_x), area.y));
}

fn draw_separator(frame: &mut Frame, area: Rect, t: Theme) {
    let line = "─".repeat(area.width as usize);
    frame.render_widget(
        Paragraph::new(Span::styled(line, Style::new().fg(t.border))),
        area,
    );
}

fn draw_list(frame: &mut Frame, app: &mut App, area: Rect, t: Theme) {
    if app.items.is_empty() {
        // 启动瞬间还没拿到结果，先留白
        if !app.loaded {
            return;
        }
        let hint = app.empty_hint();
        let line = Line::from(vec![
            Span::raw("  "),
            Span::styled(hint, Style::new().fg(t.faint)),
        ]);
        frame.render_widget(
            Paragraph::new(line).style(Style::new().bg(t.background)),
            area,
        );
        return;
    }

    let rows: Vec<ListItem> = app
        .items
        .iter()
        .map(|item| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<4} ", item.kind.badge()), Style::new().fg(t.dim)),
                Span::styled(item.title.as_str(), Style::new().fg(t.fg)),
                Span::raw("  "),
                Span::styled(item.subtitle.as_str(), Style::new().fg(t.faint)),
            ]))
        })
        .collect();

    let list = List::new(rows)
        .style(Style::new().bg(t.background))
        .highlight_style(Style::new().bg(t.overlay).add_modifier(Modifier::BOLD))
        .highlight_symbol("▸ ");

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect, t: Theme) {
    let chunks = Layout::horizontal([Constraint::Min(10), Constraint::Length(36)]).split(area);

    let left = match &app.status {
        Some((msg, is_err)) => Span::styled(
            format!(" {msg}"),
            Style::new().fg(if *is_err { t.bad } else { t.good }),
        ),
        None => Span::styled(
            format!(" {} 条结果 · {}", app.items.len(), app.active_description()),
            Style::new().fg(t.dim),
        ),
    };
    frame.render_widget(
        Paragraph::new(Line::from(left)).style(Style::new().bg(t.panel)),
        chunks[0],
    );

    let right = Span::styled(
        "Enter 打开 · ^E 定位 · Esc 清空/隐藏 ",
        Style::new().fg(t.faint),
    );
    frame.render_widget(
        Paragraph::new(Line::from(right).alignment(Alignment::Right))
            .style(Style::new().bg(t.panel)),
        chunks[1],
    );
}
