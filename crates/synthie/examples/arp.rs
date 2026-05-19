//! Demonstrates the built-in arpeggiator across all four modes.
//!
//! Four two-bar phrases cycle through an A-minor progression.
//! The arpeggiator drives all melody timing; only bass and drums are explicit.
//!
//!   Phrase 1 – Up     : Am7 open  (A3 C4 E4 G4)
//!   Phrase 2 – Down   : Dm7       (D4 F4 A4 C5)
//!   Phrase 3 – `UpDown` : Em7       (E4 G4 B4 D5)
//!   Phrase 4 – Random : Am7 close (A3 E4 A4 C5)
//!
//! Channel 0 – Arp Lead (arpeggiator-driven)
//! Channel 1 – C64 Bass (root note each section)

use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use synthie::audio::engine::setup_audio;
use synthie::params::{ArpMode, ArpParams, AudioEvent, ChannelNo, DrumHit, MidiNote, SynthParams};
use synthie::presets::sid::default_patches;

const BPM: f32 = 140.0;
/// 16th-note step rate (4 steps per beat).
const ARP_RATE: f32 = BPM * 4.0 / 60.0;
const ARP_GATE: f32 = 0.75;
/// Two 4-beat bars per section.
const PHRASE_BEATS: f32 = 8.0;

const CH_ARP: ChannelNo = ChannelNo(0);
const CH_BASS: ChannelNo = ChannelNo(1);

fn beats(n: f32) -> Duration {
    Duration::from_secs_f32(n * 60.0 / BPM)
}

struct Section {
    label: &'static str,
    mode: ArpMode,
    notes: [MidiNote; 4],
    bass: MidiNote,
}

static SECTIONS: &[Section] = &[
    Section {
        label: "Up     — Am7 (A3 C4 E4 G4)",
        mode: ArpMode::Up,
        notes: [MidiNote(57), MidiNote(60), MidiNote(64), MidiNote(67)],
        bass: MidiNote(45), // A2
    },
    Section {
        label: "Down   — Dm7 (D4 F4 A4 C5)",
        mode: ArpMode::Down,
        notes: [MidiNote(62), MidiNote(65), MidiNote(69), MidiNote(72)],
        bass: MidiNote(50), // D3
    },
    Section {
        label: "UpDown — Em7 (E4 G4 B4 D5)",
        mode: ArpMode::UpDown,
        notes: [MidiNote(64), MidiNote(67), MidiNote(71), MidiNote(74)],
        bass: MidiNote(52), // E3
    },
    Section {
        label: "Random — Am7 (A3 E4 A4 C5)",
        mode: ArpMode::Random,
        notes: [MidiNote(57), MidiNote(64), MidiNote(69), MidiNote(72)],
        bass: MidiNote(45), // A2
    },
];

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

    let arp_lead = find("Arp Lead")?;
    let c64_bass = find("C64 Bass")?;

    event_tx.send(AudioEvent::LoadPatchChannel(CH_BASS, Box::new(c64_bass)))?;

    println!("=== Arpeggiator Demo ===");
    println!("{BPM} BPM  |  16th-note rate ({ARP_RATE:.1} Hz)  |  2 bars per section\n");

    for section in SECTIONS {
        println!("  {}", section.label);

        // Load patch with mode set; start disabled so ArpSetNotes resets state cleanly.
        let mut params = arp_lead.clone();
        params.arp = ArpParams {
            enabled: false,
            rate: ARP_RATE,
            gate: ARP_GATE,
            mode: section.mode,
            ..ArpParams::default()
        };
        event_tx.send(AudioEvent::LoadPatchChannel(CH_ARP, Box::new(params)))?;

        // Arm note list (resets phase, step, and sounding state).
        event_tx.send(AudioEvent::ArpSetNotes(CH_ARP, section.notes, 4))?;

        // Enable arpeggiator.
        event_tx.send(AudioEvent::ArpEnabled(CH_ARP, true))?;

        let started = Instant::now();

        // Bass root note for the full section.
        event_tx.send(AudioEvent::NoteOnChannel(CH_BASS, section.bass))?;

        // Drums: 4-on-the-floor kick + 8th-note hi-hats over 2 bars.
        let mut drum_events: Vec<(Duration, DrumHit)> = Vec::new();
        for bar in 0..2u8 {
            let bar_start = f32::from(bar) * 4.0;
            for beat in 0u8..4 {
                drum_events.push((beats(bar_start + f32::from(beat)), DrumHit::Kick));
            }
            for eighth in 0u8..8 {
                drum_events.push((
                    beats(bar_start + f32::from(eighth) * 0.5),
                    DrumHit::HiHatClosed,
                ));
            }
        }
        drum_events.sort_by_key(|e| e.0);

        for (t, hit) in &drum_events {
            let deadline = started + *t;
            let now = Instant::now();
            if deadline > now {
                std::thread::sleep(deadline.duration_since(now));
            }
            event_tx.send(AudioEvent::Drum(*hit))?;
        }

        // Release bass just before phrase end.
        event_tx.send(AudioEvent::NoteOffChannel(CH_BASS, section.bass))?;

        // Wait out the phrase.
        let deadline = started + beats(PHRASE_BEATS);
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline.duration_since(now));
        }
    }

    println!();
    std::thread::sleep(beats(1.0));
    event_tx.send(AudioEvent::Panic)?;
    std::thread::sleep(Duration::from_millis(150));

    Ok(())
}
