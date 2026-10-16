//! 应用状态与事件处理

use std::sync::mpsc::{Receiver, Sender};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use crate::config::Config;
use crate::exec;
use crate::providers::{self, Action, Cmd, Evt, Item, ProviderInfo};
use crate::theme::Theme;

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
    /// 默认 provider（输入不带关键字时用）
    default_idx: usize,
    /// 当前生效的 provider
    pub active: usize,
    /// 输入的第一个词是否命中了某个 provider（用于 Tab 补全）
    matched: Option<usize>,
    /// 传给当前 provider 的参数
    args: String,
    pub theme: Theme,
    /// 状态栏消息：`(文本, 是否错误)`
    pub status: Option<(String, bool)>,
    pub should_quit: bool,
    /// 是否已经收到过第一批结果（用于避免启动瞬间闪出"列表为空"的提示）
    pub loaded: bool,
    /// 请求序号：只接受最新一次请求的结果，丢弃过期结果
    generation: u64,
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

        let (cmd_tx, evt_rx) = providers::spawn_worker(providers);

        App {
            input: String::new(),
            cursor: 0,
            items: Vec::new(),
            list_state: ListState::default(),
            infos,
            default_idx,
            active: default_idx,
            matched: None,
            args: String::new(),
            theme,
            status: None,
            should_quit: false,
            loaded: false,
            generation: 0,
            cmd_tx,
            evt_rx,
        }
    }

    /// 重新解析输入并把查询丢给后台
    pub fn refresh(&mut self) {
        let line = self.input.clone();
        let head = match line.split_once(' ') {
            Some((h, _)) => h,
            None => line.as_str(),
        };
        self.matched = if head.is_empty() {
            None
        } else {
            self.infos.iter().position(|i| i.matches(head))
        };

        let (idx, args) = providers::resolve(&self.infos, self.default_idx, &line);
        self.active = idx;
        self.args = args;
        self.queue_query();
    }

    fn queue_query(&mut self) {
        self.generation += 1;
        let _ = self.cmd_tx.send(Cmd::Query {
            generation: self.generation,
            provider: self.active,
            args: self.args.clone(),
        });
        self.status = None;
    }

    /// 收取后台结果（非阻塞）
    pub fn drain_events(&mut self) {
        while let Ok(evt) = self.evt_rx.try_recv() {
            let Evt::Results { generation, items } = evt;
            if generation != self.generation {
                continue; // 过期结果
            }
            self.items = items;
            self.loaded = true;
            self.list_state = ListState::default();
            if !self.items.is_empty() {
                self.list_state.select(Some(0));
            }
        }
    }

    pub fn active_description(&self) -> &str {
        self.infos
            .get(self.active)
            .map(|i| i.description.as_str())
            .unwrap_or("")
    }

    /// 结果为空时给出的提示
    pub fn empty_hint(&self) -> String {
        match self.infos.get(self.active) {
            Some(info) => match info.keyword.as_str() {
                "ev" => "输入文件名开始搜索（支持 Everything 语法，如 ext:pdf）".to_string(),
                "app" => "应用列表为空，去 config.toml 的 [[apps]] 里添加".to_string(),
                _ => info.description.clone(),
            },
            None => String::new(),
        }
    }

    // -----------------------------------------------------------------
    // 键位
    // -----------------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        match key.code {
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('c') if ctrl => self.should_quit = true,
            KeyCode::Char('u') if ctrl => {
                self.input.clear();
                self.cursor = 0;
                self.refresh();
            }
            KeyCode::Char('w') if ctrl => self.delete_word(),
            KeyCode::Char('r') if ctrl => self.reveal_selected(),
            KeyCode::Char('p') if ctrl => self.move_selection(-1),
            KeyCode::Char('n') if ctrl => self.move_selection(1),
            KeyCode::Char(c) if !ctrl && !alt => self.insert_char(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete_forward(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => {
                if self.cursor < self.char_len() {
                    self.cursor += 1;
                }
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.char_len(),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down => self.move_selection(1),
            KeyCode::Tab => self.complete_keyword(),
            KeyCode::Enter => {
                if ctrl || alt {
                    self.reveal_selected();
                } else {
                    self.execute();
                }
            }
            _ => {}
        }
    }

    fn execute(&mut self) {
        let Some(item) = self.selected_item() else {
            return;
        };
        match exec::run(&item.action) {
            // 成功就退出，把屏幕让给刚打开的程序
            Ok(_) => self.should_quit = true,
            Err(e) => self.status = Some((format!("{e:#}"), true)),
        }
    }

    fn reveal_selected(&mut self) {
        let Some(path) = self.selected_item().and_then(|item| match item.action {
            Action::Open(p) => Some(p),
            _ => None,
        }) else {
            return;
        };
        match exec::run(&Action::Reveal(path)) {
            Ok(msg) => self.status = Some((msg, false)),
            Err(e) => self.status = Some((format!("{e:#}"), true)),
        }
    }

    fn selected_item(&self) -> Option<Item> {
        let idx = self.list_state.selected()?;
        self.items.get(idx).cloned()
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

    /// Tab：把 `ev` 补成 `ev `
    fn complete_keyword(&mut self) {
        let Some(idx) = self.matched else { return };
        let keyword = self.infos[idx].keyword.clone();
        if self.input.trim() == keyword {
            self.input = format!("{keyword} ");
            self.cursor = self.input.chars().count();
            self.refresh();
        }
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

    fn insert_char(&mut self, c: char) {
        let at = self.byte_index(self.cursor);
        self.input.insert(at, c);
        self.cursor += 1;
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
        self.refresh();
    }

    fn delete_forward(&mut self) {
        if self.cursor >= self.char_len() {
            return;
        }
        let from = self.byte_index(self.cursor);
        let to = self.byte_index(self.cursor + 1);
        self.input.replace_range(from..to, "");
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
        self.refresh();
    }
}
