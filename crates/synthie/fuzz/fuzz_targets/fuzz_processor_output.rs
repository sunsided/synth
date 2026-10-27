//! Fuzz target: `SynthProcessor` output bounds under arbitrary control.
//!
//! Drives a processor with an arbitrary stream of note, drum, patch and panic
//! events while riding the master gain (any `f32` bit pattern, including NaN,
//! infinities, subnormals and `±f32::MAX`) and the voice normalization between
//! blocks. Whatever the input, every rendered sample must be finite and inside
//! `[-1, 1]`, and non-finite gains must never replace the current one.
//!
//! # Input layout
//! The input is consumed in 6-byte commands:
//!
//! | byte 0 (op % 6) | bytes 1..=4                      | byte 5        |
//! |-----------------|----------------------------------|---------------|
//! | 0 note on       | channel, note, –, –              | block frames  |
//! | 1 note off      | channel, note, –, –              | block frames  |
//! | 2 drum          | hit, –, –, –                     | block frames  |
//! | 3 load preset   | channel, preset, –, –            | block frames  |
//! | 4 panic         | –                                | block frames  |
//! | 5 controls      | gain as little-endian `f32` bits | normalization |
//!
//! Commands 0–4 render a block of `byte 5` frames (0 renders nothing).

#![no_main]

use libfuzzer_sys::fuzz_target;
use synthie::audio::processor::{NUM_CHANNELS, SynthProcessor, VoiceNormalization};
use synthie::params::{AudioEvent, ChannelNo, DrumHit, MidiNote};
use synthie::presets::sid::default_patches;

const DRUMS: [DrumHit; 3] = [DrumHit::Kick, DrumHit::HiHatClosed, DrumHit::HiHatOpen];

#[allow(clippy::cast_possible_truncation)] // NUM_CHANNELS is tiny
fn channel(byte: u8) -> ChannelNo {
    ChannelNo(byte % NUM_CHANNELS as u8)
}

fuzz_target!(|data: &[u8]| {
    let patches = default_patches();
    let mut proc = SynthProcessor::<NUM_CHANNELS>::new(44_100.0);
    let mut buf = [0.0_f32; 255];

    for cmd in data.as_chunks::<6>().0 {
        let event = match cmd[0] % 6 {
            0 => AudioEvent::NoteOnChannel(channel(cmd[1]), MidiNote(cmd[2] % 128)),
            1 => AudioEvent::NoteOffChannel(channel(cmd[1]), MidiNote(cmd[2] % 128)),
            2 => AudioEvent::Drum(DRUMS[usize::from(cmd[1]) % DRUMS.len()]),
            3 => {
                let patch = &patches[usize::from(cmd[2]) % patches.len()];
                AudioEvent::LoadPatchChannel(channel(cmd[1]), Box::new(patch.params.clone()))
            }
            4 => AudioEvent::Panic,
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
        proc.process_block(&[event], frames, 1);
        for &s in frames.iter() {
            assert!(s.is_finite(), "non-finite sample {s}");
            assert!((-1.0..=1.0).contains(&s), "sample {s} out of range");
        }
    }
});
