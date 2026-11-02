//! A power-chord riff played from a single pattern lane with chord cells:
//! `E2+B2+E3` starts root, fifth and octave together on one channel, and
//! `E2^07c!` writes the same chord as semitone offsets from the root.
//!
//! ```sh
//! cargo run -p synthie --example power_chords --release
//! ```

use std::time::Duration;

use anyhow::Result;
use synthie::audio::engine::setup_audio_silenced_with_sequencer;
use synthie::params::{
    ChannelNo, EnvParams, FilterMode, FilterParams, GlobalParams, Osc2Params, OscParams,
    SynthParams, Waveform,
};
use synthie::pattern::{Song, SongPlayer, Tempo};

/// A driven, slightly detuned sawtooth: close enough to a crunchy guitar.
fn crunch() -> SynthParams {
    let d = SynthParams::default();
    SynthParams {
        osc: OscParams {
            waveform: Waveform::Sawtooth,
            ..d.osc
        },
        osc2: Osc2Params {
            waveform: Waveform::Sawtooth,
            detune: 9.0,
            osc2_mix: 0.4,
            ..d.osc2
        },
        env: EnvParams {
            attack: 0.003,
            decay: 0.25,
            sustain: 0.6,
            release: 0.08,
            ..d.env
        },
        filter: FilterParams {
            filter_mode: FilterMode::LowPass,
            cutoff: 2_800.0,
            resonance: 0.2,
            drive: 0.8,
        },
        global: GlobalParams {
            volume: 0.45,
            glide_time: 0.0,
        },
        ..d
    }
}

fn bass() -> SynthParams {
    let d = SynthParams::default();
    SynthParams {
        osc: OscParams {
            waveform: Waveform::Triangle,
            ..d.osc
        },
        global: GlobalParams {
            volume: 0.6,
            glide_time: 0.0,
        },
        ..d
    }
}

fn song() -> Result<Song> {
    // One row per eighth note at 140 bpm; four bars in E.
    let song = Song::new(Tempo::bpm(140.0, 2))
        .one_shot()
        .patch(crunch()) // @0
        .patch(bass()) // @1
        .lane(
            ChannelNo(0),
            "E2+B2+E3@0 E2+B2+E3 - E2+B2+E3 - E2+B2+E3 G2+D3+G3 . |
             A2+E3+A3 . . . G2+D3+G3 . D2+A2+D3 . |
             E2^07c! E2^07c! - E2^07c! - E2^07c! G2^07c! . |
             A2^07c! . C3^07c! . B2^07c! . . - ",
        )?
        .lane(
            ChannelNo(1),
            "E1@1 E1 - E1 - E1 G1 . | A1 . . . G1 . D1 . |
             E1 E1 - E1 - E1 G1 .   | A1 . C2 . B1 . . -",
        )?
        .lane(
            ChannelNo(3),
            "k+c h s h k k s h | k h s h k h s h |
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
    use synthie::pattern::Action;

    #[test]
    fn the_riff_is_chords_on_one_lane() {
        let song = super::song().unwrap();
        for lane in &song.lanes {
            assert_eq!(lane.rows.len(), 32, "lane on {:?}", lane.channel);
        }
        let chords = song.lanes[0]
            .rows
            .iter()
            .filter(|c| matches!(c.action, Action::Chord { count: 3, .. }))
            .count();
        assert_eq!(chords, 16);
    }
}
