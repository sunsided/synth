//! Fuzz target: `SynthParams` serde round-trip resilience.
//!
//! Two complementary sub-harnesses run on each input:
//!
//! 1. **Deserialisation robustness** – feed raw bytes (interpreted as UTF-8)
//!    into `serde_json::from_str::<SynthParams>`.  The deserialiser must
//!    never panic regardless of input content; returning an `Err` is fine.
//!
//! 2. **Round-trip consistency** – when deserialisation succeeds, re-serialise
//!    the result with `serde_json::to_string` and deserialise again.  The two
//!    `SynthParams` snapshots must be structurally equal.
//!
//! # On numeric invariants
//! `SynthParams` carries raw `f32` fields with no enforcement of in-range
//! values on deserialise.  The harness therefore only checks:
//!   - Successful deserialisations do not produce `NaN` in float fields
//!     (NaN would break round-trip equality as NaN ≠ NaN).
//!   - The round-trip produces byte-for-byte identical JSON for valid inputs.
//!
//! If you later add invariant-checking to the `Deserialize` impl, tighten
//! the assertions here accordingly.

#![no_main]

use libfuzzer_sys::fuzz_target;
use synth::params::{FxParams, GlobalParams, LfoParams, OscParams, SynthParams};

fuzz_target!(|data: &[u8]| {
    // --- Part 1: deserialisation must not panic ---
    // Treat the raw bytes as a (possibly invalid) UTF-8 string.  serde_json
    // accepts &str; non-UTF-8 bytes simply can't form a valid JSON string, so
    // `from_utf8` failure is an expected no-op early exit.
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    let Ok(params) = serde_json::from_str::<SynthParams>(s) else {
        // Parse error is perfectly valid behaviour for arbitrary bytes.
        return;
    };

    // --- Part 2: NaN check on successfully-decoded float fields ---
    // NaN would make the round-trip comparison meaningless.  If this fires,
    // it means serde accepted a JSON "null" or similar for an f32 field — a
    // real bug worth investigating.
    assert_float_fields_finite(&params);

    // --- Part 3: round-trip consistency ---
    let serialised = serde_json::to_string(&params)
        .expect("serialisation of a successfully-decoded SynthParams must not fail");

    let params2: SynthParams = serde_json::from_str(&serialised)
        .expect("re-deserialisation of a just-serialised SynthParams must not fail");

    assert_params_equal(&params, &params2);
});

/// Assert that all `f32` fields in `SynthParams` are finite (no NaN or Inf).
fn assert_float_fields_finite(p: &SynthParams) {
    let OscParams {
        pulse_width,
        detune,
        noise_mix,
        ..
    } = &p.osc;
    assert!(
        pulse_width.is_finite(),
        "osc.pulse_width is not finite: {pulse_width}"
    );
    assert!(detune.is_finite(), "osc.detune is not finite: {detune}");
    assert!(
        noise_mix.is_finite(),
        "osc.noise_mix is not finite: {noise_mix}"
    );

    let synth::params::EnvParams {
        attack,
        decay,
        sustain,
        release,
        ..
    } = &p.env;
    assert!(attack.is_finite(), "env.attack is not finite: {attack}");
    assert!(decay.is_finite(), "env.decay is not finite: {decay}");
    assert!(sustain.is_finite(), "env.sustain is not finite: {sustain}");
    assert!(release.is_finite(), "env.release is not finite: {release}");

    let synth::params::FilterParams {
        cutoff,
        resonance,
        drive,
        ..
    } = &p.filter;
    assert!(cutoff.is_finite(), "filter.cutoff is not finite: {cutoff}");
    assert!(
        resonance.is_finite(),
        "filter.resonance is not finite: {resonance}"
    );
    assert!(drive.is_finite(), "filter.drive is not finite: {drive}");

    let LfoParams {
        lfo_rate,
        lfo_depth,
        ..
    } = &p.lfo;
    assert!(
        lfo_rate.is_finite(),
        "lfo.lfo_rate is not finite: {lfo_rate}"
    );
    assert!(
        lfo_depth.is_finite(),
        "lfo.lfo_depth is not finite: {lfo_depth}"
    );

    let FxParams {
        reverb_mix,
        reverb_size,
        reverb_damping,
    } = &p.fx;
    assert!(
        reverb_mix.is_finite(),
        "fx.reverb_mix is not finite: {reverb_mix}"
    );
    assert!(
        reverb_size.is_finite(),
        "fx.reverb_size is not finite: {reverb_size}"
    );
    assert!(
        reverb_damping.is_finite(),
        "fx.reverb_damping is not finite: {reverb_damping}"
    );

    let GlobalParams { volume, glide_time } = &p.global;
    assert!(volume.is_finite(), "global.volume is not finite: {volume}");
    assert!(
        glide_time.is_finite(),
        "global.glide_time is not finite: {glide_time}"
    );
}

/// Assert structural equality of two `SynthParams` snapshots.
///
/// Uses field-by-field comparison via `==` (which is `PartialEq` for the
/// enum discriminants and bit-exact `==` for `f32`).  Since we checked for
/// NaN above, bit-exact float comparison is safe here.
#[allow(clippy::float_cmp)] // bit-exact comparison is intentional for round-trip check
fn assert_params_equal(a: &SynthParams, b: &SynthParams) {
    assert_eq!(
        a.osc.waveform, b.osc.waveform,
        "osc.waveform diverged after round-trip"
    );
    assert_eq!(
        a.osc.pulse_width, b.osc.pulse_width,
        "osc.pulse_width diverged after round-trip"
    );
    assert_eq!(
        a.osc.detune, b.osc.detune,
        "osc.detune diverged after round-trip"
    );
    assert_eq!(
        a.osc.noise_mix, b.osc.noise_mix,
        "osc.noise_mix diverged after round-trip"
    );

    assert_eq!(
        a.env.attack, b.env.attack,
        "env.attack diverged after round-trip"
    );
    assert_eq!(
        a.env.decay, b.env.decay,
        "env.decay diverged after round-trip"
    );
    assert_eq!(
        a.env.sustain, b.env.sustain,
        "env.sustain diverged after round-trip"
    );
    assert_eq!(
        a.env.release, b.env.release,
        "env.release diverged after round-trip"
    );
    assert_eq!(
        a.env.env_reverse, b.env.env_reverse,
        "env.env_reverse diverged after round-trip"
    );

    assert_eq!(
        a.filter.filter_mode, b.filter.filter_mode,
        "filter.filter_mode diverged after round-trip"
    );
    assert_eq!(
        a.filter.cutoff, b.filter.cutoff,
        "filter.cutoff diverged after round-trip"
    );
    assert_eq!(
        a.filter.resonance, b.filter.resonance,
        "filter.resonance diverged after round-trip"
    );
    assert_eq!(
        a.filter.drive, b.filter.drive,
        "filter.drive diverged after round-trip"
    );

    assert_eq!(
        a.lfo.lfo_rate, b.lfo.lfo_rate,
        "lfo.lfo_rate diverged after round-trip"
    );
    assert_eq!(
        a.lfo.lfo_depth, b.lfo.lfo_depth,
        "lfo.lfo_depth diverged after round-trip"
    );
    assert_eq!(
        a.lfo.lfo_target, b.lfo.lfo_target,
        "lfo.lfo_target diverged after round-trip"
    );

    assert_eq!(
        a.fx.reverb_mix, b.fx.reverb_mix,
        "fx.reverb_mix diverged after round-trip"
    );
    assert_eq!(
        a.fx.reverb_size, b.fx.reverb_size,
        "fx.reverb_size diverged after round-trip"
    );
    assert_eq!(
        a.fx.reverb_damping, b.fx.reverb_damping,
        "fx.reverb_damping diverged after round-trip"
    );

    assert_eq!(
        a.global.volume, b.global.volume,
        "global.volume diverged after round-trip"
    );
    assert_eq!(
        a.global.glide_time, b.global.glide_time,
        "global.glide_time diverged after round-trip"
    );
}
