//! Ring modulation demo: plays a C major arpeggio and cycles through all four
//! `RingModMode` variants so you can hear how each changes the timbre.
//!
//! Both oscillators use sine waves so the ring mod sidebands are cleanly
//! audible. OSC2 is tuned a perfect fifth (+700 ct) above OSC1; at that
//! interval the analog mode produces sum/difference tones at 2.5x and 0.5x
//! the fundamental, giving a characteristic bell-like quality.
//!
//! Run:  `cargo run -p synthie --example ring_mod`

use std::time::{Duration, Instant};

use anyhow::Result;
use synthie::audio::engine::setup_audio;
use synthie::params::{
    AudioEvent, ChorusParams, CrusherParams, DelayParams, EnvParams, FilterMode, FilterParams,
    FxParams, GlobalParams, LfoParams, LfoShape, LfoTarget, MidiNote, ModEnvParams, Osc2Params,
    OscParams, RingModMode, SynthParams, Waveform,
};

const BPM: f32 = 90.0;

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
/// Eight eighth-notes; one bar at 90 BPM.
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

struct Phase {
    label: &'static str,
    desc: &'static str,
    mode: RingModMode,
}

static PHASES: &[Phase] = &[
    Phase {
        label: "off",
        desc: "no ring mod - two sines blended (reference)",
        mode: RingModMode::Off,
    },
    Phase {
        label: "SID",
        desc: "OSC2 x sign(OSC1 MSB) - hollow, SID-chip flavour",
        mode: RingModMode::Osc2ByOsc1Sign,
    },
    Phase {
        label: "rev",
        desc: "OSC1 x sign(OSC2 MSB) - asymmetric reversed-SID",
        mode: RingModMode::Osc1ByOsc2Sign,
    },
    Phase {
        label: "analog",
        desc: "OSC1 x OSC2 - sum/difference tones, bell-like",
        mode: RingModMode::Analog,
    },
];

fn base_patch() -> SynthParams {
    SynthParams {
        osc: OscParams {
            waveform: Waveform::Sine,
            pulse_width: 0.5,
            detune: 0.0,
            noise_mix: 0.0,
        },
        env: EnvParams {
            attack: 0.01,
            decay: 0.05,
            sustain: 0.9,
            release: 0.15,
            env_reverse: false,
        },
        filter: FilterParams {
            filter_mode: FilterMode::LowPass,
            cutoff: 18000.0,
            resonance: 0.0,
            drive: 0.0,
        },
        lfo: LfoParams {
            lfo_rate: 0.0,
            lfo_depth: 0.0,
            lfo_target: LfoTarget::Pitch,
            lfo_shape: LfoShape::Sine,
        },
        fx: FxParams {
            reverb_mix: 0.05,
            reverb_size: 0.3,
            reverb_damping: 0.6,
        },
        osc2: Osc2Params {
            waveform: Waveform::Sine,
            detune: 700.0,
            osc2_mix: 0.7,
            hard_sync: false,
            ring_mod: RingModMode::Off,
        },
        crusher: CrusherParams::default(),
        chorus: ChorusParams::default(),
        delay: DelayParams::default(),
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

    println!("=== Ring Modulation Demo ===");
    println!("C major arpeggio  |  {BPM} BPM  |  sine+sine, OSC2 at +700 ct (perfect fifth)\n");
    println!("{:<8}  description", "mode");
    println!("{}", "-".repeat(58));

    let phrase = build_phrase();

    for phase in PHASES {
        let mut patch = base_patch();
        patch.osc2.ring_mod = phase.mode;

        event_tx.send(AudioEvent::LoadPatch(Box::new(patch)))?;
        println!("  {:<6}  {}", phase.label, phase.desc);

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
