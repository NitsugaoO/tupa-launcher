//! 应用状态与事件处理

use std::sync::mpsc::{Receiver, Sender};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use crate::config::Config;
use crate::exec;
use crate::i18n;
use crate::providers::{
    self, Action, Cmd, Column, Control, DEFAULT_COLUMNS, Evt, Item, Parsed, ProviderInfo, commands,
};
use crate::theme::Theme;
use crate::{favorites, history};

/// 命令 / 修饰符的补全候选状态。
///
/// `start..end` 是要被替换的**字符**区间（光标前的那个词里、最后一个 `:` 之后的部分）。
#[derive(Debug, Clone)]
pub struct Completion {
    pub start: usize,
    pub end: usize,
    pub candidates: Vec<String>,
    /// 与 `candidates` 一一对应的说明（状态栏里显示）
    pub descriptions: Vec<String>,
    /// 已经 Tab 到第几个了（`None` = 还没开始循环）
    pub index: Option<usize>,
}

pub struct App {
    /// 输入行内容
    pub input: String,
    /// 光标位置（**字符**下标，不是字节）
    pub cursor: usize,
    /// 当前结果
    pub items: Vec<Item>,
    pub list_state: ListState,
    /// 所有 provider 的元信息
    pub infos: Vec<ProviderInfo>,
    /// 默认 provider（输入不带 `:` 前缀时用）
    default_idx: usize,
    /// 命令列表 provider 的下标
    commands_idx: Option<usize>,
    /// 当前生效的 provider
    pub active: usize,
    /// 命令前缀的结束位置（字节下标），用于高亮
    pub prefix_end: Option<usize>,
    /// 当前结果的列宽规格
    pub columns: &'static [Column],
    /// 传给当前 provider 的参数
    args: String,
    /// 关键字上的修饰符（`:run:admin` → `["admin"]`）
    flags: Vec<String>,
    pub theme: Theme,
    /// 状态栏消息：`(文本, 是否错误)`
    pub status: Option<(String, bool)>,
    pub should_quit: bool,
    /// 请求隐藏面板（不是退出）—— 进程继续跑，下次热键瞬时唤起
    pub should_hide: bool,
    /// 是否已经收到过第一批结果（用于避免启动瞬间闪出"列表为空"的提示）
    pub loaded: bool,
    /// 后台查询还没回来（状态栏显示一个省略号）
    pub pending: bool,
    /// 详细信息模式（Ctrl+Tab）：每列纵向铺开
    pub detail: bool,
    /// 选择模式（Shift+Tab）：Tab 循环、1-9 跳转
    pub select_mode: bool,
    /// 补全候选（有候选时在输入行下面多显示一行）
    pub completion: Option<Completion>,
    /// 输入法预编辑文本（只有 `--gui` 会填，TUI 里终端自己画）
    pub preedit: String,
    /// 每帧 +1，给选中行的左右滚动计步
    pub tick: u64,
    /// 是否显示右下角的快捷键提示
    pub show_hints: bool,
    /// 当前画面有没有需要持续重绘的东西（选中行超宽、要左右滚动）
    pub animate: bool,
    /// 开屏（以及 Esc 清空后）的输入内容：命令前缀或空串，由配置决定
    home: String,
    /// 命令前缀（`:` / `@` …），修饰符分隔符也用它
    prefix: char,
    /// 请求序号：只接受最新一次请求的结果，丢弃过期结果
    generation: u64,
    /// 收藏的内存快照（渲染时判断星标用）
    favorites: Vec<Item>,
    /// 命令历史，最新的在前
    history: Vec<String>,
    /// 历史浏览游标；`None` = 正在编辑新输入
    history_cursor: Option<usize>,
    /// 进入历史浏览前正在输入的内容，`Ctrl+↓` 回到它
    draft: String,
    /// 开关 / 选项 / 数值的写操作正在后台跑（防止连按回车重复提交）
    applying: bool,
    /// 刷新后要恢复的选中位置（切换开关后不要跳回第一项）
    pending_selection: Option<usize>,
    cmd_tx: Sender<Cmd>,
    evt_rx: Receiver<Evt>,
}

impl App {
    pub fn new(cfg: Config, theme: Theme) -> Self {
        let default_keyword = cfg.general.default_provider.clone();

        let providers = providers::build(&cfg);
        let infos = providers::infos(&providers);
        let default_idx = infos
            .iter()
            .position(|i| i.keyword == default_keyword)
            .unwrap_or(0);
        let commands_idx = providers::commands_index(&infos);

        let (cmd_tx, evt_rx) = providers::spawn_worker(providers);

        let prefix = cfg.general.command_prefix.chars().next().unwrap_or(':');

        // 开屏显示命令列表：预填前缀的话就直接在命令模式里
        let home = if cfg.general.prefill_colon {
            prefix.to_string()
        } else {
            String::new()
        };
        let cursor = home.chars().count();

        App {
            input: home.clone(),
            cursor,
            items: Vec::new(),
            list_state: ListState::default(),
            infos,
            default_idx,
            commands_idx,
            active: default_idx,
            prefix_end: None,
            columns: DEFAULT_COLUMNS,
            args: String::new(),
            flags: Vec::new(),
            theme,
            status: None,
            should_quit: false,
            should_hide: false,
            loaded: false,
            pending: false,
            detail: false,
            select_mode: false,
            completion: None,
            preedit: String::new(),
            tick: 0,
            show_hints: cfg.general.show_hints,
            animate: false,
            home,
            prefix,
            generation: 0,
            // 测试时不要读用户真实的收藏 / 历史，避免互相影响
            favorites: if cfg!(test) {
                Vec::new()
            } else {
                favorites::load()
            },
            history: if cfg!(test) {
                Vec::new()
            } else {
                history::load()
            },
            history_cursor: None,
            draft: String::new(),
            applying: false,
            pending_selection: None,
            cmd_tx,
            evt_rx,
        }
    }

    /// 重新解析输入并把查询丢给后台
    pub fn refresh(&mut self) {
        self.refresh_with(false);
    }

    /// 刷新并尽量留在原来选中的那一项（开关写入完成后用）
    pub fn refresh_keep_selection(&mut self) {
        self.refresh_with(true);
    }

    fn refresh_with(&mut self, keep_selection: bool) {
        let line = self.input.clone();
        let commands_idx = self.commands_idx;
        let default_idx = self.default_idx;

        let (idx, flags, args, is_command) = match providers::parse(&self.infos, &line, self.prefix)
        {
            Parsed::Default => (default_idx, Vec::new(), line.clone(), false),
            Parsed::Command {
                provider,
                flags,
                args,
            } => (provider, flags, args, true),
            Parsed::CommandList { query } => match commands_idx {
                Some(idx) => (idx, Vec::new(), query, true),
                None => (default_idx, Vec::new(), line.clone(), false),
            },
        };

        self.active = idx;
        self.args = args;
        self.flags = flags;
        self.prefix_end = if is_command {
            providers::prefix_end(&line, self.prefix)
        } else {
            None
        };
        self.pending_selection = if keep_selection {
            self.list_state.selected()
        } else {
            None
        };
        self.queue_query();
        self.update_completion();
    }

    fn queue_query(&mut self) {
        self.generation += 1;
        let _ = self.cmd_tx.send(Cmd::Query {
            generation: self.generation,
            provider: self.active,
            args: self.args.clone(),
            flags: self.flags.clone(),
        });
        self.status = None;
        self.pending = true;
    }

    /// 收取后台结果（非阻塞）
    pub fn drain_events(&mut self) {
        while let Ok(evt) = self.evt_rx.try_recv() {
            match evt {
                Evt::Results {
                    generation,
                    items,
                    columns,
                } => {
                    if generation != self.generation {
                        continue; // 过期结果
                    }
                    self.items = items;
                    self.columns = columns;
                    self.loaded = true;
                    self.pending = false;

                    // 切换开关后刷新时留在原位置，不要跳回第一条
                    let keep = self.pending_selection.take();
                    self.list_state = ListState::default();
                    if !self.items.is_empty() {
                        let index = keep.filter(|index| *index < self.items.len()).unwrap_or(0);
                        self.list_state.select(Some(index));
                    }
                }
                Evt::Applied { generation, result } => {
                    // 无论结果是否过期都要复位，否则以后按回车就再也发不出请求了
                    self.applying = false;
                    if generation != self.generation {
                        continue; // 等待期间输入变了，状态栏就不再提示
                    }
                    let (message, is_error) = match result {
                        Ok(message) => (message, false),
                        Err(error) => (error, true),
                    };
                    // 重新读一遍状态（queue_query 会清空 status，所以之后再写回）
                    self.refresh_keep_selection();
                    self.status = Some((message, is_error));
                }
            }
        }
    }

    pub fn active_description(&self) -> &str {
        self.infos
            .get(self.active)
            .map(|i| i.description.as_str())
            .unwrap_or("")
    }

    /// 当前生效的 provider 关键字
    fn active_keyword(&self) -> &str {
        self.infos
            .get(self.active)
            .map(|i| i.keyword.as_str())
            .unwrap_or("")
    }

    /// 这一条是否已收藏（渲染星标用）
    pub fn is_favorite(&self, item: &Item) -> bool {
        favorites::contains(&self.favorites, item)
    }

    /// 正在 Tab 循环补全时，当前候选的说明（状态栏显示）
    pub fn completion_description(&self) -> Option<String> {
        let completion = self.completion.as_ref()?;
        let index = completion.index?;
        let candidate = completion.candidates.get(index)?;
        let description = completion.descriptions.get(index)?;
        if description.is_empty() {
            return None;
        }
        Some(i18n::completion_description(candidate, description))
    }

    /// 结果为空时给出的提示
    pub fn empty_hint(&self) -> String {
        match self.infos.get(self.active) {
            Some(info) => match info.keyword.as_str() {
                ":" => i18n::hint_commands().to_string(),
                "ev" => i18n::hint_ev().to_string(),
                "app" => i18n::hint_app().to_string(),
                "switch" => i18n::hint_switch().to_string(),
                "pi" => i18n::hint_pi().to_string(),
                _ => info.description.clone(),
            },
            None => String::new(),
        }
    }

    /// 状态栏右下角的提示（选中控件时换一套按键说明）
    pub fn shortcut_hint(&self) -> &'static str {
        let is_control = self
            .list_state
            .selected()
            .and_then(|index| self.items.get(index))
            .is_some_and(|item| item.control.is_control());
        if is_control {
            i18n::shortcut_hint_control()
        } else {
            i18n::shortcut_hint()
        }
    }

    // -----------------------------------------------------------------
    // 键位
    // -----------------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        // 选择模式只认这几个键，其他键（输入、翻历史……）都退回普通模式
        if self.select_mode && !is_select_mode_key(key.code) {
            self.select_mode = false;
        }

        match key.code {
            KeyCode::Esc => {
                if self.select_mode {
                    self.select_mode = false;
                } else if self.at_home() {
                    // 已经在开屏状态：隐藏面板
                    self.should_hide = true;
                } else {
                    // 清空输入，回到开屏（命令列表）
                    self.set_home();
                }
            }
            KeyCode::Char('c') if ctrl => self.should_quit = true,
            KeyCode::Char('d') if ctrl => self.toggle_favorite(),
            KeyCode::Char('w') if ctrl => self.delete_word(),
            KeyCode::Char('o') if ctrl => self.toggle_detail(),
            KeyCode::Char('e') if ctrl => self.reveal_selected(),
            KeyCode::Char('p') if ctrl => self.move_selection(-1),
            KeyCode::Char('n') if ctrl => self.move_selection(1),
            KeyCode::Char(c @ '1'..='9') if self.select_mode => {
                self.move_selection(c as isize - '0' as isize);
            }
            KeyCode::Char(c) if !ctrl && !alt => self.insert_char(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete_forward(),
            KeyCode::Left => {
                // 开关 / 选项 / 数值：←→ 调值，不移动输入光标
                if !ctrl && !alt && self.adjust_control(-1) {
                    return;
                }
                self.cursor = self.cursor.saturating_sub(1);
                self.update_completion();
            }
            KeyCode::Right => {
                if !ctrl && !alt && self.adjust_control(1) {
                    return;
                }
                if self.cursor < self.char_len() {
                    self.cursor += 1;
                }
                self.update_completion();
            }
            KeyCode::Home => {
                self.cursor = 0;
                self.update_completion();
            }
            KeyCode::End => {
                self.cursor = self.char_len();
                self.update_completion();
            }
            KeyCode::Up if ctrl => self.history_prev(),
            KeyCode::Down if ctrl => self.history_next(),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down => self.move_selection(1),
            // Ctrl+Tab 在部分终端里会被识别成 BackTab + Ctrl
            KeyCode::BackTab if ctrl => self.toggle_detail(),
            KeyCode::Tab if ctrl => self.toggle_detail(),
            // Shift+Tab：进入 / 退出选择模式
            KeyCode::BackTab => self.toggle_select_mode(),
            KeyCode::Tab => {
                if self.select_mode {
                    self.move_selection(1);
                } else {
                    self.complete();
                }
            }
            KeyCode::Enter => {
                if ctrl || alt {
                    self.reveal_selected();
                } else {
                    self.select_mode = false;
                    // 控件行：回车应用新值，不隐藏面板
                    if !self.activate_control() {
                        self.execute();
                    }
                }
            }
            _ => {}
        }
    }

    fn execute(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };

        // 命令列表里的条目：只是把输入补成 `:命令 `
        if let Action::FillInput(text) = &item.action {
            let text = text.clone();
            self.set_input(text);
            return;
        }

        // 控件行不应该走到这里（回车会先走 activate_control），防御一下
        if item.control.is_control() {
            self.activate_control();
            return;
        }

        match exec::run(&item.action) {
            // 成功就隐藏面板，把屏幕让给刚打开的程序（进程留着，下次唤起更快）
            Ok(_) => {
                self.remember_input();
                self.should_hide = true;
            }
            Err(e) => self.status = Some((format!("{e:#}"), true)),
        }
    }

    /// 回车 / 空格：把控件的新值写出去（不隐藏面板）。
    ///
    /// 返回 `false` 表示当前选中的不是控件，调用方继续走普通执行。
    fn activate_control(&mut self) -> bool {
        let Some(index) = self.list_state.selected() else {
            return false;
        };
        let Some(item) = self.items.get(index).cloned() else {
            return false;
        };
        if !item.control.is_control() {
            return false;
        }

        let Action::SetControl { id, .. } = &item.action else {
            return false;
        };

        // 上一次写操作还没回来：忽略这次回车，避免重复执行
        if self.applying {
            return true;
        }

        let value = match &item.control {
            Control::Toggle { value } => {
                if *value {
                    "off".to_string()
                } else {
                    "on".to_string()
                }
            }
            Control::Choice { options, index } => match options.get(*index) {
                Some(option) => option.clone(),
                None => return true,
            },
            Control::Number { value, .. } => value.to_string(),
            Control::Unavailable => {
                self.status = Some((i18n::control_unavailable_hint().to_string(), true));
                return true;
            }
            Control::Run => return false,
        };

        let id = id.clone();
        self.applying = true;
        self.status = Some((i18n::control_applying(item.title()), false));
        let _ = self.cmd_tx.send(Cmd::Apply {
            generation: self.generation,
            action: Action::SetControl { id, value },
        });
        true
    }

    /// ←→：只改本地显示值，回车才真正写出去
    fn adjust_control(&mut self, delta: isize) -> bool {
        let Some(index) = self.list_state.selected() else {
            return false;
        };
        let Some(item) = self.items.get_mut(index) else {
            return false;
        };
        if !item.control.adjust(delta) {
            return false;
        }
        let title = item.title().to_string();
        let label = item.control.label();
        self.status = Some((i18n::control_value(&title, &label), false));
        true
    }

    /// 收藏 / 取消收藏当前选中项
    fn toggle_favorite(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };
        if matches!(item.action, Action::FillInput(_)) {
            self.status = Some((i18n::cannot_favorite_command().to_string(), false));
            return;
        }

        let result = favorites::toggle(&item);

        // 内存快照跟磁盘保持一致
        self.favorites = favorites::load();

        // 正在看 fav 列表时，删掉一条要刷新列表
        if self.active_keyword() == "fav" {
            self.queue_query();
        }

        self.status = Some(match result {
            Ok(true) => (i18n::favorited(item.title()), false),
            Ok(false) => (i18n::unfavorited(item.title()), false),
            Err(e) => (format!("{e:#}"), true),
        });
    }

    /// 详细信息模式：每列纵向铺开
    fn toggle_detail(&mut self) {
        self.detail = !self.detail;
        self.status = Some((
            if self.detail {
                i18n::detail_mode_on().to_string()
            } else {
                i18n::detail_mode_off().to_string()
            },
            false,
        ));
    }

    /// 选择模式：不用上下键也能移动指针
    fn toggle_select_mode(&mut self) {
        self.select_mode = !self.select_mode;
        if self.select_mode {
            self.status = Some((i18n::select_mode().to_string(), false));
        } else {
            self.status = None;
        }
    }

    // -----------------------------------------------------------------
    // 命令补全
    // -----------------------------------------------------------------

    fn update_completion(&mut self) {
        self.completion = self.compute_completion();
    }

    /// 光标前那个词里，若最后一个 `:` 后面还在输入，就给出候选：
    ///
    /// - `:ap` → 补全命令名
    /// - `:run:ad` → 补全修饰符
    /// - 已经有空格（进入参数区）→ 不补全
    fn compute_completion(&self) -> Option<Completion> {
        let chars: Vec<char> = self.input.chars().collect();
        if self.cursor == 0 || self.cursor > chars.len() {
            return None;
        }

        let mut start = self.cursor;
        while start > 0 && !chars[start - 1].is_whitespace() {
            start -= 1;
        }
        let token: String = chars[start..self.cursor].iter().collect();

        // 分隔符既可以是命令前缀，也可以是 `:`（和 providers::parse 保持一致）
        let colon = token.rfind([self.prefix, ':'])?;
        let separator = token[colon..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
        let typed = &token[colon + separator..];
        let typed_before = token[..colon].chars().count();
        let is_command = token[..colon].is_empty();

        let (candidates, descriptions) = if is_command {
            let candidates = providers::match_prefix(
                self.infos
                    .iter()
                    .map(|info| info.keyword.as_str())
                    .filter(|keyword| *keyword != commands::KEYWORD),
                typed,
            );
            let descriptions = candidates
                .iter()
                .map(|candidate| {
                    self.infos
                        .iter()
                        .find(|info| &info.keyword == candidate)
                        .map(|info| info.description.clone())
                        .unwrap_or_default()
                })
                .collect();
            (candidates, descriptions)
        } else {
            let candidates = providers::match_prefix(providers::KNOWN_FLAGS.iter().copied(), typed);
            let descriptions = candidates
                .iter()
                .map(|candidate| i18n::flag_description(candidate).to_string())
                .collect();
            (candidates, descriptions)
        };

        if candidates.is_empty() {
            return None;
        }

        Some(Completion {
            start: start + typed_before + 1,
            end: self.cursor,
            candidates,
            descriptions,
            index: None,
        })
    }

    /// Tab：在候选里循环，替换光标前那段文字
    fn complete(&mut self) {
        let Some(state) = self.completion.clone() else {
            return;
        };
        if state.candidates.is_empty() {
            return;
        }

        let next = match state.index {
            None => 0,
            Some(i) => (i + 1) % state.candidates.len(),
        };
        let candidate = state.candidates[next].clone();
        self.replace_chars(state.start, state.end, &candidate);

        let end = state.start + candidate.chars().count();
        self.refresh();
        // refresh 会重算候选（游标清空），这里把循环游标贴回去
        self.completion = Some(Completion {
            start: state.start,
            end,
            candidates: state.candidates,
            descriptions: state.descriptions,
            index: Some(next),
        });
    }

    // -----------------------------------------------------------------
    // 命令历史
    // -----------------------------------------------------------------

    /// 动作执行成功后把当前输入记进历史（失败的不记，免得污染历史）
    fn remember_input(&mut self) {
        let line = self.input.trim().to_string();
        self.history_cursor = None;
        self.draft.clear();
        if line.is_empty() {
            return;
        }
        history::push(&mut self.history, &line);
        if let Err(e) = history::save(&self.history) {
            eprintln!("保存历史失败：{e:#}");
        }
    }

    /// Ctrl+↑：上一条（更早的）输入
    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.history_cursor {
            None => {
                // 第一次进入历史浏览：先记住正在输入的内容
                self.draft = self.input.clone();
                0
            }
            Some(i) => (i + 1).min(self.history.len() - 1),
        };
        self.history_cursor = Some(next);
        let line = self.history[next].clone();
        self.set_input(line);
    }

    /// Ctrl+↓：下一条（更新的）输入，到底就回到原来的草稿
    fn history_next(&mut self) {
        match self.history_cursor {
            None => {}
            Some(0) => {
                self.history_cursor = None;
                let draft = std::mem::take(&mut self.draft);
                self.set_input(draft);
            }
            Some(i) => {
                self.history_cursor = Some(i - 1);
                let line = self.history[i - 1].clone();
                self.set_input(line);
            }
        }
    }

    /// 任何手动编辑都退出历史浏览模式
    fn leave_history(&mut self) {
        self.history_cursor = None;
        self.draft.clear();
    }

    /// 现在是不是停在开屏状态
    fn at_home(&self) -> bool {
        self.input == self.home
    }

    /// 回到开屏状态（`:` 或空串，取决于配置）
    fn set_home(&mut self) {
        self.input = self.home.clone();
        self.cursor = self.input.chars().count();
        self.leave_history();
        self.refresh();
    }

    /// 整体替换输入并把光标放到末尾
    fn set_input(&mut self, line: String) {
        self.input = line;
        self.cursor = self.input.chars().count();
        self.refresh();
    }

    fn reveal_selected(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };

        let target = match &item.action {
            Action::Open(path) | Action::Reveal(path) => path.clone(),
            // app 条目：解析出真实的 exe 位置（`.lnk` 会跟到目标）
            Action::Launch { command, .. } => match crate::util::resolve_executable(command) {
                Some(path) => path,
                None => {
                    self.status = Some((i18n::not_found(command), true));
                    return;
                }
            },
            Action::OpenUrl(_)
            | Action::LaunchInTerminal { .. }
            | Action::RunCommand { .. }
            | Action::OpenInEditor { .. }
            | Action::CreateFile(_)
            | Action::SetControl { .. }
            | Action::FillInput(_) => return,
        };

        match exec::run(&Action::Reveal(target)) {
            Ok(msg) => self.status = Some((msg, false)),
            Err(e) => self.status = Some((format!("{e:#}"), true)),
        }
    }

    fn selected_item(&self) -> Option<Item> {
        let idx = self.list_state.selected()?;
        self.items.get(idx).cloned()
    }

    /// 隐藏前把状态清干净，下次唤起时是初始界面
    pub fn reset(&mut self) {
        self.items.clear();
        self.preedit.clear();
        self.list_state = ListState::default();
        self.status = None;
        self.loaded = false;
        self.pending = false;
        self.select_mode = false;
        self.applying = false;
        self.pending_selection = None;
        self.set_home();
    }

    fn move_selection(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let len = self.items.len() as isize;
        let cur = self.list_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(len);
        self.list_state.select(Some(next as usize));
    }

    // -----------------------------------------------------------------
    // 输入编辑
    // -----------------------------------------------------------------

    fn char_len(&self) -> usize {
        self.input.chars().count()
    }

    /// 字符下标 → 字节下标（中文参数也能安全编辑）
    fn byte_index(&self, char_idx: usize) -> usize {
        self.input
            .char_indices()
            .nth(char_idx)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len())
    }

    /// 替换 `[start, end)` 这段字符，光标放到替换内容之后
    fn replace_chars(&mut self, start: usize, end: usize, text: &str) {
        let from = self.byte_index(start);
        let to = self.byte_index(end);
        self.input.replace_range(from..to, text);
        self.cursor = start + text.chars().count();
        self.leave_history();
    }

    fn insert_char(&mut self, c: char) {
        let at = self.byte_index(self.cursor);
        self.input.insert(at, c);
        self.cursor += 1;
        self.leave_history();
        self.refresh();
    }

    /// 光标处插入一段文本（输入法上屏用），只刷新一次
    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let at = self.byte_index(self.cursor);
        self.input.insert_str(at, text);
        self.cursor += text.chars().count();
        self.leave_history();
        self.refresh();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let from = self.byte_index(self.cursor - 1);
        let to = self.byte_index(self.cursor);
        self.input.replace_range(from..to, "");
        self.cursor -= 1;
        self.leave_history();
        self.refresh();
    }

    fn delete_forward(&mut self) {
        if self.cursor >= self.char_len() {
            return;
        }
        let from = self.byte_index(self.cursor);
        let to = self.byte_index(self.cursor + 1);
        self.input.replace_range(from..to, "");
        self.leave_history();
        self.refresh();
    }

    /// Ctrl+W：删掉光标前的一个词
    fn delete_word(&mut self) {
        let chars: Vec<char> = self.input.chars().collect();
        let mut i = self.cursor;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        let from = self.byte_index(i);
        let to = self.byte_index(self.cursor);
        self.input.replace_range(from..to, "");
        self.cursor = i;
        self.leave_history();
        self.refresh();
    }
}

/// 选择模式里保持模式不放的键
fn is_select_mode_key(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Esc
            | KeyCode::Enter
            | KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Char('1'..='9')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的 App：不动真实配置文件（历史在内存里替换掉）
    fn app() -> App {
        let mut app = App::new(Config::default(), Theme::default());
        app.history = vec!["app code".to_string(), "报告".to_string()];
        app
    }

    #[test]
    fn prefill_colon_setting() {
        let mut cfg = Config::default();
        cfg.general.prefill_colon = true;
        let mut app = App::new(cfg, Theme::default());

        assert_eq!(app.input, ":");
        app.refresh();
        assert_eq!(app.active_keyword(), ":");

        // 输入内容后按 Esc：回到 `:`，而不是隐藏
        app.insert_char('a');
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.input, ":");
        assert!(!app.should_hide);

        // 已经在 `:` 上再按 Esc 才隐藏
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.should_hide);
    }

    #[test]
    fn opens_with_empty_input_and_command_list() {
        let mut app = app();
        assert_eq!(app.input, "");
        assert_eq!(app.prefix_end, None);

        app.refresh();
        // 空输入 → 命令列表（`:` provider）
        assert_eq!(app.active_keyword(), ":");
    }

    #[test]
    fn history_walks_back_and_forth() {
        let mut app = app();
        app.set_input("go proj".to_string());

        app.history_prev(); // 最新的
        assert_eq!(app.input, "app code");
        app.history_prev(); // 更早的
        assert_eq!(app.input, "报告");
        app.history_prev(); // 到头了，停住
        assert_eq!(app.input, "报告");

        app.history_next();
        assert_eq!(app.input, "app code");
        app.history_next(); // 回到进入历史前正在输的内容
        assert_eq!(app.input, "go proj");
        assert_eq!(app.history_cursor, None);
    }

    #[test]
    fn editing_exits_history_mode() {
        let mut app = app();
        app.history_prev();
        assert!(app.history_cursor.is_some());

        app.insert_char('x');
        assert_eq!(app.history_cursor, None);
        assert_eq!(app.draft, "");

        // 重新浏览时从最新一条开始（替换掉刚编辑过的内容），而不是接着旧游标
        app.history_prev();
        assert_eq!(app.input, "app code");
        assert_eq!(app.history_cursor, Some(0));
    }

    #[test]
    fn completion_offers_commands_after_colon() {
        let mut app = app();
        app.set_input(":".to_string());
        let candidates = app
            .completion
            .as_ref()
            .map(|c| c.candidates.clone())
            .unwrap_or_default();
        assert!(candidates.iter().any(|c| c == "app"));

        app.set_input(":ap".to_string());
        let comp = app.completion.clone().expect("应该有补全候选");
        assert_eq!(comp.candidates.first().map(String::as_str), Some("app"));

        // Tab 之后输入变成 `:app`，并且可以继续循环
        app.complete();
        assert_eq!(app.input, ":app");
        assert!(app.completion.is_some());
    }

    #[test]
    fn no_completion_in_argument_position() {
        let mut app = app();
        app.set_input(":app code".to_string());
        assert!(app.completion.is_none());
    }

    /// 前缀是 `@` 的 App（用来测 `@app:ad` 这种写法）
    fn app_with_prefix(prefix: &str) -> App {
        let mut cfg = Config::default();
        cfg.general.command_prefix = prefix.to_string();
        App::new(cfg, Theme::default())
    }

    #[test]
    fn flags_complete_with_colon_separator() {
        let mut app = app_with_prefix("@");
        app.set_input("@app:ad".to_string());

        let comp = app.completion.clone().expect("@app:ad 应该有候选");
        assert_eq!(comp.candidates.first().map(String::as_str), Some("admin"));

        app.complete();
        assert_eq!(app.input, "@app:admin");
    }

    #[test]
    fn flags_complete_with_prefix_separator() {
        let mut app = app_with_prefix("@");
        app.set_input("@app@ad".to_string());

        let comp = app.completion.clone().expect("@app@ad 应该有候选");
        assert_eq!(comp.candidates.first().map(String::as_str), Some("admin"));

        app.complete();
        assert_eq!(app.input, "@app@admin");
    }

    #[test]
    fn flags_are_completed() {
        let mut app = app();
        app.set_input(":run:ad".to_string());
        let comp = app.completion.clone().expect("应该补全修饰符");
        assert_eq!(comp.candidates.first().map(String::as_str), Some("admin"));
    }

    /// 带控件的列表：id 故意不存在，测试里不会真的改系统设置
    fn app_with_controls() -> App {
        use crate::providers::{Action, Kind};

        let mut app = app();
        app.items = vec![
            Item::with_control(
                Kind::Switch,
                vec!["Dark mode".to_string(), "Appearance".to_string()],
                Action::SetControl {
                    id: "__test_toggle__".to_string(),
                    value: String::new(),
                },
                Control::Toggle { value: true },
            ),
            Item::with_control(
                Kind::Switch,
                vec!["Brightness".to_string(), "Display".to_string()],
                Action::SetControl {
                    id: "__test_number__".to_string(),
                    value: String::new(),
                },
                Control::Number {
                    value: 80,
                    min: 0,
                    max: 100,
                    step: 10,
                    suffix: "%".to_string(),
                },
            ),
        ];
        app.loaded = true;
        app.list_state.select(Some(0));
        app
    }

    #[test]
    fn enter_applies_control_without_hiding() {
        let mut app = app_with_controls();
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(!app.should_hide, "控件不应该隐藏面板");
        assert!(app.applying, "已经发出写请求");
    }

    #[test]
    fn arrows_adjust_control_instead_of_moving_cursor() {
        let mut app = app_with_controls();
        app.list_state.select(Some(1));

        let before = app.cursor;
        app.on_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.cursor, before, "调整数值时不应该移动光标");
        assert!(matches!(
            app.items[1].control,
            Control::Number { value: 90, .. }
        ));

        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(matches!(
            app.items[1].control,
            Control::Number { value: 80, .. }
        ));
    }

    #[test]
    fn unavailable_control_does_not_send_request() {
        let mut app = app_with_controls();
        app.items[0].control = Control::Unavailable;
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(!app.applying);
        assert!(app.status.as_ref().is_some_and(|(_, error)| *error));
        assert!(!app.should_hide);
    }
}
