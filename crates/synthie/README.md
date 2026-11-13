# synthie

[![docs.rs](https://docs.rs/synthie/badge.svg)](https://docs.rs/synthie)
[![crates.io](https://img.shields.io/crates/v/synthie.svg)](https://crates.io/crates/synthie)
[![npm](https://img.shields.io/npm/v/@sunsided/synthie.svg)](https://www.npmjs.com/package/@sunsided/synthie)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)
[![Lint and Test](https://github.com/sunsided/synthie/actions/workflows/test.yml/badge.svg)](https://github.com/sunsided/synthie/actions/workflows/test.yml)
[![Fuzz Build](https://github.com/sunsided/synthie/actions/workflows/fuzz-check.yml/badge.svg)](https://github.com/sunsided/synthie/actions/workflows/fuzz-check.yml)

A Rust synthesizer engine library with polyphonic voices, ADSR envelopes, state-variable filters, LFO modulation, and reverb.

## Features

- **Polyphonic voice engine** with configurable channel count
- **Five waveforms:** Pulse, Sawtooth, Triangle, Noise, Pulse+Saw
- **ADSR amplitude envelope** with optional reverse (swell/duck) mode
- **State-variable filter** - Low-pass, Band-pass, and High-pass with cutoff, resonance, and drive
- **LFO** targeting pitch, pulse width, filter cutoff, or volume
- **Reverb FX** with room size and damping controls
- **Sound-effect modulation:** pitch slide with acceleration and a cutoff frequency, one-shot pitch jump, pulse-width sweep, and repeat
- **Sound-effect generators** by category, plus randomize and mutate (`sfx` module)
- **Offline rendering** of a note to samples, and 16-bit WAV export (`render` module)
- **Multi-timbral channels** via `ChannelNo` newtypes
- **Drum machine** beside the voice pools: kick, snare, hi-hats, crash, toms and clap
- **Sample-accurate sequencing** on any host via `Sequencer` and `SequencedProcessor`
- **Serde support** via the `serde` feature (enabled by default)

## Quick Start

```rust
use synthie::audio::engine::setup_audio;
use synthie::params::{AudioEvent, MidiNote};

fn main() -> anyhow::Result<()> {
    let (stream, tx, _scope_rx) = setup_audio()?;
    let _stream = stream; // keep alive

    tx.send(AudioEvent::NoteOn(MidiNote(60)))?;
    std::thread::sleep(std::time::Duration::from_secs(1));
    tx.send(AudioEvent::NoteOff(MidiNote(60)))?;
    Ok(())
}
```

See [`examples/`](examples/) for full usage.

## Bring Your Own Audio Backend

`SynthProcessor` has no I/O and renders into any interleaved `f32` buffer, so
it can run inside kira, a game engine's stream, or an `AudioWorklet`. Pair it
with a `Sequencer` for events that land on the exact frame, whatever buffer
size the backend uses:

```rust
use synthie::prelude::*;

struct FourOnTheFloor;

impl Sequencer for FourOnTheFloor {
    fn tick(&mut self, sample_rate: f32, out: &mut Vec<AudioEvent>) -> usize {
        out.push(AudioEvent::Drum(DrumHit::Kick));
        (sample_rate * 0.5) as usize // 120 bpm
    }
}

let mut synth = SequencedProcessor::new(SynthProcessor::<4>::new(48_000.0), FourOnTheFloor);
let mut buf = vec![0.0; 2 * 512]; // stereo, interleaved
synth.process_block(&[], &mut buf, 2); // call from the audio callback
```

Already using [kira](https://crates.io/crates/kira)? Enable the `kira` feature
and play the synth as a kira sound instead, mixed, faded and paused like any
other:

```rust,ignore
use synthie::kira::SynthSoundData;
use synthie::pattern::SongPlayer;

let mut synth = manager.play(SynthSoundData::new().with_sequencer(SongPlayer::new(song)))?;
synth.drum(DrumHit::Kick)?;
synth.set_volume(-6.0, Tween::default());
```

## Game Music

`synthie::pattern` plays tracker-style songs written in a compact notation,
sample-accurately through the same `Sequencer` trait:

```rust
use synthie::params::ChannelNo;
use synthie::pattern::{Song, SongPlayer, Tempo};

// One row per eighth note at 152 bpm.
let song = Song::new(Tempo::bpm(152.0, 2))
    .lane(ChannelNo(0), "E4 . G4 . A4 . B4 . | E5 . . . - . . .")?
    .lane(ChannelNo(1), "E2 E2 E2 E2 G2 A2 . . | E2 . . . - . . .")?
    .lane(ChannelNo(3), "k+c h s h k k s h | k h s h k k s o")?;
let player = SongPlayer::new(song); // hand to any host as a Sequencer
```

Chord cells play several notes from one lane on the lane's channel, e.g. a
power chord as `E2+B2+E3`, or as semitone offsets from the root, `E2^07c!`.
See the `synthie::pattern` docs for the full notation (notes, chords, holds,
note-offs, drum hits, chip arpeggios, patch changes).

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `serde` | enabled | `Serialize`/`Deserialize` on all parameter types |
| `io` | enabled | cpal output stream (`audio::engine`) |
| `arp` | enabled | Built-in arpeggiator, and arpeggio cells in patterns |
| `kira` | disabled | Play the synth as a [kira](https://crates.io/crates/kira) sound (`synthie::kira`); independent of `io` |

## Terminal UI

The [`synth-tui`](../synth-tui/) crate provides a full ratatui-based terminal UI. See the [repository root](https://github.com/sunsided/synthie) for screenshots.
