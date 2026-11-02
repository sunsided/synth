//! Keyboard event handler for the tracker.
//!
//! ## Piano keyboard layout (note entry)
//!
//! Bottom row (octave N):
//! ```text
//! Key:  Z  S  X  D  C  V  G  B  H  N  J  M
//! Note: C  C# D  D# E  F  F# G  G# A  A# B
//! ```
//!
//! Upper row (octave N+1):
//! ```text
//! Key:  Q  2  W  3  E  R  5  T  6  Y  7  U
//! Note: C  C# D  D# E  F  F# G  G# A  A# B
//! ```
//!
//! `1` or `` ` `` enters a note-off (`===`).
//!
//! ## Drum track keys (when cursor is on the drum column)
//!
//! | Key | Action               |
//! |-----|----------------------|
//! | `k` | Toggle kick          |
//! | `s` | Toggle snare         |
//! | `h` | Toggle closed hi-hat |
//! | `o` | Toggle open hi-hat   |
//! | `c` | Toggle crash         |
//! | `x` | Toggle clap          |
//! | `1` | Toggle low tom       |
//! | `2` | Toggle mid tom       |
//! | `3` | Toggle high tom      |

use synthie::params::{DrumHit, MidiNote};
use synthie_ui::{Key, KeyCode, KeyKind, ModalAction};

use crate::app::CAN_QUIT;
use crate::app::state::TrackerState;
use crate::app::ui::{ToolbarAction, toolbar, toolbar_hints};
use crate::tracker::DRUM_TRACK;

/// Map a key code to a semitone offset within the piano layout.
///
/// Returns `None` for non-piano keys.
fn key_to_semitone(code: KeyCode) -> Option<u8> {
    match code {
        // Bottom row – octave N
        KeyCode::Char('z') => Some(0),  // C
        KeyCode::Char('s') => Some(1),  // C#
        KeyCode::Char('x') => Some(2),  // D
        KeyCode::Char('d') => Some(3),  // D#
        KeyCode::Char('c') => Some(4),  // E
        KeyCode::Char('v') => Some(5),  // F
        KeyCode::Char('g') => Some(6),  // F#
        KeyCode::Char('b') => Some(7),  // G
        KeyCode::Char('h') => Some(8),  // G#
        KeyCode::Char('n') => Some(9),  // A
        KeyCode::Char('j') => Some(10), // A#
        KeyCode::Char('m') => Some(11), // B
        // Upper row – octave N+1
        KeyCode::Char('q') => Some(12), // C
        KeyCode::Char('2') => Some(13), // C#
        KeyCode::Char('w') => Some(14), // D
        KeyCode::Char('3') => Some(15), // D#
        KeyCode::Char('e') => Some(16), // E
        KeyCode::Char('r') => Some(17), // F
        KeyCode::Char('5') => Some(18), // F#
        KeyCode::Char('t') => Some(19), // G
        KeyCode::Char('6') => Some(20), // G#
        KeyCode::Char('y') => Some(21), // A
        KeyCode::Char('7') => Some(22), // A#
        KeyCode::Char('u') => Some(23), // B
        _ => None,
    }
}

/// Convert an octave + semitone offset to a clamped MIDI note.
fn make_midi(octave: i8, semitone: u8) -> MidiNote {
    let raw = 12_i32 * (i32::from(octave) + 1) + i32::from(semitone);
    MidiNote(u8::try_from(raw.clamp(0, 127)).expect("value clamped to 0..=127"))
}

/// Process a single key event.  Returns `true` when the application should quit.
pub fn handle_key(key: Key, state: &mut TrackerState) -> bool {
    // Ignore key-release events; we only act on press.
    if key.kind != KeyKind::Press {
        return false;
    }

    // Ctrl+C / Ctrl+Q → quit
    if key.ctrl {
        return CAN_QUIT && matches!(key.code, KeyCode::Char('c' | 'q'));
    }

    // ── Quit dialog / command line ────────────────────────────────────────────
    match state.modal.handle_key(key) {
        ModalAction::Ignored => {}
        ModalAction::Consumed => return false,
        ModalAction::Quit => return true,
        ModalAction::Message(msg) => {
            state.status_msg = msg;
            return false;
        }
    }

    // ── Help overlay ──────────────────────────────────────────────────────────
    if key.code == KeyCode::F(1) {
        state.show_help = !state.show_help;
        return false;
    }
    if state.show_help {
        // Any key dismisses the help overlay.
        state.show_help = false;
        return false;
    }

    // ── Global controls ───────────────────────────────────────────────────────
    match key.code {
        KeyCode::F(12) if CAN_QUIT => return true,
        KeyCode::Esc => {
            // Stop the sequencer first, or the next step would sound again.
            state.stop();
            state.panic_all();
            if CAN_QUIT {
                state.modal.confirm_quit();
            }
        }
        KeyCode::Char(':') if CAN_QUIT => state.modal.open_command(),
        KeyCode::Char(' ') => state.toggle_play(),
        KeyCode::Up => state.cursor_up(),
        KeyCode::Down => state.cursor_down(),
        KeyCode::Left => state.cursor_left(),
        KeyCode::Right => state.cursor_right(),
        KeyCode::Delete | KeyCode::Backspace => state.clear_step(),
        KeyCode::Char('+' | '=') => state.bpm_up(),
        KeyCode::Char('-' | '_') => state.bpm_down(),
        KeyCode::Char('[' | ',') => state.octave_down(),
        KeyCode::Char(']' | '.') => state.octave_up(),
        // Tab / Shift-Tab: cycle preset on current synth track
        KeyCode::Tab => {
            if key.shift {
                state.prev_patch();
            } else {
                state.next_patch();
            }
        }
        // Page Up / Page Down: adjust pattern length
        KeyCode::PageUp => state.pattern_len_up(),
        KeyCode::PageDown => state.pattern_len_down(),
        // Note entry (drum or synth depending on track)
        _ => handle_note_key(key.code, state),
    }

    false
}

/// Process a left click at cell `(col, row)`.  Returns `true` when the
/// application should quit.
///
/// A click on a footer hint does what its key does; on a two-key cap such
/// as `↑↓` it presses the key under the cursor, on the label the first one.
/// Like a key, any click closes the help overlay. While a dialog is open,
/// clicks go to it (the quit dialog's buttons work like `Y` and `N`) and
/// nothing else.
pub fn handle_click(col: u16, row: u16, state: &mut TrackerState) -> bool {
    match state.modal.handle_click(state.ui_area.get(), col, row) {
        ModalAction::Ignored => {}
        ModalAction::Consumed => return false,
        ModalAction::Quit => return true,
        ModalAction::Message(msg) => {
            state.status_msg = msg;
            return false;
        }
    }
    if state.show_help {
        state.show_help = false;
        return false;
    }
    let hit = synthie_ui::widgets::footer_hint_at(
        state.footer_area.get(),
        &toolbar_hints(),
        &state.status_msg,
        col,
        row,
    );
    let Some(hit) = hit else {
        return false;
    };
    let code = match toolbar()[hit.index].2 {
        ToolbarAction::Key(code) => code,
        ToolbarAction::Pair(first, second) => match hit.cap_glyph {
            // Second half of the cap (the middle glyph of `+ -` counts as
            // the first).
            Some((glyph, glyphs)) if 2 * glyph >= glyphs => second,
            _ => first,
        },
        ToolbarAction::ConfirmQuit => {
            state.stop();
            state.panic_all();
            state.modal.confirm_quit();
            return false;
        }
    };
    handle_key(Key::press(code), state)
}

/// Handle a note-entry key.
fn handle_note_key(code: KeyCode, state: &mut TrackerState) {
    if state.cursor_track == DRUM_TRACK {
        handle_drum_key(code, state);
    } else {
        handle_synth_key(code, state);
    }
}

/// The drum toggled by `code` on the drum track.
fn drum_for_key(code: KeyCode) -> Option<DrumHit> {
    match code {
        KeyCode::Char('k') => Some(DrumHit::Kick),
        KeyCode::Char('s') => Some(DrumHit::Snare),
        KeyCode::Char('h') => Some(DrumHit::HiHatClosed),
        KeyCode::Char('o') => Some(DrumHit::HiHatOpen),
        KeyCode::Char('c') => Some(DrumHit::Crash),
        KeyCode::Char('x') => Some(DrumHit::Clap),
        KeyCode::Char('1') => Some(DrumHit::TomLow),
        KeyCode::Char('2') => Some(DrumHit::TomMid),
        KeyCode::Char('3') => Some(DrumHit::TomHigh),
        _ => None,
    }
}

fn handle_drum_key(code: KeyCode, state: &mut TrackerState) {
    if let Some(hit) = drum_for_key(code) {
        state.toggle_drum(hit);
    }
}

fn handle_synth_key(code: KeyCode, state: &mut TrackerState) {
    if matches!(code, KeyCode::Char('1' | '`')) {
        state.enter_note_off();
    } else if let Some(semi) = key_to_semitone(code) {
        let midi = make_midi(state.octave, semi);
        state.enter_note(midi);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui::DRUM_GLYPHS;
    use crate::tracker::KIT;

    /// The drum column header and the help show these keys, so each must
    /// toggle the drum in its column.
    #[test]
    fn every_drum_column_label_is_its_key() {
        for (&hit, (key, name, _)) in KIT.iter().zip(DRUM_GLYPHS) {
            let key = KeyCode::Char(key.chars().next().unwrap());
            assert_eq!(drum_for_key(key), Some(hit), "{name}");
        }
    }

    /// A tracker drawn once into a 120 × 40 terminal, so the footer area
    /// is known, plus the footer row as a string of cells.
    fn drawn() -> (TrackerState, Vec<String>) {
        use ratatui::{Terminal, backend::TestBackend};
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;

        let (tx, _rx) = crossbeam_channel::unbounded();
        let state = TrackerState::new(
            tx,
            crate::tracker::player::Player::new().0,
            Arc::new(AtomicBool::new(true)),
        );
        let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
        term.draw(|f| crate::app::ui::draw(f, &state, &[], 0.0))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let row = (0..120)
            .map(|x| buf[(x, 39)].symbol().to_string())
            .collect();
        (state, row)
    }

    /// Column of the first cell showing `glyph`, after `from`.
    fn col(row: &[String], glyph: &str, from: usize) -> u16 {
        let x = row[from..].iter().position(|c| c == glyph).unwrap() + from;
        u16::try_from(x).unwrap()
    }

    #[test]
    fn clicking_footer_hints_acts_like_their_keys() {
        let (mut state, row) = drawn();
        assert_eq!(state.footer_area.get().y, 39);

        // Arrow caps press the arrow under the cursor.
        assert!(!handle_click(col(&row, "↓", 0), 39, &mut state));
        assert_eq!(state.cursor_row, 1);
        handle_click(col(&row, "↑", 0), 39, &mut state);
        assert_eq!(state.cursor_row, 0);
        handle_click(col(&row, "→", 0), 39, &mut state);
        assert_eq!(state.cursor_track, 1);
        // The label presses the first key of the pair.
        let track = col(&row, "t", col(&row, "→", 0).into());
        handle_click(track, 39, &mut state);
        assert_eq!(state.cursor_track, 0);

        let bpm = state.song.bpm;
        handle_click(col(&row, "+", 0), 39, &mut state);
        assert!((state.song.bpm - bpm - 1.0).abs() < f32::EPSILON);
        handle_click(col(&row, "-", col(&row, "+", 0).into()), 39, &mut state);
        assert!((state.song.bpm - bpm).abs() < f32::EPSILON);

        // F1 opens the help; any click closes it again without acting.
        let f1 = col(&row, "F", 0);
        handle_click(f1, 39, &mut state);
        assert!(state.show_help);
        handle_click(col(&row, "↓", 0), 39, &mut state);
        assert!(!state.show_help);
        assert_eq!(state.cursor_row, 0);

        // Misses: the grid, and the gap after a hint.
        handle_click(col(&row, "↓", 0), 20, &mut state);
        assert_eq!(state.cursor_row, 0);
    }

    #[test]
    fn clicking_quit_asks_first_and_the_dialog_buttons_answer() {
        let (mut state, row) = drawn();
        if !CAN_QUIT {
            return;
        }
        let area = state.ui_area.get();
        let button = |quit: bool| {
            (area.top()..area.bottom())
                .flat_map(|y| (area.left()..area.right()).map(move |x| (x, y)))
                .find(|&(x, y)| {
                    synthie_ui::widgets::quit_dialog_button_at(area, x, y) == Some(quit)
                })
                .unwrap()
        };
        // The `q` of the `:q` cap, not the one in Esc's "stop·quit" label.
        let q = col(&row, "q", col(&row, ":", 0).into());
        assert!(!handle_click(q, 39, &mut state), "must not quit outright");
        assert_eq!(state.modal, synthie_ui::Modal::ConfirmQuit);

        // The dialog swallows clicks elsewhere, footer included.
        handle_click(col(&row, "↓", 0), 39, &mut state);
        assert_eq!(state.cursor_row, 0);
        assert_eq!(state.modal, synthie_ui::Modal::ConfirmQuit);

        // "stay" closes it; "quit" quits.
        let (x, y) = button(false);
        assert!(!handle_click(x, y, &mut state));
        assert_eq!(state.modal, synthie_ui::Modal::None);
        handle_click(q, 39, &mut state);
        let (x, y) = button(true);
        assert!(handle_click(x, y, &mut state), "quit button");
    }
}
