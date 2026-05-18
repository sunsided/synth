//! Bitcrusher FX demo: plays a sustained A4 sawtooth and steps through
//! bit depth and sample rate decimation settings so you can hear each axis
//! of the effect in isolation and combined.
//!
//! Run:  `cargo run -p synthie --example bitcrusher`
//!
//! Extend this file as more FX are added to the engine — the dry patch and
//! phase loop form a reusable skeleton for any single-param sweep.

use std::time::Duration;

use anyhow::Result;
use synthie::audio::engine::setup_audio;
#[cfg(feature = "arp")]
use synthie::params::ArpParams;
use synthie::params::{
    AudioEvent, ChorusParams, CrusherParams, DelayParams, EnvParams, FilterMode, FilterParams,
    FxParams, GlobalParams, LfoParams, LfoShape, LfoTarget, MidiNote, ModEnvParams, Osc2Params,
    OscParams, SynthParams, Waveform,
};

/// Seconds to hold each phase.
const PHASE_SECS: u64 = 3;

/// Dry sawtooth patch: no reverb, open filter, sustained envelope.
/// Pass-through crusher by default (bits=16, rate=1).
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
            lfo_shape: LfoShape::Sine,
        },
        fx: FxParams {
            reverb_mix: 0.0,
            reverb_size: 0.5,
            reverb_damping: 0.5,
        },
        crusher: CrusherParams::default(),
        chorus: ChorusParams::default(),
        delay: DelayParams::default(),
        osc2: Osc2Params::default(),
        lfo2: LfoParams::default(),
        filter_env: ModEnvParams::default(),
        pitch_env: ModEnvParams::default(),
        #[cfg(feature = "arp")]
        arp: ArpParams::default(),
        global: GlobalParams {
            volume: 0.7,
            glide_time: 0.0,
        },
    }
}

//  label         bits  rate  description
static PHASES: &[(&str, f32, f32, &str)] = &[
    ("clean", 16.0, 1.0, "pass-through (no effect)"),
    ("4-bit", 4.0, 1.0, "bit depth only"),
    ("2-bit", 2.0, 1.0, "extreme quantization"),
    ("rate /8", 16.0, 8.0, "decimation only"),
    ("4-bit /8", 4.0, 8.0, "depth + decimation"),
    ("clean", 16.0, 1.0, "restored"),
];

fn main() -> Result<()> {
    let (_stream, event_tx, _scope_rx) = setup_audio()?;

    event_tx.send(AudioEvent::LoadPatch(Box::new(dry_sawtooth())))?;
    event_tx.send(AudioEvent::NoteOn(MidiNote::A4))?;

    println!("=== Bitcrusher / FX Demo ===");
    println!("Sawtooth A4  |  {PHASE_SECS}s per phase\n");
    println!("{:<12}  {:>6}  {:>6}  description", "phase", "bits", "rate");
    println!("{}", "-".repeat(54));

    for &(label, bits, rate, desc) in PHASES {
        let mut patch = dry_sawtooth();
        patch.crusher = CrusherParams { bits, rate };
        event_tx.send(AudioEvent::LoadPatch(Box::new(patch)))?;

        println!("  {label:<10}  {bits:>6.0}  {rate:>6.0}  {desc}");

        std::thread::sleep(Duration::from_secs(PHASE_SECS));
    }

    println!("\nDone.");

    event_tx.send(AudioEvent::NoteOff(MidiNote::A4))?;
    std::thread::sleep(Duration::from_millis(200));
    event_tx.send(AudioEvent::Panic)?;
    std::thread::sleep(Duration::from_millis(100));

    Ok(())
}
