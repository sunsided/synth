//! Oscillator richness demo: plays a C major arpeggio and cycles through six
//! oscillator configurations — single sine, single saw, unison detuning, and
//! hard sync — so you can hear each mode in a musical context.
//!
//! Run:  `cargo run -p synthie --example osc_richness`

use std::time::{Duration, Instant};

use anyhow::Result;
use synthie::audio::engine::setup_audio;
use synthie::params::{
    AudioEvent, ChorusParams, CrusherParams, DelayParams, EnvParams, FilterMode, FilterParams,
    FxParams, GlobalParams, LfoParams, LfoShape, LfoTarget, MidiNote, ModEnvParams, Osc2Params,
    OscParams, SynthParams, Waveform,
};

const BPM: f32 = 108.0;

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

/// Ascending+descending C major arpeggio: C4-E4-G4-C5-B4-G4-E4-C4.
/// Eight eighth-notes; one bar at 108 BPM.
fn build_phrase() -> Vec<TimedEvent> {
    const NOTES: [(u8, f32); 8] = [
        (60, 0.0), // C4
        (64, 0.5), // E4
        (67, 1.0), // G4
        (72, 1.5), // C5
        (71, 2.0), // B4
        (67, 2.5), // G4
        (64, 3.0), // E4
        (60, 3.5), // C4
    ];
    let mut events: Vec<TimedEvent> = Vec::new();
    for (midi, beat) in NOTES {
        events.extend(note(beat, 0.4, midi));
    }
    events.sort_by_key(|e| e.at);
    events
}

struct OscPhase {
    label: &'static str,
    desc: &'static str,
    osc1_waveform: Waveform,
    osc2: Osc2Params,
}

static PHASES: &[OscPhase] = &[
    OscPhase {
        label: "saw",
        desc: "single sawtooth – reference",
        osc1_waveform: Waveform::Sawtooth,
        osc2: Osc2Params {
            waveform: Waveform::Sawtooth,
            detune: 0.0,
            osc2_mix: 0.0,
            hard_sync: false,
        },
    },
    OscPhase {
        label: "sine",
        desc: "single sine – pure fundamental, no overtones",
        osc1_waveform: Waveform::Sine,
        osc2: Osc2Params {
            waveform: Waveform::Sine,
            detune: 0.0,
            osc2_mix: 0.0,
            hard_sync: false,
        },
    },
    OscPhase {
        label: "unison-light",
        desc: "+7 ct detuned copy, 50% mix – subtle chorus-like width",
        osc1_waveform: Waveform::Sawtooth,
        osc2: Osc2Params {
            waveform: Waveform::Sawtooth,
            detune: 7.0,
            osc2_mix: 0.5,
            hard_sync: false,
        },
    },
    OscPhase {
        label: "unison-wide",
        desc: "+18 ct detuned copy, 60% mix – lush beating unison",
        osc1_waveform: Waveform::Sawtooth,
        osc2: Osc2Params {
            waveform: Waveform::Sawtooth,
            detune: 18.0,
            osc2_mix: 0.6,
            hard_sync: false,
        },
    },
    OscPhase {
        label: "hard-sync",
        desc: "OSC2 +700 ct, synced to OSC1 – nasal, rasping timbre",
        osc1_waveform: Waveform::Sawtooth,
        osc2: Osc2Params {
            waveform: Waveform::Sawtooth,
            detune: 700.0,
            osc2_mix: 0.7,
            hard_sync: true,
        },
    },
    OscPhase {
        label: "sine-unison",
        desc: "sine + sine +7 ct, 50% mix – smooth, warm doubling",
        osc1_waveform: Waveform::Sine,
        osc2: Osc2Params {
            waveform: Waveform::Sine,
            detune: 7.0,
            osc2_mix: 0.5,
            hard_sync: false,
        },
    },
];

fn base_patch() -> SynthParams {
    SynthParams {
        osc: OscParams {
            waveform: Waveform::Sawtooth,
            pulse_width: 0.5,
            detune: 0.0,
            noise_mix: 0.0,
        },
        env: EnvParams {
            attack: 0.01,
            decay: 0.05,
            sustain: 0.8,
            release: 0.1,
            env_reverse: false,
        },
        filter: FilterParams {
            filter_mode: FilterMode::LowPass,
            cutoff: 5000.0,
            resonance: 0.2,
            drive: 0.0,
        },
        lfo: LfoParams {
            lfo_rate: 0.0,
            lfo_depth: 0.0,
            lfo_target: LfoTarget::Pitch,
            lfo_shape: LfoShape::Sine,
        },
        fx: FxParams {
            reverb_mix: 0.1,
            reverb_size: 0.4,
            reverb_damping: 0.5,
        },
        crusher: CrusherParams::default(),
        chorus: ChorusParams::default(),
        delay: DelayParams::default(),
        osc2: Osc2Params::default(),
        lfo2: LfoParams::default(),
        filter_env: ModEnvParams::default(),
        pitch_env: ModEnvParams::default(),
        global: GlobalParams {
            volume: 0.7,
            glide_time: 0.0,
        },
    }
}

fn main() -> Result<()> {
    let (_stream, event_tx, _scope_rx) = setup_audio()?;

    println!("=== Oscillator Richness Demo ===");
    println!("C major arpeggio  |  {BPM} BPM  |  1 bar per phase\n");
    println!("{:<16}  description", "phase");
    println!("{}", "-".repeat(58));

    let phrase = build_phrase();

    for phase in PHASES {
        let mut patch = base_patch();
        patch.osc.waveform = phase.osc1_waveform;
        patch.osc2 = phase.osc2.clone();

        event_tx.send(AudioEvent::LoadPatch(Box::new(patch)))?;
        println!("  {:<14}  {}", phase.label, phase.desc);

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

        // One bar silence so the reverb tail decays before the next phase.
        std::thread::sleep(beats(4.0));
    }

    println!("\nDone.");
    event_tx.send(AudioEvent::Panic)?;
    std::thread::sleep(Duration::from_millis(150));

    Ok(())
}
