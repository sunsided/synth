//! A basic rock beat on the drum machine: kick, snare on the backbeat, eighth
//! hi-hats, a crash on the downbeat and a tom fill into the next phrase.
//!
//! The beat runs on a [`Sequencer`] inside the audio callback, so every hit
//! lands on its sixteenth regardless of the audio buffer size.
//!
//! ```sh
//! cargo run -p synthie --example drums
//! ```

use std::time::Duration;

use anyhow::Result;
use synthie::audio::engine::{Sequencer, setup_audio_silenced_with_sequencer};
use synthie::params::{AudioEvent, DrumHit};

const BPM: f32 = 120.0;
const STEPS_PER_BEAT: f32 = 4.0;
const STEPS_PER_BAR: usize = 16;
const BARS: usize = 8;

/// One bar as sixteenth-note lanes: `x` hits, anything else rests.
struct Bar {
    kick: &'static str,
    snare: &'static str,
    hat: &'static str,
    open_hat: &'static str,
    crash: &'static str,
    toms: &'static str,
    clap: &'static str,
}

const GROOVE: Bar = Bar {
    kick: "x.......x.x.....",
    snare: "....x.......x...",
    hat: "x.x.x.x.x.x.x.x.",
    open_hat: "................",
    crash: "................",
    toms: "................",
    clap: "................",
};

/// First bar of a phrase: the groove with a crash on the one.
const DOWNBEAT: Bar = Bar {
    crash: "x...............",
    hat: "..x.x.x.x.x.x.x.",
    ..GROOVE
};

/// Second half of the phrase: claps double the snare, open hat into the fill.
const LIFT: Bar = Bar {
    clap: "....x.......x...",
    hat: "x.x.x.x.x.x.x...",
    open_hat: "..............x.",
    ..GROOVE
};

/// Last bar of the phrase: a tom fill (h = high, m = mid, l = low) into the
/// crash of the next bar.
const FILL: Bar = Bar {
    kick: "x.......x.......",
    snare: "....x...........",
    hat: "x.x.x.x.........",
    toms: "........hhmmllll",
    ..GROOVE
};

const SONG: [Bar; BARS] = [DOWNBEAT, GROOVE, GROOVE, FILL, DOWNBEAT, LIFT, LIFT, FILL];

/// Plays [`SONG`] once, one sixteenth per tick.
struct Beat {
    step: usize,
    /// Fractional frames carried between ticks, so the tempo does not drift.
    frame_error: f32,
}

impl Beat {
    fn hits(&self) -> impl Iterator<Item = DrumHit> {
        let bar = &SONG[(self.step / STEPS_PER_BAR) % BARS];
        let i = self.step % STEPS_PER_BAR;
        let at = move |lane: &str| lane.as_bytes()[i];
        let lanes = [
            (bar.kick, DrumHit::Kick),
            (bar.snare, DrumHit::Snare),
            (bar.hat, DrumHit::HiHatClosed),
            (bar.open_hat, DrumHit::HiHatOpen),
            (bar.crash, DrumHit::Crash),
            (bar.clap, DrumHit::Clap),
        ];
        let tom = match at(bar.toms) {
            b'h' => Some(DrumHit::TomHigh),
            b'm' => Some(DrumHit::TomMid),
            b'l' => Some(DrumHit::TomLow),
            _ => None,
        };
        lanes
            .into_iter()
            .filter(move |(lane, _)| at(lane) == b'x')
            .map(|(_, hit)| hit)
            .chain(tom)
    }
}

impl Sequencer for Beat {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn tick(&mut self, sample_rate: f32, out: &mut Vec<AudioEvent>) -> usize {
        if self.step < BARS * STEPS_PER_BAR {
            out.extend(self.hits().map(AudioEvent::Drum));
        } else if self.step == BARS * STEPS_PER_BAR {
            // Ring out on a final crash.
            out.extend([
                AudioEvent::Drum(DrumHit::Crash),
                AudioEvent::Drum(DrumHit::Kick),
            ]);
        }
        self.step += 1;

        let frames = sample_rate * 60.0 / BPM / STEPS_PER_BEAT + self.frame_error;
        let whole = frames.floor();
        self.frame_error = frames - whole;
        whole as usize
    }
}

fn main() -> Result<()> {
    let beat = Beat {
        step: 0,
        frame_error: 0.0,
    };
    let (_stream, _events, _scope) = setup_audio_silenced_with_sequencer(beat, |_| {})?;

    #[allow(clippy::cast_precision_loss)]
    let seconds = (BARS * STEPS_PER_BAR) as f32 / STEPS_PER_BEAT * 60.0 / BPM + 2.5;
    std::thread::sleep(Duration::from_secs_f32(seconds));
    Ok(())
}
