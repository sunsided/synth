# Ring Modulation Between Oscillators Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a configurable `RingModMode` enum to `Osc2Params` and apply it in `Voice::process()` to produce SID-style and analog ring modulation effects.

**Architecture:** Three-change sequence: (1) add `RingModMode` enum + field to params, (2) expose `phase_sign()` on `Oscillator`, (3) apply the new field in `Voice::process()` with a `match` on the mode. All new behavior is gated by the existing `osc2_mix > 0.001` guard. Default mode is `Off` — backward-compatible, old JSON deserializes cleanly.

**Tech Stack:** Rust (no_std compatible), `serde` feature flag for JSON serialization, `cargo test` / `task test`

---

### Task 1: `RingModMode` enum and `Osc2Params::ring_mod` field

**Files:**
- Modify: `crates/synthie/src/params.rs`

- [ ] **Step 1: Write the failing test**

In `params.rs`, in the `#[cfg(all(test, feature = "serde"))]` test module, add one assertion to the existing `synth_params_osc2_defaults_when_missing_from_json` test:

```rust
assert_eq!(params.osc2.ring_mod, RingModMode::Off);  // add after the hard_sync assertion
```

- [ ] **Step 2: Run test to verify it fails**

```
cargo test -p synthie --features serde -- params::tests::synth_params_osc2_defaults_when_missing_from_json
```

Expected: compile error — `no field 'ring_mod'` and `cannot find type 'RingModMode'`

- [ ] **Step 3: Add `RingModMode` enum**

In `params.rs`, after the `LfoShape` impl block and before the `LfoParams` struct, insert:

```rust
/// Ring modulation mode between OSC1 and OSC2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum RingModMode {
    /// No ring modulation (bypass). Default.
    #[default]
    Off,
    /// SID-style: OSC2 output multiplied by sign of OSC1 phase accumulator MSB.
    Osc2ByOsc1Sign,
    /// Symmetric: OSC1 output multiplied by sign of OSC2 phase accumulator MSB.
    Osc1ByOsc2Sign,
    /// True analog ring mod: OSC1 × OSC2, produces sum/difference frequencies.
    Analog,
}

impl RingModMode {
    /// Ordered slice of all variants, used for cycling.
    pub const ALL: &'static [RingModMode] = &[
        RingModMode::Off,
        RingModMode::Osc2ByOsc1Sign,
        RingModMode::Osc1ByOsc2Sign,
        RingModMode::Analog,
    ];

    /// Short display name shown in the UI.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            RingModMode::Off => "Off",
            RingModMode::Osc2ByOsc1Sign => "SID",
            RingModMode::Osc1ByOsc2Sign => "Sym",
            RingModMode::Analog => "Analog",
        }
    }

    /// Return the next variant, wrapping around.
    #[must_use]
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&m| m == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// Return the previous variant, wrapping around.
    #[must_use]
    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|&m| m == self).unwrap_or(0);
        let len = Self::ALL.len();
        Self::ALL[(idx + len - 1) % len]
    }
}
```

- [ ] **Step 4: Add `ring_mod` field to `Osc2Params` and update its `Default`**

Replace the `Osc2Params` struct definition:

```rust
pub struct Osc2Params {
    /// Waveform shape for the second oscillator.
    pub waveform: Waveform,
    /// Detune relative to OSC1 in cents, -100 .. 100.
    pub detune: f32,
    /// Blend of OSC2 into the output, 0..1.  At 0.0 OSC2 is bypassed.
    pub osc2_mix: f32,
    /// When true, OSC1 period boundary resets OSC2 phase (hard sync).
    pub hard_sync: bool,
    /// Ring modulation mode applied to the OSC2 contribution.
    #[cfg_attr(feature = "serde", serde(default))]
    pub ring_mod: RingModMode,
}
```

Replace `Osc2Params::default()`:

```rust
impl Default for Osc2Params {
    fn default() -> Self {
        Self {
            waveform: Waveform::Sawtooth,
            detune: 7.0,
            osc2_mix: 0.0,
            hard_sync: false,
            ring_mod: RingModMode::Off,
        }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

```
cargo test -p synthie --features serde -- params::tests
```

Expected: all params tests pass.

- [ ] **Step 6: Run clippy**

```
cargo clippy -p synthie -- -D warnings
```

Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/synthie/src/params.rs
git commit -m "feat(synthie): add RingModMode enum and Osc2Params::ring_mod field"
```

---

### Task 2: `Oscillator::phase_sign()`

**Files:**
- Modify: `crates/synthie/src/audio/osc.rs`

- [ ] **Step 1: Write the failing test**

In `osc.rs`, in the `#[cfg(test)]` tests module, add:

```rust
#[test]
fn phase_sign_matches_phase() {
    let mut osc = Oscillator::default();
    // freq=1 Hz, sample_rate=100 Hz → phase increments 0.01 per sample.
    // After 25 samples: phase ≈ 0.25 (< 0.5) → sign must be +1.0.
    for _ in 0..25 {
        osc.next_sample(1.0, 100.0, Waveform::Sawtooth, 0.5, 0.0);
    }
    assert_eq!(osc.phase_sign(), 1.0, "phase ~0.25 should give +1.0");

    // After 50 more samples: phase ≈ 0.75 (>= 0.5) → sign must be -1.0.
    for _ in 0..50 {
        osc.next_sample(1.0, 100.0, Waveform::Sawtooth, 0.5, 0.0);
    }
    assert_eq!(osc.phase_sign(), -1.0, "phase ~0.75 should give -1.0");
}
```

- [ ] **Step 2: Run test to verify it fails**

```
cargo test -p synthie -- osc::tests::phase_sign_matches_phase
```

Expected: compile error — `no method named 'phase_sign' found for struct 'Oscillator'`

- [ ] **Step 3: Implement `phase_sign()`**

In `osc.rs`, in the `Oscillator` impl block, after the `just_wrapped()` method:

```rust
/// Returns the sign of the phase accumulator MSB: +1.0 if phase < 0.5, else -1.0.
/// Used as the ring modulation carrier signal (matches SID accumulator MSB behaviour).
#[must_use]
pub fn phase_sign(&self) -> f32 {
    if self.phase < 0.5 { 1.0 } else { -1.0 }
}
```

- [ ] **Step 4: Run test to verify it passes**

```
cargo test -p synthie -- osc::tests::phase_sign_matches_phase
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/synthie/src/audio/osc.rs
git commit -m "feat(synthie): add Oscillator::phase_sign() for ring modulation"
```

---

### Task 3: Ring modulation in `Voice::process()`

**Files:**
- Modify: `crates/synthie/src/audio/voice.rs`

- [ ] **Step 1: Write failing tests**

In `voice.rs`, update the import in the `#[cfg(test)]` module (near line 274):

```rust
use crate::params::{MidiNote, RingModMode, SynthParams, Waveform};
```

Then add three tests:

```rust
#[test]
fn ring_mod_modes_alter_output() {
    // Each active RingModMode must produce output different from Off.
    // Fails before implementation because the match block is absent
    // and all modes fall through to the same unmodulated path.
    use crate::params::EnvParams;

    let base_params = {
        let mut p = SynthParams::default();
        p.osc.waveform = Waveform::Triangle;
        p.osc2.osc2_mix = 1.0;
        p.osc2.waveform = Waveform::Triangle;
        p.osc2.detune = 700.0; // ~5x faster than OSC1, ensures phase difference
        p.env = EnvParams {
            attack: 0.0,
            decay: 0.0,
            sustain: 1.0,
            release: 0.0,
            env_reverse: false,
        };
        p
    };

    // Baseline: Off mode.
    let mut params_off = base_params.clone();
    params_off.osc2.ring_mod = RingModMode::Off;
    let mut voice_off = Voice::new();
    voice_off.note_on(MidiNote::A4, &params_off, 44100.0);
    let sum_off: f32 = (0..500).map(|_| voice_off.process(&params_off, 44100.0)).sum();

    for mode in [
        RingModMode::Osc2ByOsc1Sign,
        RingModMode::Osc1ByOsc2Sign,
        RingModMode::Analog,
    ] {
        let mut params_rm = base_params.clone();
        params_rm.osc2.ring_mod = mode;
        let mut voice_rm = Voice::new();
        voice_rm.note_on(MidiNote::A4, &params_rm, 44100.0);
        let sum_rm: f32 = (0..500)
            .map(|_| voice_rm.process(&params_rm, 44100.0))
            .sum();
        assert_ne!(
            sum_off, sum_rm,
            "RingModMode::{mode:?} produced same output as Off — ring mod not applied"
        );
    }
}

#[test]
fn ring_mod_analog_output_is_finite() {
    // Analog mode multiplies osc1 × osc2. The raw product is bounded to -1..1,
    // but downstream SVF processing can exceed ±1 near Nyquist — check
    // finiteness only, not a hard amplitude bound.
    use crate::params::EnvParams;

    let mut params = SynthParams::default();
    params.osc2.osc2_mix = 1.0;
    params.osc2.waveform = Waveform::Sawtooth;
    params.osc2.detune = 700.0;
    params.osc2.ring_mod = RingModMode::Analog;
    params.env = EnvParams {
        attack: 0.0,
        decay: 0.0,
        sustain: 1.0,
        release: 0.0,
        env_reverse: false,
    };

    let mut voice = Voice::new();
    voice.note_on(MidiNote::A4, &params, 44100.0);
    for i in 0..1000 {
        let s = voice.process(&params, 44100.0);
        assert!(s.is_finite(), "sample {i}: non-finite output: {s}");
    }
}

#[test]
fn ring_mod_off_is_deterministic() {
    // RingModMode::Off must produce bit-identical output across two independent
    // voices with identical params (regression guard: Off must not activate
    // any modulation path).
    let mut params_a = SynthParams::default();
    params_a.osc2.osc2_mix = 0.5;
    params_a.osc2.waveform = Waveform::Sawtooth;
    params_a.osc2.detune = 7.0;
    params_a.osc2.ring_mod = RingModMode::Off;

    let params_b = params_a.clone(); // ring_mod is Off in both

    let mut voice_a = Voice::new();
    voice_a.note_on(MidiNote::A4, &params_a, 44100.0);
    let mut voice_b = Voice::new();
    voice_b.note_on(MidiNote::A4, &params_b, 44100.0);

    for i in 0..200 {
        let a = voice_a.process(&params_a, 44100.0);
        let b = voice_b.process(&params_b, 44100.0);
        assert_eq!(a, b, "sample {i}: Off mode diverged from default");
    }
}
```

- [ ] **Step 2: Run tests to verify the key test fails**

```
cargo test -p synthie -- audio::voice::tests::ring_mod
```

Expected: `ring_mod_modes_alter_output` FAILS with `RingModMode::Osc2ByOsc1Sign produced same output as Off`. The other two tests may pass — that is acceptable; `ring_mod_modes_alter_output` is the primary TDD gate.

- [ ] **Step 3: Add `RingModMode` to the top-level imports in `voice.rs`**

Change line 12:
```rust
use crate::params::{MidiNote, SynthParams};
```
to:
```rust
use crate::params::{MidiNote, RingModMode, SynthParams};
```

- [ ] **Step 4: Replace the OSC2 block in `Voice::process()`**

Locate the comment `// Oscillator 2 (unchanged)` (around line 221) and replace the entire `let osc_out = if params.osc2.osc2_mix > 0.001 { ... }` block with:

```rust
// Oscillator 2 with optional ring modulation
let osc_out = if params.osc2.osc2_mix > 0.001 {
    if params.osc2.hard_sync && self.osc.just_wrapped() {
        self.osc2.reset();
    }
    let osc2_freq = detune_hz(final_freq, params.osc2.detune);
    let secondary =
        self.osc2
            .next_sample(osc2_freq, sample_rate, params.osc2.waveform, pw, 0.0);

    let modulated = match params.osc2.ring_mod {
        RingModMode::Off => secondary,
        RingModMode::Osc2ByOsc1Sign => secondary * self.osc.phase_sign(),
        RingModMode::Osc1ByOsc2Sign => osc_out * self.osc2.phase_sign(),
        RingModMode::Analog => osc_out * secondary,
    };

    osc_out * (1.0 - params.osc2.osc2_mix) + modulated * params.osc2.osc2_mix
} else {
    osc_out
};
```

- [ ] **Step 5: Run all synthie tests**

```
cargo test -p synthie
```

Expected: all tests pass, including existing hard sync and osc2 tests.

- [ ] **Step 6: Run clippy**

```
cargo clippy -p synthie -- -D warnings
```

Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/synthie/src/audio/voice.rs
git commit -m "feat(synthie): implement ring modulation between OSC1 and OSC2 (issue #23)"
```

---

### Task 4: Final verification

- [ ] **Step 1: Run full check suite**

```
task check
```

Expected: format, clippy, and compile all pass across all crates.

- [ ] **Step 2: Run full test suite**

```
task test
```

Expected: all tests pass.

- [ ] **Step 3: Build fuzz targets (if nightly toolchain is available)**

```
task fuzz:build
```

Expected: `fuzz_osc_safety` (exercises oscillator) and `fuzz_params_serde` (exercises params serde including new `ring_mod` field) compile without error.
