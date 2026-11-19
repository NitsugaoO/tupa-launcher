//! 输入框的 IME 支持。
//!
//! GPUI 的文本输入走 [`EntityInputHandler`]：平台把「提交的文字」和「输入法预编辑
//! （preedit，拼音还没上屏那一段）」分别推给 `replace_text_in_range` /
//! `replace_and_mark_text_in_range`，候选窗位置则由 `bounds_for_range` 返回的光标矩形决定
//! （Windows 端 GPUI 会拿它调 `ImmSetCandidateWindow`）。
//!
//! 这套实现挂在 [`Panel`] 上，直接读写 `App` 的 `input` / `cursor`，不新增一份文本状态。
//! 注册入口在 `mod.rs::input_row` 的 `canvas`：`window.handle_input(...)` 只能在 paint
//! 阶段调用。

use std::ops::Range;
use std::time::Duration;

use gpui::{
    Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window, point, px, size,
};

use crate::gui::{Panel, style};

/// 字符下标 → 字节下标
pub fn byte_index(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

/// 字符下标 → UTF-16 长度
pub fn utf16_len(text: &str, chars: usize) -> usize {
    text.chars().take(chars).map(char::len_utf16).sum()
}

/// UTF-16 偏移 → 字符下标（中文在 BMP 内是 1，emoji 是 2）
pub fn utf16_to_char(text: &str, offset: usize) -> usize {
    let mut utf16 = 0;
    for (index, ch) in text.chars().enumerate() {
        if utf16 >= offset {
            return index;
        }
        utf16 += ch.len_utf16();
    }
    text.chars().count()
}

impl EntityInputHandler for Panel {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        if self.text_blocked() {
            return None;
        }
        let start = utf16_to_char(&self.app.input, range.start);
        let end = utf16_to_char(&self.app.input, range.end);
        *adjusted_range = Some(range);
        Some(
            self.app
                .input
                .chars()
                .skip(start)
                .take(end.saturating_sub(start))
                .collect(),
        )
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        // 有选区就如实报给系统：IME 提交、系统级复制都按这个范围处理。
        // 之前永远返回「光标处空范围」，所以 Shift+方向键根本没法建选区：
        // 就算建了，系统下一次替换也会把它当成零宽点。
        let (start, end) = self
            .app
            .selection()
            .unwrap_or((self.app.cursor, self.app.cursor));
        let from = utf16_len(&self.app.input, start);
        let to = utf16_len(&self.app.input, end);
        Some(UTF16Selection {
            range: from..to,
            // 活动端（cursor）在锚点左边 = 反向选区
            reversed: self.app.anchor.is_some_and(|anchor| anchor > self.app.cursor),
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let (start, text) = self.preedit.as_ref()?;
        let from = utf16_len(&self.app.input, *start);
        Some(from..from + text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.preedit = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 帮助浮层开着（或刚关）：`?` 那一下的 `WM_CHAR` 不能落进来
        if self.text_blocked() {
            return;
        }
        // 列表模式下打字 = 想回输入框（用户 2026-11-19：检测输入自动回去）
        self.leave_list_mode();

        // 中文输入法把 Shift+/ 直接当全角「？」提交（没有对应的 WM_KEYDOWN），
        // `handle_key` 里那个帮助开关就永远收不到。这里补一道：
        // **输入框为空时**单个问号当帮助键；已经打了字就照常插入
        // （别让 `@web https://x?a=b` 这类没法输入）。
        if (text == "?" || text == "？") && self.app.input.is_empty() && self.preedit.is_none() {
            self.help = true;
            self.text_block = Some(std::time::Instant::now() + Duration::from_millis(50));
            cx.notify();
            return;
        }

        let (start, end) = match range {
            Some(range) => (
                utf16_to_char(&self.app.input, range.start),
                utf16_to_char(&self.app.input, range.end),
            ),
            None => self
                .app
                .selection()
                .unwrap_or((self.app.cursor, self.app.cursor)),
        };
        let from = byte_index(&self.app.input, start);
        let to = byte_index(&self.app.input, end);
        self.app.input.replace_range(from..to, text);
        self.app.cursor = start + text.chars().count();
        self.app.anchor = None;
        self.preedit = None;
        self.app.refresh();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 预编辑：只是「显示在光标处」，先不动输入框本身，等 commit 再落进去
        if self.text_blocked() {
            return;
        }
        // 中文输入法在列表模式下开始组字：同样回输入框，否则候选窗没地方贴
        self.leave_list_mode();
        let start = match range {
            Some(range) => utf16_to_char(&self.app.input, range.start),
            None => self.app.cursor,
        };
        self.preedit = (!text.is_empty()).then(|| (start, text.to_string()));
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // `element_bounds` 来自输入行最前面那个 0 宽 canvas，所以它 left() 就是行首。
        // 等宽字体下：字符数 × 单字符宽度 = 光标横坐标。
        // 已激活的命令 chip 比原来的字符宽（胶囊内边距 + 字形），整体往右补一个固定偏移 ——
        // 它只影响输入法候选窗位置，不需要像素级精确。
        let chars = utf16_to_char(&self.app.input, range_utf16.start);
        let advance = style::INPUT_SIZE * 0.6;
        let chip_extra = match self.app.chip_end {
            Some(end) if chars >= end => {
                style::CHIP_INPUT_PAD_X * 2.0
                    + style::CHIP_INPUT_INNER_GAP
                    + style::CHIP_INPUT_GAP
                    + style::CHIP_INPUT_SIZE
                    - advance
            }
            _ => 0.0,
        };
        let x = element_bounds.left() + px(advance * (style::PROMPT_CHARS + chars as f32) + chip_extra);
        let y = element_bounds.top() + px((style::INPUT_HEIGHT - style::CURSOR_HEIGHT) / 2.0);
        Some(Bounds::new(
            point(x, y),
            size(px(advance), px(style::CURSOR_HEIGHT)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        None // 单行输入框不支持点选定位
    }
}
