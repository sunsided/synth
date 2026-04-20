//! Stereo tune example with panned chords, lead, and drums.

use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use synth::audio::engine::setup_audio;
use synth::params::{AudioEvent, DrumHit};
use synth::presets::sid::default_patches;

const BPM: f32 = 110.0;
const BARS: usize = 16;
const BEATS_PER_BAR: f32 = 4.0;
const CHORD_HOLD_BEATS: f32 = 3.7;
const MELODY_HOLD_BEATS: f32 = 0.85;
const BEAT_OFFSETS: [f32; 4] = [0.0, 1.0, 2.0, 3.0];

struct PanMap {
    chord: f32,
    melody: f32,
    kick: f32,
    hat_closed: f32,
    hat_open: f32,
}

impl Default for PanMap {
    fn default() -> Self {
        Self {
            chord: -0.35,
            melody: 0.35,
            kick: 0.0,
            hat_closed: 0.6,
            hat_open: -0.6,
        }
    }
}

#[derive(Clone, Copy)]
enum ScheduledKind {
    NoteOn { midi: u8, pan: f32 },
    NoteOff(u8),
    Drum { hit: DrumHit, pan: f32 },
}

#[derive(Clone, Copy)]
struct TimedEvent {
    at: Duration,
    kind: ScheduledKind,
}

fn beats(n: f32) -> Duration {
    Duration::from_secs_f32(n * 60.0 / BPM)
}

fn build_drum_pattern(pans: &PanMap) -> Vec<TimedEvent> {
    let mut drum_events = Vec::with_capacity(BARS * 10);
    let hat_offsets: [f32; 8] = [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5];

    for (_bar_idx, bar_u8) in (0..BARS).zip(0u8..) {
        let bar_start_beats = f32::from(bar_u8) * BEATS_PER_BAR;

        drum_events.push(TimedEvent {
            at: beats(bar_start_beats),
            kind: ScheduledKind::Drum {
                hit: DrumHit::Kick,
                pan: pans.kick,
            },
        });
        drum_events.push(TimedEvent {
            at: beats(bar_start_beats + 2.0),
            kind: ScheduledKind::Drum {
                hit: DrumHit::Kick,
                pan: pans.kick,
            },
        });

        for offset in hat_offsets {
            let hit = if (offset - 3.5).abs() < f32::EPSILON {
                DrumHit::HiHatOpen
            } else {
                DrumHit::HiHatClosed
            };
            let pan = match hit {
                DrumHit::Kick => pans.kick,
                DrumHit::HiHatClosed => pans.hat_closed,
                DrumHit::HiHatOpen => pans.hat_open,
            };
            drum_events.push(TimedEvent {
                at: beats(bar_start_beats + offset),
                kind: ScheduledKind::Drum { hit, pan },
            });
        }
    }

    drum_events
}

fn build_tune() -> Vec<TimedEvent> {
    let pans = PanMap::default();
    let progression: [[u8; 3]; 4] = [
        [60, 64, 67], // C major
        [55, 59, 62], // G major
        [57, 60, 64], // A minor
        [53, 57, 60], // F major
    ];

    let melody: [u8; 16] = [
        72, 74, 76, 79, 76, 74, 72, 71, 72, 74, 76, 79, 81, 79, 76, 74,
    ];

    let mut events = Vec::with_capacity(BARS * 3 * 2 + BARS * 4 * 2);

    for (bar_idx, bar_u8) in (0..BARS).zip(0u8..) {
        let chord = progression[bar_idx % progression.len()];
        let bar_start_beats = f32::from(bar_u8) * BEATS_PER_BAR;

        for note in chord {
            events.push(TimedEvent {
                at: beats(bar_start_beats),
                kind: ScheduledKind::NoteOn {
                    midi: note,
                    pan: pans.chord,
                },
            });
            events.push(TimedEvent {
                at: beats(bar_start_beats + CHORD_HOLD_BEATS),
                kind: ScheduledKind::NoteOff(note),
            });
        }

        for (beat_idx, beat_offset) in BEAT_OFFSETS.into_iter().enumerate() {
            let step = bar_idx * 4 + beat_idx;
            let note = melody[step % melody.len()];
            let start = bar_start_beats + beat_offset;
            events.push(TimedEvent {
                at: beats(start),
                kind: ScheduledKind::NoteOn {
                    midi: note,
                    pan: pans.melody,
                },
            });
            events.push(TimedEvent {
                at: beats(start + MELODY_HOLD_BEATS),
                kind: ScheduledKind::NoteOff(note),
            });
        }
    }

    events.extend(build_drum_pattern(&pans));
    events.sort_by_key(|event| event.at);
    events
}

fn main() -> Result<()> {
    let (_stream, event_tx, _scope_rx) = setup_audio()?;

    let patch = default_patches()
        .into_iter()
        .find(|patch| patch.name == "PWM Lead")
        .ok_or_else(|| anyhow!("preset 'PWM Lead' not found"))?;
    event_tx.send(AudioEvent::LoadPatch(Box::new(patch.params)))?;

    let events = build_tune();
    let started = Instant::now();

    for event in events {
        let deadline = started + event.at;
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline.duration_since(now));
        }

        let audio_event = match event.kind {
            ScheduledKind::NoteOn { midi, pan } => AudioEvent::note_on_panned(midi, pan),
            ScheduledKind::NoteOff(midi) => AudioEvent::NoteOff(midi),
            ScheduledKind::Drum { hit, pan } => AudioEvent::drum_panned(hit, pan),
        };
        event_tx.send(audio_event)?;
    }

    std::thread::sleep(beats(2.0));
    event_tx.send(AudioEvent::Panic)?;
    std::thread::sleep(Duration::from_millis(120));

    Ok(())
}
