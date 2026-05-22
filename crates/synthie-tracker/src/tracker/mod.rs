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

// ─── Demo song ───────────────────────────────────────────────────────────────

/// Return a pre-loaded demo pattern: A-minor groove at 140 BPM.
///
/// Layout:
/// * Track 0 – C64 Bass:    walking A-minor bass line (octave 2–3)
/// * Track 1 – Arp Lead:    Kernkraft-inspired 10-note melodic hook (octave 4–5)
/// * Track 2 – Plucky Pulse: syncopated stabs
/// * Track 3 – PWM Lead:    sparse root notes on strong beats
/// * Drums: 4-on-the-floor kick, off-beat closed hi-hat, open hat on 7/15
#[must_use]
pub fn demo_song() -> Song {
    let note = |n: u8| Step {
        note: Some(MidiNote(n)),
    };

    let mut song = Song {
        bpm: 140.0,
        pattern: Pattern::default(),
        track_patches: [0, 1, 2, 3],
    };
    let p = &mut song.pattern;

    // Track 0 – C64 Bass (octave 2–3)
    p.synth[0][0] = note(45); // A2
    p.synth[0][2] = note(45); // A2
    p.synth[0][4] = note(52); // E3
    p.synth[0][6] = note(50); // D3
    p.synth[0][8] = note(45); // A2
    p.synth[0][10] = note(45); // A2
    p.synth[0][12] = note(43); // G2
    p.synth[0][13] = note(45); // A2
    p.synth[0][14] = note(47); // B2
    p.synth[0][15] = note(52); // E3

    // Track 1 – Arp Lead: A B D E F E D C B A … G A (Kernkraft hook)
    p.synth[1][0] = note(69); // A4
    p.synth[1][1] = note(71); // B4
    p.synth[1][2] = note(74); // D5
    p.synth[1][3] = note(76); // E5
    p.synth[1][4] = note(77); // F5
    p.synth[1][5] = note(76); // E5
    p.synth[1][6] = note(74); // D5
    p.synth[1][7] = note(72); // C5
    p.synth[1][8] = note(71); // B4
    p.synth[1][9] = note(69); // A4
    p.synth[1][12] = note(67); // G4
    p.synth[1][13] = note(69); // A4

    // Track 2 – Plucky Pulse stabs
    p.synth[2][0] = note(57); // A3
    p.synth[2][3] = note(64); // E4
    p.synth[2][6] = note(62); // D4
    p.synth[2][8] = note(57); // A3
    p.synth[2][11] = note(62); // D4
    p.synth[2][14] = note(64); // E4

    // Track 3 – PWM Lead (root per chord on every 4th step)
    p.synth[3][0] = note(69); // A4
    p.synth[3][4] = note(64); // E4
    p.synth[3][8] = note(62); // D4
    p.synth[3][12] = note(67); // G4

    // Drums: 4-on-the-floor kick + off-beat closed hat + open hat on "and of 2"
    for s in [0_usize, 4, 8, 12] {
        p.drums[s].kick = true;
    }
    for s in [2_usize, 6, 10, 14] {
        p.drums[s].hihat_closed = true;
    }
    p.drums[7].hihat_open = true;
    p.drums[15].hihat_open = true;

    song
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
