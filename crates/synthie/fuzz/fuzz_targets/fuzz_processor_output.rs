//! Fuzz target: `SynthProcessor` output bounds under arbitrary control.
//!
//! Drives a processor with an arbitrary stream of note, drum, patch, panic,
//! live parameter and arpeggiator events while riding the master gain (any `f32` bit pattern, including NaN,
//! infinities, subnormals and `±f32::MAX`) and the voice normalization between
//! blocks. Whatever the input, every rendered sample must be finite and inside
//! `[-1, 1]`, and non-finite gains must never replace the current one.
//!
//! # Input layout
//! The input is consumed in 6-byte commands:
//!
//! | byte 0 (op % 8) | bytes 1..=4                           | byte 5        |
//! |-----------------|---------------------------------------|---------------|
//! | 0 note on       | channel, note, –, –                   | block frames  |
//! | 1 note off      | channel, note, –, –                   | block frames  |
//! | 2 drum          | hit, –, –, –                          | block frames  |
//! | 3 load preset   | channel, preset, –, –                 | block frames  |
//! | 4 panic         | –                                     | block frames  |
//! | 5 controls      | gain as little-endian `f32` bits      | normalization |
//! | 6 set param     | channel, param, value (high 16 bits)  | block frames  |
//! | 7 arpeggiator   | channel/count/enable, three notes     | block frames  |
//!
//! Commands other than 5 render a block of `byte 5` frames (0 renders
//! nothing). A set-param value is the top half of an `f32`: every sign and
//! exponent, so NaN, infinities, huge and tiny values all occur. For the
//! arpeggiator, byte 1 holds the channel (bits 0–1), the note count (bits
//! 2–5, beyond the 4 the arpeggiator holds) and the enable flag (bit 6).

#![no_main]

use libfuzzer_sys::fuzz_target;
use synthie::audio::processor::{NUM_CHANNELS, SynthProcessor, VoiceNormalization};
use synthie::param::Param;
use synthie::params::{AudioEvent, ChannelNo, DrumHit, MidiNote};
use synthie::presets::sid::default_patches;

const DRUMS: [DrumHit; 9] = DrumHit::ALL;

#[allow(clippy::cast_possible_truncation)] // NUM_CHANNELS is tiny
fn channel(byte: u8) -> ChannelNo {
    ChannelNo(byte % NUM_CHANNELS as u8)
}

fuzz_target!(|data: &[u8]| {
    let patches = default_patches();
    let mut proc = SynthProcessor::<NUM_CHANNELS>::new(44_100.0);
    let mut buf = [0.0_f32; 255];

    for cmd in data.as_chunks::<6>().0 {
        let events = match cmd[0] % 8 {
            0 => vec![AudioEvent::NoteOnChannel(
                channel(cmd[1]),
                MidiNote(cmd[2] % 128),
            )],
            1 => vec![AudioEvent::NoteOffChannel(
                channel(cmd[1]),
                MidiNote(cmd[2] % 128),
            )],
            2 => vec![AudioEvent::Drum(DRUMS[usize::from(cmd[1]) % DRUMS.len()])],
            3 => {
                let patch = &patches[usize::from(cmd[2]) % patches.len()];
                vec![AudioEvent::LoadPatchChannel(
                    channel(cmd[1]),
                    Box::new(patch.params.clone()),
                )]
            }
            4 => vec![AudioEvent::Panic],
            6 => {
                let param = Param::ALL[usize::from(cmd[2]) % Param::ALL.len()];
                let value = f32::from_bits(u32::from(u16::from_le_bytes([cmd[3], cmd[4]])) << 16);
                vec![AudioEvent::SetParam(channel(cmd[1]), param, value)]
            }
            7 => {
                let ch = channel(cmd[1]);
                let notes = [cmd[2], cmd[3], cmd[4], cmd[2] ^ cmd[3]].map(|n| MidiNote(n % 128));
                vec![
                    AudioEvent::ArpEnabled(ch, cmd[1] & 0x40 != 0),
                    AudioEvent::ArpSetNotes(ch, notes, (cmd[1] >> 2) & 0x0f),
                ]
            }
            _ => {
                let gain = f32::from_le_bytes([cmd[1], cmd[2], cmd[3], cmd[4]]);
                let before = proc.master_gain();
                proc.set_master_gain(gain);
                let expected = if gain.is_finite() { gain } else { before };
                assert_eq!(proc.master_gain().to_bits(), expected.to_bits());

                proc.set_voice_normalization(if cmd[5] & 1 == 0 {
                    VoiceNormalization::Polyphony
                } else {
                    VoiceNormalization::Off
                });
                continue;
            }
        };

        let frames = &mut buf[..usize::from(cmd[5])];
        proc.process_block(&events, frames, 1);
        for &s in frames.iter() {
            assert!(s.is_finite(), "non-finite sample {s}");
            assert!((-1.0..=1.0).contains(&s), "sample {s} out of range");
        }
    }
});
