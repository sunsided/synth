//! Fuzz target: the pattern notation parser and song playback.
//!
//! Patterns are text people write (and the tracker loads), so the parser
//! must take any string. The first byte picks the lane's channel (any value,
//! including channels the processor does not have); the rest is the pattern.
//!
//! - A rejected pattern's error must point at its token: the offset is a
//!   character boundary and the text there starts with the token, and the
//!   error formats without panicking.
//! - An accepted pattern is played as a one-shot song with a single patch
//!   (so `@n` can name a patch that does not exist) through a
//!   `SequencedProcessor`; every sample must be finite and inside `[-1, 1]`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use synthie::audio::processor::{NUM_CHANNELS, SynthProcessor};
use synthie::audio::sequencer::SequencedProcessor;
use synthie::params::{ChannelNo, SynthParams};
use synthie::pattern::{Lane, Song, SongPlayer, Tempo};

fuzz_target!(|data: &[u8]| {
    let Some((&channel, rest)) = data.split_first() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    let channel = ChannelNo(channel);

    if let Err(e) = Lane::parse(channel, text) {
        assert!(
            text.get(e.offset..).is_some_and(|t| t.starts_with(&e.token)),
            "error at {} does not point at `{}`",
            e.offset,
            e.token
        );
        let _ = e.to_string();
        return;
    }

    let song = Song::new(Tempo::bpm(600.0, 4))
        .one_shot()
        .patch(SynthParams::default())
        .lane(channel, text)
        .expect("Lane::parse accepted it");
    let mut seq = SequencedProcessor::new(
        SynthProcessor::<NUM_CHANNELS>::new(44_100.0),
        SongPlayer::new(song),
    );
    let mut buf = [0.0_f32; 4_096];
    // 40 rows a second: 64 blocks cover the first ~240 rows.
    for _ in 0..64 {
        seq.process_block(&[], &mut buf, 1);
        for &s in &buf {
            assert!(s.is_finite() && (-1.0..=1.0).contains(&s), "sample {s}");
        }
        if seq.sequencer().is_finished() {
            break;
        }
    }
});
