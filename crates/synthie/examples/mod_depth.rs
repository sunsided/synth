//! Modulation depth demo: plays an A-minor phrase and cycles through eight
//! configurations — filter envelope, pitch envelope, four LFO shapes on the
//! filter cutoff, and a dual-LFO setup — so you can hear each in a musical
//! context rather than on a static drone.
//!
//! Run:  `cargo run -p synthie --example mod_depth`

use std::time::{Duration, Instant};

use anyhow::Result;
use synthie::audio::engine::setup_audio;
#[cfg(feature = "arp")]
use synthie::params::ArpParams;
use synthie::params::{
    AudioEvent, ChorusParams, CrusherParams, DelayParams, EnvParams, FilterMode, FilterParams,
    FxParams, GlobalParams, LfoParams, LfoShape, LfoTarget, MidiNote, ModEnvParams, Osc2Params,
    OscParams, SynthParams, Waveform,
};

const BPM: f32 = 100.0;

fn beats(n: f32) -> Duration {
    Duration::from_secs_f32(n * 60.0 / BPM)
}

struct TimedEvent {
    at: Duration,
    on: bool,
    midi: u8,
}

fn note(at: f32, hold: f32, midi: u8) -> [TimedEvent; 2] {
    [
        TimedEvent {
            at: beats(at),
            on: true,
            midi,
        },
        TimedEvent {
            at: beats(at + hold),
            on: false,
            midi,
        },
    ]
}

/// A minor ascending-then-descending phrase over 2 bars: A4–C5–D5–E5–E5–D5–C5–A4.
fn build_phrase() -> Vec<TimedEvent> {
    const NOTES: [(u8, f32); 8] = [
        (69, 0.0), // A4
        (72, 1.0), // C5
        (74, 2.0), // D5
        (76, 3.0), // E5
        (76, 4.0), // E5
        (74, 5.0), // D5
        (72, 6.0), // C5
        (69, 7.0), // A4
    ];
    let mut events: Vec<TimedEvent> = Vec::new();
    for (midi, beat) in NOTES {
        events.extend(note(beat, 0.75, midi));
    }
    events.sort_by_key(|e| e.at);
    events
}

/// Base patch: sawtooth through a closed low-pass filter with mild resonance.
/// All modulation slots are neutral — each phase overrides only what it needs.
fn base_patch() -> SynthParams {
    SynthParams {
        osc: OscParams {
            waveform: Waveform::Sawtooth,
            pulse_width: 0.5,
            detune: 0.0,
            noise_mix: 0.0,
        },
        env: EnvParams {
            attack: 0.003,
            decay: 0.18,
            sustain: 0.7,
            release: 0.18,
            env_reverse: false,
        },
        filter: FilterParams {
            filter_mode: FilterMode::LowPass,
            cutoff: 400.0,
            resonance: 0.55,
            drive: 0.1,
        },
        lfo: LfoParams::default(),
        lfo2: LfoParams::default(),
        filter_env: ModEnvParams::default(),
        pitch_env: ModEnvParams::default(),
        #[cfg(feature = "arp")]
        arp: ArpParams::default(),
        fx: FxParams {
            reverb_mix: 0.12,
            reverb_size: 0.4,
            reverb_damping: 0.6,
        },
        crusher: CrusherParams::default(),
        chorus: ChorusParams::default(),
        delay: DelayParams::default(),
        osc2: Osc2Params::default(),
        global: GlobalParams {
            volume: 0.75,
            glide_time: 0.0,
        },
    }
}

struct Phase {
    label: &'static str,
    desc: &'static str,
    apply: fn(&mut SynthParams),
}

fn apply_dry(_p: &mut SynthParams) {}

fn apply_filter_env(p: &mut SynthParams) {
    // Fast attack, medium decay, low sustain: classic SID-style filter pluck.
    p.filter_env = ModEnvParams {
        attack: 0.004,
        decay: 0.35,
        sustain: 0.08,
        release: 0.2,
        depth: 1.0,
    };
}

fn apply_pitch_env(p: &mut SynthParams) {
    // Short upward pitch swoop on attack — videogame / SID percussion flavour.
    p.pitch_env = ModEnvParams {
        attack: 0.003,
        decay: 0.22,
        sustain: 0.0,
        release: 0.08,
        depth: 0.9,
    };
}

fn apply_lfo_sine(p: &mut SynthParams) {
    p.lfo = LfoParams {
        lfo_rate: 4.5,
        lfo_depth: 0.6,
        lfo_target: LfoTarget::Cutoff,
        lfo_shape: LfoShape::Sine,
    };
}

fn apply_lfo_square(p: &mut SynthParams) {
    p.lfo = LfoParams {
        lfo_rate: 3.0,
        lfo_depth: 0.55,
        lfo_target: LfoTarget::Cutoff,
        lfo_shape: LfoShape::Square,
    };
}

fn apply_lfo_saw(p: &mut SynthParams) {
    p.lfo = LfoParams {
        lfo_rate: 4.0,
        lfo_depth: 0.55,
        lfo_target: LfoTarget::Cutoff,
        lfo_shape: LfoShape::Sawtooth,
    };
}

fn apply_lfo_sh(p: &mut SynthParams) {
    p.lfo = LfoParams {
        lfo_rate: 3.5,
        lfo_depth: 0.75,
        lfo_target: LfoTarget::Cutoff,
        lfo_shape: LfoShape::SampleHold,
    };
}

fn apply_dual_lfo(p: &mut SynthParams) {
    // LFO1: slow sine vibrato on pitch.
    p.lfo = LfoParams {
        lfo_rate: 5.0,
        lfo_depth: 0.18,
        lfo_target: LfoTarget::Pitch,
        lfo_shape: LfoShape::Sine,
    };
    // LFO2: fast S&H on filter cutoff — random texture on top of the vibrato.
    p.lfo2 = LfoParams {
        lfo_rate: 7.0,
        lfo_depth: 0.55,
        lfo_target: LfoTarget::Cutoff,
        lfo_shape: LfoShape::SampleHold,
    };
}

static PHASES: &[Phase] = &[
    Phase {
        label: "dry",
        desc: "no modulation — reference sound",
        apply: apply_dry,
    },
    Phase {
        label: "filter-env",
        desc: "filter env: fast attack, 350 ms decay — SID-style cutoff pluck",
        apply: apply_filter_env,
    },
    Phase {
        label: "pitch-env",
        desc: "pitch env: short upward swoop on attack — videogame zap",
        apply: apply_pitch_env,
    },
    Phase {
        label: "lfo-sine",
        desc: "LFO1 sine 4.5 Hz → cutoff — smooth filter wobble",
        apply: apply_lfo_sine,
    },
    Phase {
        label: "lfo-square",
        desc: "LFO1 square 3 Hz → cutoff — hard filter gate",
        apply: apply_lfo_square,
    },
    Phase {
        label: "lfo-saw",
        desc: "LFO1 sawtooth 4 Hz → cutoff — rising filter ramp",
        apply: apply_lfo_saw,
    },
    Phase {
        label: "lfo-s&h",
        desc: "LFO1 S&H 3.5 Hz → cutoff — random stepped filter",
        apply: apply_lfo_sh,
    },
    Phase {
        label: "dual-lfo",
        desc: "LFO1 sine 5 Hz → pitch (vibrato) + LFO2 S&H 7 Hz → cutoff (texture)",
        apply: apply_dual_lfo,
    },
];

fn main() -> Result<()> {
    let (_stream, event_tx, _scope_rx) = setup_audio()?;

    println!("=== Modulation Depth Demo ===");
    println!("A minor phrase  |  {BPM} BPM  |  2 bars per phase\n");
    println!("{:<14}  description", "phase");
    println!("{}", "-".repeat(62));

    let phrase = build_phrase();

    for phase in PHASES {
        let mut patch = base_patch();
        (phase.apply)(&mut patch);

        event_tx.send(AudioEvent::LoadPatch(Box::new(patch)))?;
        println!("  {:<12}  {}", phase.label, phase.desc);

        let started = Instant::now();
        for ev in &phrase {
            let deadline = started + ev.at;
            let now = Instant::now();
            if deadline > now {
                std::thread::sleep(deadline.duration_since(now));
            }
            let msg = if ev.on {
                AudioEvent::NoteOn(MidiNote(ev.midi))
            } else {
                AudioEvent::NoteOff(MidiNote(ev.midi))
            };
            event_tx.send(msg)?;
        }

        // One bar silence — lets reverb tail clear before the next phase.
        std::thread::sleep(beats(4.0));
    }

    println!("\nDone.");
    event_tx.send(AudioEvent::Panic)?;
    std::thread::sleep(Duration::from_millis(150));

    Ok(())
}
