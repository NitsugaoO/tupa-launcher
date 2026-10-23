//! 界面渲染

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::App;
use crate::i18n;
use crate::providers::{Column, Control, Item};
use crate::theme::Theme;

/// 选中标记（`▸ ` / 两个空格）
const PREFIX: u16 = 2;
/// 徽标列宽（app / file / web / cmd）
const BADGE: u16 = 4;
/// 收藏星标占 1 列
const STAR: u16 = 1;
/// 徽标与星标之间的间隔（避免 4 字宽徽标和星标黏在一起）
const BADGE_GAP: u16 = 1;
/// 星标与第一列之间的间隔
const STAR_GAP: u16 = 1;
/// 列之间的间隔
const GAP: u16 = 2;

pub fn draw(frame: &mut Frame, app: &mut App) {
    // Theme 是 Copy，先取一份，后面要可变借用 app
    let t = app.theme;

    // 铺满背景
    frame.render_widget(
        Block::new().style(Style::new().bg(t.background)),
        frame.area(),
    );

    // 有补全候选时，输入行下面多一行
    let completion_height = u16::from(app.completion.is_some());

    let chunks = Layout::vertical([
        Constraint::Length(1),                 // 输入行
        Constraint::Length(completion_height), // 补全候选
        Constraint::Length(1),                 // 分隔线
        Constraint::Min(1),                    // 结果列表
        Constraint::Length(1),                 // 状态栏
    ])
    .split(frame.area());

    draw_input(frame, app, chunks[0], t);
    if completion_height > 0 {
        draw_completion(frame, app, chunks[1], t);
    }
    draw_separator(frame, chunks[2], t);
    draw_list(frame, app, chunks[3], t);
    draw_status(frame, app, chunks[4], t);
}

fn draw_input(frame: &mut Frame, app: &App, area: Rect, t: Theme) {
    let mut spans = vec![Span::styled(" ❯ ", Style::new().fg(t.accent))];

    // 光标处拆成前后两段：输入法的预编辑文本插在中间
    let at = app
        .input
        .char_indices()
        .nth(app.cursor)
        .map(|(index, _)| index)
        .unwrap_or(app.input.len());
    let (before, after) = app.input.split_at(at);

    // 命令前缀（`:app` / `:run:admin`）高亮成黄色，其余保持正文色
    match app.prefix_end {
        Some(end) if end <= before.len() => {
            spans.push(Span::styled(
                &before[..end],
                Style::new().fg(t.warn).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(&before[end..], Style::new().fg(t.fg)));
        }
        Some(end) if end <= app.input.len() => {
            // 光标停在命令前缀中间（少见）：整段都按前缀画
            spans.push(Span::styled(
                before,
                Style::new().fg(t.warn).add_modifier(Modifier::BOLD),
            ));
        }
        _ => spans.push(Span::styled(before, Style::new().fg(t.fg))),
    }

    // 预编辑（拼音还没上屏）：带下划线，不参与查询
    if !app.preedit.is_empty() {
        spans.push(Span::styled(
            app.preedit.clone(),
            Style::new()
                .fg(t.accent)
                .add_modifier(Modifier::UNDERLINED),
        ));
    }

    spans.push(Span::styled(after, Style::new().fg(t.fg)));

    // 输入太长时把左边裁掉（跟随光标），否则光标会一直贴在右边缘
    let available = area.width.saturating_sub(PROMPT);
    let cursor_col = cursor_column(&app.input, app.cursor)
        .saturating_add(UnicodeWidthStr::width(app.preedit.as_str()) as u16);
    let skip = if cursor_col < available {
        0
    } else {
        cursor_col + 1 - available
    };
    let (spans, cut) = trim_left(spans, skip);

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(t.panel)),
        area,
    );

    // 把终端光标画到输入位置（" ❯ " 占 3 列）。
    // 注意要按**显示宽度**算：中文/全角字符占 2 列，按字符数算会偏左。
    let x = area.x + PROMPT + cursor_col.saturating_sub(cut);
    let max_x = area.right().saturating_sub(1);
    frame.set_cursor_position(Position::new(x.min(max_x), area.y));
}

/// 从左边裁掉若干显示列，返回（剩下的 spans, 实际裁掉了多少列）。
///
/// 宽字符跨不过去时整个保留（宁可多留一列，也不把汉字劈成半格）。
fn trim_left<'a>(spans: Vec<Span<'a>>, skip: u16) -> (Vec<Span<'a>>, u16) {
    if skip == 0 {
        return (spans, 0);
    }

    let mut out: Vec<Span<'a>> = Vec::new();
    let mut remaining = skip as usize;
    let mut cut = 0usize;

    for span in spans {
        if remaining == 0 {
            out.push(span);
            continue;
        }

        let mut kept = String::new();
        let mut skipped = 0usize;
        let mut done = false;
        for c in span.content.chars() {
            if !done {
                let w = UnicodeWidthChar::width(c).unwrap_or(0);
                if skipped + w <= remaining {
                    skipped += w;
                    continue;
                }
                done = true;
                remaining = 0; // 从这里开始全保留
            }
            kept.push(c);
        }

        remaining = remaining.saturating_sub(skipped);
        cut += skipped;
        if !kept.is_empty() {
            out.push(Span::styled(kept, span.style));
        }
    }

    (out, cut as u16)
}

/// 输入框提示符号的宽度（`" ❯ "`）
const PROMPT: u16 = 3;

/// 光标在输入内容里的显示列（到此前的字符宽度之和）
fn cursor_column(input: &str, cursor: usize) -> u16 {
    input
        .chars()
        .take(cursor)
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(0) as u16)
        .sum()
}

/// 补全候选行：`Tab  app  ev  run ...`（当前循环到的那个高亮）
fn draw_completion(frame: &mut Frame, app: &App, area: Rect, t: Theme) {
    let Some(completion) = &app.completion else {
        return;
    };

    let label = format!("   {} ", i18n::completion_label());
    let available = (area.width as usize).saturating_sub(UnicodeWidthStr::width(label.as_str()));
    let current = completion.index.unwrap_or(0);
    let (start, end) = completion_window(&completion.candidates, current, available);

    let mut spans = vec![Span::styled(label, Style::new().fg(t.faint))];
    for (offset, candidate) in completion.candidates[start..end].iter().enumerate() {
        let index = start + offset;
        if index > start {
            spans.push(Span::raw("  "));
        }
        let style = if Some(index) == completion.index {
            Style::new().fg(t.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(t.dim)
        };
        spans.push(Span::styled(candidate.clone(), style));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(t.panel)),
        area,
    );
}

/// 从候选里挑出一段能放进 `available` 列的窗口，并保证 `current` 在里面
fn completion_window(candidates: &[String], current: usize, available: usize) -> (usize, usize) {
    if candidates.is_empty() || available == 0 {
        return (0, 0);
    }
    let current = current.min(candidates.len() - 1);

    let mut start = current;
    let mut width = UnicodeWidthStr::width(candidates[current].as_str());

    // 往前放，直到放不下
    while start > 0 {
        let next = UnicodeWidthStr::width(candidates[start - 1].as_str()) + 2;
        if width + next > available {
            break;
        }
        width += next;
        start -= 1;
    }

    // 再往后放
    let mut end = current + 1;
    while end < candidates.len() {
        let next = UnicodeWidthStr::width(candidates[end].as_str()) + 2;
        if width + next > available {
            break;
        }
        width += next;
        end += 1;
    }

    (start, end)
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

    let selected = app.list_state.selected();
    let tick = app.tick;
    let detail = app.detail;
    let columns = app.columns;

    // 开关 / 选项 / 数值：最后额外留一列显示当前状态
    let state_width = state_column_width(&app.items);

    // 徽标 / 星标 / 固定间隔之后的宽度，留给数据列（扣除列间隔）
    let column_count = app
        .items
        .iter()
        .map(|item| item.columns.len())
        .max()
        .unwrap_or(0);
    let fixed =
        PREFIX + BADGE + BADGE_GAP + STAR + STAR_GAP + if detail { 0 } else { state_width };
    let usable = area.width.saturating_sub(fixed);
    let widths = if detail {
        // 详细信息：每行只有一列，所以每列都能用整行的宽度
        vec![usable; column_count]
    } else {
        let gaps = GAP * column_count.saturating_sub(1) as u16;
        layout_columns(columns, column_count, usable.saturating_sub(gaps))
    };

    let rows: Vec<ListItem> = app
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let is_selected = selected == Some(index);
            let star = app.is_favorite(item);
            ListItem::new(item_lines(
                item,
                &widths,
                is_selected,
                detail,
                star,
                tick,
                state_width,
                t,
            ))
        })
        .collect();

    // 只有「选中行真的有内容超出列宽」时才需要每帧重绘（滚动），
    // 否则主循环会把帧率降下来（见 main::run）
    app.animate = selected
        .and_then(|index| app.items.get(index))
        .map(|item| {
            let overflow = item.columns.iter().enumerate().any(|(index, text)| {
                let width = widths.get(index).copied().unwrap_or(0) as usize;
                UnicodeWidthStr::width(text.as_str()) > width
            });
            let detail_overflow = detail
                && item
                    .detail_line()
                    .map(|line| UnicodeWidthStr::width(line.as_str()) > usable as usize)
                    .unwrap_or(false);
            overflow || detail_overflow
        })
        .unwrap_or(false);

    // 不用 List 自带的 highlight_symbol：自己在行首画 `▸ `，
    // 这样详细信息模式的多行条目也能对齐
    let list = List::new(rows)
        .style(Style::new().bg(t.background))
        .highlight_style(Style::new().bg(t.overlay).add_modifier(Modifier::BOLD));

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

/// 把一个条目渲染成 1 行（紧凑）或 N 行（详细信息）
#[allow(clippy::too_many_arguments)]
fn item_lines(
    item: &Item,
    widths: &[u16],
    is_selected: bool,
    detail: bool,
    star: bool,
    tick: u64,
    state_width: u16,
    t: Theme,
) -> Vec<Line<'static>> {
    let unavailable = matches!(item.control, Control::Unavailable);
    let marker = if is_selected {
        Span::styled("▸ ", Style::new().fg(t.accent))
    } else {
        Span::raw("  ")
    };
    let badge = Span::styled(
        format!("{:<width$}", item.kind.badge(), width = BADGE as usize),
        Style::new().fg(t.dim),
    );
    let star = Span::styled(
        if star { "★" } else { " " },
        Style::new().fg(t.warn).add_modifier(Modifier::BOLD),
    );

    if !detail {
        let mut spans = vec![
            marker,
            badge,
            Span::raw(" ".repeat(BADGE_GAP as usize)),
            star,
            Span::raw(" ".repeat(STAR_GAP as usize)),
        ];
        for (index, text) in item.columns.iter().enumerate() {
            if index > 0 {
                spans.push(Span::raw(" ".repeat(GAP as usize)));
            }
            let style = if index == 0 && !unavailable {
                Style::new().fg(t.fg)
            } else {
                Style::new().fg(t.faint)
            };
            spans.extend(cell(
                text,
                widths.get(index).copied().unwrap_or(0),
                is_selected,
                true,
                tick,
                style,
            ));
        }
        if state_width > 0 {
            spans.extend(state_span(&item.control, state_width, t));
        }
        return vec![Line::from(spans)];
    }

    // 详细信息：第一行是徽标 + 第一列，其余列各自一行，缩进对齐到第一列
    let indent = Span::raw(" ".repeat((PREFIX + BADGE + BADGE_GAP + STAR + STAR_GAP) as usize));
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(item.columns.len().max(1));

    let mut first = vec![
        marker,
        badge,
        Span::raw(" ".repeat(BADGE_GAP as usize)),
        star,
        Span::raw(" ".repeat(STAR_GAP as usize)),
    ];
    first.extend(cell(
        item.title(),
        widths.first().copied().unwrap_or(0),
        is_selected,
        false,
        tick,
        Style::new().fg(if unavailable { t.faint } else { t.fg }),
    ));
    lines.push(Line::from(first));

    for (index, text) in item.columns.iter().enumerate().skip(1) {
        let mut spans = vec![indent.clone()];
        spans.extend(cell(
            text,
            widths.get(index).copied().unwrap_or(0),
            is_selected,
            false,
            tick,
            Style::new().fg(t.faint),
        ));
        lines.push(Line::from(spans));
    }

    // 最后一行：完整的网址 / 路径 / 命令行（列里已经有的就不重复了）
    if let Some(detail) = item.detail_line() {
        let mut spans = vec![indent.clone()];
        spans.extend(cell(
            &detail,
            widths.first().copied().unwrap_or(0),
            is_selected,
            false,
            tick,
            Style::new().fg(t.muted),
        ));
        lines.push(Line::from(spans));
    }

    // 控件：状态单独一行
    if state_width > 0 && item.control.is_control() {
        lines.push(Line::from(vec![
            indent,
            Span::styled(item.control.label(), state_style(&item.control, t)),
        ]));
    }
    lines
}

/// 状态列宽度（没有任何控件时为 0）。包含与前一列之间的间隔。
fn state_column_width(items: &[Item]) -> u16 {
    let widest = items
        .iter()
        .filter(|item| item.control.is_control())
        .map(|item| UnicodeWidthStr::width(item.control.label().as_str()))
        .max()
        .unwrap_or(0);
    if widest == 0 {
        0
    } else {
        (widest as u16).saturating_add(GAP).min(20)
    }
}

fn state_style(control: &Control, t: Theme) -> Style {
    match control {
        Control::Toggle { value: true } => Style::new().fg(t.good),
        Control::Toggle { value: false } => Style::new().fg(t.dim),
        Control::Choice { .. } | Control::Number { .. } => Style::new().fg(t.accent),
        Control::Unavailable => Style::new().fg(t.faint),
        Control::Run => Style::new(),
    }
}

/// 状态列：右对齐
fn state_span(control: &Control, width: u16, t: Theme) -> Vec<Span<'static>> {
    let label = control.label();
    let used = UnicodeWidthStr::width(label.as_str());
    let pad = (width as usize).saturating_sub(used);
    vec![
        Span::raw(" ".repeat(pad)),
        Span::styled(label, state_style(control, t)),
    ]
}

/// 一列的内容：放不下就截断（`ellipsis` 时补 `…`），选中时改成左右滚动
fn cell(
    text: &str,
    width: u16,
    is_selected: bool,
    ellipsis: bool,
    tick: u64,
    style: Style,
) -> Vec<Span<'static>> {
    let width = width as usize;
    if width == 0 {
        return Vec::new();
    }

    let total = UnicodeWidthStr::width(text);
    if total <= width {
        let pad = width - total;
        let mut spans = vec![Span::styled(text.to_string(), style)];
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        return spans;
    }

    // 选中：从 tick 推导滚动位置，把整段内容慢慢走一遍
    if is_selected {
        let offset = scroll_offset(total, width, tick);
        let (shown, used) = slice_by_width(text, offset, width);
        let mut spans = vec![Span::styled(shown, style)];
        if used < width {
            spans.push(Span::raw(" ".repeat(width - used)));
        }
        return spans;
    }

    // 不滚动：直接截断（详细信息模式下不补省略号，让窗口自己裁）
    if !ellipsis {
        let (shown, used) = slice_by_width(text, 0, width);
        let mut spans = vec![Span::styled(shown, style)];
        if used < width {
            spans.push(Span::raw(" ".repeat(width - used)));
        }
        return spans;
    }

    let (shown, used) = slice_by_width(text, 0, width.saturating_sub(1));
    let mut spans = vec![Span::styled(shown, style), Span::styled("…", style)];
    let pad = width.saturating_sub(used + 1);
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
    spans
}

/// 滚动偏移：每 2 帧走 1 列；从左到右 → 停一下 → 再从右到左 → 停一下，
/// 形成一个来回的周期（而不是从头"跳"回去）
fn scroll_offset(total: usize, width: usize, tick: u64) -> usize {
    if total <= width {
        return 0;
    }
    let over = (total - width) as u64;
    const STEP: u64 = 2; // 每 2 帧走 1 列
    const PAUSE: u64 = 16; // 每一端停 16 帧

    let travel = over * STEP;
    let cycle = (travel + PAUSE) * 2;
    let t = tick % cycle;

    if t < travel {
        (t / STEP) as usize
    } else if t < travel + PAUSE {
        over as usize
    } else if t < travel + PAUSE + travel {
        let back = (t - travel - PAUSE) / STEP;
        (over - back.min(over)) as usize
    } else {
        0
    }
}

/// 按**显示宽度**切一段文本，返回 `(文本, 实际宽度)`
fn slice_by_width(text: &str, offset: usize, width: usize) -> (String, usize) {
    let mut out = String::new();
    let mut skipped = 0usize;
    let mut used = 0usize;
    for c in text.chars() {
        let w = UnicodeWidthChar::width(c).unwrap_or(0);
        if skipped + w <= offset {
            skipped += w;
            continue;
        }
        if used + w > width {
            break;
        }
        out.push(c);
        used += w;
    }
    (out, used)
}

/// 按「份数 + 最短 + 最长」把可用宽度分给各列。
///
/// 1. 按 `weight` 比例分配整个可用宽度
/// 2. 夹到 `[min, max]`（`max = 0` 表示不限）
/// 3. 把夹完之后多出/少掉的宽度补给别人，保证总和刚好等于可用宽度
/// 4. 窄到连所有 `min` 都放不下时，按 `min` 的比例压缩
pub fn layout_columns(specs: &[Column], count: usize, available: u16) -> Vec<u16> {
    if count == 0 {
        return Vec::new();
    }
    if available == 0 {
        return vec![0; count];
    }

    let fallback = Column::new(2, 4, 0);
    let specs: Vec<Column> = (0..count)
        .map(|i| specs.get(i).copied().unwrap_or(fallback))
        .collect();

    let available = available as i64;
    let total_min: i64 = specs.iter().map(|spec| spec.min as i64).sum();

    // 太窄：按 min 的比例压缩
    if total_min >= available {
        let mut widths: Vec<u16> = specs
            .iter()
            .map(|spec| (spec.min as i64 * available / total_min.max(1)) as u16)
            .collect();
        let mut sum: i64 = widths.iter().map(|w| *w as i64).sum();
        let mut i = 0;
        while sum < available && i < widths.len() {
            widths[i] += 1;
            sum += 1;
            i += 1;
        }
        return widths;
    }

    // 1) 按份数把可用宽度分掉
    let total_weight: u32 = specs.iter().map(|spec| spec.weight.max(1) as u32).sum();
    let mut widths: Vec<u16> = specs
        .iter()
        .map(|spec| (available as u32 * spec.weight.max(1) as u32 / total_weight) as u16)
        .collect();
    let mut sum: i64 = widths.iter().map(|w| *w as i64).sum();
    let mut i = 0;
    while sum < available {
        widths[i % count] += 1;
        sum += 1;
        i += 1;
    }

    // 2) 最短 / 最长约束
    for (i, spec) in specs.iter().enumerate() {
        if (widths[i] as i64) < spec.min as i64 {
            widths[i] = spec.min;
        }
        if spec.max > 0 && widths[i] > spec.max {
            widths[i] = spec.max;
        }
    }
    sum = widths.iter().map(|w| *w as i64).sum();

    // 3a) 少了（被 max 卡掉）：轮流补给还有空间的列
    while sum < available {
        let mut grew = false;
        for (i, spec) in specs.iter().enumerate() {
            if spec.max == 0 || widths[i] < spec.max {
                widths[i] += 1;
                sum += 1;
                grew = true;
                if sum >= available {
                    break;
                }
            }
        }
        if !grew {
            break; // 所有列都到 max 了，剩下的留白
        }
    }

    // 3b) 多了（被 min 抬上去）：从最宽的、还能缩的列往下扣
    while sum > available {
        let mut widest: Option<usize> = None;
        for (i, spec) in specs.iter().enumerate() {
            if (widths[i] as i64) <= spec.min as i64 {
                continue;
            }
            widest = match widest {
                None => Some(i),
                Some(j) if widths[i] > widths[j] => Some(i),
                other => other,
            };
        }
        let Some(pick) = widest else {
            break;
        };
        widths[pick] -= 1;
        sum -= 1;
    }

    widths
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect, t: Theme) {
    // 提示的宽度按实际文案算：中文 54 列、英文 62 列，写死会截断英文
    let hint_width = if app.show_hints {
        UnicodeWidthStr::width(app.shortcut_hint()) as u16
    } else {
        0
    };
    let chunks =
        Layout::horizontal([Constraint::Min(10), Constraint::Length(hint_width)]).split(area);

    let left = match &app.status {
        Some((msg, is_err)) => Span::styled(
            format!(" {msg}"),
            Style::new().fg(if *is_err { t.bad } else { t.good }),
        ),
        // 正在 Tab 循环补全时，显示当前候选的说明
        None => match app.completion_description() {
            Some(description) => Span::styled(format!(" {description}"), Style::new().fg(t.accent)),
            None => {
                let position = app
                    .list_state
                    .selected()
                    .map(|index| index + 1)
                    .unwrap_or(0);
                Span::styled(
                    i18n::status_summary(
                        position,
                        app.items.len(),
                        app.active_description(),
                        app.detail,
                        app.pending,
                    ),
                    Style::new().fg(t.dim),
                )
            }
        },
    };
    frame.render_widget(
        Paragraph::new(Line::from(left)).style(Style::new().bg(t.panel)),
        chunks[0],
    );

    if app.show_hints {
        let right = Span::styled(app.shortcut_hint(), Style::new().fg(t.faint));
        frame.render_widget(
            Paragraph::new(Line::from(right).alignment(Alignment::Right))
                .style(Style::new().bg(t.panel)),
            chunks[1],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_follow_weights() {
        let specs = [Column::new(5, 12, 0), Column::new(3, 0, 0)];
        let widths = layout_columns(&specs, 2, 100);
        assert_eq!(widths, vec![63, 37]);
        assert_eq!(widths.iter().map(|w| *w as u32).sum::<u32>(), 100);
    }

    #[test]
    fn columns_respect_max() {
        let specs = [Column::new(1, 0, 10), Column::new(1, 0, 0)];
        let widths = layout_columns(&specs, 2, 100);
        assert_eq!(widths[0], 10);
        assert_eq!(widths[1], 90);
    }

    #[test]
    fn columns_shrink_when_narrow() {
        let specs = [Column::new(5, 12, 0), Column::new(3, 10, 0)];
        let widths = layout_columns(&specs, 2, 11);
        assert_eq!(widths.iter().map(|w| *w as u32).sum::<u32>(), 11);
    }

    #[test]
    fn slices_by_display_width() {
        // 中文按 2 列算：宽 4 只能放下「终端」
        let (text, used) = slice_by_width("终端 ab", 0, 4);
        assert_eq!(text, "终端");
        assert_eq!(used, 4);

        let (text, used) = slice_by_width("abcdef", 2, 3);
        assert_eq!(text, "cde");
        assert_eq!(used, 3);
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::config::Config;
    use crate::providers::{Action, Control, Item, Kind};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    // 测试数据全用 ASCII：TestBackend 里宽字符占两格，取文本时会掺进空格
    fn app_with_items() -> App {
        let mut app = App::new(Config::default(), Theme::default());
        app.items = vec![
            Item::new(
                Kind::App,
                vec!["Terminal".to_string(), "wt".to_string()],
                Action::RunCommand {
                    command: "wt".to_string(),
                    elevated: false,
                    background: false,
                },
            ),
            Item::new(
                Kind::App,
                vec!["Visual Studio Code".to_string(), "code".to_string()],
                Action::RunCommand {
                    command: "code".to_string(),
                    elevated: false,
                    background: false,
                },
            ),
        ];
        app.loaded = true;
        app.list_state.select(Some(0));
        app
    }

    fn screen(app: &mut App, width: u16, height: u16) -> Vec<String> {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|frame| draw(frame, app)).expect("draw");
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    }

    /// 子串所在的行内**列**（不是字节偏移：`▸` 占 3 字节）
    fn column_of(line: &str, needle: &str) -> Option<usize> {
        line.find(needle).map(|byte| line[..byte].chars().count())
    }

    #[test]
    fn compact_rows_put_columns_on_one_line() {
        let mut app = app_with_items();
        let lines = screen(&mut app, 80, 12);

        assert!(lines[2].contains("Terminal"), "{:?}", lines[2]);
        assert!(lines[2].contains("wt"), "{:?}", lines[2]);
        assert!(lines[3].contains("Visual Studio Code"), "{:?}", lines[3]);
        // 第二列在所有行里从同一列开始
        assert_eq!(column_of(&lines[2], "wt"), column_of(&lines[3], "code"));
    }

    #[test]
    fn detail_mode_spreads_columns_vertically() {
        let mut app = app_with_items();
        app.detail = true;
        let lines = screen(&mut app, 80, 12);

        // 第一项占两行：标题一行、副标题一行
        assert!(lines[2].contains("Terminal"), "{:?}", lines[2]);
        assert!(!lines[2].contains("wt"), "{:?}", lines[2]);
        assert!(lines[3].contains("wt"), "{:?}", lines[3]);
        assert!(!lines[3].contains("Terminal"), "{:?}", lines[3]);

        // 副标题对齐到标题列（「与标题列对齐」）
        assert_eq!(column_of(&lines[2], "Terminal"), column_of(&lines[3], "wt"));

        // 第二项的标题紧跟在第一项的副标题之后
        assert!(lines[4].contains("Visual Studio Code"), "{:?}", lines[4]);
    }

    #[test]
    fn cursor_column_counts_display_width() {
        assert_eq!(cursor_column("", 0), 0);
        assert_eq!(cursor_column("abc", 2), 2);
        assert_eq!(cursor_column("中文", 1), 2);
        assert_eq!(cursor_column("中文", 2), 4);
        assert_eq!(cursor_column("中a", 2), 3);
        // ":app 中文" 共 7 个字符：到「中」结束是 7 列，全宽是 9 列
        assert_eq!(cursor_column(":app 中文", 6), 7);
        assert_eq!(cursor_column(":app 中文", 7), 9);
    }

    #[test]
    fn trim_left_keeps_wide_chars_whole() {
        let spans = vec![Span::raw("ab".to_string()), Span::raw("中文".to_string())];
        let (out, cut) = trim_left(spans, 2);
        let text: String = out.iter().map(|span| span.content.to_string()).collect();
        assert_eq!(text, "中文");
        assert_eq!(cut, 2);

        // 3 列：先吃掉 "ab"，再遇到 2 列宽的「中」——跨不过去就整字保留
        let spans = vec![Span::raw("ab".to_string()), Span::raw("中文".to_string())];
        let (out, cut) = trim_left(spans, 3);
        let text: String = out.iter().map(|span| span.content.to_string()).collect();
        assert_eq!(text, "中文");
        assert_eq!(cut, 2);

        // 全部裁掉
        let spans = vec![Span::raw("abc".to_string())];
        let (out, cut) = trim_left(spans, 10);
        assert!(out.is_empty());
        assert_eq!(cut, 3);
    }

    #[test]
    fn scroll_bounces_back_and_forth() {
        // 文本 20 列、可见 10 列 → 要滚 10 列（每 2 帧走 1 列，两端各停 16 帧）
        assert_eq!(scroll_offset(20, 10, 0), 0);
        assert_eq!(scroll_offset(20, 10, 10), 5);
        assert_eq!(scroll_offset(20, 10, 20), 10); // 到底，开始停
        assert_eq!(scroll_offset(20, 10, 30), 10);
        assert_eq!(scroll_offset(20, 10, 40), 8); // 往回走
        assert_eq!(scroll_offset(20, 10, 50), 3);
        assert_eq!(scroll_offset(20, 10, 56), 0); // 回到开头，停一会儿
        assert_eq!(scroll_offset(20, 10, 72), 0); // 下一个周期
        assert_eq!(scroll_offset(5, 10, 40), 0); // 放得下就不滚
    }

    #[test]
    fn detail_mode_uses_full_width_without_ellipsis() {
        let mut app = app_with_items();
        app.items[0].columns = vec!["abcdefghij".repeat(20), "y".to_string()];
        app.detail = true;
        app.list_state.select(Some(1)); // 未选中 → 不滚动
        let lines = screen(&mut app, 60, 12);

        // 不补省略号，而是占满整行（60 列：前缀/徽标/星标/间隔 9 + 51）
        assert!(!lines[2].contains('…'), "{:?}", lines[2]);
        assert!(lines[2].contains(&"abcdefghij".repeat(5)), "{:?}", lines[2]);
        assert_eq!(lines[2].chars().count(), 60);
    }

    #[test]
    fn detail_mode_shows_full_url_for_web_items() {
        let mut app = App::new(Config::default(), Theme::default());
        app.items = vec![Item::new(
            Kind::Web,
            vec![
                "Example Domain".to_string(),
                "example.com".to_string(),
                "3 次".to_string(),
            ],
            Action::OpenUrl("https://example.com/some/long/path?a=1".to_string()),
        )];
        app.loaded = true;
        app.detail = true;
        app.list_state.select(Some(0));

        let lines = screen(&mut app, 90, 12);
        // 第一行标题、第二行域名、第三行访问次数、第四行完整 URL
        let url_row = lines
            .iter()
            .position(|line| line.contains("https://example.com/some/long/path?a=1"))
            .expect("详细信息里应该有完整 URL");
        // 缩进和标题列对齐
        assert_eq!(
            column_of(&lines[url_row], "https://example.com"),
            column_of(&lines[2], "Example Domain")
        );
    }

    #[test]
    fn completion_window_keeps_current_visible() {
        let candidates: Vec<String> = ["app", "ev", "run", "go", "web", "new", "fav", "cfg"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        // 全部放得下
        assert_eq!(completion_window(&candidates, 3, 200), (0, 8));

        // 只够放两项时，当前项必须在窗口里
        let (start, end) = completion_window(&candidates, 6, 10);
        assert!(start <= 6 && 6 < end, "window {start}..{end}");

        // 当前项是第一个时，窗口从 0 开始
        let (start, _) = completion_window(&candidates, 0, 10);
        assert_eq!(start, 0);

        assert_eq!(completion_window(&[], 0, 10), (0, 0));
    }

    #[test]
    fn status_shows_completion_description() {
        let mut cfg = Config::default();
        cfg.general.command_prefix = "@".to_string();
        let mut app = App::new(cfg, Theme::default());
        app.input = "@app:ad".to_string();
        app.cursor = app.input.chars().count();
        app.refresh();

        // 还没开始 Tab 循环 → 状态栏是摘要，不显示候选说明
        let before = screen(&mut app, 100, 12);
        assert!(!before[11].contains("admin"), "{:?}", before[11]);

        // 按一下 Tab，状态栏换成候选说明
        app.on_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        ));
        let after = screen(&mut app, 100, 12);
        assert!(after[11].contains("admin"), "{:?}", after[11]);
    }

    #[test]
    fn hints_can_be_hidden() {
        let mut cfg = Config::default();
        cfg.general.show_hints = false;
        let mut app = App::new(cfg, Theme::default());
        app.loaded = true;

        let lines = screen(&mut app, 100, 12);
        assert!(
            !lines[11].contains("Enter"),
            "提示应该被隐藏：{:?}",
            lines[11]
        );
    }

    #[test]
    fn completion_row_shows_flag_candidates() {
        let mut cfg = Config::default();
        cfg.general.command_prefix = "@".to_string();
        let mut app = App::new(cfg, Theme::default());
        app.input = "@app:ad".to_string();
        app.cursor = app.input.chars().count();
        app.refresh();

        let lines = screen(&mut app, 80, 12);
        // 输入行下面那行是补全候选
        assert!(lines[1].contains("admin"), "{:?}", lines[1]);
    }

    #[test]
    fn detail_line_skips_content_already_in_columns() {
        let item = Item::new(
            Kind::App,
            vec!["Terminal".to_string(), "wt".to_string()],
            Action::RunCommand {
                command: "wt".to_string(),
                elevated: false,
                background: false,
            },
        );
        assert_eq!(item.detail_line(), None);

        let item = Item::new(
            Kind::Web,
            vec!["Example".to_string(), "example.com".to_string()],
            Action::OpenUrl("https://example.com/a".to_string()),
        );
        assert_eq!(item.detail_line().as_deref(), Some("https://example.com/a"));
    }

    #[test]
    fn long_text_is_truncated_with_ellipsis() {
        let mut app = app_with_items();
        app.items[0].columns = vec!["abcdefghij".repeat(20), "y".to_string()];
        // 选中第二项 → 第一项不滚动，走省略号
        app.list_state.select(Some(1));
        let lines = screen(&mut app, 60, 12);
        assert!(lines[2].contains('…'), "{:?}", lines[2]);
    }

    #[test]
    fn selected_row_scrolls_instead_of_truncating() {
        let mut app = app_with_items();
        app.items[0].columns = vec!["abcdefghij".repeat(20), "y".to_string()];
        app.list_state.select(Some(0));

        let start = screen(&mut app, 60, 12);
        assert!(!start[2].contains('…'), "{:?}", start[2]);
        assert!(start[2].contains("abcdefghij"), "{:?}", start[2]);

        // tick 推进之后，开头那几个字符应该被滚走（30/2 = 偏移 15）
        app.tick = 30;
        let scrolled = screen(&mut app, 60, 12);
        assert_ne!(start[2], scrolled[2]);
        // 偏移 15 → 开头变成了 fghij…
        assert!(scrolled[2].contains("fghij"), "{:?}", scrolled[2]);
    }

    #[test]
    fn control_rows_show_state_column() {
        let mut app = App::new(Config::default(), Theme::default());
        app.items = vec![
            Item::with_control(
                Kind::Switch,
                vec!["Dark mode".to_string(), "Appearance".to_string()],
                Action::SetControl {
                    id: "dark".to_string(),
                    value: String::new(),
                },
                Control::Toggle { value: true },
            ),
            Item::with_control(
                Kind::Switch,
                vec!["Brightness".to_string(), "Display".to_string()],
                Action::SetControl {
                    id: "brightness".to_string(),
                    value: String::new(),
                },
                Control::Toggle { value: false },
            ),
        ];
        app.loaded = true;
        app.list_state.select(Some(0));

        let lines = screen(&mut app, 80, 12);
        assert!(lines[2].trim_end().ends_with('开'), "{:?}", lines[2]);
        assert!(lines[3].trim_end().ends_with('关'), "{:?}", lines[3]);
        // 状态列右对齐：两行的状态从同一列开始
        assert_eq!(column_of(&lines[2], "开"), column_of(&lines[3], "关"));

        // 状态栏换成控件专用的按键提示（宽字符在 TestBackend 里会被拆成两格，
        // 所以只断言 ASCII 部分）
        assert!(lines[11].contains("←→"), "{:?}", lines[11]);
        assert!(lines[11].contains("Enter"), "{:?}", lines[11]);
    }

    #[test]
    fn normal_rows_keep_the_open_hint() {
        let mut app = app_with_items();
        let lines = screen(&mut app, 100, 12);
        assert!(lines[11].contains("^E"), "{:?}", lines[11]);
        assert!(!lines[11].contains("←→"), "{:?}", lines[11]);
    }
}
