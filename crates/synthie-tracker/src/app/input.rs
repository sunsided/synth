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
//! ## Drum track keys (when cursor is on the drum column)
//!
//! | Key | Action             |
//! |-----|--------------------|
//! | `k` | Toggle kick        |
//! | `h` | Toggle closed hi-hat |
//! | `o` | Toggle open hi-hat |

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use synthie::params::MidiNote;

use crate::app::state::{DrumKind, TrackerState};
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
pub fn handle_key(key: KeyEvent, state: &mut TrackerState) -> bool {
    // Ignore key-release events; we only act on press.
    if key.kind == KeyEventKind::Release || key.kind == KeyEventKind::Repeat {
        return false;
    }

    // Ctrl+C / Ctrl+Q → quit
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return matches!(key.code, KeyCode::Char('c' | 'q'));
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
        KeyCode::F(12) => return true,
        KeyCode::Esc => state.panic_all(),
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
            if key.modifiers.contains(KeyModifiers::SHIFT) {
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

/// Handle a note-entry key.
fn handle_note_key(code: KeyCode, state: &mut TrackerState) {
    if state.cursor_track == DRUM_TRACK {
        handle_drum_key(code, state);
    } else {
        handle_synth_key(code, state);
    }
}

fn handle_drum_key(code: KeyCode, state: &mut TrackerState) {
    match code {
        KeyCode::Char('k') => state.toggle_drum(DrumKind::Kick),
        KeyCode::Char('h') => state.toggle_drum(DrumKind::HiHatClosed),
        KeyCode::Char('o') => state.toggle_drum(DrumKind::HiHatOpen),
        _ => {}
    }
}

fn handle_synth_key(code: KeyCode, state: &mut TrackerState) {
    if let Some(semi) = key_to_semitone(code) {
        let midi = make_midi(state.octave, semi);
        state.enter_note(midi);
    }
}
