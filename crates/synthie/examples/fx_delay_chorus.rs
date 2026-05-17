//! Delay and chorus FX demo: plays a 4-bar C–Am–F–G phrase with the "PWM Lead"
//! preset and cycles through delay and chorus settings so you can hear each
//! effect in context rather than on a static drone.
//!
//! Run:  `cargo run -p synthie --example fx_delay_chorus`

use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use synthie::audio::engine::setup_audio;
use synthie::params::{AudioEvent, ChorusParams, DelayParams, MidiNote};
use synthie::presets::sid::default_patches;

const BPM: f32 = 100.0;
const BEATS_PER_BAR: f32 = 4.0;

// --- timing helpers ----------------------------------------------------------

fn beats(n: f32) -> Duration {
    Duration::from_secs_f32(n * 60.0 / BPM)
}

// --- phrase ------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Ev {
    On(u8),
    Off(u8),
}

/// One note on/off pair with beat-relative timing.
#[derive(Clone, Copy)]
struct TimedEvent {
    at: Duration,
    ev: Ev,
}

fn note(at: f32, hold: f32, midi: u8) -> [TimedEvent; 2] {
    [
        TimedEvent {
            at: beats(at),
            ev: Ev::On(midi),
        },
        TimedEvent {
            at: beats(at + hold),
            ev: Ev::Off(midi),
        },
    ]
}

/// 4-bar C–Am–F–G phrase: sustained chords on beats 1 and 3, ascending melody.
///
/// Chords give the chorus something wide to thicken; the spaced melody lets
/// delay repeats speak clearly before the next note.
fn build_phrase() -> Vec<TimedEvent> {
    const CHORD_HOLD: f32 = 1.8;
    const MEL_HOLD: f32 = 0.85;

    // Chord roots / thirds / fifths – one per half-bar (beats 0 and 2).
    let chords: [([u8; 3], f32); 8] = [
        ([60, 64, 67], 0.0),  // C  – bar 0, beat 1
        ([60, 64, 67], 2.0),  // C  – bar 0, beat 3
        ([57, 60, 64], 4.0),  // Am – bar 1, beat 1
        ([57, 60, 64], 6.0),  // Am – bar 1, beat 3
        ([53, 57, 60], 8.0),  // F  – bar 2, beat 1
        ([53, 57, 60], 10.0), // F  – bar 2, beat 3
        ([55, 59, 62], 12.0), // G  – bar 3, beat 1
        ([55, 59, 62], 14.0), // G  – bar 3, beat 3
    ];

    // Simple quarter-note melody across 4 bars.
    let melody: [(u8, f32); 16] = [
        (72, 0.0),  // C5
        (76, 1.0),  // E5
        (79, 2.0),  // G5
        (76, 3.0),  // E5
        (74, 4.0),  // D5
        (72, 5.0),  // C5
        (69, 6.0),  // A4
        (71, 7.0),  // B4
        (72, 8.0),  // C5
        (69, 9.0),  // A4
        (65, 10.0), // F4
        (67, 11.0), // G4
        (67, 12.0), // G4
        (69, 13.0), // A4
        (71, 14.0), // B4
        (72, 15.0), // C5
    ];

    let mut events: Vec<TimedEvent> = Vec::new();

    for ([r, t, f], beat) in chords {
        for midi in [r, t, f] {
            events.extend(note(beat, CHORD_HOLD, midi));
        }
    }
    for (midi, beat) in melody {
        events.extend(note(beat, MEL_HOLD, midi));
    }

    events.sort_by_key(|e| e.at);
    events
}

// --- FX phases ---------------------------------------------------------------

struct FxPhase {
    label: &'static str,
    desc: &'static str,
    delay: DelayParams,
    chorus: ChorusParams,
}

static PHASES: &[FxPhase] = &[
    FxPhase {
        label: "dry",
        desc: "no FX – reference sound",
        delay: DelayParams {
            time_ms: 375.0,
            feedback: 0.0,
            mix: 0.0,
        },
        chorus: ChorusParams {
            rate: 0.5,
            depth_ms: 3.0,
            mix: 0.0,
        },
    },
    FxPhase {
        label: "slapback",
        desc: "80 ms echo, no feedback – rockabilly/chiptune flavour",
        delay: DelayParams {
            time_ms: 80.0,
            feedback: 0.0,
            mix: 0.6,
        },
        chorus: ChorusParams {
            rate: 0.5,
            depth_ms: 3.0,
            mix: 0.0,
        },
    },
    FxPhase {
        label: "dotted echo",
        desc: "375 ms dotted-eighth echo with feedback – rhythmic tails",
        delay: DelayParams {
            time_ms: 375.0,
            feedback: 0.45,
            mix: 0.45,
        },
        chorus: ChorusParams {
            rate: 0.5,
            depth_ms: 3.0,
            mix: 0.0,
        },
    },
    FxPhase {
        label: "chorus",
        desc: "gentle 0.5 Hz chorus – chords thicken, melody shimmers",
        delay: DelayParams {
            time_ms: 375.0,
            feedback: 0.0,
            mix: 0.0,
        },
        chorus: ChorusParams {
            rate: 0.5,
            depth_ms: 3.0,
            mix: 0.7,
        },
    },
    FxPhase {
        label: "deep chorus",
        desc: "wide 2.5 Hz chorus – strong pitch modulation",
        delay: DelayParams {
            time_ms: 375.0,
            feedback: 0.0,
            mix: 0.0,
        },
        chorus: ChorusParams {
            rate: 2.5,
            depth_ms: 6.0,
            mix: 0.8,
        },
    },
    FxPhase {
        label: "delay+chorus",
        desc: "echo tails with chorus thickening – classic retro pad sound",
        delay: DelayParams {
            time_ms: 375.0,
            feedback: 0.4,
            mix: 0.4,
        },
        chorus: ChorusParams {
            rate: 0.5,
            depth_ms: 3.0,
            mix: 0.6,
        },
    },
    FxPhase {
        label: "dry",
        desc: "restored – confirm no tail bleed between phases",
        delay: DelayParams {
            time_ms: 375.0,
            feedback: 0.0,
            mix: 0.0,
        },
        chorus: ChorusParams {
            rate: 0.5,
            depth_ms: 3.0,
            mix: 0.0,
        },
    },
];

// --- main --------------------------------------------------------------------

fn main() -> Result<()> {
    let (_stream, event_tx, _scope_rx) = setup_audio()?;

    let base = default_patches()
        .into_iter()
        .find(|p| p.name == "PWM Lead")
        .ok_or_else(|| anyhow!("preset 'PWM Lead' not found"))?
        .params;

    println!("=== Delay / Chorus FX Demo ===");
    println!("PWM Lead  |  C–Am–F–G  |  {BPM} BPM  |  4 bars per phase\n");

    let phrase = build_phrase();

    for phase in PHASES {
        let mut patch = base.clone();
        patch.delay = phase.delay.clone();
        patch.chorus = phase.chorus.clone();

        event_tx.send(AudioEvent::LoadPatch(Box::new(patch)))?;
        println!("[{}]  {}", phase.label, phase.desc);

        let started = Instant::now();
        for ev in &phrase {
            let deadline = started + ev.at;
            let now = Instant::now();
            if deadline > now {
                std::thread::sleep(deadline.duration_since(now));
            }
            let msg = match ev.ev {
                Ev::On(midi) => AudioEvent::NoteOn(MidiNote(midi)),
                Ev::Off(midi) => AudioEvent::NoteOff(MidiNote(midi)),
            };
            event_tx.send(msg)?;
        }

        // Brief silence so any delay/reverb tail is audible before the next phase.
        std::thread::sleep(beats(BEATS_PER_BAR));
    }

    println!("\nDone.");
    event_tx.send(AudioEvent::Panic)?;
    std::thread::sleep(Duration::from_millis(150));

    Ok(())
}
