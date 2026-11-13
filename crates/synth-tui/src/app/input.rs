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

use crate::app::CAN_QUIT;
use crate::app::state::AppState;
use synthie::params::MidiNote;
use synthie_ui::{Key, KeyCode, KeyKind, ModalAction};

/// Map a key code to a semitone offset (0 = C of the bottom row, 12 = C of
/// the upper row).  Returns `None` for non-note keys.
pub(crate) fn key_to_semitone(code: KeyCode) -> Option<u8> {
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
fn make_midi(octave: i8, semitone: u8) -> MidiNote {
    let raw = 12_i32 * (i32::from(octave) + 1) + i32::from(semitone);
    MidiNote(u8::try_from(raw.clamp(0, 127)).expect("clamped to 0..=127"))
}

/// Process a single key event.  Returns `true` if the application should quit.
pub fn handle_key(key: Key, state: &mut AppState) -> bool {
    // Ctrl+C / Ctrl+Q → quit
    if key.ctrl {
        match key.code {
            KeyCode::Char('c' | 'q') => return CAN_QUIT,
            KeyCode::Char('s') => {
                // Ctrl+S: save current params as user patch (quick save)
                let name = format!("User {}", state.patches.len() + 1);
                state.save_current_as_preset(&name);
                return false;
            }
            #[cfg(not(target_arch = "wasm32"))]
            KeyCode::Char('e') => {
                state.export_wav();
                return false;
            }
            _ => return false,
        }
    }

    // Quit dialog / command line capture key presses while open.
    match state.modal.handle_key(key) {
        ModalAction::Ignored => {}
        ModalAction::Consumed => return false,
        ModalAction::Quit => return true,
        ModalAction::Message(msg) => {
            state.status_msg = msg;
            return false;
        }
    }

    // Key-release events: send note-off for piano keys.
    if key.kind == KeyKind::Release {
        if let Some(semi) = key_to_semitone(key.code) {
            state.piano_key_up(semi);
        }
        return false;
    }

    // From here on we only handle Press (and Repeat for held keys – repeat
    // does NOT re-trigger note-on to avoid LFSR/envelope resets).
    if key.kind == KeyKind::Repeat {
        return false;
    }

    // Piano note keys (Press only)
    if let Some(semi) = key_to_semitone(key.code) {
        let midi = make_midi(state.octave, semi);
        state.piano_key_down(semi, midi);
        return false;
    }

    // Application control keys
    match key.code {
        KeyCode::F(12) if CAN_QUIT => return true,

        KeyCode::F(1) => {
            state.show_help = !state.show_help;
        }

        // Esc closes help; otherwise silence everything and ask to quit.
        KeyCode::Esc if state.show_help => state.show_help = false,
        KeyCode::Esc => {
            state.panic_all_notes();
            if CAN_QUIT {
                state.modal.confirm_quit();
            }
        }

        KeyCode::Char(':') if CAN_QUIT => state.modal.open_command(),

        KeyCode::Tab => {
            if key.shift {
                state.prev_section();
            } else {
                state.next_section();
            }
        }

        KeyCode::Char('p' | 'P') => state.toggle_presets(),

        KeyCode::Left => state.prev_param(),
        KeyCode::Right => state.next_param(),

        KeyCode::Up => state.adjust_param(1.0),
        KeyCode::Down => state.adjust_param(-1.0),

        KeyCode::Enter if state.selected_section == crate::app::state::Section::Presets => {
            let idx = state.selected_preset;
            state.load_preset(idx);
        }
        KeyCode::Enter if state.selected_section == crate::app::state::Section::Sfx => {
            state.run_sfx_action();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::Section;
    use proptest::prelude::*;

    fn key() -> impl Strategy<Value = Key> {
        let code = prop_oneof![
            4 => prop::sample::select(
                "zsxdcvgbhnjmq2w3er5t6y7u:pP+-=_[],.12345678yYqwx! ".chars().collect::<Vec<_>>()
            ).prop_map(KeyCode::Char),
            1 => any::<char>().prop_map(KeyCode::Char),
            3 => prop::sample::select(vec![
                KeyCode::Enter, KeyCode::Esc, KeyCode::Backspace, KeyCode::Tab,
                KeyCode::Up, KeyCode::Down, KeyCode::Left, KeyCode::Right,
            ]),
            1 => (1_u8..=12).prop_map(KeyCode::F),
        ];
        let kind = prop::sample::select(vec![KeyKind::Press, KeyKind::Repeat, KeyKind::Release]);
        // No Ctrl: Ctrl+S and Ctrl+E write files.
        (code, kind, any::<bool>()).prop_map(|(code, kind, shift)| Key {
            code,
            kind,
            ctrl: false,
            shift,
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// Any key sequence keeps the UI consistent: the cursor stays on a
        /// real row, the patch stays in range and no note is held twice.
        #[test]
        fn any_key_sequence_keeps_the_state_consistent(keys in prop::collection::vec(key(), 0..200)) {
            let (tx, _rx) = crossbeam_channel::unbounded();
            let mut state = AppState::new(tx);
            for key in keys {
                if handle_key(key, &mut state) {
                    break;
                }
                let section = state.selected_section;
                if section != Section::Presets {
                    prop_assert!(state.selected_param < section.param_count());
                }
                prop_assert!(state.selected_preset < state.patches.len());
                let mut sane = state.params.clone();
                sane.sanitize();
                prop_assert_eq!(&sane, &state.params);
                let mut notes = state.held_notes.clone();
                notes.sort_unstable();
                notes.dedup();
                prop_assert_eq!(notes.len(), state.held_notes.len());
            }
        }
    }
}
