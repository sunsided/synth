//! Synthie feature showcase — a single Am7 phrase that walks through every
//! synthesis layer in sequence.
//!
//! Arrangement (120 BPM, 2 bars = 4 s per phase):
//!   Channel 0 – Lead  (feature varies by section)
//!   Channel 1 – Bass  (C64 Bass preset, A2 root)
//!   Built-in  – Drums (kick on 1&3, closed hi-hat on 8th notes)
//!
//! Sections:
//!   § 1  OSC waveforms  — Sine, Triangle, Sawtooth, Pulse, `PulseSaw`
//!   § 2  OSC2 pairing   — unison ×2, hard sync, ring mod (analog)
//!   § 3  Filter + mod   — filter env, LFO→cutoff, pitch env
//!   § 4  FX chain       — delay, chorus, bitcrusher
//!   § 5  Arpeggiator    — Up, Down, `UpDown`, Random
//!
//! Run:  `cargo run -p synthie --example showcase`

use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use synthie::audio::engine::setup_audio;
#[cfg(feature = "arp")]
use synthie::params::{ArpMode, ArpParams};
use synthie::params::{
    AudioEvent, ChannelNo, ChorusParams, CrusherParams, DelayParams, DrumHit, EnvParams,
    FilterMode, FilterParams, FxParams, GlobalParams, LfoParams, LfoShape, LfoTarget, MidiNote,
    ModEnvParams, Osc2Params, OscParams, RingModMode, SynthParams, Waveform,
};
use synthie::presets::sid::default_patches;

const BPM: f32 = 120.0;
/// Hold slightly under 0.5 beats so consecutive 8th notes don't bleed together.
const HOLD: f32 = 0.43;
/// Two bars at 120 BPM.
const PHRASE_BEATS: f32 = 8.0;

const CH_LEAD: ChannelNo = ChannelNo(0);
const CH_BASS: ChannelNo = ChannelNo(1);
/// Bass root: A2.
const BASS: MidiNote = MidiNote(45);

fn beats(n: f32) -> Duration {
    Duration::from_secs_f32(n * 60.0 / BPM)
}

// ---------------------------------------------------------------------------
// Melody + percussion
// ---------------------------------------------------------------------------

/// Am7 ascending-descending: A4 C5 E5 G5 G5 E5 C5 A4, repeated for bar 2.
static MELODY: &[(f32, u8)] = &[
    (0.0, 69),
    (0.5, 72),
    (1.0, 76),
    (1.5, 79),
    (2.0, 79),
    (2.5, 76),
    (3.0, 72),
    (3.5, 69),
    (4.0, 69),
    (4.5, 72),
    (5.0, 76),
    (5.5, 79),
    (6.0, 79),
    (6.5, 76),
    (7.0, 72),
    (7.5, 69),
];

/// Play one 2-bar melody phrase with bass + drums.
/// Caller must load a patch on `CH_LEAD` before invoking.
fn play_phrase(send: &impl Fn(AudioEvent) -> Result<()>) -> Result<()> {
    let mut all: Vec<(Duration, AudioEvent)> = Vec::new();

    for &(beat, midi) in MELODY {
        all.push((
            beats(beat),
            AudioEvent::NoteOnChannel(CH_LEAD, MidiNote(midi)),
        ));
        all.push((
            beats(beat + HOLD),
            AudioEvent::NoteOffChannel(CH_LEAD, MidiNote(midi)),
        ));
    }

    for bar in 0u8..2 {
        let b = f32::from(bar) * 4.0;
        all.push((beats(b), AudioEvent::Drum(DrumHit::Kick)));
        all.push((beats(b + 2.0), AudioEvent::Drum(DrumHit::Kick)));
        for e in 0u8..8 {
            all.push((
                beats(b + f32::from(e) * 0.5),
                AudioEvent::Drum(DrumHit::HiHatClosed),
            ));
        }
    }

    all.sort_by_key(|e| e.0);
    send(AudioEvent::NoteOnChannel(CH_BASS, BASS))?;

    let t0 = Instant::now();
    for (t, ev) in all {
        let dl = t0 + t;
        let now = Instant::now();
        if dl > now {
            std::thread::sleep(dl - now);
        }
        send(ev)?;
    }

    send(AudioEvent::NoteOffChannel(CH_BASS, BASS))?;

    let end = t0 + beats(PHRASE_BEATS);
    let now = Instant::now();
    if end > now {
        std::thread::sleep(end - now);
    }
    Ok(())
}

/// Play one 2-bar arpeggiator phrase with bass + drums.
/// Caller must configure the arpeggiator on `CH_LEAD` before invoking.
#[cfg(feature = "arp")]
fn play_arp_phrase(send: &impl Fn(AudioEvent) -> Result<()>) -> Result<()> {
    let mut drums: Vec<(Duration, DrumHit)> = Vec::new();
    for bar in 0u8..2 {
        let b = f32::from(bar) * 4.0;
        drums.push((beats(b), DrumHit::Kick));
        drums.push((beats(b + 2.0), DrumHit::Kick));
        for e in 0u8..8 {
            drums.push((beats(b + f32::from(e) * 0.5), DrumHit::HiHatClosed));
        }
    }
    drums.sort_by_key(|e| e.0);

    send(AudioEvent::NoteOnChannel(CH_BASS, BASS))?;

    let t0 = Instant::now();
    for (t, hit) in drums {
        let dl = t0 + t;
        let now = Instant::now();
        if dl > now {
            std::thread::sleep(dl - now);
        }
        send(AudioEvent::Drum(hit))?;
    }

    send(AudioEvent::NoteOffChannel(CH_BASS, BASS))?;

    let end = t0 + beats(PHRASE_BEATS);
    let now = Instant::now();
    if end > now {
        std::thread::sleep(end - now);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Base patch
// ---------------------------------------------------------------------------

/// Neutral sawtooth lead: open filter, fast attack, no extra modulation.
fn lead_base() -> SynthParams {
    SynthParams {
        osc: OscParams {
            waveform: Waveform::Sawtooth,
            pulse_width: 0.5,
            detune: 0.0,
            noise_mix: 0.0,
        },
        env: EnvParams {
            attack: 0.005,
            decay: 0.08,
            sustain: 0.8,
            release: 0.12,
            env_reverse: false,
        },
        filter: FilterParams {
            filter_mode: FilterMode::LowPass,
            cutoff: 5000.0,
            resonance: 0.2,
            drive: 0.0,
        },
        lfo: LfoParams::default(),
        lfo2: LfoParams::default(),
        filter_env: ModEnvParams::default(),
        pitch_env: ModEnvParams::default(),
        fx: FxParams {
            reverb_mix: 0.1,
            reverb_size: 0.4,
            reverb_damping: 0.5,
        },
        crusher: CrusherParams::default(),
        chorus: ChorusParams::default(),
        delay: DelayParams::default(),
        osc2: Osc2Params::default(),
        #[cfg(feature = "arp")]
        arp: ArpParams::default(),
        global: GlobalParams {
            volume: 0.7,
            glide_time: 0.0,
        },
    }
}

// ---------------------------------------------------------------------------
// Section data types
// ---------------------------------------------------------------------------

struct Osc2Phase {
    label: &'static str,
    desc: &'static str,
    osc1: Waveform,
    osc2: Osc2Params,
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
fn main() -> Result<()> {
    let (_stream, event_tx, _scope_rx) = setup_audio()?;

    let patches = default_patches();
    let find = |name: &str| -> Result<SynthParams> {
        patches
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| anyhow!("preset '{name}' not found"))
            .map(|p| p.params.clone())
    };

    let send = |ev: AudioEvent| event_tx.send(ev).map_err(|e| anyhow!("{e}"));

    event_tx.send(AudioEvent::LoadPatchChannel(
        CH_BASS,
        Box::new(find("C64 Bass")?),
    ))?;

    println!("=== Synthie Showcase ===");
    println!("{BPM} BPM  |  Am7  |  2 bars (4 s) per phase\n");

    // -------------------------------------------------------------------
    // § 1  OSC waveforms
    // -------------------------------------------------------------------
    println!("§ 1  OSC Waveforms");
    println!("     {:<10}  description", "waveform");
    println!("     {}", "-".repeat(50));

    for (waveform, desc) in [
        (Waveform::Sine, "pure fundamental, no overtones"),
        (Waveform::Triangle, "odd harmonics, mellow buzz"),
        (Waveform::Sawtooth, "all harmonics, bright edge"),
        (Waveform::Pulse, "hollow nasal, 50% duty cycle"),
        (Waveform::PulseSaw, "pulse+saw mix, thick & buzzy"),
    ] {
        let mut p = lead_base();
        p.osc.waveform = waveform;
        send(AudioEvent::LoadPatchChannel(CH_LEAD, Box::new(p)))?;
        println!("     {:<10}  {}", waveform.name(), desc);
        play_phrase(&send)?;
    }

    // -------------------------------------------------------------------
    // § 2  OSC2 pairing
    // -------------------------------------------------------------------
    println!("\n§ 2  OSC2 Pairing");
    println!("     {:<14}  description", "mode");
    println!("     {}", "-".repeat(56));

    let osc2_phases = [
        Osc2Phase {
            label: "unison +7 ct",
            desc: "subtle chorus-like width",
            osc1: Waveform::Sawtooth,
            osc2: Osc2Params {
                waveform: Waveform::Sawtooth,
                detune: 7.0,
                osc2_mix: 0.5,
                hard_sync: false,
                ring_mod: RingModMode::Off,
            },
        },
        Osc2Phase {
            label: "unison +18 ct",
            desc: "lush beating unison",
            osc1: Waveform::Sawtooth,
            osc2: Osc2Params {
                waveform: Waveform::Sawtooth,
                detune: 18.0,
                osc2_mix: 0.6,
                hard_sync: false,
                ring_mod: RingModMode::Off,
            },
        },
        Osc2Phase {
            label: "hard sync",
            desc: "OSC2 +700 ct synced to OSC1 — nasal rasping timbre",
            osc1: Waveform::Sawtooth,
            osc2: Osc2Params {
                waveform: Waveform::Sawtooth,
                detune: 700.0,
                osc2_mix: 0.7,
                hard_sync: true,
                ring_mod: RingModMode::Off,
            },
        },
        Osc2Phase {
            label: "ring mod",
            desc: "sine×sine +fifth (700 ct) — bell-like sum/difference tones",
            osc1: Waveform::Sine,
            osc2: Osc2Params {
                waveform: Waveform::Sine,
                detune: 700.0,
                osc2_mix: 0.7,
                hard_sync: false,
                ring_mod: RingModMode::Analog,
            },
        },
    ];

    for phase in &osc2_phases {
        let mut p = lead_base();
        p.osc.waveform = phase.osc1;
        p.osc2 = phase.osc2.clone();
        send(AudioEvent::LoadPatchChannel(CH_LEAD, Box::new(p)))?;
        println!("     {:<14}  {}", phase.label, phase.desc);
        play_phrase(&send)?;
    }

    // -------------------------------------------------------------------
    // § 3  Filter + modulation
    // -------------------------------------------------------------------
    println!("\n§ 3  Filter + Modulation");
    println!("     {:<14}  description", "mod source");
    println!("     {}", "-".repeat(60));

    {
        // Filter env: fast pluck into a nearly-closed LP filter.
        let mut p = lead_base();
        p.filter.cutoff = 350.0;
        p.filter.resonance = 0.55;
        p.filter_env = ModEnvParams {
            attack: 0.004,
            decay: 0.38,
            sustain: 0.05,
            release: 0.2,
            depth: 1.0,
        };
        send(AudioEvent::LoadPatchChannel(CH_LEAD, Box::new(p)))?;
        println!(
            "     {:<14}  fast attack, 380 ms decay — SID-style cutoff pluck",
            "filter env"
        );
        play_phrase(&send)?;
    }

    {
        // LFO → cutoff: sine wah.
        let mut p = lead_base();
        p.filter.cutoff = 400.0;
        p.filter.resonance = 0.5;
        p.lfo = LfoParams {
            lfo_rate: 4.5,
            lfo_depth: 0.65,
            lfo_target: LfoTarget::Cutoff,
            lfo_shape: LfoShape::Sine,
        };
        send(AudioEvent::LoadPatchChannel(CH_LEAD, Box::new(p)))?;
        println!(
            "     {:<14}  LFO1 sine 4.5 Hz → cutoff — smooth filter wah",
            "lfo→cutoff"
        );
        play_phrase(&send)?;
    }

    {
        // LFO² S&H → cutoff + LFO¹ sine → pitch (dual LFO).
        let mut p = lead_base();
        p.lfo = LfoParams {
            lfo_rate: 5.0,
            lfo_depth: 0.2,
            lfo_target: LfoTarget::Pitch,
            lfo_shape: LfoShape::Sine,
        };
        p.lfo2 = LfoParams {
            lfo_rate: 6.5,
            lfo_depth: 0.6,
            lfo_target: LfoTarget::Cutoff,
            lfo_shape: LfoShape::SampleHold,
        };
        send(AudioEvent::LoadPatchChannel(CH_LEAD, Box::new(p)))?;
        println!(
            "     {:<14}  LFO1 vibrato + LFO2 S&H → cutoff — dual texture",
            "dual lfo"
        );
        play_phrase(&send)?;
    }

    {
        // Pitch env: short upward swoop on attack.
        let mut p = lead_base();
        p.pitch_env = ModEnvParams {
            attack: 0.003,
            decay: 0.25,
            sustain: 0.0,
            release: 0.08,
            depth: 0.85,
        };
        send(AudioEvent::LoadPatchChannel(CH_LEAD, Box::new(p)))?;
        println!(
            "     {:<14}  short upward pitch swoop on attack — zap feel",
            "pitch env"
        );
        play_phrase(&send)?;
    }

    // -------------------------------------------------------------------
    // § 4  FX chain
    // -------------------------------------------------------------------
    println!("\n§ 4  FX Chain");
    println!("     {:<14}  description", "effect");
    println!("     {}", "-".repeat(60));

    {
        // Slapback delay.
        let mut p = lead_base();
        p.delay = DelayParams {
            time_ms: 80.0,
            feedback: 0.0,
            mix: 0.6,
        };
        send(AudioEvent::LoadPatchChannel(CH_LEAD, Box::new(p)))?;
        println!(
            "     {:<14}  80 ms slapback, no feedback — chiptune echo",
            "delay"
        );
        play_phrase(&send)?;
    }

    {
        // Dotted-eighth delay with feedback.
        let mut p = lead_base();
        p.delay = DelayParams {
            time_ms: 375.0,
            feedback: 0.45,
            mix: 0.45,
        };
        send(AudioEvent::LoadPatchChannel(CH_LEAD, Box::new(p)))?;
        println!(
            "     {:<14}  375 ms dotted-eighth with feedback — rhythmic tails",
            "delay fb"
        );
        play_phrase(&send)?;
    }

    {
        // Chorus.
        let mut p = lead_base();
        p.chorus = ChorusParams {
            rate: 0.5,
            depth_ms: 3.0,
            mix: 0.75,
        };
        send(AudioEvent::LoadPatchChannel(CH_LEAD, Box::new(p)))?;
        println!(
            "     {:<14}  0.5 Hz gentle chorus — shimmer and width",
            "chorus"
        );
        play_phrase(&send)?;
    }

    {
        // Bitcrusher.
        let mut p = lead_base();
        p.crusher = CrusherParams {
            bits: 4.0,
            rate: 4.0,
        };
        send(AudioEvent::LoadPatchChannel(CH_LEAD, Box::new(p)))?;
        println!(
            "     {:<14}  4-bit depth, rate/4 decimation — lo-fi crunch",
            "bitcrusher"
        );
        play_phrase(&send)?;
    }

    // -------------------------------------------------------------------
    // § 5  Arpeggiator
    // -------------------------------------------------------------------
    #[cfg(feature = "arp")]
    {
        println!("\n§ 5  Arpeggiator (Am7: A3 C4 E4 G4  |  16th-note rate)");
        println!("     {:<8}  description", "mode");
        println!("     {}", "-".repeat(44));

        let arp_rate = BPM * 4.0 / 60.0; // 16th notes
        let arp_notes = [MidiNote(57), MidiNote(60), MidiNote(64), MidiNote(67)];

        let arp_lead = find("Arp Lead")?;

        for (mode, desc) in [
            (ArpMode::Up, "ascending, then wrap to root"),
            (ArpMode::Down, "descending, then wrap to top"),
            (ArpMode::UpDown, "ping-pong, no endpoint repeat"),
            (ArpMode::Random, "LFSR pseudo-random sequence"),
        ] {
            let mut p = arp_lead.clone();
            p.arp = ArpParams {
                enabled: false,
                rate: arp_rate,
                gate: 0.75,
                mode,
                ..ArpParams::default()
            };
            send(AudioEvent::LoadPatchChannel(CH_LEAD, Box::new(p)))?;
            send(AudioEvent::ArpSetNotes(CH_LEAD, arp_notes, 4))?;
            send(AudioEvent::ArpEnabled(CH_LEAD, true))?;

            let mode_name = match mode {
                ArpMode::Up => "Up",
                ArpMode::Down => "Down",
                ArpMode::UpDown => "UpDown",
                ArpMode::Random => "Random",
            };
            println!("     {mode_name:<8}  {desc}");
            play_arp_phrase(&send)?;
        }

        send(AudioEvent::ArpEnabled(CH_LEAD, false))?;
    }

    println!("\nDone.");
    send(AudioEvent::Panic)?;
    std::thread::sleep(Duration::from_millis(150));

    Ok(())
}
