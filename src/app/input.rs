//! Keyboard event handler: piano note mapping and application control keys.
//!
//! ## Piano keyboard layout
//!
//! Bottom octave row (maps to octave N):
//! ```text
//! Key:  Z  S  X  D  C  V  G  B  H  N  J  M
//! Note: C  C# D  D# E  F  F# G  G# A  A# B
//! ```
//!
//! Upper octave row (maps to octave N+1):
//! ```text
//! Key:  Q  2  W  3  E  R  5  T  6  Y  7  U
//! Note: C  C# D  D# E  F  F# G  G# A  A# B
//! ```
//!
//! MIDI note = 12 × (octave + 1) + semitone

use crate::app::state::AppState;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// Map a key code to a semitone offset (0 = C of the bottom row, 12 = C of
/// the upper row).  Returns `None` for non-note keys.
fn key_to_semitone(code: KeyCode) -> Option<u8> {
    match code {
        // Bottom row (octave N)
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
        // Upper row (octave N+1)
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

/// Convert octave + semitone offset to a MIDI note number, clamped to 0..=127.
fn make_midi(octave: i8, semitone: u8) -> u8 {
    let raw = 12_i32 * (i32::from(octave) + 1) + i32::from(semitone);
    u8::try_from(raw.clamp(0, 127)).expect("clamped to 0..=127")
}

/// Process a single key event.  Returns `true` if the application should quit.
pub fn handle_key(key: KeyEvent, state: &mut AppState) -> bool {
    // Ctrl+C / Ctrl+Q → quit
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c' | 'q') => return true,
            KeyCode::Char('s') => {
                // Ctrl+S: save current params as user patch (quick save)
                let name = format!("User {}", state.patches.len() + 1);
                state.save_current_as_preset(&name);
                return false;
            }
            _ => return false,
        }
    }

    // Key-release events: send note-off for piano keys.
    if key.kind == KeyEventKind::Release {
        if let Some(semi) = key_to_semitone(key.code) {
            let midi = make_midi(state.octave, semi);
            state.note_off(midi);
        }
        return false;
    }

    // From here on we only handle Press (and Repeat for held keys – repeat
    // does NOT re-trigger note-on to avoid LFSR/envelope resets).
    if key.kind == KeyEventKind::Repeat {
        return false;
    }

    // Piano note keys (Press only)
    if let Some(semi) = key_to_semitone(key.code) {
        let midi = make_midi(state.octave, semi);
        state.note_on(midi);
        return false;
    }

    // Application control keys
    match key.code {
        KeyCode::F(12) => return true,

        KeyCode::F(1) => {
            state.show_help = !state.show_help;
        }

        KeyCode::Esc => {
            state.panic_all_notes();
        }

        KeyCode::Tab => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                state.prev_section();
            } else {
                state.next_section();
            }
        }

        KeyCode::Left => state.prev_param(),
        KeyCode::Right => state.next_param(),

        KeyCode::Up => state.adjust_param(1.0),
        KeyCode::Down => state.adjust_param(-1.0),

        KeyCode::Enter if state.selected_section == crate::app::state::Section::Presets => {
            let idx = state.selected_preset;
            state.load_preset(idx);
        }

        KeyCode::Char('+' | '=') => state.volume_up(),
        KeyCode::Char('-' | '_') => state.volume_down(),

        KeyCode::Char('[' | ',') => state.octave_down(),
        KeyCode::Char(']' | '.') => state.octave_up(),

        // Quick-select presets 1–8 (number keys, only in Presets section)
        KeyCode::Char(ch @ '1'..='8') => {
            let idx = (ch as u8 - b'1') as usize;
            if state.selected_section == crate::app::state::Section::Presets {
                state.load_preset(idx);
            }
        }

        _ => {}
    }

    false
}
