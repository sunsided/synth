# synth

[![docs.rs](https://docs.rs/synth/badge.svg)](https://docs.rs/synth)
[![crates.io](https://img.shields.io/crates/v/synth.svg)](https://crates.io/crates/synth)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg)](https://github.com/rust-secure-code/safety-dance/)
[![Lint and Test](https://github.com/sunsided/synth/actions/workflows/test.yml/badge.svg)](https://github.com/sunsided/synth/actions/workflows/test.yml)
[![Fuzz Build](https://github.com/sunsided/synth/actions/workflows/fuzz-check.yml/badge.svg)](https://github.com/sunsided/synth/actions/workflows/fuzz-check.yml)

A Rust synthesizer engine library with polyphonic voices, ADSR envelopes, state-variable filters, LFO modulation, and reverb.

## Features

- **Polyphonic voice engine** with configurable channel count
- **Five waveforms:** Pulse, Sawtooth, Triangle, Noise, Pulse+Saw
- **ADSR amplitude envelope** with optional reverse (swell/duck) mode
- **State-variable filter** - Low-pass, Band-pass, and High-pass with cutoff, resonance, and drive
- **LFO** targeting pitch, pulse width, filter cutoff, or volume
- **Reverb FX** with room size and damping controls
- **Multi-timbral channels** via `ChannelNo` newtypes
- **Serde support** via the `serde` feature (enabled by default)

## Quick Start

```rust
use synth::audio::engine::setup_audio;
use synth::params::{AudioEvent, MidiNote};

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

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `serde` | enabled | `Serialize`/`Deserialize` on all parameter types |

## Terminal UI

The [`synth-tui`](../synth-tui/) crate provides a full ratatui-based terminal UI. See the [repository root](https://github.com/sunsided/synth) for screenshots.
