//! Plays the Lazy Jones melody (David Whittaker, 1984) — the hook that became
//! Kernkraft 400 by Zombie Nation.
//!
//! Source: assets/lazy.mid (140 BPM, B minor, 8th-note grid).
//! Channel 0 – Arp Lead (melody, note ≥ 64).
//! Channel 1 – C64 Bass  (bass,   note < 64).

use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use synthie::audio::engine::setup_audio;
use synthie::params::{AudioEvent, ChannelNo, DrumHit, MidiNote};
use synthie::presets::sid::default_patches;

const BPM: f32 = 140.0;
const BEATS_PER_BAR: f32 = 4.0;
const HOLD: f32 = 0.45; // just under a half-beat (8th note duration)
const PHRASE_BEATS: f32 = 16.0; // one 4-bar phrase
const BARS: usize = 8; // 2 passes × 4 bars

// Channel indices
const MELODY: u8 = 0;
const BASS: u8 = 1;

// Notes, beat-offset / channel / MIDI note.
// note >= 64 → melody (ch 0);  note < 64 → bass (ch 1).
static PHRASE: &[(f32, u8, u8)] = &[
    (0.0, BASS, 47),    // B2
    (0.5, MELODY, 71),  // B4
    (1.0, MELODY, 74),  // D5
    (1.5, MELODY, 76),  // E5
    (2.0, MELODY, 78),  // F#5
    (2.5, BASS, 59),    // B3
    (3.0, BASS, 47),    // B2
    (3.5, BASS, 59),    // B3
    (4.0, BASS, 47),    // B2
    (4.5, MELODY, 71),  // B4
    (5.0, MELODY, 74),  // D5
    (5.5, MELODY, 76),  // E5
    (6.0, MELODY, 78),  // F#5
    (6.5, MELODY, 79),  // G5
    (7.0, MELODY, 78),  // F#5
    (7.5, MELODY, 74),  // D5
    (8.0, MELODY, 76),  // E5
    (8.5, MELODY, 64),  // E4
    (9.0, BASS, 52),    // E3
    (9.5, MELODY, 64),  // E4
    (10.0, MELODY, 74), // D5
    (10.5, MELODY, 66), // F#4
    (11.0, MELODY, 78), // F#5
    (11.5, MELODY, 71), // B4
    (12.0, BASS, 47),   // B2
    (12.5, BASS, 59),   // B3
    (13.0, BASS, 47),   // B2
    (13.5, BASS, 59),   // B3
    (14.0, BASS, 47),   // B2
    (14.5, BASS, 59),   // B3
    (15.0, BASS, 47),   // B2
    (15.5, BASS, 59),   // B3
];

// Extra low-register bass.
// Added only on the second phrase pass, on 8th-note offbeats.
// B1 sustains the Bm section; E2/F#2 mark the harmonic change.
static SECOND_PASS_BASS: &[(f32, u8)] = &[
    (0.5, 35), // B1
    (1.5, 35),
    (2.5, 35),
    (3.5, 35),
    (4.5, 35),
    (5.5, 35),
    (6.5, 35),
    (7.5, 35),
    (8.5, 40), // E2
    (9.5, 40),
    (10.5, 42), // F#2
    (11.5, 42),
    (12.5, 35), // B1
    (13.5, 35),
    (14.5, 35),
    (15.5, 35),
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

fn build_song() -> Vec<TimedEvent> {
    let mut events = Vec::new();

    // Pass 1: melody + bass
    for &(off, ch, midi) in PHRASE {
        push_note(&mut events, off, ch, midi);
    }

    // Pass 2: same phrase + extra low bass
    for &(off, ch, midi) in PHRASE {
        push_note(&mut events, PHRASE_BEATS + off, ch, midi);
    }
    for &(off, midi) in SECOND_PASS_BASS {
        push_note(&mut events, PHRASE_BEATS + off, BASS, midi);
    }

    // Drums: 4-on-the-floor kick, 8th-note hi-hats
    for (_, bar) in (0..BARS).zip(0u8..) {
        let bar_start = f32::from(bar) * BEATS_PER_BAR;
        for beat in 0u8..4 {
            events.push(TimedEvent {
                at: beats(bar_start + f32::from(beat)),
                kind: ScheduledKind::Drum(DrumHit::Kick),
            });
        }
        for eighth in 0u8..8 {
            events.push(TimedEvent {
                at: beats(bar_start + f32::from(eighth) * 0.5),
                kind: ScheduledKind::Drum(DrumHit::HiHatClosed),
            });
        }
    }

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

    std::thread::sleep(beats(2.0));
    event_tx.send(AudioEvent::Panic)?;
    std::thread::sleep(Duration::from_millis(120));

    Ok(())
}
