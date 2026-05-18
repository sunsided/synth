# Ring Modulation Between Oscillators

**Issue:** #23
**Date:** 2026-05-18
**Status:** Approved

## Summary

Add SID-inspired ring modulation between OSC1 and OSC2 via a configurable `RingModMode` enum. Completes the classic SID modulation triad: hard sync (done), PWM (done), ring mod (this).

## Params (`params.rs`)

Add `RingModMode` enum:

```rust
pub enum RingModMode {
    Off,             // default - bypass, no change to existing behaviour
    Osc2ByOsc1Sign,  // SID-exact: secondary * sign(osc1_phase_msb)
    Osc1ByOsc2Sign,  // symmetric: osc1_out * sign(osc2_phase_msb)
    Analog,          // true ring mod: osc1_out * secondary
}
```

- Derives `Default` (`Off`), `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.
- `#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]`

Add to `Osc2Params`:

```rust
#[cfg_attr(feature = "serde", serde(default))]
pub ring_mod: RingModMode,
```

Default = `Off`. Old JSON without the field deserializes to `Off` - no breaking change.

## Oscillator (`osc.rs`)

Add to `Oscillator` impl:

```rust
pub fn phase_sign(&self) -> f32 {
    if self.phase < 0.5 { 1.0 } else { -1.0 }
}
```

Returns sign of the phase accumulator MSB, matching SID chip convention. Called after `next_sample()` so phase reflects the current sample.

## Voice (`voice.rs`)

Inside the `osc2_mix > 0.001` guard, replace the direct `secondary` with a `modulated` value:

```rust
let modulated = match params.osc2.ring_mod {
    RingModMode::Off => secondary,
    RingModMode::Osc2ByOsc1Sign => secondary * self.osc.phase_sign(),
    RingModMode::Osc1ByOsc2Sign => osc_out * self.osc2.phase_sign(),
    RingModMode::Analog => osc_out * secondary,
};

osc_out * (1.0 - params.osc2.osc2_mix) + modulated * params.osc2.osc2_mix
```

The raw `Analog` ring-mod product (OSC1 × OSC2) is bounded to -1..1 because both operands are in -1..1. Downstream processing (filter, drive, FX) can exceed ±1 - voice output is guaranteed finite, not bounded to ±1.

## Tests

### `osc.rs`
- `phase_sign_matches_phase`: advance oscillator through one period, verify sign is +1 in [0, 0.5) and -1 in [0.5, 1.0).

### `voice.rs`
- `ring_mod_off_is_deterministic`: `RingModMode::Off` produces bit-identical output across two independent voices (regression guard).
- `ring_mod_modes_alter_output`: each active mode produces measurably different output from `Off` (abs_diff sum over 500 samples).
- `ring_mod_analog_output_is_finite`: 1000 samples, `Analog` mode - all samples finite (SVF can exceed ±1 near Nyquist, so bounds check is intentionally omitted).

## Out of Scope

- No new presets (no change to `sid.rs`).
- No TUI changes (ring mod exposed via params struct; UI work is separate).
- No change to `hard_sync` interaction - both flags operate independently on the same `osc2_mix` guard.
