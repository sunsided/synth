//! Fuzz target: untrusted patches played through `SynthProcessor`.
//!
//! Patches reach the engine from places it does not control: user-edited
//! JSON passed to the npm package's `loadParams`, preset files, the network.
//! This target starts from the default patch, overwrites its numbers and
//! flags with fuzzer-chosen values (any finite `f32`, since JSON cannot
//! carry NaN or infinities), plays it, and then checks that
//!
//! * nothing panics or hangs (run with `-timeout`), in debug builds
//!   including every DSP stage's `debug_assert!` preconditions,
//! * every sample is finite and inside `[-1, 1]`, and
//! * after a panic, the default patch plays again. This caught a chorus
//!   rate that overflowed the LFO to NaN and left the delay and reverb
//!   feedback poisoned, silencing the engine for good.
#![no_main]

use libfuzzer_sys::fuzz_target;
use serde_json::Value;
use synthie::audio::processor::{NUM_CHANNELS, SynthProcessor};
use synthie::params::{AudioEvent, DrumHit, MidiNote, SynthParams};

/// Replace numeric leaves of `v`, in order, with finite floats from `data`.
fn mutate(v: &mut Value, data: &mut &[u8]) {
    match v {
        Value::Number(_) => {
            if let Some((head, rest)) = data.split_first_chunk::<4>() {
                *data = rest;
                let f = f32::from_le_bytes(*head);
                if f.is_finite()
                    && let Some(n) = serde_json::Number::from_f64(f64::from(f))
                {
                    *v = Value::Number(n);
                }
            }
        }
        Value::Bool(b) => {
            if let Some((head, rest)) = data.split_first() {
                *data = rest;
                *b = head & 1 == 1;
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| mutate(x, data)),
        Value::Object(o) => o.values_mut().for_each(|x| mutate(x, data)),
        _ => {}
    }
}

fn peak(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0, |m, s| m.max(s.abs()))
}

fuzz_target!(|data: &[u8]| {
    let mut json = serde_json::to_value(SynthParams::default()).unwrap();
    let mut rest = data;
    mutate(&mut json, &mut rest);
    let Ok(params) = serde_json::from_value::<SynthParams>(json) else {
        return;
    };

    let mut proc = SynthProcessor::<NUM_CHANNELS>::new(44_100.0);
    let mut buf = vec![0.0_f32; 4096];
    proc.process_block(
        &[
            AudioEvent::LoadPatch(Box::new(params)),
            AudioEvent::NoteOn(MidiNote(60)),
            AudioEvent::NoteOn(MidiNote(67)),
            AudioEvent::Drum(DrumHit::Kick),
        ],
        &mut buf,
        1,
    );
    proc.process_block(&[AudioEvent::NoteOff(MidiNote(60))], &mut buf, 1);
    assert!(buf.iter().all(|s| s.is_finite() && s.abs() <= 1.0));

    // Recovery: a sane patch after a panic must play again, on any channel.
    proc.process_block(
        &[
            AudioEvent::Panic,
            AudioEvent::LoadPatch(Box::default()),
            AudioEvent::NoteOn(MidiNote(69)),
        ],
        &mut buf,
        1,
    );
    assert!(
        peak(&buf) > 1e-3,
        "engine stayed silent after a bad patch (peak {})",
        peak(&buf)
    );
});
