//! Import of 4-channel `ProTracker` `.mod` files into the tracker grid.
//!
//! Only the note data survives: each MOD channel becomes a synth track
//! played by a synthie preset, so samples, volumes and most effects are
//! dropped. The original sound is available through
//! [`crate::reference::Reference`], for A/B listening.

use synthie::params::MidiNote;

use super::{STEPS, SYNTH_TRACKS, Song, Step};

/// Header size up to the first pattern.
const HEADER_LEN: usize = 1084;
/// Rows per MOD pattern.
const ROWS: usize = 64;
/// Bytes per pattern: rows × 4 channels × 4 bytes.
const PATTERN_LEN: usize = ROWS * SYNTH_TRACKS * 4;
/// Signatures of 4-channel modules at offset 1080.
const MAGICS: [&[u8; 4]; 4] = [b"M.K.", b"M!K!", b"4CHN", b"FLT4"];
/// `ProTracker` period of C-2, mapped to MIDI note 60.
const PERIOD_C2: f32 = 428.0;

/// Why a file cannot be imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModError {
    /// Shorter than the header plus its patterns.
    Truncated,
    /// Not a 4-channel `ProTracker` module; holds the signature found.
    Signature([u8; 4]),
    /// The start position is past the song's order list.
    StartPastEnd { start: usize, len: usize },
}

impl core::fmt::Display for ModError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Truncated => write!(f, "file is truncated or not a MOD"),
            Self::Signature(sig) => write!(
                f,
                "signature {:?} is not a 4-channel ProTracker module",
                String::from_utf8_lossy(sig)
            ),
            Self::StartPastEnd { start, len } => {
                write!(
                    f,
                    "start position {start} is past the song's {len} positions"
                )
            }
        }
    }
}

impl std::error::Error for ModError {}

/// Import up to [`STEPS`] rows of `bytes`, playing the order list from
/// position `start`. Returns the module title and the song.
///
/// Rows follow playback order: pattern breaks (`Dxx`) and position jumps
/// (`Bxx`) are taken. A note's period becomes a MIDI note (C-2 = 60), a
/// volume-zero effect (`C00`) without a note becomes a note-off, and the
/// first `Fxx` sets the tempo. Track presets keep their defaults.
///
/// # Errors
///
/// Fails on anything but a well-formed 4-channel module; see [`ModError`].
pub fn import(bytes: &[u8], start: usize) -> Result<(String, Song), ModError> {
    if bytes.len() < HEADER_LEN {
        return Err(ModError::Truncated);
    }
    let sig: [u8; 4] = bytes[1080..1084].try_into().expect("4-byte slice");
    if !MAGICS.contains(&&sig) {
        return Err(ModError::Signature(sig));
    }
    let song_len = usize::from(bytes[950]).clamp(1, 128);
    let order = &bytes[952..1080];
    // ProTracker stores every pattern up to the highest in the whole table.
    let patterns = usize::from(*order.iter().max().expect("128 entries")) + 1;
    if bytes.len() < HEADER_LEN + patterns * PATTERN_LEN {
        return Err(ModError::Truncated);
    }
    if start >= song_len {
        return Err(ModError::StartPastEnd {
            start,
            len: song_len,
        });
    }

    let title = String::from_utf8_lossy(&bytes[..20])
        .trim_end_matches(['\0', ' '])
        .to_owned();
    let mut song = Song::default();
    let (mut speed, mut bpm) = (None, None);
    let (mut pos, mut row) = (start, 0);
    let mut len = 0;

    while len < STEPS && pos < song_len {
        let at = HEADER_LEN + usize::from(order[pos]) * PATTERN_LEN + row * SYNTH_TRACKS * 4;
        let (mut jump, mut brk) = (None, None);
        for (track, cell) in bytes[at..at + SYNTH_TRACKS * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
        {
            let period = u16::from(cell[0] & 0x0F) << 8 | u16::from(cell[1]);
            let (effect, param) = (cell[2] & 0x0F, cell[3]);
            song.pattern.synth[track][len] = match (period, effect, param) {
                (0, 0xC, 0) => Step::Off,
                (0, ..) => Step::Empty,
                (p, ..) => Step::Note(period_to_midi(p)),
            };
            match (effect, param) {
                (0xB, p) => jump = Some(usize::from(p)),
                (0xD, p) => {
                    brk = Some((usize::from(p >> 4) * 10 + usize::from(p & 0x0F)).min(ROWS - 1));
                }
                (0xF, p @ 1..0x20) => speed = speed.or(Some(p)),
                (0xF, p @ 0x20..) => bpm = bpm.or(Some(p)),
                _ => {}
            }
        }
        len += 1;
        if jump.is_some() || brk.is_some() {
            pos = jump.unwrap_or(pos + 1);
            row = brk.unwrap_or(0);
        } else {
            row += 1;
            if row == ROWS {
                row = 0;
                pos += 1;
            }
        }
    }

    song.pattern.len = len;
    // Tracker BPM is the row rate / 4; a MOD row lasts speed × 2.5 / bpm s.
    song.bpm =
        (6.0 * f32::from(bpm.unwrap_or(125)) / f32::from(speed.unwrap_or(6))).clamp(30.0, 300.0);
    Ok((title, song))
}

/// MIDI note of an Amiga period, with `ProTracker`'s C-2 (428) at 60.
fn period_to_midi(period: u16) -> MidiNote {
    let semis = (12.0 * (PERIOD_C2 / f32::from(period)).log2()).round();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 0..=127
    MidiNote((60.0 + semis).clamp(0.0, 127.0) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A module with `patterns` empty patterns, played in order 0, 1, ….
    fn module(patterns: u8) -> Vec<u8> {
        let mut b = vec![0; HEADER_LEN + usize::from(patterns) * PATTERN_LEN];
        b[..4].copy_from_slice(b"test");
        b[950] = patterns;
        for p in 0..patterns {
            b[952 + usize::from(p)] = p;
        }
        b[1080..1084].copy_from_slice(b"M.K.");
        b
    }

    fn set(
        b: &mut [u8],
        pattern: usize,
        row: usize,
        ch: usize,
        period: u16,
        effect: u8,
        param: u8,
    ) {
        let at = HEADER_LEN + pattern * PATTERN_LEN + (row * SYNTH_TRACKS + ch) * 4;
        b[at..at + 4].copy_from_slice(&[
            period.to_be_bytes()[0],
            period.to_be_bytes()[1],
            effect,
            param,
        ]);
    }

    #[test]
    fn periods_map_to_midi_notes() {
        assert_eq!(period_to_midi(428), MidiNote(60));
        assert_eq!(period_to_midi(856), MidiNote(48));
        assert_eq!(period_to_midi(214), MidiNote(72));
        assert_eq!(period_to_midi(404), MidiNote(61)); // C#2
    }

    #[test]
    fn imports_notes_offs_and_tempo() {
        let mut b = module(1);
        set(&mut b, 0, 0, 1, 428, 0xF, 3);
        set(&mut b, 0, 1, 2, 0, 0xF, 0x7D);
        set(&mut b, 0, 2, 1, 0, 0xC, 0);
        set(&mut b, 0, 3, 1, 0, 0xC, 0x20); // volume change, not an off
        let (title, song) = import(&b, 0).unwrap();
        assert_eq!(title, "test");
        assert_eq!(song.pattern.len, ROWS);
        assert_eq!(song.pattern.synth[1][0], Step::Note(MidiNote(60)));
        assert_eq!(song.pattern.synth[1][2], Step::Off);
        assert_eq!(song.pattern.synth[1][3], Step::Empty);
        assert!((song.bpm - 250.0).abs() < f32::EPSILON); // 6 · 125 / 3
    }

    #[test]
    fn follows_order_and_truncates_to_steps() {
        let mut b = module(3);
        set(&mut b, 1, 0, 0, 856, 0, 0);
        let (_, song) = import(&b, 0).unwrap();
        assert_eq!(song.pattern.len, STEPS);
        assert_eq!(song.pattern.synth[0][ROWS], Step::Note(MidiNote(48)));

        let (_, song) = import(&b, 1).unwrap();
        assert_eq!(song.pattern.synth[0][0], Step::Note(MidiNote(48)));
    }

    #[test]
    fn takes_pattern_breaks_and_jumps() {
        let mut b = module(2);
        set(&mut b, 0, 1, 0, 0, 0xD, 0x10); // break to row 10 of pattern 1
        set(&mut b, 1, 10, 0, 428, 0xB, 0); // then jump back to position 0
        let (_, song) = import(&b, 0).unwrap();
        assert_eq!(song.pattern.synth[0][2], Step::Note(MidiNote(60)));
        assert_eq!(song.pattern.len, STEPS);
    }

    #[test]
    fn stops_at_song_end() {
        let mut b = module(1);
        set(&mut b, 0, 9, 0, 0, 0xD, 0);
        b[950] = 1;
        // A break on the last position ends the song after 10 rows.
        assert_eq!(import(&b, 0).unwrap().1.pattern.len, 10);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(import(&[0; 100], 0).unwrap_err(), ModError::Truncated);
        let mut b = module(1);
        b[1080..1084].copy_from_slice(b"8CHN");
        assert_eq!(import(&b, 0).unwrap_err(), ModError::Signature(*b"8CHN"));
        let mut b = module(2);
        b.truncate(HEADER_LEN + PATTERN_LEN);
        assert_eq!(import(&b, 0).unwrap_err(), ModError::Truncated);
        assert_eq!(
            import(&module(1), 1).unwrap_err(),
            ModError::StartPastEnd { start: 1, len: 1 }
        );
    }
}
