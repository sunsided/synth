//! Plays the Zombie Nation / Kernkraft 400 structure (Zombie Nation, 1999).
//!
//! Source: assets/zombie.mid — 70 BPM half-time notation (Bb minor).
//! Song sections: Intro → Hook × 4 → Interlude → Hook × 4.
//!
//! Channel 0 – Arp Lead (melody, note ≥ 58 = A#3).
//! Channel 1 – C64 Bass  (bass,   note < 58).

use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use synthie::audio::engine::setup_audio;
use synthie::params::{AudioEvent, ChannelNo, DrumHit, MidiNote};
use synthie::presets::sid::default_patches;

const BPM: f32 = 70.0;
const BEATS_PER_BAR: f32 = 4.0;
const HOLD: f32 = 0.23; // just under a 16th note (0.25 beats at 70 BPM)
const HOOK_PHRASE_BEATS: f32 = 8.0; // 2-bar hook phrase
const HOOK_REPS: u8 = 4; // repetitions per hook section
const INTRO_BEATS: u8 = 16; // 4 bars
const INTERLUDE_BEATS: u8 = 8; // 2 bars

// Channel indices
const MELODY: u8 = 0;
const BASS: u8 = 1;

// Hook melody beats 48–56 (relative offset).
// Notes ≥ 58 (A#3). Bb minor arpeggios shifting to Ebm and Fm.
static HOOK_MELODY: &[(f32, u8)] = &[
    (0.25, 58), // A#3 — Bbm arpeggio start
    (0.50, 61), // C#4
    (0.75, 63), // D#4
    (1.00, 65), // F4  — arpeggio peak
    (2.25, 58), // A#3 — Bbm repeat
    (2.50, 61), // C#4
    (2.75, 63), // D#4
    (3.00, 65), // F4
    (3.25, 66), // F#4 — variation note leading into chord change
    (3.50, 65), // F4
    (3.75, 61), // C#4
    (4.00, 63), // D#4 — Ebm section
    (5.00, 61), // C#4 — Fm section
    (5.50, 65), // F4
    (5.75, 58), // A#3 — resolution
];

// Hook bass beats 48–56.
// Notes < 58. Alternating A#1/A#2 ostinato; shifts to D#2/D#3 (Ebm) then F2/F3 (Fm).
static HOOK_BASS: &[(f32, u8)] = &[
    (0.00, 34), // A#1
    (0.25, 46), // A#2
    (0.50, 34), // A#1
    (0.75, 46), // A#2
    (1.00, 34), // A#1
    (1.25, 46), // A#2
    (1.50, 34), // A#1
    (1.75, 46), // A#2
    (2.00, 34), // A#1
    (2.25, 46), // A#2
    (2.50, 34), // A#1
    (2.75, 46), // A#2
    (3.00, 34), // A#1
    (3.25, 46), // A#2
    (3.50, 34), // A#1
    (3.75, 46), // A#2
    (4.00, 39), // D#2 — Ebm
    (4.25, 51), // D#3
    (4.50, 39), // D#2
    (4.75, 51), // D#3
    (5.00, 41), // F2  — Fm
    (5.25, 53), // F3
    (5.50, 41), // F2
    (5.75, 53), // F3
    (6.00, 34), // A#1 — Bbm return
    (6.25, 46), // A#2
    (6.50, 34), // A#1
    (6.75, 46), // A#2
    (7.00, 34), // A#1
    (7.25, 46), // A#2
    (7.50, 34), // A#1
    (7.75, 46), // A#2
];

#[derive(Clone, Copy)]
enum ScheduledKind {
    NoteOn(ChannelNo, MidiNote),
    NoteOff(ChannelNo, MidiNote),
    Drum(DrumHit),
}

#[derive(Clone, Copy)]
struct TimedEvent {
    at: Duration,
    kind: ScheduledKind,
}

fn beats(n: f32) -> Duration {
    Duration::from_secs_f32(n * 60.0 / BPM)
}

fn push_note(events: &mut Vec<TimedEvent>, at: f32, ch: u8, midi: u8) {
    let ch = ChannelNo(ch);
    let note = MidiNote(midi);
    events.push(TimedEvent {
        at: beats(at),
        kind: ScheduledKind::NoteOn(ch, note),
    });
    events.push(TimedEvent {
        at: beats(at + HOLD),
        kind: ScheduledKind::NoteOff(ch, note),
    });
}

/// Alternating A#1/A#2 bass ostinato on every 16th note for `num_beats` beats.
fn bass_ostinato(events: &mut Vec<TimedEvent>, start: f32, num_beats: u8) {
    let slots = u16::from(num_beats) * 4; // 16th-note slots
    for slot in 0u16..slots {
        let t = start + f32::from(slot) * 0.25;
        let midi: u8 = if slot % 2 == 0 { 34 } else { 46 }; // A#1 / A#2
        push_note(events, t, BASS, midi);
    }
}

/// One repetition of the 8-beat hook phrase starting at `start`.
fn push_hook_phrase(events: &mut Vec<TimedEvent>, start: f32) {
    for &(off, midi) in HOOK_MELODY {
        push_note(events, start + off, MELODY, midi);
    }
    for &(off, midi) in HOOK_BASS {
        push_note(events, start + off, BASS, midi);
    }
}

/// Kick on every beat + 16th-note hi-hats for `num_bars` bars starting at `start`.
fn push_drums(events: &mut Vec<TimedEvent>, start: f32, num_bars: u8) {
    for (_, bar) in (0..usize::from(num_bars)).zip(0u8..) {
        let bar_start = start + f32::from(bar) * BEATS_PER_BAR;
        for beat in 0u8..4 {
            events.push(TimedEvent {
                at: beats(bar_start + f32::from(beat)),
                kind: ScheduledKind::Drum(DrumHit::Kick),
            });
        }
        for sixteenth in 0u8..16 {
            events.push(TimedEvent {
                at: beats(bar_start + f32::from(sixteenth) * 0.25),
                kind: ScheduledKind::Drum(DrumHit::HiHatClosed),
            });
        }
    }
}

fn build_song() -> Vec<TimedEvent> {
    let mut events = Vec::new();
    let intro_f = f32::from(INTRO_BEATS);

    // Intro: bass ostinato only (melody enters with the hook)
    bass_ostinato(&mut events, 0.0, INTRO_BEATS);
    push_drums(&mut events, 0.0, INTRO_BEATS / 4);

    // Hook 1
    let hook1_start = intro_f;
    for rep in 0u8..HOOK_REPS {
        let start = hook1_start + f32::from(rep) * HOOK_PHRASE_BEATS;
        push_hook_phrase(&mut events, start);
        push_hook_phrase(&mut events, start); // Track 2 (identical duplicate — for testing)
    }
    push_drums(&mut events, hook1_start, HOOK_REPS * 2); // 2 bars per phrase

    // Interlude: bass ostinato
    let interlude_start = hook1_start + f32::from(HOOK_REPS) * HOOK_PHRASE_BEATS;
    bass_ostinato(&mut events, interlude_start, INTERLUDE_BEATS);
    push_drums(&mut events, interlude_start, INTERLUDE_BEATS / 4);

    // Hook 2
    let hook2_start = interlude_start + f32::from(INTERLUDE_BEATS);
    for rep in 0u8..HOOK_REPS {
        let start = hook2_start + f32::from(rep) * HOOK_PHRASE_BEATS;
        push_hook_phrase(&mut events, start);
        push_hook_phrase(&mut events, start); // Track 2 (identical duplicate — for testing)
    }
    push_drums(&mut events, hook2_start, HOOK_REPS * 2);

    events.sort_by_key(|e| e.at);
    events
}

fn main() -> Result<()> {
    let (_stream, event_tx, _scope_rx) = setup_audio()?;

    let patches = default_patches();
    let find = |name: &str| {
        patches
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| anyhow!("preset '{name}' not found"))
    };

    event_tx.send(AudioEvent::LoadPatch(Box::new(
        find("Arp Lead")?.params.clone(),
    )))?;
    event_tx.send(AudioEvent::LoadPatchChannel(
        ChannelNo(1),
        Box::new(find("C64 Bass")?.params.clone()),
    ))?;

    let events = build_song();
    let started = Instant::now();

    for event in events {
        let deadline = started + event.at;
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline.duration_since(now));
        }
        let audio_event = match event.kind {
            ScheduledKind::NoteOn(ch, note) => AudioEvent::NoteOnChannel(ch, note),
            ScheduledKind::NoteOff(ch, note) => AudioEvent::NoteOffChannel(ch, note),
            ScheduledKind::Drum(hit) => AudioEvent::Drum(hit),
        };
        event_tx.send(audio_event)?;
    }

    // Let the last notes ring out
    std::thread::sleep(beats(2.0));
    event_tx.send(AudioEvent::Panic)?;
    std::thread::sleep(Duration::from_millis(120));

    Ok(())
}
