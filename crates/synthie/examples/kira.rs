//! Play a sequenced synthie loop as a kira sound, next to kira's own mixer.
//!
//! The song runs on a [`SongPlayer`] inside kira's render callback, so it
//! stays on the beat whatever block size kira uses. The main thread acts
//! like a game: it fires a drum hit through the handle, fades the volume,
//! pauses, resumes and finally stops the music.
//!
//! ```sh
//! cargo run -p synthie --example kira --features kira --release
//! ```

use std::thread::sleep;
use std::time::Duration;

use anyhow::Result;
use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Tween};
use synthie::kira::SynthSoundData;
use synthie::params::{ChannelNo, DrumHit};
use synthie::pattern::{Song, SongPlayer, Tempo};
use synthie::presets::sid::Preset;

/// A two-bar loop in sixteenths at 128 bpm.
fn song() -> Result<Song> {
    Ok(Song::new(Tempo::bpm(128.0, 4))
        .lane(
            ChannelNo(0),
            "A1 . A2 . A1 . A2 . A1 . A2 . G1 . G2 . | F1 . F2 . F1 . F2 . G1 . G2 . E1 . E2 .",
        )?
        .lane(
            ChannelNo(1),
            "A4 . . . C5 . E5 . . . D5 . C5 . B4 . | A4 . . . . . - . G4 . A4 . B4 . - .",
        )?
        .lane(
            ChannelNo(3),
            "k+c . h . s . h . k . k . s . h o | k . h . s . h . k . h . s+x . h h",
        )?)
}

fn main() -> Result<()> {
    let mut manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())?;

    let synth = SynthSoundData::new()
        .with_patches([Preset::C64Bass.params(), Preset::PwmLead.params()])
        .with_sequencer(SongPlayer::new(song()?));
    let mut handle = manager.play(synth)?;

    let bar = Duration::from_secs_f32(4.0 * 60.0 / 128.0);
    sleep(bar * 4);

    println!("live hits from the game thread");
    for _ in 0..4 {
        handle.drum(DrumHit::TomHigh)?;
        sleep(bar / 8);
    }
    sleep(bar * 2);

    println!("fade down to -12 dB and back");
    handle.set_volume(-12.0, tween(bar * 2));
    sleep(bar * 2);
    handle.set_volume(0.0, tween(bar));
    sleep(bar * 2);

    println!("pause");
    handle.pause(tween(bar / 4));
    sleep(bar * 2);
    println!("resume");
    handle.resume(tween(bar / 4));
    sleep(bar * 4);

    println!("stop");
    handle.stop(tween(bar));
    sleep(bar * 2);
    Ok(())
}

fn tween(duration: Duration) -> Tween {
    Tween {
        duration,
        ..Tween::default()
    }
}
