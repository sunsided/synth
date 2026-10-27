//! Keyboard-modal states shared by both front-ends: the "really quit?"
//! confirmation and a vim-style `:` command line.
//!
//! Both front-ends map most letter keys to piano notes, so `:q` cannot be
//! detected from plain key presses; `:` opens a small command line in the
//! footer instead, and the command runs on `Enter`, as in vim.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};

/// Modal input state; while not [`Modal::None`] it captures key presses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Modal {
    /// Normal operation.
    #[default]
    None,
    /// "Quit?" dialog is showing.
    ConfirmQuit,
    /// Command line is open; holds the text typed after the `:`.
    Command(String),
}

/// What the application should do after [`Modal::handle_key`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalAction {
    /// The modal did not take the key; handle it normally.
    Ignored,
    /// The modal took the key; nothing else to do.
    Consumed,
    /// Leave the application.
    Quit,
    /// Show this message in the status bar (e.g. an unknown command).
    Message(String),
}

impl Modal {
    /// `true` while a modal captures input.
    #[must_use]
    pub fn is_open(&self) -> bool {
        *self != Modal::None
    }

    /// Show the quit confirmation.
    pub fn confirm_quit(&mut self) {
        *self = Modal::ConfirmQuit;
    }

    /// Open an empty command line.
    pub fn open_command(&mut self) {
        *self = Modal::Command(String::new());
    }

    /// Route a key event through the open modal.
    ///
    /// Release and repeat events are never consumed, so held piano keys still
    /// get their note-off after a modal opens.
    pub fn handle_key(&mut self, key: KeyEvent) -> ModalAction {
        if key.kind != KeyEventKind::Press {
            return ModalAction::Ignored;
        }
        match self {
            Modal::None => ModalAction::Ignored,
            Modal::ConfirmQuit => match key.code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => ModalAction::Quit,
                KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                    *self = Modal::None;
                    ModalAction::Consumed
                }
                _ => ModalAction::Consumed,
            },
            Modal::Command(text) => match key.code {
                KeyCode::Esc => {
                    *self = Modal::None;
                    ModalAction::Consumed
                }
                KeyCode::Backspace => {
                    // Backspace on an empty line closes it, as in vim.
                    if text.pop().is_none() {
                        *self = Modal::None;
                    }
                    ModalAction::Consumed
                }
                KeyCode::Enter => {
                    let action = run_command(text.trim());
                    *self = Modal::None;
                    action
                }
                KeyCode::Char(c) => {
                    text.push(c);
                    ModalAction::Consumed
                }
                _ => ModalAction::Consumed,
            },
        }
    }
}

/// Execute a command-line entry.
fn run_command(cmd: &str) -> ModalAction {
    match cmd {
        "" => ModalAction::Consumed,
        "q" | "q!" | "qa" | "qa!" | "quit" | "quit!" | "wq" | "x" => ModalAction::Quit,
        other => ModalAction::Message(format!("E492: Not an editor command: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_command(modal: &mut Modal, text: &str) -> ModalAction {
        modal.open_command();
        for c in text.chars() {
            assert_eq!(
                modal.handle_key(press(KeyCode::Char(c))),
                ModalAction::Consumed
            );
        }
        modal.handle_key(press(KeyCode::Enter))
    }

    #[test]
    fn colon_q_quits() {
        for cmd in ["q", "q!", "wq", "x", " quit "] {
            assert_eq!(type_command(&mut Modal::None, cmd), ModalAction::Quit);
        }
    }

    #[test]
    fn unknown_command_reports_and_closes() {
        let mut modal = Modal::None;
        assert!(
            matches!(type_command(&mut modal, "foo"), ModalAction::Message(m) if m.contains("foo"))
        );
        assert!(!modal.is_open());
    }

    #[test]
    fn confirm_quit_yes_and_no() {
        let mut modal = Modal::ConfirmQuit;
        assert_eq!(
            modal.handle_key(press(KeyCode::Char('y'))),
            ModalAction::Quit
        );
        assert_eq!(modal.handle_key(press(KeyCode::Esc)), ModalAction::Consumed);
        assert!(!modal.is_open());
    }

    #[test]
    fn releases_pass_through() {
        let mut modal = Modal::ConfirmQuit;
        let mut release = press(KeyCode::Char('z'));
        release.kind = KeyEventKind::Release;
        assert_eq!(modal.handle_key(release), ModalAction::Ignored);
        assert!(modal.is_open());
    }

    #[test]
    fn backspace_on_empty_line_closes() {
        let mut modal = Modal::None;
        modal.open_command();
        modal.handle_key(press(KeyCode::Backspace));
        assert!(!modal.is_open());
    }
}
