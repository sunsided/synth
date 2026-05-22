//! Core data model for the step-sequencer tracker.
//!
//! A [`Pattern`] holds a fixed grid of [`Step`]s (rows × synth tracks) plus a
//! parallel [`DrumStep`] column.  The [`Song`] bundles one pattern with a BPM
//! setting and per-track preset assignments.

pub mod player;

use synthie::params::MidiNote;

/// Number of steps (rows) in a pattern.
pub const STEPS: usize = 16;

/// Number of independent synth tracks per pattern.
pub const SYNTH_TRACKS: usize = 4;

/// Total column count: synth tracks + 1 drum column.
pub const TOTAL_TRACKS: usize = SYNTH_TRACKS + 1;

/// Index of the drum track within `TOTAL_TRACKS`.
pub const DRUM_TRACK: usize = SYNTH_TRACKS;

// ─── Step ────────────────────────────────────────────────────────────────────

/// A single step on a synth track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Step {
    /// MIDI note triggered at this step, or `None` for a rest.
    pub note: Option<MidiNote>,
}

// ─── DrumStep ────────────────────────────────────────────────────────────────

/// Which drum hits are active at a single step.
#[derive(Debug, Clone, Copy, Default)]
pub struct DrumStep {
    /// Kick drum hit.
    pub kick: bool,
    /// Closed hi-hat hit.
    pub hihat_closed: bool,
    /// Open hi-hat hit.
    pub hihat_open: bool,
}

// ─── Pattern ─────────────────────────────────────────────────────────────────

/// One pattern: `SYNTH_TRACKS` columns × `STEPS` rows plus a drum track.
#[derive(Debug, Clone)]
pub struct Pattern {
    /// `synth[track][row]` – the step at `(row, track)`.
    pub synth: [[Step; STEPS]; SYNTH_TRACKS],
    /// Drum hits per row.
    pub drums: [DrumStep; STEPS],
    /// Active step count (1..=`STEPS`).  Rows ≥ `len` are silenced.
    pub len: usize,
}

impl Default for Pattern {
    fn default() -> Self {
        Self {
            synth: [[Step::default(); STEPS]; SYNTH_TRACKS],
            drums: [DrumStep::default(); STEPS],
            len: STEPS,
        }
    }
}

// ─── Song ────────────────────────────────────────────────────────────────────

/// Full song data: one pattern + BPM + per-track preset assignments.
///
/// Cloned when the UI sends a snapshot to the player thread.
#[derive(Debug, Clone)]
pub struct Song {
    /// Playback tempo in beats per minute.
    pub bpm: f32,
    /// The pattern to play.
    pub pattern: Pattern,
    /// Preset (patch) index from the preset bank, one per synth track.
    pub track_patches: [usize; SYNTH_TRACKS],
}

impl Default for Song {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            pattern: Pattern::default(),
            track_patches: [0, 1, 2, 3],
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Format a MIDI note number as a tracker-style string (e.g. `"C-4"`, `"C#4"`).
///
/// The returned string is always 3 characters wide.
#[must_use]
pub fn note_name(note: MidiNote) -> String {
    let n = note.as_u8();
    let octave = n / 12;
    let semitone = n % 12;
    let name = match semitone {
        0 => "C-",
        1 => "C#",
        2 => "D-",
        3 => "D#",
        4 => "E-",
        5 => "F-",
        6 => "F#",
        7 => "G-",
        8 => "G#",
        9 => "A-",
        10 => "A#",
        11 => "B-",
        _ => unreachable!(),
    };
    format!("{name}{octave}")
}
