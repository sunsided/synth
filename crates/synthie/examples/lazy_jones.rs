//! Plays the Lazy Jones hook (David Whittaker, 1984), the riff that later
//! became Kernkraft 400, exactly as the tracker opens with it:
//! [`synthie::songs::lazy_jones`], two four-bar passes at 140 bpm.
//!
//! The song plays once through on a [`SongPlayer`], on the same engine setup
//! the tracker uses, then lets the last notes ring out.
//!
//! ```sh
//! cargo run -p synthie --example lazy_jones --release
//! ```

use std::time::Duration;

use anyhow::Result;
use synthie::audio::engine::setup_audio_silenced_with_sequencer;
use synthie::pattern::SongPlayer;
use synthie::songs::lazy_jones;

fn main() -> Result<()> {
    let song = lazy_jones().one_shot();
    let seconds = song.duration_secs() + 1.5;
    let (_stream, _events, _scope) =
        setup_audio_silenced_with_sequencer(SongPlayer::new(song), |err| {
            eprintln!("audio stream error: {err}");
        })?;
    std::thread::sleep(Duration::from_secs_f32(seconds));
    Ok(())
}
