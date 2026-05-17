# Oscillator Richness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `Waveform::Sine`, a second detunable oscillator per voice, and hard sync (OSC1 resets OSC2 phase) to close issue #12.

**Architecture:** `Osc2Params` (new struct in `params.rs`) carries OSC2 waveform, detune, mix, and hard_sync flag; `Voice` gains a second `Oscillator` field mixed post-OSC1; `Oscillator` grows a `just_wrapped` bool to signal phase boundary to callers. TUI OSC panel grows from 6 to 10 lines and exposes the four new params.

**Tech Stack:** Rust stable, `synthie` library crate, `synth-tui` binary crate, `ratatui` TUI, inline `#[cfg(test)]` modules.

---

## File Map

| File | Change |
|------|--------|
| `crates/synthie/src/params.rs` | Add `Waveform::Sine`; add `Osc2Params` struct + `Default`; add `osc2: Osc2Params` field to `SynthParams` |
| `crates/synthie/src/audio/osc.rs` | Add `just_wrapped: bool` to `Oscillator`; add `pub fn just_wrapped()`; add `Waveform::Sine` arm; remove `#[allow(dead_code)]` on `reset()` |
| `crates/synthie/src/audio/voice.rs` | Add `pub osc2: Oscillator`; update `process()` to mix OSC2 and apply hard sync |
| `crates/synth-tui/src/app/state.rs` | `Section::Osc` param_count 4→9; extend `section_params()` with separator + 4 OSC2 rows; add cases 4–8 to `adjust_osc()` |
| `crates/synth-tui/src/app/ui.rs` | OSC constraint `Length(6)` → `Length(10)` |

---

## Task 1: Add `Waveform::Sine` to `params.rs`

**Files:**
- Modify: `crates/synthie/src/params.rs`

- [ ] **Step 1: Add `Sine` variant to the enum and update `ALL` / `name()`**

  In `params.rs`, find the `Waveform` enum and its `impl` block. Make these changes:

  ```rust
  pub enum Waveform {
      Pulse,
      Sawtooth,
      Triangle,
      Noise,
      PulseSaw,
      Sine,   // ← new
  }
  ```

  ```rust
  pub const ALL: &'static [Waveform] = &[
      Waveform::Pulse,
      Waveform::Sawtooth,
      Waveform::Triangle,
      Waveform::Noise,
      Waveform::PulseSaw,
      Waveform::Sine,   // ← new
  ];
  ```

  ```rust
  pub fn name(self) -> &'static str {
      match self {
          Waveform::Pulse    => "Pulse",
          Waveform::Sawtooth => "Saw",
          Waveform::Triangle => "Tri",
          Waveform::Noise    => "Noise",
          Waveform::PulseSaw => "Pls+Saw",
          Waveform::Sine     => "Sine",   // ← new
      }
  }
  ```

  `next()` and `prev()` use `ALL` by position — no changes needed there.

- [ ] **Step 2: Run tests to confirm compile + existing tests pass**

  ```bash
  cargo test -p synthie
  ```

  Expected: all existing tests pass, no new failures.

- [ ] **Step 3: Commit**

  ```bash
  git add crates/synthie/src/params.rs
  git commit -m "feat(params): add Waveform::Sine variant"
  ```

---

## Task 2: Add `Osc2Params` to `params.rs`

**Files:**
- Modify: `crates/synthie/src/params.rs`

- [ ] **Step 1: Add the struct and its `Default` impl**

  Directly after the closing brace of `OscParams` (around line 157), insert:

  ```rust
  /// Second oscillator section parameters.
  #[derive(Debug, Clone)]
  #[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
  pub struct Osc2Params {
      /// Waveform shape for the second oscillator.
      pub waveform: Waveform,
      /// Detune relative to OSC1 in cents, -100 .. 100.
      pub detune: f32,
      /// Blend of OSC2 into the output, 0..1.  At 0.0 OSC2 is bypassed.
      pub osc2_mix: f32,
      /// When true, OSC1 period boundary resets OSC2 phase (hard sync).
      pub hard_sync: bool,
  }

  impl Default for Osc2Params {
      fn default() -> Self {
          Self {
              waveform: Waveform::Sawtooth,
              detune: 7.0,
              osc2_mix: 0.0,
              hard_sync: false,
          }
      }
  }
  ```

- [ ] **Step 2: Add `osc2` field to `SynthParams`**

  In the `SynthParams` struct, add after the `delay` field:

  ```rust
  /// Second oscillator parameters.
  #[cfg_attr(feature = "serde", serde(default))]
  pub osc2: Osc2Params,
  ```

  In `SynthParams::default()`, add after `delay: DelayParams::default()`:

  ```rust
  osc2: Osc2Params::default(),
  ```

- [ ] **Step 3: Write and run a compile test**

  ```bash
  cargo test -p synthie
  ```

  Expected: compiles and all tests pass. (`SynthParams::default()` will now include `osc2_mix = 0.0`.)

- [ ] **Step 4: Commit**

  ```bash
  git add crates/synthie/src/params.rs
  git commit -m "feat(params): add Osc2Params and wire into SynthParams"
  ```

---

## Task 3: Add `just_wrapped` + `Sine` arm to `Oscillator`

**Files:**
- Modify: `crates/synthie/src/audio/osc.rs`

- [ ] **Step 1: Write the failing tests first**

  At the bottom of `osc.rs`, inside the existing `#[cfg(test)] mod tests { ... }` block, add:

  ```rust
  #[test]
  fn osc_sine_bounds() {
      let mut osc = Oscillator::default();
      for _ in 0..4410 {
          let s = osc.next_sample(440.0, 44100.0, Waveform::Sine, 0.5, 0.0);
          assert!((-1.0..=1.0).contains(&s), "sine out of bounds: {s}");
      }
  }

  #[test]
  fn osc_just_wrapped_fires() {
      let mut osc = Oscillator::default();
      let mut wrapped_count = 0;
      // 440 Hz at 44100 Hz: wraps every ~100.2 samples → ~44 wraps in 4410 samples
      for _ in 0..4410 {
          osc.next_sample(440.0, 44100.0, Waveform::Sawtooth, 0.5, 0.0);
          if osc.just_wrapped() {
              wrapped_count += 1;
          }
      }
      assert!(
          (40..=50).contains(&wrapped_count),
          "unexpected wrap count: {wrapped_count}"
      );
  }
  ```

- [ ] **Step 2: Run to confirm compile error (method not yet defined)**

  ```bash
  cargo test -p synthie 2>&1 | head -20
  ```

  Expected: compile error — `no method named 'just_wrapped' found`.

- [ ] **Step 3: Add `just_wrapped` field to `Oscillator` struct and `Default`**

  ```rust
  pub struct Oscillator {
      phase: f32,
      noise_lfsr: u32,
      last_noise: f32,
      just_wrapped: bool,   // ← new
  }
  ```

  Update the `Default` impl:

  ```rust
  impl Default for Oscillator {
      fn default() -> Self {
          Self {
              phase: 0.0,
              noise_lfsr: 0xACE1_FEED,
              last_noise: 0.0,
              just_wrapped: false,   // ← new
          }
      }
  }
  ```

- [ ] **Step 4: Update `next_sample()` to set/clear `just_wrapped` and add `Sine` arm**

  At the very start of `next_sample()`, before `let inc = ...`:

  ```rust
  self.just_wrapped = false;
  ```

  Inside the phase-wrap block (where `self.phase -= 1.0`):

  ```rust
  if self.phase >= 1.0 {
      self.phase -= 1.0;
      self.just_wrapped = true;    // ← new
      self.last_noise = self.tick_lfsr();
  }
  ```

  Add the `Sine` arm to the `match waveform` block (after `PulseSaw`):

  ```rust
  Waveform::Sine => (TAU * p).sin(),
  ```

- [ ] **Step 5: Add `pub fn just_wrapped()` accessor and remove the dead_code allow on `reset()`**

  After `next_sample()`, add:

  ```rust
  /// Returns `true` if the phase wrapped during the most recent `next_sample()` call.
  pub fn just_wrapped(&self) -> bool {
      self.just_wrapped
  }
  ```

  Remove the `#[allow(dead_code)]` attribute above `pub fn reset()`.

- [ ] **Step 6: Run tests — all must pass**

  ```bash
  cargo test -p synthie
  ```

  Expected: `osc_sine_bounds` passes, `osc_just_wrapped_fires` passes, all existing tests still pass.

- [ ] **Step 7: Commit**

  ```bash
  git add crates/synthie/src/audio/osc.rs
  git commit -m "feat(osc): add Waveform::Sine, just_wrapped signal for hard sync"
  ```

---

## Task 4: Add `osc2` to `Voice` — mixing and hard sync

**Files:**
- Modify: `crates/synthie/src/audio/voice.rs`

- [ ] **Step 1: Write failing smoke tests**

  At the bottom of `voice.rs`, add a new test module:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use crate::params::{MidiNote, SynthParams, Waveform};

      #[test]
      fn voice_osc2_mix_zero_is_finite() {
          let mut voice = Voice::new();
          let params = SynthParams::default(); // osc2_mix = 0.0
          voice.note_on(MidiNote::A4, &params, 44100.0);
          for _ in 0..1000 {
              let s = voice.process(&params, 44100.0);
              assert!(s.is_finite(), "non-finite sample with osc2 off: {s}");
          }
      }

      #[test]
      fn voice_osc2_hard_sync_is_finite() {
          let mut voice = Voice::new();
          let mut params = SynthParams::default();
          params.osc2.osc2_mix = 0.5;
          params.osc2.hard_sync = true;
          params.osc2.waveform = Waveform::Sawtooth;
          params.osc2.detune = 7.0;
          voice.note_on(MidiNote::A4, &params, 44100.0);
          for _ in 0..1000 {
              let s = voice.process(&params, 44100.0);
              assert!(s.is_finite(), "non-finite sample with hard sync on: {s}");
          }
      }
  }
  ```

- [ ] **Step 2: Run tests to confirm compile error (field not yet added)**

  ```bash
  cargo test -p synthie 2>&1 | head -20
  ```

  Expected: compile error — `no field 'osc2' on type 'Voice'` (Tasks 1–3 are prerequisites; once those are done, `SynthParams::osc2` exists, but `Voice::osc2` does not yet). Either way, the test must fail before we implement.

- [ ] **Step 3: Add `osc2` field to `Voice` struct and `Voice::new()`**

  In the `Voice` struct, after `pub osc: Oscillator,`:

  ```rust
  /// Second oscillator for unison/detune/hard-sync effects.
  pub osc2: Oscillator,
  ```

  In `Voice::new()`, after `osc: Oscillator::default(),`:

  ```rust
  osc2: Oscillator::default(),
  ```

- [ ] **Step 4: Update `process()` to mix OSC2**

  Find the existing OSC block in `process()`:

  ```rust
  // Oscillator
  let osc_out = self.osc.next_sample(
      final_freq,
      sample_rate,
      params.osc.waveform,
      pw,
      params.osc.noise_mix,
  );
  ```

  Replace it with:

  ```rust
  // Oscillator
  let osc_out = self.osc.next_sample(
      final_freq,
      sample_rate,
      params.osc.waveform,
      pw,
      params.osc.noise_mix,
  );

  let osc_out = if params.osc2.osc2_mix > 0.001 {
      if params.osc2.hard_sync && self.osc.just_wrapped() {
          self.osc2.reset();
      }
      let osc2_freq = detune_hz(final_freq, params.osc2.detune);
      let osc2_out = self
          .osc2
          .next_sample(osc2_freq, sample_rate, params.osc2.waveform, pw, 0.0);
      osc_out * (1.0 - params.osc2.osc2_mix) + osc2_out * params.osc2.osc2_mix
  } else {
      osc_out
  };
  ```

- [ ] **Step 5: Run tests — all must pass**

  ```bash
  cargo test -p synthie
  ```

  Expected: `voice_osc2_mix_zero_is_finite` passes, `voice_osc2_hard_sync_is_finite` passes, all existing tests still pass.

- [ ] **Step 6: Commit**

  ```bash
  git add crates/synthie/src/audio/voice.rs
  git commit -m "feat(voice): add second oscillator with mix and hard sync"
  ```

---

## Task 5: TUI state — extend OSC section to 9 params

**Files:**
- Modify: `crates/synth-tui/src/app/state.rs`

- [ ] **Step 1: Update `param_count` for `Section::Osc`**

  Find the `param_count` match arm for `Section::Osc`:

  ```rust
  Section::Osc | Section::Filter => 4,
  ```

  Split it so `Osc` gets its own count:

  ```rust
  Section::Osc => 9, // wave, pw, det, noise | separator | wave2, det2, mix2, sync
  Section::Filter => 4,
  ```

- [ ] **Step 2: Extend `section_params()` for `Section::Osc`**

  Find the `Section::Osc =>` arm in `section_params()`:

  ```rust
  Section::Osc => vec![
      ("Wave", p.osc.waveform.name().to_string()),
      ("PW", format!("{:.2}", p.osc.pulse_width)),
      ("Det", format!("{:+.0}ct", p.osc.detune)),
      ("Nse", format!("{:.2}", p.osc.noise_mix)),
  ],
  ```

  Replace it with:

  ```rust
  Section::Osc => vec![
      ("Wave", p.osc.waveform.name().to_string()),
      ("PW", format!("{:.2}", p.osc.pulse_width)),
      ("Det", format!("{:+.0}ct", p.osc.detune)),
      ("Nse", format!("{:.2}", p.osc.noise_mix)),
      ("", "─ OSC2 ─".to_string()),
      ("Wv2", p.osc2.waveform.name().to_string()),
      ("Det2", format!("{:+.0}ct", p.osc2.detune)),
      ("Mix2", format!("{:.2}", p.osc2.osc2_mix)),
      ("Sync", if p.osc2.hard_sync { "ON" } else { "off" }.to_string()),
  ],
  ```

- [ ] **Step 3: Extend `adjust_osc()` with cases 4–8**

  Find `adjust_osc()`. After the existing `3 => { ... }` arm, add:

  ```rust
  4 => {} // separator row — no-op
  5 => {
      self.params.osc2.waveform = if d > 0.0 {
          self.params.osc2.waveform.next()
      } else {
          self.params.osc2.waveform.prev()
      };
  }
  6 => {
      self.params.osc2.detune =
          (self.params.osc2.detune + d * 5.0).clamp(-100.0, 100.0);
  }
  7 => {
      self.params.osc2.osc2_mix =
          (self.params.osc2.osc2_mix + d * 0.05).clamp(0.0, 1.0);
  }
  8 if d != 0.0 => {
      self.params.osc2.hard_sync = !self.params.osc2.hard_sync;
  }
  ```

- [ ] **Step 4: Verify compile**

  ```bash
  cargo check -p synth-tui
  ```

  Expected: exits 0 with no errors.

- [ ] **Step 5: Commit**

  ```bash
  git add crates/synth-tui/src/app/state.rs
  git commit -m "feat(tui): extend OSC section with OSC2 params (9-param list)"
  ```

---

## Task 6: TUI layout — grow OSC panel from 6 to 10 lines

**Files:**
- Modify: `crates/synth-tui/src/app/ui.rs`

- [ ] **Step 1: Update the left-column constraint for the OSC panel**

  Find the `left_sections` layout in `draw()` (around line 62):

  ```rust
  let left_sections = Layout::default()
      .direction(Direction::Vertical)
      .constraints([
          Constraint::Length(3), // title
          Constraint::Length(6), // OSC
          Constraint::Length(8), // ENV
          Constraint::Length(6), // FILTER
          Constraint::Length(5), // LFO
          Constraint::Length(5), // FX
          Constraint::Min(0),    // spacer
      ])
      .split(left);
  ```

  Change `Constraint::Length(6), // OSC` to:

  ```rust
  Constraint::Length(10), // OSC (includes OSC2 sub-panel)
  ```

- [ ] **Step 2: Build the TUI binary to confirm no layout issues**

  ```bash
  cargo build -p synth-tui
  ```

  Expected: builds successfully.

- [ ] **Step 3: Commit**

  ```bash
  git add crates/synth-tui/src/app/ui.rs
  git commit -m "feat(tui): grow OSC panel to 10 lines for OSC2 params"
  ```

---

## Task 7: Full checks + final commit

- [ ] **Step 1: Run the full test suite**

  ```bash
  cargo test
  ```

  Expected: all tests pass.

- [ ] **Step 2: Run the full check (format, clippy, compile)**

  ```bash
  task check
  ```

  If `task` is not available:

  ```bash
  cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo check
  ```

  Expected: exits 0. Fix any clippy warnings before proceeding.

- [ ] **Step 3: Smoke-test the TUI**

  ```bash
  cargo run -p synth-tui
  ```

  - Tab to OSC section — should now show 9 rows: 4 OSC1 params, separator `─ OSC2 ─`, 4 OSC2 params.
  - Navigate to `Mix2`, press Up — value increments; a note should now produce a detuned unison.
  - Navigate to `Sync`, press Up — toggles `ON`; hard sync effect audible (especially with OSC2 at lower frequency).
  - Tabbing past OSC moves to ENV; Shift+Tab goes back.
  - Load a preset — OSC2 defaults to `osc2_mix = 0.0` (bypassed) on old presets.

- [ ] **Step 4: Close the GitHub issue**

  ```bash
  gh issue close 12 --comment "Implemented: Waveform::Sine, dual OSC with osc2_mix blend, hard sync (OSC1 master). OSC2 bypassed when mix=0. TUI OSC panel extended to 10 lines."
  ```
