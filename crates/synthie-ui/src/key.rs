//! Backend-neutral key events.
//!
//! The front-ends run in a terminal (crossterm) and in the browser (DOM
//! keyboard events).  Input handling works on [`Key`] so it is shared by both;
//! the `crossterm` and `web` features provide the conversions.

/// Which key was pressed, as far as the front-ends care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    /// A printable character, already shifted (`'A'`, `':'`, …).
    Char(char),
    Enter,
    Esc,
    Backspace,
    Delete,
    Tab,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    /// Function key `F1`…`F12`.
    F(u8),
    /// Any key the front-ends do not handle.
    Other,
}

/// Whether the key went down, auto-repeated, or came up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Press,
    Repeat,
    Release,
}

/// A key event with the modifiers the front-ends use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub code: KeyCode,
    pub kind: KeyKind,
    pub ctrl: bool,
    pub shift: bool,
}

impl Key {
    /// A plain key press without modifiers.
    #[must_use]
    pub const fn press(code: KeyCode) -> Self {
        Self {
            code,
            kind: KeyKind::Press,
            ctrl: false,
            shift: false,
        }
    }
}

#[cfg(feature = "crossterm")]
impl From<crossterm::event::KeyEvent> for Key {
    fn from(event: crossterm::event::KeyEvent) -> Self {
        use crossterm::event::{KeyCode as C, KeyEventKind, KeyModifiers};

        let mut shift = event.modifiers.contains(KeyModifiers::SHIFT);
        let code = match event.code {
            C::Char(c) => KeyCode::Char(c),
            C::Enter => KeyCode::Enter,
            C::Esc => KeyCode::Esc,
            C::Backspace => KeyCode::Backspace,
            C::Delete => KeyCode::Delete,
            C::Tab => KeyCode::Tab,
            // Terminals report Shift+Tab as its own key.
            C::BackTab => {
                shift = true;
                KeyCode::Tab
            }
            C::Up => KeyCode::Up,
            C::Down => KeyCode::Down,
            C::Left => KeyCode::Left,
            C::Right => KeyCode::Right,
            C::PageUp => KeyCode::PageUp,
            C::PageDown => KeyCode::PageDown,
            C::F(n) => KeyCode::F(n),
            _ => KeyCode::Other,
        };
        let kind = match event.kind {
            KeyEventKind::Press => KeyKind::Press,
            KeyEventKind::Repeat => KeyKind::Repeat,
            KeyEventKind::Release => KeyKind::Release,
        };
        Self {
            code,
            kind,
            ctrl: event.modifiers.contains(KeyModifiers::CONTROL),
            shift,
        }
    }
}

#[cfg(feature = "web")]
impl From<&web_sys::KeyboardEvent> for Key {
    /// Converts a `keydown` / `keyup` event.  Like a terminal, this uses the
    /// layout-mapped `key` value, not the physical key position.
    fn from(event: &web_sys::KeyboardEvent) -> Self {
        let key = event.key();
        let mut chars = key.chars();
        let code = match (chars.next(), chars.next()) {
            (Some(c), None) => KeyCode::Char(c),
            _ => match key.as_str() {
                "Enter" => KeyCode::Enter,
                "Escape" => KeyCode::Esc,
                "Backspace" => KeyCode::Backspace,
                "Delete" => KeyCode::Delete,
                "Tab" => KeyCode::Tab,
                "ArrowUp" => KeyCode::Up,
                "ArrowDown" => KeyCode::Down,
                "ArrowLeft" => KeyCode::Left,
                "ArrowRight" => KeyCode::Right,
                "PageUp" => KeyCode::PageUp,
                "PageDown" => KeyCode::PageDown,
                f => f
                    .strip_prefix('F')
                    .and_then(|n| n.parse().ok())
                    .map_or(KeyCode::Other, KeyCode::F),
            },
        };
        let kind = if event.type_() == "keyup" {
            KeyKind::Release
        } else if event.repeat() {
            KeyKind::Repeat
        } else {
            KeyKind::Press
        };
        Self {
            code,
            kind,
            ctrl: event.ctrl_key(),
            shift: event.shift_key(),
        }
    }
}
