//! GPUI 的 `Keystroke` → `crossterm::event::KeyEvent`。
//!
//! `App` 状态机只认 crossterm 的键事件（TUI 与 GUI 共用同一份逻辑），
//! 这里做一次翻译，别在 GUI 里另写一套按键分支。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use gpui::Keystroke;

/// 翻译一个按键；认不出来的（F 键、多媒体键……）返回 `None`，直接丢掉。
pub fn to_key(keystroke: &Keystroke) -> Option<KeyEvent> {
    let mods = &keystroke.modifiers;
    let mut modifiers = KeyModifiers::empty();
    if mods.control {
        modifiers |= KeyModifiers::CONTROL;
    }
    if mods.alt {
        modifiers |= KeyModifiers::ALT;
    }
    if mods.shift {
        modifiers |= KeyModifiers::SHIFT;
    }

    let code = match keystroke.key.as_str() {
        "enter" => KeyCode::Enter,
        "escape" => KeyCode::Esc,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        // Shift+Tab：`App` 两种写法都认（BackTab 走选择模式，Tab+Ctrl 走详情）
        "tab" if mods.shift => KeyCode::BackTab,
        "tab" => KeyCode::Tab,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "space" => KeyCode::Char(' '),
        key => {
            // 带 Ctrl/Alt 时 `key_char` 可能是控制字符（`\u{3}` 之类），只信 `key`；
            // 其余情况优先 `key_char`（已经过 Shift 与键盘布局换算，比如中文输入法外的符号）。
            let text = if mods.control || mods.alt {
                key
            } else {
                keystroke.key_char.as_deref().unwrap_or(key)
            };
            let mut chars = text.chars();
            let ch = chars.next()?;
            if chars.next().is_some() {
                return None; // 组合字符（emoji 等）交给 IME 路径处理
            }
            // 中文输入法下 Shift+/ 打出的是全角「？」，归一成 ASCII，
            // 否则 `handle_key` 里的帮助开关（只认 `?`）会漏掉它。
            if ch == '？' {
                KeyCode::Char('?')
            } else {
                KeyCode::Char(ch)
            }
        }
    };

    Some(KeyEvent::new(code, modifiers))
}
