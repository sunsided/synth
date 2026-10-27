//! Core data model for the step-sequencer tracker.
//!
//! A [`Pattern`] holds a fixed grid of [`Step`]s (rows × synth tracks) plus a
//! parallel [`DrumStep`] column.  The [`Song`] bundles one pattern with a BPM
//! setting and per-track preset assignments.

pub mod player;

use synthie::params::MidiNote;

/// Maximum number of steps (rows) in a pattern: eight bars of 16th notes.
pub const STEPS: usize = 128;

/// Number of independent synth tracks per pattern.
pub const SYNTH_TRACKS: usize = 4;

/// Total column count: synth tracks + 1 drum column.
pub const TOTAL_TRACKS: usize = SYNTH_TRACKS + 1;

/// Index of the drum track within `TOTAL_TRACKS`.
pub const DRUM_TRACK: usize = SYNTH_TRACKS;

// ─── Step ────────────────────────────────────────────────────────────────────

/// A single step on a synth track.
///
/// Notes sustain until the next [`Step::Note`] or [`Step::Off`] on the same
/// track, so empty steps let the current note ring on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Step {
    /// No event; a sounding note keeps ringing.
    #[default]
    Empty,
    /// Trigger a note, releasing the previous one on this track.
    Note(MidiNote),
    /// Release the sounding note (shown as `===`).
    Off,
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

/// The Lazy Jones hook (David Whittaker, 1984), the riff that later became
/// Kernkraft 400: one monophonic line of straight 8th notes in which melody
/// and bass alternate.  One entry per 8th note, MIDI note numbers.
const LAZY_JONES_LINE: [u8; 32] = [
    47, 71, 74, 76, 78, 59, 47, 59, // B2  B4 D5 E5 F#5  B3 B2 B3
    47, 71, 74, 76, 78, 79, 78, 74, // B2  B4 D5 E5 F#5 G5 F#5 D5
    76, 64, 52, 64, 74, 66, 78, 71, // E5 E4  E3  E4 D5 F#4 F#5 B4
    47, 59, 47, 59, 47, 59, 47, 59, // B2 B3 B2 B3 B2 B3 B2 B3
];

/// Notes from this pitch up go to the lead track, the rest to the bass.
const LEAD_SPLIT: u8 = 64;

/// Sub-bass of the second pass: one note per beat on the off-beat 8th.
const LAZY_JONES_SUB: [u8; 16] = [
    35, 35, 35, 35, 35, 35, 35, 35, // B1
    40, 40, // E2
    42, 42, // F#2
    35, 35, 35, 35, // B1
];

/// Length of one pass through the hook, in steps (four bars).
const PASS: usize = 64;

/// Return the startup demo: the Lazy Jones / Kernkraft 400 hook at 140 BPM
/// as one 128-step pattern (two four-bar passes).
///
/// Layout:
/// * Track 0 – Arp Lead: the melody notes of the line
/// * Track 1 – C64 Bass: the bass notes of the line, filling the melody's gaps
/// * Track 2 – C64 Bass: B1 / E2 / F#2 off-beat sub-bass, second pass only
/// * Track 3 – PWM Lead: left empty to play along
/// * Drums: four-on-the-floor kick, off-beat open hats, 16th closed hats,
///   kick fill into each pass
#[must_use]
pub fn demo_song() -> Song {
    let mut song = Song {
        bpm: 140.0,
        pattern: Pattern::default(),
        track_patches: [1, 0, 0, 3],
    };
    let p = &mut song.pattern;
    let mut put = |track: usize, step: usize, n: u8| {
        p.synth[track][step] = Step::Note(MidiNote(n));
        p.synth[track][step + 1] = Step::Off; // every note lasts one 8th
    };

    for pass in 0..STEPS / PASS {
        for (i, &n) in LAZY_JONES_LINE.iter().enumerate() {
            let track = usize::from(n < LEAD_SPLIT);
            put(track, pass * PASS + i * 2, n);
        }
    }
    for (beat, &n) in LAZY_JONES_SUB.iter().enumerate() {
        put(2, PASS + beat * 4 + 2, n);
    }

    // Notes span two steps, so a note-off directly followed by a new note on
    // the same track is redundant: drop it for a legato line.
    for track in &mut p.synth {
        for step in 0..STEPS - 1 {
            if track[step] == Step::Off && matches!(track[step + 1], Step::Note(_)) {
                track[step] = Step::Empty;
            }
        }
    }

    // Drums: four-on-the-floor, off-beat open hats, 16th closed hats on the
    // "e" and "a", and a kick fill on the last beat of each pass.
    for (step, drum) in p.drums.iter_mut().enumerate() {
        let fill = step % PASS >= PASS - 2;
        drum.kick = step % 4 == 0 || fill;
        drum.hihat_open = step % 4 == 2 && !fill;
        drum.hihat_closed = step % 2 == 1;
    }

    song
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Format a MIDI note number as a tracker-style string (e.g. `"C-4"`, `"C#4"`).
///
/// The returned string is always 3 characters wide.
#[must_use]
pub fn note_name(note: MidiNote) -> String {
    let n = note.as_u8();
    let octave = (n / 12).cast_signed() - 1_i8;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn notes(track: &[Step]) -> Vec<(usize, u8)> {
        track
            .iter()
            .enumerate()
            .filter_map(|(i, s)| match s {
                Step::Note(n) => Some((i, n.as_u8())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn demo_song_plays_the_line_as_straight_eighths() {
        let song = demo_song();
        assert_eq!(song.pattern.len, STEPS);

        // Lead and bass together form one note on every 8th, never both.
        let mut line = notes(&song.pattern.synth[0]);
        line.extend(notes(&song.pattern.synth[1]));
        line.sort_unstable();
        let expected: Vec<(usize, u8)> = (0..STEPS / 2)
            .map(|i| (i * 2, LAZY_JONES_LINE[i % LAZY_JONES_LINE.len()]))
            .collect();
        assert_eq!(line, expected);

        // The hook opens on B4 D5 E5 F#5.
        let lead: Vec<u8> = notes(&song.pattern.synth[0])
            .iter()
            .take(4)
            .map(|&(_, n)| n)
            .collect();
        assert_eq!(lead, [71, 74, 76, 78]);

        // Sub-bass only in the second pass.
        assert!(
            notes(&song.pattern.synth[2])
                .iter()
                .all(|&(s, _)| s >= PASS)
        );
    }
}
