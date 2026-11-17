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
        let at = utf16_len(&self.app.input, self.app.cursor);
        Some(UTF16Selection {
            range: at..at,
            reversed: false,
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
        let (start, end) = match range {
            Some(range) => (
                utf16_to_char(&self.app.input, range.start),
                utf16_to_char(&self.app.input, range.end),
            ),
            None => (self.app.cursor, self.app.cursor),
        };

        let from = byte_index(&self.app.input, start);
        let to = byte_index(&self.app.input, end);
        self.app.input.replace_range(from..to, text);
        self.app.cursor = start + text.chars().count();
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
        let chars = utf16_to_char(&self.app.input, range_utf16.start);
        let advance = style::INPUT_SIZE * 0.6;
        let x = element_bounds.left() + px(advance * (style::PROMPT_CHARS + chars as f32));
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
