//! Built-in demo songs, written in the [pattern notation](crate::pattern).
//!
//! The songs here are shared by the examples and the tracker, so
//! `cargo run -p synthie --example lazy_jones` plays exactly what the tracker
//! opens with.

use crate::params::ChannelNo;
use crate::pattern::{ParseError, Song, Tempo};
use crate::presets::sid::Preset;

// ─── Lazy Jones ──────────────────────────────────────────────────────────────

const REST: &str = ". . . . . . . . . . . . . . . .";

/// The melody of the hook: B4 D5 E5 F#5 climbing out of the bass line.
const LEAD: [&str; 4] = [
    ". . B4 . D5 . E5 . F#5 - . . . . . .",
    ". . B4 . D5 . E5 . F#5 . G5 . F#5 . D5 .",
    "E5 . E4 - . . E4 . D5 . F#4 . F#5 . B4 -",
    REST,
];

/// The bass notes of the same line, filling the melody's gaps.
const BASS: [&str; 4] = [
    "B2 - . . . . . . . . B3 . B2 . B3 .",
    "B2 - . . . . . . . . . . . . . .",
    ". . . . E3 - . . . . . . . . . .",
    "B2 . B3 . B2 . B3 . B2 . B3 . B2 . B3 .",
];

/// The last bar of the bass, releasing B3 before the song loops.
const BASS_END: &str = "B2 . B3 . B2 . B3 . B2 . B3 . B2 . B3 -";

/// Off-beat sub-bass under the second pass.
const SUB: [&str; 4] = [
    ". . B1 - . . B1 - . . B1 - . . B1 -",
    ". . B1 - . . B1 - . . B1 - . . B1 -",
    ". . E2 - . . E2 - . . F#2 - . . F#2 -",
    ". . B1 - . . B1 - . . B1 - . . B1 -",
];

/// Four on the floor, sixteenth closed hats, off-beat open hats.
const BEAT: &str = "k h o h k h o h k h o h k h o h";
/// The last bar of a pass: a kick fill into the next one.
const FILL: &str = "k h o h k h o h k h o h k h k k+h";

/// The Lazy Jones hook (David Whittaker, 1984), the riff that later became
/// Kernkraft 400: two four-bar passes at 140 bpm in sixteenths, looping.
///
/// | Channel | Preset | Part |
/// |---|---|---|
/// | 0 | Arp Lead | melody of the line |
/// | 1 | C64 Bass | bass notes of the line |
/// | 2 | C64 Bass | off-beat sub-bass, second pass only |
/// | 3 | PWM Lead | empty, to play along |
///
/// Drums: four on the floor, sixteenth closed hats, off-beat open hats and
/// a kick fill into each pass. Each melodic lane starts with a patch change
/// to its preset.
///
/// # Panics
///
/// Panics if a preset it uses is missing from the built-in bank, or if its
/// patterns do not parse; tests pin both.
#[must_use]
pub fn lazy_jones() -> Song {
    try_lazy_jones().expect("the built-in Lazy Jones song is valid")
}

fn try_lazy_jones() -> Result<Song, ParseError> {
    let lead = [LEAD, LEAD].concat();
    let bass = [&BASS[..], &BASS[..3], &[BASS_END]].concat();
    let sub = [[REST; 4], SUB].concat();
    let free = [REST; 8];
    let drums = [BEAT, BEAT, BEAT, FILL].repeat(2);

    Song::new(Tempo::bpm(140.0, 4))
        .patch(Preset::ArpLead.params())
        .patch(Preset::C64Bass.params())
        .patch(Preset::PwmLead.params())
        .lane(ChannelNo(0), &with_patch(&lead, 0))?
        .lane(ChannelNo(1), &with_patch(&bass, 1))?
        .lane(ChannelNo(2), &with_patch(&sub, 1))?
        .lane(ChannelNo(3), &with_patch(&free, 2))?
        // Drum hits ignore the lane's channel.
        .lane(ChannelNo(3), &drums.join(" | "))
}

/// Join `bars` into a lane whose first cell also switches to patch `index`:
/// `.` becomes `@n`, a note `X` becomes `X@n`, so no row is added.
fn with_patch(bars: &[&str], index: u16) -> String {
    let lane = bars.join(" | ");
    let (first, rest) = lane.split_once(' ').unwrap_or((&lane, ""));
    let first = if first == "." { "" } else { first };
    format!("{first}@{index} {rest}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::{Action, Cell};

    #[test]
    fn lazy_jones_is_eight_bars_of_sixteenths_at_140_bpm() {
        let song = lazy_jones();
        assert!(song.looped);
        assert!((song.tempo.as_rows_per_minute() - 560.0).abs() < f32::EPSILON);
        assert_eq!(song.lanes.len(), 5);
        for lane in &song.lanes {
            assert_eq!(lane.rows.len(), 128, "lane on {:?}", lane.channel);
        }
    }

    #[test]
    fn every_melodic_lane_starts_on_its_preset_without_an_extra_row() {
        let song = lazy_jones();
        let first = |lane: usize| song.lanes[lane].rows[0];
        assert_eq!(
            first(0),
            Cell {
                patch: Some(0),
                action: Action::Hold
            }
        );
        assert_eq!(first(1).patch, Some(1));
        assert!(matches!(first(1).action, Action::Note(_)));
        assert_eq!(first(2).patch, Some(1));
        assert_eq!(first(3).patch, Some(2));
        assert_eq!(first(4).patch, None);
        assert_eq!(song.patches[0], Preset::ArpLead.params());
    }
}
