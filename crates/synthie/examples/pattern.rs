//! A short chiptune song written in the pattern notation and played by a
//! [`SongPlayer`]: bass, lead, a chip-arpeggio chord lane and drums.
//!
//! ```sh
//! cargo run -p synthie --example pattern --release
//! ```

use std::time::Duration;

use anyhow::Result;
use synthie::audio::engine::setup_audio_silenced_with_sequencer;
use synthie::params::ChannelNo;
use synthie::pattern::{Song, SongPlayer, Tempo};
use synthie::presets::sid::Preset;

fn song() -> Result<Song> {
    // Chords on a single channel: the arpeggiator cycles the cell's notes
    // fast enough to blur into a chord, the classic chip trick.
    let mut chords = Preset::TriPad.params();
    chords.arp.rate = 50.0;
    chords.arp.gate = 1.0;
    chords.global.volume = 0.5;

    // One row per eighth note at 152 bpm; 8 bars of E minor.
    let song = Song::new(Tempo::bpm(152.0, 2))
        .one_shot()
        .patch(Preset::C64Bass.params()) // @0
        .patch(Preset::PwmLead.params()) // @1
        .patch(chords) // @2
        .lane(
            ChannelNo(0),
            "E2@0 E2 E2 E2 E2 E2 G2 A2 | E2 E2 E2 E2 E2 E2 D2 C2 |
             E2 E2 E2 E2 E2 E2 G2 A2 | Bb2 . A2 . G2 . D2 . |
             C2 . C2 C2 C2 . C2 C2   | D2 . D2 D2 D2 . D2 D2 |
             B1 . B1 B1 B1 . B1 B1   | E2 . . . . . - .",
        )?
        .lane(
            ChannelNo(1),
            "@1 . . . . . . . | - . . . . . . . |
             - . . . . . . .  | B4 . . . . . . . |
             G4 . E4 . G4 . A4 . | B4 . . . A4 . G4 . |
             A4 . . . G4 . F#4 . | E4 . . . . . - .",
        )?
        .lane(
            ChannelNo(2),
            "E4^037@2 . . . . . . . | E4^037 . . . . . . . |
             E4^037 . . . . . . .   | D4^047 . . . G4^047 . . . |
             C4^047 . . . . . . .   | D4^047 . . . . . . . |
             B3^047 . . . . . . .   | E4^037c . . . . . - .",
        )?
        .lane(
            ChannelNo(3),
            "k+c h s h k k s h | k h s h k k s h |
             k h s h k k s h   | k h s h k ht mt lt |
             k+c h s h k k s h | k h s+x h k k s o |
             k+c h s h k k s h | k+c . . . . . . .",
        )?;
    Ok(song)
}

fn main() -> Result<()> {
    let song = song()?;
    let seconds = song.duration_secs() + 2.0;
    let (_stream, _events, _scope) =
        setup_audio_silenced_with_sequencer(SongPlayer::new(song), |_| {})?;
    std::thread::sleep(Duration::from_secs_f32(seconds));
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_song_parses_and_every_lane_is_eight_bars() {
        let song = super::song().unwrap();
        for lane in &song.lanes {
            assert_eq!(lane.rows.len(), 64, "lane on {:?}", lane.channel);
        }
    }
}
