//! Tracker-style pattern sequencer for game music.
//!
//! A [`Song`] is a set of lanes that advance one row at a time at a fixed
//! tempo. Each lane is written in a compact text notation and plays on one
//! synthesis channel. [`SongPlayer`] implements [`Sequencer`], so a song
//! plays sample-accurately on any host that drives a
//! [`SequencedProcessor`](crate::audio::sequencer::SequencedProcessor),
//! including the cpal engine.
//!
//! # Notation
//!
//! A lane is a whitespace-separated list of cells, one per row. Line breaks
//! are whitespace too, so long patterns can be laid out a few bars per line.
//!
//! | Cell | Meaning |
//! |---|---|
//! | `C4`, `F#3`, `Bb2` | Note on (C4 is middle C, MIDI 60). Releases the lane's previous note. |
//! | `C4^047` | Chip arpeggio: cycle the notes at the given hex semitone offsets from the root (2-4 digits) on the channel's arpeggiator. Needs the `arp` feature. |
//! | `.` | Hold: nothing changes. |
//! | `-` | Note off: release the lane's note. |
//! | `k` `s` `h` `o` `c` `x` | Kick, snare, closed hi-hat, open hi-hat, crash, clap. |
//! | `lt` `mt` `ht` | Low, mid, high tom. |
//! | `k+c`, `s+h` | Several drum hits on the same row. |
//! | `@2`, `C4@2` | Switch the lane's channel to patch 2 of the song (then play the note). |
//! | `\|` | Bar line; ignored, for readability. |
//!
//! Each lane is monophonic like a sound-chip channel: a new note releases the
//! lane's previous one. Chords need several lanes, which may share a channel
//! (and its patch and four voices), or an arpeggio. Drum hits ring out on the
//! drum machine independently of the lane.
//!
//! ```
//! use synthie::params::ChannelNo;
//! use synthie::pattern::{Song, Tempo};
//!
//! // One row per eighth note at 152 bpm.
//! let song = Song::new(Tempo::bpm(152.0, 2))
//!     .lane(ChannelNo(0), "E4 . G4 . A4 . B4 . | E5 . . . - . . .")?
//!     .lane(ChannelNo(1), "E2 E2 E2 E2 G2 A2 . . | E2 . . . - . . .")?
//!     .lane(ChannelNo(3), "k+c h s h k k s h | k h s h k k s o")?;
//! assert_eq!(song.len(), 16);
//! # Ok::<(), synthie::pattern::ParseError>(())
//! ```
//!
//! # Timing
//!
//! Rows last `60 / rows_per_minute` seconds. The player carries the
//! fractional frames of each row over to the next, so the tempo does not
//! drift however the row length divides the sample rate. Lanes shorter than
//! the song hold their last state until it loops.

use core::fmt;

use crate::audio::sequencer::Sequencer;
use crate::params::{AudioEvent, ChannelNo, DrumHit, MidiNote, SynthParams};

/// Most notes an arpeggio cell can cycle through (the arpeggiator's limit).
pub const MAX_ARP_NOTES: usize = 4;

/// How fast a song advances.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    rows_per_minute: f32,
}

impl Tempo {
    /// `bpm` beats per minute with `rows_per_beat` rows to a beat, e.g. 2
    /// for one row per eighth note or 4 for sixteenths.
    ///
    /// # Panics
    ///
    /// Panics unless the resulting row rate is finite and positive.
    #[must_use]
    pub fn bpm(bpm: f32, rows_per_beat: u16) -> Self {
        Self::rows_per_minute(bpm * f32::from(rows_per_beat))
    }

    /// A raw row rate.
    ///
    /// # Panics
    ///
    /// Panics unless `rows_per_minute` is finite and positive.
    #[must_use]
    pub fn rows_per_minute(rows_per_minute: f32) -> Self {
        assert!(
            rows_per_minute.is_finite() && rows_per_minute > 0.0,
            "tempo must be finite and positive, got {rows_per_minute} rows per minute"
        );
        Self { rows_per_minute }
    }

    /// Rows per minute.
    #[must_use]
    pub fn as_rows_per_minute(self) -> f32 {
        self.rows_per_minute
    }

    /// Length of one row in frames at `sample_rate`.
    #[must_use]
    pub fn frames_per_row(self, sample_rate: f32) -> f64 {
        f64::from(sample_rate) * 60.0 / f64::from(self.rows_per_minute)
    }
}

/// A set of drum hits that fire on the same row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DrumHits(u16);

impl DrumHits {
    /// No hits.
    pub const NONE: Self = Self(0);

    /// Add `hit` to the set.
    #[must_use]
    pub fn with(self, hit: DrumHit) -> Self {
        Self(self.0 | 1 << Self::index(hit))
    }

    /// Whether `hit` is in the set.
    #[must_use]
    pub fn contains(self, hit: DrumHit) -> bool {
        self.0 & (1 << Self::index(hit)) != 0
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The hits in kit order.
    pub fn iter(self) -> impl Iterator<Item = DrumHit> {
        DrumHit::ALL
            .into_iter()
            .filter(move |&hit| self.contains(hit))
    }

    fn index(hit: DrumHit) -> usize {
        DrumHit::ALL
            .iter()
            .position(|&h| h == hit)
            .expect("DrumHit::ALL lists every hit")
    }
}

impl FromIterator<DrumHit> for DrumHits {
    fn from_iter<I: IntoIterator<Item = DrumHit>>(iter: I) -> Self {
        iter.into_iter().fold(Self::NONE, Self::with)
    }
}

/// What a cell does to its lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `.`: nothing changes.
    Hold,
    /// `-`: release the lane's note.
    Off,
    /// Note on, releasing the lane's previous note.
    Note(MidiNote),
    /// Note on as a chip arpeggio over `notes[..count]`.
    Arp {
        /// The notes to cycle through; only `..count` are used.
        notes: [MidiNote; MAX_ARP_NOTES],
        /// Number of notes, `2..=4`.
        count: u8,
    },
    /// One or more drum hits.
    Drums(DrumHits),
}

/// One row of a lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// Switch the lane's channel to this patch of the song first.
    pub patch: Option<u16>,
    /// What the cell plays.
    pub action: Action,
}

impl Cell {
    /// A cell that holds.
    pub const HOLD: Self = Self {
        patch: None,
        action: Action::Hold,
    };
}

/// One track of a song: the channel it plays on and its rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lane {
    /// The synthesis channel the lane's notes play on.
    pub channel: ChannelNo,
    /// One cell per row.
    pub rows: Vec<Cell>,
}

impl Lane {
    /// Parse a lane written in the [notation](self#notation).
    ///
    /// # Errors
    ///
    /// Fails on the first token that is not a valid cell; the error locates
    /// it in `pattern`. Its `lane` is 0.
    pub fn parse(channel: ChannelNo, pattern: &str) -> Result<Self, ParseError> {
        let mut rows = Vec::new();
        for (offset, token) in tokens(pattern) {
            if token == "|" {
                continue;
            }
            let cell =
                parse_cell(token).map_err(|kind| ParseError::new(pattern, offset, token, kind))?;
            rows.push(cell);
        }
        Ok(Self { channel, rows })
    }
}

/// Whitespace-separated tokens with their byte offsets.
fn tokens(pattern: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut rest = pattern;
    core::iter::from_fn(move || {
        let start = rest.find(|c: char| !c.is_whitespace())?;
        let len = rest[start..]
            .find(char::is_whitespace)
            .unwrap_or(rest.len() - start);
        let offset = pattern.len() - rest.len() + start;
        let token = &rest[start..start + len];
        rest = &rest[start + len..];
        Some((offset, token))
    })
}

fn parse_cell(token: &str) -> Result<Cell, ParseErrorKind> {
    let (body, patch) = match token.split_once('@') {
        Some((body, index)) => {
            let index = index
                .parse::<u16>()
                .map_err(|_| ParseErrorKind::InvalidPatch)?;
            (body, Some(index))
        }
        None => (token, None),
    };

    let action = match body {
        "" if patch.is_some() => Action::Hold,
        "." => Action::Hold,
        "-" => Action::Off,
        _ if body.starts_with(|c: char| c.is_ascii_uppercase()) => parse_note_or_arp(body)?,
        _ => {
            if patch.is_some() {
                return Err(ParseErrorKind::PatchOnDrums);
            }
            Action::Drums(parse_drums(body)?)
        }
    };
    Ok(Cell { patch, action })
}

fn parse_drums(body: &str) -> Result<DrumHits, ParseErrorKind> {
    body.split('+')
        .map(|name| match name {
            "k" => Ok(DrumHit::Kick),
            "s" => Ok(DrumHit::Snare),
            "h" => Ok(DrumHit::HiHatClosed),
            "o" => Ok(DrumHit::HiHatOpen),
            "c" => Ok(DrumHit::Crash),
            "x" => Ok(DrumHit::Clap),
            "lt" => Ok(DrumHit::TomLow),
            "mt" => Ok(DrumHit::TomMid),
            "ht" => Ok(DrumHit::TomHigh),
            _ => Err(ParseErrorKind::UnknownToken),
        })
        .collect()
}

fn parse_note_or_arp(body: &str) -> Result<Action, ParseErrorKind> {
    let Some((name, offsets)) = body.split_once('^') else {
        return parse_note(body).map(Action::Note);
    };
    let root = parse_note(name)?;
    if !cfg!(feature = "arp") {
        return Err(ParseErrorKind::ArpUnsupported);
    }
    if !(2..=MAX_ARP_NOTES).contains(&offsets.len()) {
        return Err(ParseErrorKind::InvalidArp);
    }
    let mut notes = [root; MAX_ARP_NOTES];
    for (slot, digit) in notes.iter_mut().zip(offsets.chars()) {
        let offset = digit.to_digit(16).ok_or(ParseErrorKind::InvalidArp)?;
        let midi = u32::from(root.0) + offset;
        *slot = MidiNote(
            u8::try_from(midi)
                .ok()
                .filter(|&m| m <= 127)
                .ok_or(ParseErrorKind::NoteOutOfRange)?,
        );
    }
    #[allow(clippy::cast_possible_truncation, reason = "2..=4 offsets")]
    let count = offsets.len() as u8;
    Ok(Action::Arp { notes, count })
}

/// Parse a note name such as `C4`, `F#3`, `Bb2` or `C-1` (C4 is MIDI 60).
///
/// # Errors
///
/// Fails on a malformed name or a note outside MIDI 0..=127.
pub fn parse_note(name: &str) -> Result<MidiNote, ParseErrorKind> {
    let mut chars = name.chars();
    let semitone: i32 = match chars.next() {
        Some('C') => 0,
        Some('D') => 2,
        Some('E') => 4,
        Some('F') => 5,
        Some('G') => 7,
        Some('A') => 9,
        Some('B') => 11,
        _ => return Err(ParseErrorKind::InvalidNote),
    };
    let rest = chars.as_str();
    let (accidental, octave) = match rest.as_bytes().first() {
        Some(b'#') => (1, &rest[1..]),
        Some(b'b') => (-1, &rest[1..]),
        _ => (0, rest),
    };
    let octave: i32 = octave.parse().map_err(|_| ParseErrorKind::InvalidNote)?;
    let midi = (octave + 1) * 12 + semitone + accidental;
    u8::try_from(midi)
        .ok()
        .filter(|&m| m <= 127)
        .map(MidiNote)
        .ok_or(ParseErrorKind::NoteOutOfRange)
}

/// Why a token could not be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseErrorKind {
    /// Not a note, drum, `.`, `-`, `@` or `|`.
    UnknownToken,
    /// Starts like a note (`A`-`G`) but is not one.
    InvalidNote,
    /// A note, or an arpeggio note, outside MIDI 0..=127.
    NoteOutOfRange,
    /// An arpeggio suffix that is not 2-4 hex digits.
    InvalidArp,
    /// An arpeggio without the `arp` feature.
    ArpUnsupported,
    /// An `@` not followed by a patch number.
    InvalidPatch,
    /// A patch change on a drum cell; drums have no patch.
    PatchOnDrums,
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownToken => {
                "unknown token; expected a note, `.`, `-`, a drum (k s h o c x lt mt ht), `@n` or `|`"
            }
            Self::InvalidNote => "invalid note; expected e.g. `C4`, `F#3` or `Bb2`",
            Self::NoteOutOfRange => "note outside MIDI range C-1..G9",
            Self::InvalidArp => "invalid arpeggio; expected 2-4 hex semitone offsets, e.g. `C4^047`",
            Self::ArpUnsupported => "arpeggios need the `arp` feature",
            Self::InvalidPatch => "invalid patch change; expected `@` and a number, e.g. `@2`",
            Self::PatchOnDrums => "drum cells cannot change the patch",
        })
    }
}

/// A token in a lane that could not be parsed, with its location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Index of the lane in the song (0 for [`Lane::parse`]).
    pub lane: usize,
    /// 1-based line of the token within the pattern.
    pub line: usize,
    /// 1-based column (in characters) of the token within its line.
    pub column: usize,
    /// Byte offset of the token within the pattern.
    pub offset: usize,
    /// The offending token.
    pub token: String,
    /// What is wrong with it.
    pub kind: ParseErrorKind,
}

impl ParseError {
    fn new(pattern: &str, offset: usize, token: &str, kind: ParseErrorKind) -> Self {
        let before = &pattern[..offset];
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        Self {
            lane: 0,
            line: before.matches('\n').count() + 1,
            column: before[line_start..].chars().count() + 1,
            offset,
            token: token.into(),
            kind,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "lane {}, line {}, column {}: `{}`: {}",
            self.lane, self.line, self.column, self.token, self.kind
        )
    }
}

impl core::error::Error for ParseError {}

/// A piece of music: lanes played in lockstep at a fixed tempo.
#[derive(Debug, Clone, PartialEq)]
pub struct Song {
    /// How fast rows advance.
    pub tempo: Tempo,
    /// Whether to jump back to the first row after the last (music) or stop
    /// and release every note (jingles).
    pub looped: bool,
    /// Patches that `@n` cells switch to.
    pub patches: Vec<SynthParams>,
    /// The tracks.
    pub lanes: Vec<Lane>,
}

impl Song {
    /// An empty, looping song.
    #[must_use]
    pub fn new(tempo: Tempo) -> Self {
        Self {
            tempo,
            looped: true,
            patches: Vec::new(),
            lanes: Vec::new(),
        }
    }

    /// Play once and release every note at the end instead of looping.
    #[must_use]
    pub fn one_shot(mut self) -> Self {
        self.looped = false;
        self
    }

    /// Add a patch for `@n` cells; the first one added is `@0`.
    #[must_use]
    pub fn patch(mut self, params: SynthParams) -> Self {
        self.patches.push(params);
        self
    }

    /// Parse and add a lane playing on `channel`.
    ///
    /// # Errors
    ///
    /// Fails on the first invalid token; the error names the lane by its
    /// index in the song.
    pub fn lane(mut self, channel: ChannelNo, pattern: &str) -> Result<Self, ParseError> {
        let lane = Lane::parse(channel, pattern).map_err(|e| ParseError {
            lane: self.lanes.len(),
            ..e
        })?;
        self.lanes.push(lane);
        Ok(self)
    }

    /// Length in rows: that of the longest lane.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lanes.iter().map(|l| l.rows.len()).max().unwrap_or(0)
    }

    /// Whether the song has no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Length of one pass in seconds, without release tails.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn duration_secs(&self) -> f32 {
        self.len() as f32 * 60.0 / self.tempo.rows_per_minute
    }
}

/// What a lane is currently holding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Sounding {
    #[default]
    Nothing,
    Note(ChannelNo, MidiNote),
    #[cfg(feature = "arp")]
    Arp(ChannelNo, [MidiNote; MAX_ARP_NOTES], u8),
}

/// Plays a [`Song`] as a [`Sequencer`].
///
/// Hand it to a
/// [`SequencedProcessor`](crate::audio::sequencer::SequencedProcessor) or
/// to the cpal engine's `setup_audio_*_with_sequencer`. The first row plays
/// on the first tick.
///
/// Patch changes (`@n`) clone the patch into a boxed
/// [`AudioEvent::LoadPatchChannel`], so they allocate on the audio thread;
/// everything else does not.
#[derive(Debug, Clone)]
pub struct SongPlayer {
    song: Song,
    /// Next row to play; `song.len()` once a one-shot song has ended.
    row: usize,
    /// Fractional frames owed to the next row.
    frame_error: f64,
    lanes: Vec<Sounding>,
    finished: bool,
}

impl SongPlayer {
    /// Prepare to play `song` from the top.
    #[must_use]
    pub fn new(song: Song) -> Self {
        let lanes = vec![Sounding::Nothing; song.lanes.len()];
        Self {
            song,
            row: 0,
            frame_error: 0.0,
            lanes,
            finished: false,
        }
    }

    /// The song being played.
    #[must_use]
    pub fn song(&self) -> &Song {
        &self.song
    }

    /// The row that plays on the next tick.
    #[must_use]
    pub fn row(&self) -> usize {
        self.row
    }

    /// Whether a one-shot song has played its last row and released its
    /// notes. Always `false` for a looping song.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Change the tempo; takes effect from the next row.
    pub fn set_tempo(&mut self, tempo: Tempo) {
        self.song.tempo = tempo;
    }

    /// Jump to `row` on the next tick, keeping held notes.
    pub fn seek(&mut self, row: usize) {
        self.row = row;
        self.finished = false;
    }

    fn release(lane: &mut Sounding, out: &mut Vec<AudioEvent>) {
        match core::mem::take(lane) {
            Sounding::Nothing => {}
            Sounding::Note(ch, note) => out.push(AudioEvent::NoteOffChannel(ch, note)),
            #[cfg(feature = "arp")]
            Sounding::Arp(ch, notes, count) => {
                for &note in &notes[..usize::from(count)] {
                    out.push(AudioEvent::NoteOffChannel(ch, note));
                }
                out.push(AudioEvent::ArpEnabled(ch, false));
            }
        }
    }

    fn play_row(&mut self, out: &mut Vec<AudioEvent>) {
        for (lane, state) in self.song.lanes.iter().zip(&mut self.lanes) {
            let cell = lane.rows.get(self.row).copied().unwrap_or(Cell::HOLD);
            let ch = lane.channel;
            if let Some(params) = cell
                .patch
                .and_then(|i| self.song.patches.get(usize::from(i)))
            {
                out.push(AudioEvent::LoadPatchChannel(ch, Box::new(params.clone())));
            }
            match cell.action {
                Action::Hold => {}
                Action::Off => Self::release(state, out),
                Action::Note(note) => {
                    Self::release(state, out);
                    out.push(AudioEvent::NoteOnChannel(ch, note));
                    *state = Sounding::Note(ch, note);
                }
                #[cfg(feature = "arp")]
                Action::Arp { notes, count } => {
                    Self::release(state, out);
                    out.push(AudioEvent::ArpEnabled(ch, true));
                    for &note in &notes[..usize::from(count)] {
                        out.push(AudioEvent::NoteOnChannel(ch, note));
                    }
                    *state = Sounding::Arp(ch, notes, count);
                }
                #[cfg(not(feature = "arp"))]
                Action::Arp { .. } => {}
                Action::Drums(hits) => out.extend(hits.iter().map(AudioEvent::Drum)),
            }
        }
    }
}

impl Sequencer for SongPlayer {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a row is a positive number of frames well below usize::MAX"
    )]
    fn tick(&mut self, sample_rate: f32, out: &mut Vec<AudioEvent>) -> usize {
        if self.finished {
            return usize::MAX;
        }
        let len = self.song.len();
        if self.row >= len {
            if self.song.looped && len > 0 {
                self.row = 0;
            } else {
                for state in &mut self.lanes {
                    Self::release(state, out);
                }
                self.finished = true;
                return usize::MAX;
            }
        }

        self.play_row(out);
        self.row += 1;

        let frames = self.song.tempo.frames_per_row(sample_rate) + self.frame_error;
        let whole = frames.floor().max(1.0);
        self.frame_error = frames - whole;
        whole as usize
    }

    fn is_finished(&self) -> bool {
        self.finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CH0: ChannelNo = ChannelNo(0);
    const CH1: ChannelNo = ChannelNo(1);

    fn note(n: &str) -> MidiNote {
        parse_note(n).unwrap()
    }

    fn cell(token: &str) -> Result<Cell, ParseErrorKind> {
        parse_cell(token)
    }

    #[test]
    fn note_names() {
        assert_eq!(note("C4"), MidiNote(60));
        assert_eq!(note("A4"), MidiNote(69));
        assert_eq!(note("F#3"), MidiNote(54));
        assert_eq!(note("Bb2"), MidiNote(46));
        assert_eq!(note("C-1"), MidiNote(0));
        assert_eq!(note("G9"), MidiNote(127));
        assert_eq!(parse_note("G#9"), Err(ParseErrorKind::NoteOutOfRange));
        assert_eq!(parse_note("Cb-1"), Err(ParseErrorKind::NoteOutOfRange));
        assert_eq!(parse_note("H4"), Err(ParseErrorKind::InvalidNote));
        assert_eq!(parse_note("C"), Err(ParseErrorKind::InvalidNote));
        assert_eq!(parse_note("C#"), Err(ParseErrorKind::InvalidNote));
        assert_eq!(parse_note("Cx4"), Err(ParseErrorKind::InvalidNote));
    }

    #[test]
    fn cells() {
        assert_eq!(cell("."), Ok(Cell::HOLD));
        assert_eq!(cell("-").unwrap().action, Action::Off);
        assert_eq!(cell("E2").unwrap().action, Action::Note(note("E2")));
        assert_eq!(
            cell("k+c").unwrap().action,
            Action::Drums([DrumHit::Kick, DrumHit::Crash].into_iter().collect())
        );
        assert_eq!(
            cell("lt+mt+ht+x+o+h+s").unwrap().action,
            Action::Drums(
                [
                    DrumHit::TomLow,
                    DrumHit::TomMid,
                    DrumHit::TomHigh,
                    DrumHit::Clap,
                    DrumHit::HiHatOpen,
                    DrumHit::HiHatClosed,
                    DrumHit::Snare
                ]
                .into_iter()
                .collect()
            )
        );
        assert_eq!(
            cell("C4@2"),
            Ok(Cell {
                patch: Some(2),
                action: Action::Note(MidiNote(60))
            })
        );
        assert_eq!(
            cell("@3"),
            Ok(Cell {
                patch: Some(3),
                action: Action::Hold
            })
        );
        assert_eq!(cell("@"), Err(ParseErrorKind::InvalidPatch));
        assert_eq!(cell("C4@x"), Err(ParseErrorKind::InvalidPatch));
        assert_eq!(cell("k@1"), Err(ParseErrorKind::PatchOnDrums));
        assert_eq!(cell("q"), Err(ParseErrorKind::UnknownToken));
        assert_eq!(cell("k+"), Err(ParseErrorKind::UnknownToken));
        assert_eq!(cell("K"), Err(ParseErrorKind::InvalidNote));
    }

    #[cfg(feature = "arp")]
    #[test]
    fn arpeggio_cells() {
        assert_eq!(
            cell("C4^047").unwrap().action,
            Action::Arp {
                notes: [MidiNote(60), MidiNote(64), MidiNote(67), MidiNote(60)],
                count: 3
            }
        );
        assert_eq!(
            cell("A3^037c").unwrap().action,
            Action::Arp {
                notes: [MidiNote(57), MidiNote(60), MidiNote(64), MidiNote(69)],
                count: 4
            }
        );
        assert_eq!(cell("C4^0"), Err(ParseErrorKind::InvalidArp));
        assert_eq!(cell("C4^01234"), Err(ParseErrorKind::InvalidArp));
        assert_eq!(cell("C4^0g"), Err(ParseErrorKind::InvalidArp));
        assert_eq!(cell("G9^04"), Err(ParseErrorKind::NoteOutOfRange));
    }

    #[cfg(not(feature = "arp"))]
    #[test]
    fn arpeggios_need_the_arp_feature() {
        assert_eq!(cell("C4^047"), Err(ParseErrorKind::ArpUnsupported));
    }

    #[test]
    fn bar_lines_are_ignored() {
        let lane = Lane::parse(CH0, "C4 . | - .\n|\tk").unwrap();
        assert_eq!(lane.rows.len(), 5);
    }

    #[test]
    fn errors_point_at_the_offending_token() {
        let err = Song::new(Tempo::bpm(120.0, 2))
            .lane(CH0, "C4 . . .")
            .unwrap()
            .lane(CH1, "C4 . . . |\n  E4 H4 . .")
            .unwrap_err();
        assert_eq!(err.lane, 1);
        assert_eq!((err.line, err.column), (2, 6));
        assert_eq!(err.offset, 16);
        assert_eq!(err.token, "H4");
        assert_eq!(err.kind, ParseErrorKind::InvalidNote);
        assert_eq!(
            err.to_string(),
            "lane 1, line 2, column 6: `H4`: invalid note; expected e.g. `C4`, `F#3` or `Bb2`"
        );
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let err = Lane::parse(CH0, "C4 ü ?").unwrap_err();
        assert_eq!((err.line, err.column, err.token.as_str()), (1, 4, "ü"));
    }

    /// `AudioEvent` has no `PartialEq`; compare the debug output.
    macro_rules! assert_events {
        ($a:expr, $b:expr $(,)?) => {
            assert_eq!(format!("{:?}", $a), format!("{:?}", $b))
        };
    }

    /// Tick `player` `rows` times; returns the events of each row.
    fn play(player: &mut SongPlayer, rows: usize) -> Vec<Vec<AudioEvent>> {
        (0..rows)
            .map(|_| {
                let mut out = Vec::new();
                player.tick(44_100.0, &mut out);
                out
            })
            .collect()
    }

    #[test]
    fn lanes_are_monophonic_and_release_on_off() {
        let song = Song::new(Tempo::bpm(120.0, 2))
            .lane(CH0, "C4 . E4 - .")
            .unwrap()
            .lane(CH1, "k . k+h . .")
            .unwrap();
        let rows = play(&mut SongPlayer::new(song), 5);
        let (c4, e4) = (MidiNote(60), MidiNote(64));
        assert_events!(
            rows,
            vec![
                vec![
                    AudioEvent::NoteOnChannel(CH0, c4),
                    AudioEvent::Drum(DrumHit::Kick)
                ],
                vec![],
                vec![
                    AudioEvent::NoteOffChannel(CH0, c4),
                    AudioEvent::NoteOnChannel(CH0, e4),
                    AudioEvent::Drum(DrumHit::Kick),
                    AudioEvent::Drum(DrumHit::HiHatClosed),
                ],
                vec![AudioEvent::NoteOffChannel(CH0, e4)],
                vec![],
            ]
        );
    }

    #[test]
    fn patch_changes_load_before_the_note() {
        let mut lead = SynthParams::default();
        lead.global.volume = 0.25;
        let song = Song::new(Tempo::bpm(120.0, 2))
            .patch(SynthParams::default())
            .patch(lead.clone())
            .lane(CH1, "C4@1 @0 @9")
            .unwrap();
        let rows = play(&mut SongPlayer::new(song), 3);
        assert_events!(
            rows[0],
            vec![
                AudioEvent::LoadPatchChannel(CH1, Box::new(lead)),
                AudioEvent::NoteOnChannel(CH1, MidiNote(60)),
            ]
        );
        assert_events!(
            rows[1],
            vec![AudioEvent::LoadPatchChannel(
                CH1,
                Box::<SynthParams>::default()
            )]
        );
        assert!(rows[2].is_empty(), "unknown patches are ignored");
    }

    #[cfg(feature = "arp")]
    #[test]
    fn arpeggios_drive_the_channels_arpeggiator() {
        let song = Song::new(Tempo::bpm(120.0, 2))
            .lane(CH0, "C4^047 G4 -")
            .unwrap();
        let rows = play(&mut SongPlayer::new(song), 3);
        let n = MidiNote;
        assert_events!(
            rows[0],
            vec![
                AudioEvent::ArpEnabled(CH0, true),
                AudioEvent::NoteOnChannel(CH0, n(60)),
                AudioEvent::NoteOnChannel(CH0, n(64)),
                AudioEvent::NoteOnChannel(CH0, n(67)),
            ]
        );
        assert_events!(
            rows[1],
            vec![
                AudioEvent::NoteOffChannel(CH0, n(60)),
                AudioEvent::NoteOffChannel(CH0, n(64)),
                AudioEvent::NoteOffChannel(CH0, n(67)),
                AudioEvent::ArpEnabled(CH0, false),
                AudioEvent::NoteOnChannel(CH0, n(67)),
            ]
        );
        assert_events!(rows[2], vec![AudioEvent::NoteOffChannel(CH0, n(67))]);
    }

    #[test]
    fn looping_songs_wrap_and_short_lanes_hold() {
        let song = Song::new(Tempo::bpm(120.0, 2))
            .lane(CH0, "C4 - . .")
            .unwrap()
            .lane(CH1, "k")
            .unwrap();
        let mut player = SongPlayer::new(song);
        let rows = play(&mut player, 6);
        assert_events!(rows[4], rows[0]);
        assert_events!(rows[5], rows[1]);
        assert!(rows[2].is_empty() && rows[3].is_empty());
        assert!(!player.is_finished());
    }

    #[test]
    fn one_shot_songs_release_everything_and_stop() {
        let song = Song::new(Tempo::bpm(120.0, 2))
            .one_shot()
            .lane(CH0, "C4 .")
            .unwrap()
            .lane(CH1, "E4 G4")
            .unwrap();
        let mut player = SongPlayer::new(song);
        play(&mut player, 2);
        assert!(!player.is_finished());
        let mut out = Vec::new();
        assert_eq!(player.tick(44_100.0, &mut out), usize::MAX);
        assert_events!(
            out,
            vec![
                AudioEvent::NoteOffChannel(CH0, MidiNote(60)),
                AudioEvent::NoteOffChannel(CH1, MidiNote(67)),
            ]
        );
        assert!(player.is_finished());
        out.clear();
        assert_eq!(player.tick(44_100.0, &mut out), usize::MAX);
        assert!(out.is_empty());
    }

    #[test]
    fn an_empty_song_finishes_at_once() {
        let mut player = SongPlayer::new(Song::new(Tempo::bpm(120.0, 2)));
        assert_eq!(player.tick(44_100.0, &mut Vec::new()), usize::MAX);
        assert!(player.is_finished());
    }

    #[test]
    fn the_tempo_does_not_drift() {
        // 152 bpm in eighths at 44.1 kHz: 8 703.9... frames per row.
        let song = Song::new(Tempo::bpm(152.0, 2)).lane(CH0, "C4").unwrap();
        let mut player = SongPlayer::new(song);
        let total: usize = (0..304)
            .map(|_| player.tick(44_100.0, &mut Vec::new()))
            .sum();
        assert_eq!(total, 44_100 * 60, "304 rows at 304 rows/min is one minute");
    }

    #[test]
    #[should_panic(expected = "tempo must be finite and positive")]
    fn a_zero_tempo_is_rejected() {
        let _ = Tempo::bpm(0.0, 4);
    }

    #[test]
    fn songs_play_through_the_processor_independent_of_buffer_size() {
        use crate::audio::processor::SynthProcessor;
        use crate::audio::sequencer::SequencedProcessor;
        use crate::presets::sid::default_patches;

        let patches = default_patches();
        let song = || {
            Song::new(Tempo::bpm(150.0, 4))
                .patch(patches[0].params.clone())
                .patch(patches[3].params.clone())
                .lane(CH0, "E2@0 . E2 . G2 . A2 . | E2 . E2 . - . . .")
                .unwrap()
                .lane(CH1, "E4@1 . G4 . A4 . B4 . | E5 . . . . . - .")
                .unwrap()
                .lane(ChannelNo(3), "k+c h s h k h s o | k h s h k x s+h ht")
                .unwrap()
        };
        let render = |block: usize| {
            let mut synth = SequencedProcessor::new(
                SynthProcessor::<4>::new(44_100.0),
                SongPlayer::new(song()),
            );
            let mut out = vec![0.0f32; 44_100];
            for chunk in out.chunks_mut(block) {
                synth.process_block(&[], chunk, 1);
            }
            out
        };
        let a = render(4_096);
        assert!(a.iter().any(|s| s.abs() > 0.05), "song is silent");
        let b = render(97);
        assert!(a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits()));
    }
}
