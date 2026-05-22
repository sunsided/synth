# synthie

[![Test](https://github.com/sunsided/synth/actions/workflows/test.yml/badge.svg)](https://github.com/sunsided/synth/actions/workflows/test.yml)
[![Format](https://github.com/sunsided/synth/actions/workflows/format.yml/badge.svg)](https://github.com/sunsided/synth/actions/workflows/format.yml)
[![Fuzz Build](https://github.com/sunsided/synth/actions/workflows/fuzz-check.yml/badge.svg)](https://github.com/sunsided/synth/actions/workflows/fuzz-check.yml)

A polyphonic DSP engine library for Rust. Ships with a terminal synthesizer as its primary demo.

## Workspace

| Crate | Type | Description |
|---|---|---|
| [`synthie`](crates/synthie/) | library | DSP engine: oscillators, envelopes, filter, LFO, reverb, preset system |
| [`synth-tui`](crates/synth-tui/) | binary | Terminal synthesizer — interactive demo of the engine |
| [`synthie-tracker`](crates/synthie-tracker/) | binary | **Step-sequencer tracker TUI** — 4-channel pattern editor with drum track |

## Library

### Features

- **Five waveforms:** Pulse, Sawtooth, Triangle, Noise, Pulse+Saw
- **ADSR amplitude envelope** with optional reverse (swell/duck) mode
- **State-variable filter** — Low-pass, Band-pass, and High-pass modes with cutoff, resonance, and pre-filter drive
- **LFO** with selectable targets: pitch (vibrato), pulse width, filter cutoff, or volume (tremolo)
- **Reverb FX** with room size and high-frequency damping controls
- **Global controls:** master volume and portamento (glide) time
- **Preset system** with built-in SID-style patches and save/load support
- **Optional `serde` support** for patch serialisation (enabled by default)

### Quick Start

Add as a path or git dependency, then:

```rust
use synthie::prelude::*; // SynthParams, Patch, AudioEvent, MidiNote, setup_audio, …

fn main() -> anyhow::Result<()> {
    let patches = presets::sid::default_patches();
    let (_stream, tx, _rx) = setup_audio()?;

    // Load first preset
    let params = Box::new(patches[0].params.clone());
    tx.send(AudioEvent::LoadPatch(params))?;

    // Play middle C on channel 0
    tx.send(AudioEvent::NoteOn(ChannelNo(0), MidiNote(60)))?;
    std::thread::sleep(std::time::Duration::from_secs(1));
    tx.send(AudioEvent::NoteOff(ChannelNo(0), MidiNote(60)))?;
    Ok(())
}
```

### Examples

Run any example with `cargo run -p synth --example <name> --release`.

| Example | Description |
|---|---|
| `tune` | Multi-channel chords, melody, and drums — general engine showcase |
| `lazy_jones` | Lazy Jones (David Whittaker, 1984) — the C64 hook that became Kernkraft 400 |
| `kernkraft` | Kernkraft 400 (Zombie Nation, 1999) — full song structure, two channels |

## Synthie Tracker

A 16-step sequencer with 4 independent synth channels and a drum track (kick, closed/open hi-hat), powered by the same `synthie` DSP engine.

```sh
task tracker          # or: cargo run -p synthie-tracker --release
```

### Controls

| Key | Action |
|---|---|
| `Space` | Play / Stop |
| `Esc` | Panic (silence all voices) |
| `↑` / `↓` / `←` / `→` | Move cursor |
| `Tab` / `Shift+Tab` | Next / previous preset on current track |
| `Z`–`M`, `Q`–`U` | Enter note (lower / upper octave row, same layout as synth-tui) |
| `Del` / `Bksp` | Clear step |
| `k` / `h` / `o` | Toggle kick / closed hi-hat / open hi-hat (drum track) |
| `+` / `-` | BPM up / down |
| `[` / `]` | Octave down / up |
| `PgUp` / `PgDn` | Extend / shorten pattern |
| `F1` | Toggle key-reference overlay |
| `Ctrl+C` / `Ctrl+Q` | Quit |

> **Terminal compatibility:** Note-off on note entry requires keyboard enhancement support (kitty protocol, WezTerm). In terminals without it, each note entry is still correctly gated per step.

## TUI Demo (synth-tui)

![Screenshot](docs/screenshot.png)

### Build and Run

```sh
cargo run -p synth-tui --release
```

Or build first, then run directly:

```sh
cargo build --release
./target/release/synth-tui
```

> **Note:** The synthesizer runs in the terminal's alternate screen with raw mode enabled. Your normal terminal session is fully restored on exit.

### Controls

#### Piano Keyboard

The keyboard is split into two chromatic octave rows. The lower row plays octave **N**; the upper row plays octave **N+1**.

| Lower row key | `Z` | `S` | `X` | `D` | `C` | `V` | `G` | `B` | `H` | `N` | `J` | `M` |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Note | C | C# | D | D# | E | F | F# | G | G# | A | A# | B |

| Upper row key | `Q` | `2` | `W` | `3` | `E` | `R` | `5` | `T` | `6` | `Y` | `7` | `U` |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Note | C | C# | D | D# | E | F | F# | G | G# | A | A# | B |

> **Terminal compatibility:** Note-off (key release) events require keyboard enhancement support (e.g. kitty protocol or WezTerm). In terminals that do not report key release, a note will sustain until another key is pressed.

#### Navigation

| Key | Action |
|---|---|
| `Tab` | Next parameter section |
| `Shift+Tab` | Previous parameter section |
| `←` / `→` | Select previous / next parameter within the section |
| `↑` / `↓` | Increase / decrease the selected parameter value |
| `Enter` | Load the selected preset (in the Presets section) |
| `1` – `8` | Quick-load preset slot 1–8 (in the Presets section) |

#### Utility

| Key | Action |
|---|---|
| `F1` | Toggle help overlay |
| `Esc` | Panic — silence all active notes immediately |
| `Ctrl+S` | Save current parameters as a new user patch |
| `Ctrl+C` / `Ctrl+Q` | Quit |
| `F12` | Quit |

#### Octave and Volume

| Key | Action |
|---|---|
| `[` or `,` | Octave down |
| `]` or `.` | Octave up |
| `+` or `=` | Volume up |
| `-` or `_` | Volume down |

## Development

### Pre-commit hooks

This repository uses [`prek`](https://github.com/j178/prek) (a Rust-native pre-commit manager) to enforce hygiene checks before each commit.

**One-time setup:**

```sh
cargo install prek
prek install
```

| Hook | Command | Trigger |
|---|---|---|
| `fmt-check` | `cargo fmt --all -- --check` | Any `.rs` change |
| `clippy` | `cargo clippy --all-targets -- -D warnings` | Any `.rs` or `Cargo.toml` change |
| `fuzz-build` | `task fuzz:build` | Any `.rs`, `Cargo.toml`, or `Cargo.lock` change |

The `fuzz-build` hook requires `cargo-fuzz` and the nightly toolchain (see [Fuzzing](#fuzzing) below).

Run all hooks manually without committing:

```sh
prek run --all-files
```

### Fuzzing

The DSP core (`SvFilter`, `Oscillator`) and `SynthParams` serialisation paths are covered by
[`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) harnesses under `fuzz/fuzz_targets/`.

**One-time setup:**

```sh
cargo install cargo-fuzz
rustup toolchain install nightly
```

**List available targets:**

```sh
task fuzz:targets
```

Expected output:

```
fuzz_filter_stability
fuzz_osc_safety
fuzz_params_serde
```

**Run a target:**

```sh
task fuzz -- fuzz_filter_stability
task fuzz -- fuzz_osc_safety
task fuzz -- fuzz_params_serde
```

The `fuzz` task sets `RUSTFLAGS=-Z sanitizer=address` and uses the nightly toolchain automatically.
Pass additional `cargo fuzz` flags after `--` if needed:

```sh
task fuzz -- fuzz_filter_stability -- -max_total_time=60
```

**Harness summary:**

| Target | What it tests | Invariant |
|---|---|---|
| `fuzz_filter_stability` | `SvFilter::process` across all modes/params | Output always finite |
| `fuzz_osc_safety` | `Oscillator::next_sample` for all waveforms | Output always finite |
| `fuzz_params_serde` | `SynthParams` JSON deserialise + round-trip | No panic; round-trip consistent |

## License

Licensed under the [European Union Public Licence v1.2 (EUPL-1.2)](https://joinup.ec.europa.eu/collection/eupl/eupl-text-eupl-12).
