//! A short rock riff on the presets from Cheesed!: Drive Guitar power chords
//! (chord cells holding root, fifth and octave), a Rock Lead on top, Tri Bass
//! underneath and drums.
//!
//! ```sh
//! cargo run -p synthie --example rock --release
//! ```

use std::time::Duration;

use anyhow::Result;
use synthie::audio::engine::setup_audio_silenced_with_sequencer;
use synthie::params::ChannelNo;
use synthie::pattern::{Song, SongPlayer, Tempo};
use synthie::presets::sid::Preset;

fn song() -> Result<Song> {
    // One row per eighth note at 132 bpm; four bars in E.
    let song = Song::new(Tempo::bpm(132.0, 2))
        .one_shot()
        .patch(Preset::DriveGuitar.params()) // @0
        .patch(Preset::RockLead.params()) // @1
        .patch(Preset::TriBass.params()) // @2
        // Power chords (root, fifth, octave) as interval chord cells.
        .lane(
            ChannelNo(0),
            "E2^07c!@0 E2^07c! - E2^07c! G2^07c! . A2^07c! . |
             E2^07c! E2^07c! - E2^07c! D2^07c! . C2^07c! . |
             E2^07c! E2^07c! - E2^07c! G2^07c! . A2^07c! . |
             C2^07c! . D2^07c! . E2^07c! . . -",
        )?
        .lane(
            ChannelNo(1),
            "E4@1 . . . G4 . A4 . | B4 . . . A4 . G4 . |
             E4 . G4 . A4 . B4 .  | D5 . B4 . E5 . . -",
        )?
        .lane(
            ChannelNo(2),
            "E1@2 E1 - E1 G1 . A1 . | E1 E1 - E1 D1 . C1 . |
             E1 E1 - E1 G1 . A1 .   | C1 . D1 . E1 . . -",
        )?
        .lane(
            ChannelNo(3),
            "k+c h s h k k s h | k h s h k k s h |
             k+c h s h k k s h | k h s h k s s+c .",
        )?;
    Ok(song)
}

fn main() -> Result<()> {
    let song = song()?;
    let seconds = song.duration_secs() + 1.5;
    let (_stream, _events, _scope) =
        setup_audio_silenced_with_sequencer(SongPlayer::new(song), |_| {})?;
    std::thread::sleep(Duration::from_secs_f32(seconds));
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_lane_is_four_bars() {
        let song = super::song().unwrap();
        for lane in &song.lanes {
            assert_eq!(lane.rows.len(), 32, "lane on {:?}", lane.channel);
        }
    }
}
