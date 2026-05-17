# Oscillator Richness - Design Spec

Issue: #12 - sine wave, dual OSC, hard sync

## Overview

Three additions to the oscillator layer:

1. `Waveform::Sine` - rounds out the waveform set, unblocks FM carrier use later
2. Second detunable oscillator per voice - enables unison/supersaw effects
3. Hard sync - OSC1 resets OSC2 phase on period boundary (classic SID/retro effect)

## Params (`params.rs`)

### Waveform enum

Add `Sine` variant. Update `ALL`, `name()`, `next()`, `prev()`.

```rust
pub enum Waveform {
    Pulse, Sawtooth, Triangle, Noise, PulseSaw,
    Sine,  // new
}
```

`Sine` goes last in `ALL` to avoid shifting existing preset indices that cycle by position.

### Osc2Params (new struct)

```rust
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Osc2Params {
    pub waveform: Waveform,  // default: Sawtooth
    pub detune: f32,         // cents, -100..100; default: 7.0
    pub osc2_mix: f32,       // 0..1; default: 0.0 (off - no CPU cost)
    pub hard_sync: bool,     // default: false
}
```

Default of `osc2_mix = 0.0` means OSC2 processing is skipped entirely when not in use.
Pulse width is shared from `OscParams::pulse_width` (no separate PW for OSC2).

### SynthParams

```rust
pub struct SynthParams {
    // ...existing fields...
    #[cfg_attr(feature = "serde", serde(default))]
    pub osc2: Osc2Params,
}
```

`serde(default)` preserves backwards compat with existing serialized patches.

## DSP (`osc.rs`)

### Oscillator struct

Add `just_wrapped: bool` field. Set `true` when phase wraps; cleared at start of each `next_sample()` call. Readable via `pub fn just_wrapped(&self) -> bool`.

```rust
pub struct Oscillator {
    phase: f32,
    noise_lfsr: u32,
    last_noise: f32,
    just_wrapped: bool,  // new
}
```

### Sine waveform

In `next_sample()` match:

```rust
Waveform::Sine => (TAU * p).sin(),
```

Uses `f32::sin()`. Per issue notes, this is `no_std`-incompatible; can be swapped to `libm::sinf` in the architecture issue (#15).

## Voice (`voice.rs`)

### Voice struct

```rust
pub struct Voice {
    // ...existing fields...
    pub osc2: Oscillator,  // new
}
```

### process()

After osc1 tick:

```rust
let osc1_out = self.osc.next_sample(final_freq, sample_rate, params.osc.waveform, pw, params.osc.noise_mix);

let osc_out = if params.osc2.osc2_mix > 0.001 {
    if params.osc2.hard_sync && self.osc.just_wrapped() {
        self.osc2.reset();
    }
    let osc2_freq = detune_hz(final_freq, params.osc2.detune);
    let osc2_out = self.osc2.next_sample(osc2_freq, sample_rate, params.osc2.waveform, pw, 0.0);
    osc1_out * (1.0 - params.osc2.osc2_mix) + osc2_out * params.osc2.osc2_mix
} else {
    osc1_out
};
```

`osc2.reset()` uses the existing (currently `#[allow(dead_code)]`) `Oscillator::reset()` method - that attribute can now be removed.

Fresh note-on: both oscillators reset via `Voice::new()` on voice allocation; legato keeps both running (natural for portamento).

## TUI (`synth-tui`)

### Layout

OSC section height: `Length(6)` -> `Length(10)`.

New rows appended to the OSC panel draw function:

```
── OSC2 ─────────────
Waveform   [ Saw    ]
Detune      +7ct
Mix          0%
Hard Sync  [ off ]
```

### Section enum / input

Extend existing `Section::Osc` navigation (no new enum variant). The OSC panel's param list gains four entries; Tab/arrow keys reach them via the same row-index mechanism used for the existing four OSC params. Input handler maps up/down to waveform cycle, left/right to detune/mix nudge, enter to toggle hard_sync.

## Tests

- `osc_sine_bounds`: sine stays in -1..1 for 4410 samples at 440 Hz
- `osc_hard_sync_resets`: after osc1 wraps, osc2 phase is 0 on next sample
- Existing sawtooth/pulse tests unchanged

## Non-goals

- Per-voice CPU feature flag (Cargo feature): rejected in favour of `osc2_mix = 0` runtime bypass
- Separate pulse width for OSC2: deferred; shared PW is sufficient for unison use cases
- OSC2 noise mix: deferred
