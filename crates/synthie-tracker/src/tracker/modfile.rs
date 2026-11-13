//! Import of 4-channel `ProTracker` `.mod` files into the tracker grid.
//!
//! Only the note data survives: each MOD channel becomes a synth track
//! played by a synthie preset, so samples, volumes and most effects are
//! dropped. Drum samples are recognised and moved to the drum track, and
//! each track gets a bass, pad or lead preset matching what it plays. The
//! original sound is available through [`crate::reference::Reference`], for
//! A/B listening.

use synthie::params::{DrumHit, MidiNote};
use synthie::presets::sid::Preset;

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
/// Sample slots in a 31-sample module (numbered 1..=31).
const SAMPLES: usize = 31;
/// An unnamed sample counts as a drum when it is not looped and at least
/// this share of its notes use one pitch: drums are played at a fixed pitch,
/// melodic samples across many.
const DRUM_PITCH_SHARE: f32 = 0.9;
/// Fewest notes a sample needs before its pitch spread says anything.
const DRUM_MIN_HITS: usize = 4;
/// Zero-crossing rates (sign changes per sample byte) separating a kick (a
/// low thump), a snare and a hi-hat (noise); measured on real modules.
const KICK_MAX_ZCR: f32 = 0.08;
const SNARE_MAX_ZCR: f32 = 0.45;
/// Noisy samples longer than these many bytes ring out: open hat, crash.
const OPEN_HAT_MIN_LEN: usize = 3_000;
const CRASH_MIN_LEN: usize = 8_000;
/// A track gets a bass preset when its notes average below this MIDI note,
/// or when its main sample is a slow wave (few zero crossings): MODs often
/// play deep samples at high note values, so the notes alone miss them.
const BASS_MAX_NOTE: f32 = 55.0;
const BASS_MAX_ZCR: f32 = 0.03;
/// A track holding a looped sample this many rows per note gets a pad.
const PAD_MIN_ROWS_PER_NOTE: f32 = 4.0;
/// A lead whose main sample crosses zero this often gets a bright preset.
const BRIGHT_MIN_ZCR: f32 = 0.15;
/// Shorter samples are single-cycle chip waveforms: their zero-crossing rate
/// reflects the loop length, not the sound, and they play as bright leads.
const CHIP_MAX_LEN: usize = 512;

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
    let samples = samples(bytes, patterns);
    let drums = drum_samples(bytes, &order[..song_len], &samples);
    let mut song = Song::default();
    let (mut speed, mut bpm) = (None, None);
    let (mut pos, mut row) = (start, 0);
    let mut len = 0;
    // A note without a sample number replays the channel's last sample.
    let mut current = [0_usize; SYNTH_TRACKS];
    let mut sounding = [false; SYNTH_TRACKS];
    let mut usage = [TrackUsage::default(); SYNTH_TRACKS];

    while len < STEPS && pos < song_len {
        let at = HEADER_LEN + usize::from(order[pos]) * PATTERN_LEN + row * SYNTH_TRACKS * 4;
        let (mut jump, mut brk) = (None, None);
        for (track, cell) in bytes[at..at + SYNTH_TRACKS * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
        {
            let (period, sample, effect, param) = decode(*cell);
            if sample != 0 {
                current[track] = sample;
            }
            let step = &mut song.pattern.synth[track][len];
            match (period, effect, param) {
                (0, 0xC, 0) => *step = Step::Off,
                (0, ..) => {}
                (p, ..) => {
                    if let Some(hit) = drums[current[track]] {
                        song.pattern.drums[len].set(hit, true);
                        // A new sample on the channel cuts the melody note.
                        if sounding[track] {
                            *step = Step::Off;
                        }
                    } else {
                        let note = period_to_midi(p);
                        *step = Step::Note(note);
                        usage[track].add(note, current[track]);
                    }
                }
            }
            sounding[track] = match *step {
                Step::Note(_) => true,
                Step::Off => false,
                Step::Empty => sounding[track],
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
    for (track, usage) in usage.iter().enumerate() {
        if let Some(preset) = usage.preset(len, &samples) {
            song.track_patches[track] = preset_index(preset);
        }
    }
    // Tracker BPM is the row rate / 4; a MOD row lasts speed × 2.5 / bpm s.
    song.bpm =
        (6.0 * f32::from(bpm.unwrap_or(125)) / f32::from(speed.unwrap_or(6))).clamp(30.0, 300.0);
    Ok((title, song))
}

/// Period, sample number, effect and parameter of a pattern cell. A sample
/// number past the 31 a module has reads as 0 (none).
fn decode(cell: [u8; 4]) -> (u16, usize, u8, u8) {
    let period = u16::from(cell[0] & 0x0F) << 8 | u16::from(cell[1]);
    let sample = usize::from(cell[0] & 0xF0 | cell[2] >> 4);
    let sample = if sample > SAMPLES { 0 } else { sample };
    (period, sample, cell[2] & 0x0F, cell[3])
}

/// One sample's header and data.
#[derive(Default)]
struct Sample<'a> {
    name: String,
    looped: bool,
    /// The sample's bytes; shorter than its header says (or empty) when the
    /// file is cut off, which is common and harmless for note import.
    data: &'a [u8],
}

impl Sample<'_> {
    /// Sign changes per byte of the (signed 8-bit) sample: low for a deep
    /// thump, high for noise.
    #[allow(clippy::cast_precision_loss)] // sample lengths are far below 2^24
    fn zero_crossing_rate(&self) -> f32 {
        let crossings = self
            .data
            .windows(2)
            .filter(|w| (w[0] >= 0x80) != (w[1] >= 0x80))
            .count();
        crossings as f32 / self.data.len().max(1) as f32
    }
}

/// Samples 1..=31 at indices 1..=31 (index 0, "no sample", stays empty).
fn samples(bytes: &[u8], patterns: usize) -> Vec<Sample<'_>> {
    let mut at = HEADER_LEN + patterns * PATTERN_LEN;
    let mut out = vec![Sample::default()];
    for header in bytes[20..20 + SAMPLES * 30].as_chunks::<30>().0 {
        let words = |i: usize| usize::from(u16::from_be_bytes([header[i], header[i + 1]]));
        let len = words(22) * 2;
        let end = (at + len).min(bytes.len());
        out.push(Sample {
            name: String::from_utf8_lossy(&header[..22])
                .trim_end_matches(['\0', ' '])
                .to_owned(),
            looped: words(28) > 1,
            data: bytes.get(at..end).unwrap_or_default(),
        });
        at += len;
    }
    out
}

/// The drum hit each sample number plays, if it is a drum: by name, or for
/// an unnamed or unrecognised sample by being unlooped and played at one
/// pitch (see [`DRUM_PITCH_SHARE`]), typed by how noisy and long it is.
fn drum_samples(bytes: &[u8], order: &[u8], samples: &[Sample]) -> [Option<DrumHit>; SAMPLES + 1] {
    // Notes per sample and per (sample, period), over the whole song.
    let mut hits = [0_usize; SAMPLES + 1];
    let mut periods = std::collections::HashMap::<(usize, u16), usize>::new();
    for &pattern in order {
        let mut current = [0_usize; SYNTH_TRACKS];
        let at = HEADER_LEN + usize::from(pattern) * PATTERN_LEN;
        for (i, cell) in bytes[at..at + PATTERN_LEN]
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
        {
            let (period, sample, ..) = decode(*cell);
            let channel = i % SYNTH_TRACKS;
            if sample != 0 {
                current[channel] = sample;
            }
            if period != 0 && current[channel] != 0 {
                hits[current[channel]] += 1;
                *periods.entry((current[channel], period)).or_default() += 1;
            }
        }
    }
    let mut top = [0_usize; SAMPLES + 1];
    for (&(sample, _), &n) in &periods {
        top[sample] = top[sample].max(n);
    }

    let mut drums = [None; SAMPLES + 1];
    for (number, sample) in samples.iter().enumerate().skip(1) {
        #[allow(clippy::cast_precision_loss)] // note counts are small
        let fixed_pitch = hits[number] >= DRUM_MIN_HITS
            && top[number] as f32 >= DRUM_PITCH_SHARE * hits[number] as f32;
        drums[number] = drum_by_name(&sample.name).or_else(|| {
            (fixed_pitch && !sample.looped && !sample.data.is_empty())
                .then(|| drum_by_sound(sample))
        });
    }
    drums
}

/// A drum named in a sample's name, e.g. `BD1`, `SnareDrum`, `hihat open`.
fn drum_by_name(name: &str) -> Option<DrumHit> {
    let name = name.to_ascii_lowercase();
    let words: Vec<&str> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(|w| w.trim_end_matches(|c: char| c.is_ascii_digit()))
        .collect();
    // Short words must stand alone (`hat`, not `what`); long ones may be
    // part of a longer name (`SnareDrum`).
    let word = |w: &str| words.contains(&w);
    let part = |w: &str| name.contains(w);
    if part("kick") || part("bassdrum") || word("bd") {
        Some(DrumHit::Kick)
    } else if part("snare") || word("sd") {
        Some(DrumHit::Snare)
    } else if part("clap") {
        Some(DrumHit::Clap)
    } else if part("crash") || part("cymbal") || word("ride") {
        Some(DrumHit::Crash)
    } else if part("hihat") || word("hat") || word("hh") || word("oh") {
        Some(if word("open") || word("oh") {
            DrumHit::HiHatOpen
        } else {
            DrumHit::HiHatClosed
        })
    } else if word("tom") {
        Some(DrumHit::TomMid)
    } else {
        None
    }
}

/// The drum a sample sounds like: a kick is a slow wave, a snare noisier, a
/// hat pure noise, ringing longer for an open hat or crash.
fn drum_by_sound(sample: &Sample) -> DrumHit {
    let zcr = sample.zero_crossing_rate();
    let len = sample.data.len();
    if zcr < KICK_MAX_ZCR {
        DrumHit::Kick
    } else if zcr < SNARE_MAX_ZCR {
        DrumHit::Snare
    } else if len >= CRASH_MIN_LEN {
        DrumHit::Crash
    } else if len >= OPEN_HAT_MIN_LEN {
        DrumHit::HiHatOpen
    } else {
        DrumHit::HiHatClosed
    }
}

/// What a synth track plays over the imported rows, to pick its preset.
#[derive(Clone, Copy, Default)]
struct TrackUsage {
    notes: usize,
    midi_sum: usize,
    /// Notes per sample number.
    by_sample: [usize; SAMPLES + 1],
}

impl TrackUsage {
    fn add(&mut self, note: MidiNote, sample: usize) {
        self.notes += 1;
        self.midi_sum += usize::from(note.0);
        self.by_sample[sample] += 1;
    }

    /// A bass for low tracks or a deep main sample, a pad for long notes on a
    /// looped (non-chip) sample, else a lead, bright or soft like the track's
    /// main sample. `None` keeps the default for a track without notes.
    #[allow(clippy::cast_precision_loss)] // note counts are small
    fn preset(&self, rows: usize, samples: &[Sample]) -> Option<Preset> {
        if self.notes == 0 {
            return None;
        }
        let main = (0..=SAMPLES)
            .max_by_key(|&s| self.by_sample[s])
            .unwrap_or(0);
        let main = samples.get(main);
        let chip = main.is_none_or(|s| s.data.len() < CHIP_MAX_LEN);
        let zcr = main.map_or(0.0, Sample::zero_crossing_rate);
        let bright = chip || zcr >= BRIGHT_MIN_ZCR;
        let mean = self.midi_sum as f32 / self.notes as f32;
        let rows_per_note = rows as f32 / self.notes as f32;
        Some(if mean < BASS_MAX_NOTE || !chip && zcr < BASS_MAX_ZCR {
            if bright {
                Preset::C64Bass
            } else {
                Preset::TriBass
            }
        } else if !chip && main.is_some_and(|s| s.looped) && rows_per_note >= PAD_MIN_ROWS_PER_NOTE
        {
            Preset::TriPad
        } else if bright {
            Preset::PwmLead
        } else {
            Preset::PluckyPulse
        })
    }
}

/// Index of `preset` in the preset bank (`default_patches()` order).
fn preset_index(preset: Preset) -> usize {
    Preset::ALL.iter().position(|&p| p == preset).unwrap_or(0)
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

    /// Give `b`'s sample `number` this data; call in ascending sample order.
    fn add_sample(b: &mut Vec<u8>, number: usize, data: &[u8], looped: bool) {
        let at = 20 + 30 * (number - 1);
        let words = u16::try_from(data.len() / 2).unwrap().to_be_bytes();
        b[at + 22..at + 24].copy_from_slice(&words);
        if looped {
            b[at + 28..at + 30].copy_from_slice(&words);
        }
        b.extend_from_slice(data);
    }

    /// A wave of `len` bytes that changes sign every `half` bytes.
    fn wave(len: usize, half: usize) -> Vec<u8> {
        (0..len)
            .map(|i| {
                if (i / half).is_multiple_of(2) {
                    0x40
                } else {
                    0xC0
                }
            })
            .collect()
    }

    fn sample(data: &[u8]) -> Sample<'_> {
        Sample {
            data,
            ..Sample::default()
        }
    }

    #[test]
    fn drums_are_recognised_by_name() {
        assert_eq!(drum_by_name("BD1"), Some(DrumHit::Kick));
        assert_eq!(drum_by_name("SnareDrum 2"), Some(DrumHit::Snare));
        assert_eq!(drum_by_name("hihat open"), Some(DrumHit::HiHatOpen));
        assert_eq!(drum_by_name("HH"), Some(DrumHit::HiHatClosed));
        assert_eq!(drum_by_name("ride.cymbal"), Some(DrumHit::Crash));
        assert_eq!(drum_by_name("tom 3"), Some(DrumHit::TomMid));
        // Short words only count on their own.
        for melodic in ["SuperHyperBass", "whatever", "custom bottom", "pride"] {
            assert_eq!(drum_by_name(melodic), None, "{melodic}");
        }
    }

    #[test]
    fn unnamed_drums_are_typed_by_sound() {
        assert_eq!(drum_by_sound(&sample(&wave(2_000, 50))), DrumHit::Kick);
        assert_eq!(drum_by_sound(&sample(&wave(2_000, 4))), DrumHit::Snare);
        assert_eq!(
            drum_by_sound(&sample(&wave(1_000, 1))),
            DrumHit::HiHatClosed
        );
        assert_eq!(drum_by_sound(&sample(&wave(5_000, 1))), DrumHit::HiHatOpen);
        assert_eq!(drum_by_sound(&sample(&wave(10_000, 1))), DrumHit::Crash);
    }

    #[test]
    fn drum_samples_go_to_the_drum_track_and_cut_the_note() {
        let mut b = module(1);
        // Sample 1: unlooped and always at one pitch, a drum. Sample 2: melodic.
        add_sample(&mut b, 1, &wave(2_000, 50), false);
        add_sample(&mut b, 2, &wave(2_000, 8), true);
        let at = |row: usize| HEADER_LEN + row * SYNTH_TRACKS * 4;
        let cell = |sample: u8, period: u16| {
            let [hi, lo] = period.to_be_bytes();
            [sample & 0xF0 | hi, lo, (sample & 0x0F) << 4, 0]
        };
        b[at(0)..at(0) + 4].copy_from_slice(&cell(2, 428));
        b[at(2)..at(2) + 4].copy_from_slice(&cell(2, 381));
        for row in [4, 8, 12, 16] {
            b[at(row)..at(row) + 4].copy_from_slice(&cell(1, 428));
        }
        // No sample number: replays the channel's last sample, the drum.
        b[at(20)..at(20) + 4].copy_from_slice(&cell(0, 428));

        let (_, song) = import(&b, 0).unwrap();
        let synth = &song.pattern.synth[0];
        assert_eq!(synth[0], Step::Note(MidiNote(60)));
        assert_eq!(synth[2], Step::Note(MidiNote(62)));
        assert_eq!(synth[4], Step::Off, "the first drum cuts the melody note");
        for row in [8, 12, 16, 20] {
            assert_eq!(synth[row], Step::Empty, "row {row}: nothing left to cut");
        }
        for row in [4, 8, 12, 16, 20] {
            assert!(song.pattern.drums[row].has(DrumHit::Kick), "row {row}");
        }
        assert!(!song.pattern.drums[0].any());
    }

    #[test]
    fn tracks_get_presets_for_what_they_play() {
        let mut b = module(1);
        add_sample(&mut b, 1, &wave(2_000, 100), false); // deep: bass
        add_sample(&mut b, 2, &wave(2_000, 8), true); // looped: pad when held
        add_sample(&mut b, 3, &wave(32, 8), true); // chip waveform: bright lead
        let mut note = |row: usize, ch: usize, sample: u8, period: u16| {
            let at = HEADER_LEN + (row * SYNTH_TRACKS + ch) * 4;
            let [hi, lo] = period.to_be_bytes();
            b[at..at + 4].copy_from_slice(&[sample & 0xF0 | hi, lo, (sample & 0x0F) << 4, 0]);
        };
        for row in (0..ROWS).step_by(8) {
            note(row, 0, 1, 254 + 4 * u16::try_from(row).unwrap()); // varied pitch
            note(row, 1, 2, 428 - 2 * u16::try_from(row).unwrap());
            note(row, 2, 3, 214 + u16::try_from(row).unwrap());
        }
        let (_, song) = import(&b, 0).unwrap();
        let presets = song.track_patches.map(|i| Preset::ALL[i]);
        assert_eq!(
            presets,
            [
                Preset::TriBass,
                Preset::TriPad,
                Preset::PwmLead,
                Preset::ALL[3]
            ],
            "track 4 plays nothing and keeps its default"
        );
    }

    #[test]
    fn sample_numbers_past_31_are_ignored() {
        let mut b = module(1);
        // Sample number 0xFF on a note: no such sample, so a plain note.
        b[HEADER_LEN..HEADER_LEN + 4].copy_from_slice(&[0xF1, 0xAC, 0xF0, 0]);
        let (_, song) = import(&b, 0).unwrap();
        assert_eq!(song.pattern.synth[0][0], Step::Note(MidiNote(60)));
    }
}
