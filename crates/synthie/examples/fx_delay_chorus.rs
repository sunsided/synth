//! Delay and chorus FX demo: plays a sustained A4 sawtooth and steps through
//! delay time/feedback and chorus rate/depth settings so you can hear each
//! axis of the effect in isolation and combined.
//!
//! Run:  `cargo run -p synthie --example fx_delay_chorus`

use std::time::Duration;

use anyhow::Result;
use synthie::audio::engine::setup_audio;
use synthie::params::{
    AudioEvent, ChorusParams, CrusherParams, DelayParams, EnvParams, FilterMode, FilterParams,
    FxParams, GlobalParams, LfoParams, LfoTarget, MidiNote, OscParams, SynthParams, Waveform,
};

/// Seconds to hold each phase.
const PHASE_SECS: u64 = 4;

/// Dry sawtooth patch: no reverb, open filter, sustained envelope, all FX bypassed.
fn dry_sawtooth() -> SynthParams {
    SynthParams {
        osc: OscParams {
            waveform: Waveform::Sawtooth,
            pulse_width: 0.5,
            detune: 0.0,
            noise_mix: 0.0,
        },
        env: EnvParams {
            attack: 0.01,
            decay: 0.0,
            sustain: 1.0,
            release: 0.05,
            env_reverse: false,
        },
        filter: FilterParams {
            filter_mode: FilterMode::LowPass,
            cutoff: 8000.0,
            resonance: 0.1,
            drive: 0.0,
        },
        lfo: LfoParams {
            lfo_rate: 0.0,
            lfo_depth: 0.0,
            lfo_target: LfoTarget::Pitch,
        },
        fx: FxParams {
            reverb_mix: 0.0,
            reverb_size: 0.5,
            reverb_damping: 0.5,
        },
        crusher: CrusherParams::default(),
        chorus: ChorusParams::default(),
        delay: DelayParams::default(),
        global: GlobalParams {
            volume: 0.7,
            glide_time: 0.0,
        },
    }
}

type Phase = (&'static str, f32, f32, f32, f32, f32, f32, &'static str);
//              label        dly   fdbk  dmix  rate  dep   cmix  description

//  label           delay_ms  feedback  delay_mix  rate   depth_ms  chorus_mix  description
static PHASES: &[Phase] = &[
    ("dry", 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, "no FX"),
    (
        "slapback",
        80.0,
        0.0,
        0.6,
        0.0,
        0.0,
        0.0,
        "short echo, no feedback",
    ),
    ("echo", 375.0, 0.4, 0.5, 0.0, 0.0, 0.0, "dotted-eighth echo"),
    (
        "long echo",
        750.0,
        0.6,
        0.5,
        0.0,
        0.0,
        0.0,
        "slow decay feedback",
    ),
    (
        "chorus slow",
        0.0,
        0.0,
        0.0,
        0.5,
        3.0,
        0.7,
        "gentle thickening",
    ),
    ("chorus fast", 0.0, 0.0, 0.0, 2.5, 5.0, 0.8, "vibrato-heavy"),
    (
        "delay+chorus",
        375.0,
        0.35,
        0.4,
        0.5,
        3.0,
        0.6,
        "echo with chorus tails",
    ),
    ("dry", 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, "restored"),
];

fn main() -> Result<()> {
    let (_stream, event_tx, _scope_rx) = setup_audio()?;

    event_tx.send(AudioEvent::LoadPatch(Box::new(dry_sawtooth())))?;
    event_tx.send(AudioEvent::NoteOn(MidiNote::A4))?;

    println!("=== Delay / Chorus FX Demo ===");
    println!("Sawtooth A4  |  {PHASE_SECS}s per phase\n");
    println!(
        "{:<16}  {:>8}  {:>8}  {:>9}  {:>6}  {:>8}  {:>9}  description",
        "phase", "dly_ms", "fdbk", "dly_mix", "rate", "depth", "chorus_mix"
    );
    println!("{}", "-".repeat(96));

    for &(label, delay_ms, feedback, delay_mix, rate, depth_ms, chorus_mix, desc) in PHASES {
        let mut patch = dry_sawtooth();

        if delay_mix > 0.0 {
            patch.delay = DelayParams {
                time_ms: delay_ms,
                feedback,
                mix: delay_mix,
            };
        }
        if chorus_mix > 0.0 {
            patch.chorus = ChorusParams {
                rate,
                depth_ms,
                mix: chorus_mix,
            };
        }

        event_tx.send(AudioEvent::LoadPatch(Box::new(patch)))?;

        println!(
            "  {label:<14}  {delay_ms:>8.0}  {feedback:>8.2}  {delay_mix:>9.2}  \
             {rate:>6.1}  {depth_ms:>8.1}  {chorus_mix:>9.2}  {desc}"
        );

        std::thread::sleep(Duration::from_secs(PHASE_SECS));
    }

    println!("\nDone.");

    event_tx.send(AudioEvent::NoteOff(MidiNote::A4))?;
    std::thread::sleep(Duration::from_millis(200));
    event_tx.send(AudioEvent::Panic)?;
    std::thread::sleep(Duration::from_millis(100));

    Ok(())
}
