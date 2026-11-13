//! Core data model for the step-sequencer tracker.
//!
//! A [`Pattern`] holds a fixed grid of [`Step`]s (rows × synth tracks) plus a
//! parallel [`DrumStep`] column.  The [`Song`] bundles one pattern with a BPM
//! setting and per-track preset assignments.

#[cfg(all(feature = "mod-import", not(target_arch = "wasm32")))]
pub mod modfile;
pub mod player;

use synthie::params::{DrumHit, MidiNote, Patch};
use synthie::pattern::{self, Action};
use synthie::presets::sid::default_patches;

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

/// The drum kit, in column order: every hit the drum machine can play.
pub const KIT: [DrumHit; KIT_SIZE] = DrumHit::ALL;

/// Number of drums in [`KIT`].
pub const KIT_SIZE: usize = DrumHit::ALL.len();

/// Which drum hits are active at a single step, one flag per [`KIT`] entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DrumStep([bool; KIT_SIZE]);

impl DrumStep {
    /// Whether `hit` fires on this step.
    #[must_use]
    pub fn has(self, hit: DrumHit) -> bool {
        kit_index(hit).is_some_and(|i| self.0[i])
    }

    /// Set whether `hit` fires on this step.
    pub fn set(&mut self, hit: DrumHit, on: bool) {
        if let Some(i) = kit_index(hit) {
            self.0[i] = on;
        }
    }

    /// Toggle `hit` on this step.
    pub fn toggle(&mut self, hit: DrumHit) {
        self.set(hit, !self.has(hit));
    }

    /// Whether any drum fires on this step.
    #[must_use]
    pub fn any(self) -> bool {
        self.0.contains(&true)
    }

    /// The hits on this step, in kit order.
    pub fn hits(self) -> impl Iterator<Item = DrumHit> {
        KIT.into_iter().filter(move |&hit| self.has(hit))
    }
}

/// Column of `hit` in [`KIT`].
fn kit_index(hit: DrumHit) -> Option<usize> {
    KIT.iter().position(|&h| h == hit)
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

// ─── Loading pattern songs ───────────────────────────────────────────────────

/// Why a pattern song does not fit the tracker grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The song has no rows, or more than [`STEPS`].
    Length(usize),
    /// A note, note-off or patch change on a channel the tracker has no
    /// track for.
    Channel(u8),
    /// Two lanes write the same step of the same track.
    Overlap { track: usize, step: usize },
    /// A patch change after the first row; the tracker has one patch per
    /// track.
    PatchChange { track: usize, step: usize },
    /// A patch that is not in the preset bank.
    UnknownPatch(u16),
    /// A chip arpeggio, which the grid cannot hold.
    Arpeggio { track: usize, step: usize },
    /// A chord cell; a track plays one note per step.
    Chord { track: usize, step: usize },
}

impl core::fmt::Display for LoadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Length(n) => write!(f, "song has {n} rows; the tracker holds 1..={STEPS}"),
            Self::Channel(ch) => write!(f, "channel {ch} has no tracker track"),
            Self::Overlap { track, step } => {
                write!(f, "two lanes write track {track}, step {step}")
            }
            Self::PatchChange { track, step } => {
                write!(
                    f,
                    "patch change on track {track} at step {step}; only step 0 can set it"
                )
            }
            Self::UnknownPatch(i) => write!(f, "patch @{i} is not in the preset bank"),
            Self::Arpeggio { track, step } => {
                write!(
                    f,
                    "arpeggio on track {track}, step {step} has no tracker equivalent"
                )
            }
            Self::Chord { track, step } => {
                write!(
                    f,
                    "chord on track {track}, step {step}; a track plays one note per step"
                )
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// Load a pattern song into the tracker grid.
///
/// Rows become steps, so the song should be written in sixteenths: the
/// tracker's BPM is the row rate divided by four. Each lane's notes and
/// note-offs go to the track of its channel and its drum hits to the drum
/// track; a patch change on the first row picks the track's preset from
/// `presets` (by comparing parameters). Tracks without one keep the default
/// assignment.
///
/// # Errors
///
/// Fails on anything the grid cannot hold; see [`LoadError`].
pub fn from_pattern(song: &pattern::Song, presets: &[Patch]) -> Result<Song, LoadError> {
    let len = song.len();
    if !(1..=STEPS).contains(&len) {
        return Err(LoadError::Length(len));
    }
    let mut out = Song {
        bpm: song.tempo.as_rows_per_minute() / 4.0,
        ..Song::default()
    };
    out.pattern.len = len;

    for lane in &song.lanes {
        let track = lane.channel.as_usize();
        let melodic = || {
            if track < SYNTH_TRACKS {
                Ok(track)
            } else {
                Err(LoadError::Channel(lane.channel.0))
            }
        };
        for (step, cell) in lane.rows.iter().enumerate() {
            if let Some(index) = cell.patch {
                let track = melodic()?;
                if step > 0 {
                    return Err(LoadError::PatchChange { track, step });
                }
                let params = song
                    .patches
                    .get(usize::from(index))
                    .ok_or(LoadError::UnknownPatch(index))?;
                out.track_patches[track] = presets
                    .iter()
                    .position(|p| p.params == *params)
                    .ok_or(LoadError::UnknownPatch(index))?;
            }
            let step_value = match cell.action {
                Action::Hold => continue,
                Action::Drums(hits) => {
                    for hit in hits.iter() {
                        out.pattern.drums[step].set(hit, true);
                    }
                    continue;
                }
                Action::Off => Step::Off,
                Action::Note(note) => Step::Note(note),
                Action::Arp { .. } => {
                    return Err(LoadError::Arpeggio {
                        track: melodic()?,
                        step,
                    });
                }
                Action::Chord { .. } => {
                    return Err(LoadError::Chord {
                        track: melodic()?,
                        step,
                    });
                }
            };
            let track = melodic()?;
            let slot = &mut out.pattern.synth[track][step];
            if *slot != Step::Empty {
                return Err(LoadError::Overlap { track, step });
            }
            *slot = step_value;
        }
    }
    Ok(out)
}

// ─── Demo song ───────────────────────────────────────────────────────────────

/// The startup demo: [`synthie::songs::lazy_jones`], the Lazy Jones /
/// Kernkraft 400 hook. The `lazy_jones` example plays the same song.
///
/// # Panics
///
/// Panics if the built-in song stops fitting the grid; a test pins it.
#[must_use]
pub fn demo_song() -> Song {
    from_pattern(&synthie::songs::lazy_jones(), &default_patches())
        .expect("the built-in demo song fits the tracker")
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

    /// The Lazy Jones line (David Whittaker, 1984): one monophonic line of
    /// straight 8th notes in which melody and bass alternate. MIDI notes.
    const LAZY_JONES_LINE: [u8; 32] = [
        47, 71, 74, 76, 78, 59, 47, 59, // B2  B4 D5 E5 F#5  B3 B2 B3
        47, 71, 74, 76, 78, 79, 78, 74, // B2  B4 D5 E5 F#5 G5 F#5 D5
        76, 64, 52, 64, 74, 66, 78, 71, // E5 E4  E3  E4 D5 F#4 F#5 B4
        47, 59, 47, 59, 47, 59, 47, 59, // B2 B3 B2 B3 B2 B3 B2 B3
    ];

    /// Length of one pass through the hook, in steps (four bars).
    const PASS: usize = 64;

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

    fn load(build: impl FnOnce(pattern::Song) -> pattern::Song) -> Result<Song, LoadError> {
        let song = build(pattern::Song::new(pattern::Tempo::bpm(120.0, 4)));
        from_pattern(&song, &default_patches())
    }

    fn lane(song: pattern::Song, ch: u8, cells: &str) -> pattern::Song {
        song.lane(synthie::params::ChannelNo(ch), cells).unwrap()
    }

    #[test]
    fn loads_notes_offs_drums_tempo_and_presets() {
        let bass = default_patches()[0].params.clone();
        let song = load(|s| lane(lane(s.patch(bass), 2, "C4@0 . - E4"), 3, "k . k+h .")).unwrap();
        assert!((song.bpm - 120.0).abs() < f32::EPSILON);
        assert_eq!(song.pattern.len, 4);
        assert_eq!(song.track_patches, [0, 1, 0, 3]);
        let track = &song.pattern.synth[2];
        assert_eq!(
            track[..4],
            [
                Step::Note(MidiNote(60)),
                Step::Empty,
                Step::Off,
                Step::Note(MidiNote(64))
            ]
        );
        assert!(song.pattern.drums[0].has(DrumHit::Kick));
        assert!(song.pattern.drums[2].has(DrumHit::HiHatClosed));
        assert!(song.pattern.synth[3].iter().all(|s| *s == Step::Empty));
    }

    #[test]
    fn rejects_what_the_grid_cannot_hold() {
        assert_eq!(load(|s| s).unwrap_err(), LoadError::Length(0));
        let too_long = vec!["."; STEPS + 1].join(" ");
        assert_eq!(
            load(|s| lane(s, 0, &too_long)).unwrap_err(),
            LoadError::Length(STEPS + 1)
        );
        assert_eq!(
            load(|s| lane(s, 4, "C4")).unwrap_err(),
            LoadError::Channel(4)
        );
        assert_eq!(
            load(|s| lane(lane(s, 0, "C4"), 0, "D4")).unwrap_err(),
            LoadError::Overlap { track: 0, step: 0 }
        );
        assert_eq!(
            load(|s| lane(s, 0, "@0")).unwrap_err(),
            LoadError::UnknownPatch(0)
        );
        let odd = synthie::params::SynthParams {
            global: synthie::params::GlobalParams {
                volume: 0.123,
                glide_time: 0.0,
            },
            ..Default::default()
        };
        assert_eq!(
            load(|s| lane(s.patch(odd), 0, "C4@0")).unwrap_err(),
            LoadError::UnknownPatch(0)
        );
        let p = default_patches()[0].params.clone();
        assert_eq!(
            load(|s| lane(s.patch(p), 1, "C4 D4@0")).unwrap_err(),
            LoadError::PatchChange { track: 1, step: 1 }
        );
        assert_eq!(
            load(|s| lane(s, 0, ". C4^047")).unwrap_err(),
            LoadError::Arpeggio { track: 0, step: 1 }
        );
        assert_eq!(
            load(|s| lane(s, 2, ". . E2+B2")).unwrap_err(),
            LoadError::Chord { track: 2, step: 2 }
        );
    }

    #[test]
    fn the_demo_is_the_shared_lazy_jones_song() {
        let song = demo_song();
        assert!((song.bpm - 140.0).abs() < f32::EPSILON);
        assert_eq!(song.track_patches, [1, 0, 0, 3]);
        // Kick fill on the last two steps of each pass.
        for pass in 0..STEPS / PASS {
            let end = (pass + 1) * PASS;
            assert!(song.pattern.drums[end - 2].has(DrumHit::Kick));
            assert!(song.pattern.drums[end - 1].has(DrumHit::Kick));
        }
    }
}
