# synth

A terminal-based polyphonic synthesizer written in Rust. Play notes with your keyboard, shape the sound with a real-time parameter editor.

![Screenshot](docs/screenshot.png)

## Features

- **Two-octave keyboard layout** mapped to the `Z`/`Q` rows for chromatic note input
- **Five waveforms:** Pulse, Sawtooth, Triangle, Noise, Pulse+Saw
- **ADSR amplitude envelope** with optional reverse (swell/duck) mode
- **State-variable filter** — Low-pass, Band-pass, and High-pass modes with cutoff, resonance, and pre-filter drive
- **LFO** with selectable targets: pitch (vibrato), pulse width, filter cutoff, or volume (tremolo)
- **Reverb FX** with room size and high-frequency damping controls
- **Global controls:** master volume and portamento (glide) time
- **Preset system** with quick-load and save-to-patch support
- **60 FPS TUI** built with [ratatui](https://github.com/ratatui-org/ratatui) + [crossterm](https://github.com/crossterm-rs/crossterm)

## Getting Started

### Prerequisites

- Rust toolchain (`rustup` recommended — stable is sufficient)
- A working audio output device recognized by your OS

### Build and Run

```sh
cargo run --release
```

Or build first, then run the binary directly:

```sh
cargo build --release
./target/release/synth
```

> **Note:** The synthesizer runs in the terminal's alternate screen with raw mode enabled. Your normal terminal session is fully restored on exit.

## Controls

### Piano Keyboard

The keyboard is split into two chromatic octave rows. The lower row plays octave **N**; the upper row plays octave **N+1**.

| Lower row key | `Z` | `S` | `X` | `D` | `C` | `V` | `G` | `B` | `H` | `N` | `J` | `M` |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Note | C | C# | D | D# | E | F | F# | G | G# | A | A# | B |

| Upper row key | `Q` | `2` | `W` | `3` | `E` | `R` | `5` | `T` | `6` | `Y` | `7` | `U` |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Note | C | C# | D | D# | E | F | F# | G | G# | A | A# | B |

> **Terminal compatibility:** Note-off (key release) events require keyboard enhancement support (e.g. kitty protocol or WezTerm). In terminals that do not report key release, a note will sustain until another key is pressed.

### Navigation

| Key | Action |
|---|---|
| `Tab` | Next parameter section |
| `Shift+Tab` | Previous parameter section |
| `←` / `→` | Select previous / next parameter within the section |
| `↑` / `↓` | Increase / decrease the selected parameter value |
| `Enter` | Load the selected preset (in the Presets section) |
| `1` – `8` | Quick-load preset slot 1–8 (in the Presets section) |

### Utility

| Key | Action |
|---|---|
| `F1` | Toggle help overlay |
| `Esc` | Panic — silence all active notes immediately |
| `Ctrl+S` | Save current parameters as a new user patch |
| `Ctrl+C` / `Ctrl+Q` | Quit |
| `F12` | Quit |

### Octave and Volume

| Key | Action |
|---|---|
| `[` or `,` | Octave down |
| `]` or `.` | Octave up |
| `+` or `=` | Volume up |
| `-` or `_` | Volume down |

## License

Licensed under the [European Union Public Licence v1.2 (EUPL-1.2)](https://joinup.ec.europa.eu/collection/eupl/eupl-text-eupl-12).
