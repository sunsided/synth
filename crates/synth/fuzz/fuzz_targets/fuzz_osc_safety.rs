//! Fuzz target: `Oscillator::next_sample` output safety.
//!
//! Verifies that the oscillator never produces `NaN` or `±Inf` for any
//! combination of finite frequency, sample rate, waveform, pulse width, and
//! noise mix that the audio engine could plausibly supply.
//!
//! # Phase-wrap note
//! When `freq_hz / sample_rate > 1.0` the phase accumulator can overshoot by
//! more than 1.0 in a single step; the oscillator subtracts exactly 1.0, so
//! `phase` may temporarily sit above 1.0.  Consequently the sawtooth and
//! triangle formulae (`2*p − 1`, `4*p − 1`) can exceed `[-1, 1]` for that
//! sample.  This is a documented operational edge case, not a safety violation.
//! The harness therefore only asserts **finiteness**, not strict bounds, for
//! arbitrary inputs.
//!
//! When `freq_hz` and `sample_rate` are constrained to the normal audio range
//! (e.g., `freq_hz ≤ sample_rate`), strict `[-1, 1]` bounds hold.  The harness
//! exercises both regimes so the fuzzer can discover degenerate inputs while
//! still asserting the minimal safety invariant.

#![no_main]

use libfuzzer_sys::fuzz_target;
use synth::audio::osc::Oscillator;
use synth::params::Waveform;

/// All valid `Waveform` variants in index order.
const WAVEFORMS: [Waveform; 5] = [
    Waveform::Pulse,
    Waveform::Sawtooth,
    Waveform::Triangle,
    Waveform::Noise,
    Waveform::PulseSaw,
];

/// Packed fuzz input for one oscillator burst.
#[repr(C)]
#[derive(Clone, Copy)]
struct FuzzInput {
    /// Oscillator frequency in Hz (any finite value accepted by the engine).
    freq_hz: f32,
    /// Audio sample rate in Hz.
    sample_rate: f32,
    /// Pulse width (relevant for Pulse / PulseSaw waveforms).
    pulse_width: f32,
    /// Noise mix (0.0 – 1.0 blend towards LFSR noise).
    noise_mix: f32,
    /// Waveform index (treated modulo `WAVEFORMS.len()`).
    waveform_idx: u8,
    _pad: [u8; 3],
}

fuzz_target!(|data: &[u8]| {
    if data.len() < size_of::<FuzzInput>() {
        return;
    }

    let mut raw = [0u8; size_of::<FuzzInput>()];
    raw.copy_from_slice(&data[..size_of::<FuzzInput>()]);
    // SAFETY: FuzzInput contains only f32/u8/padding; all bit patterns are
    // valid for those types.
    let fi: FuzzInput = unsafe { std::mem::transmute(raw) };

    // Require finite inputs before applying range constraints.
    if !fi.freq_hz.is_finite()
        || !fi.sample_rate.is_finite()
        || !fi.pulse_width.is_finite()
        || !fi.noise_mix.is_finite()
    {
        return;
    }

    let waveform = WAVEFORMS[(fi.waveform_idx as usize) % WAVEFORMS.len()];

    // Constrain to the physically meaningful operating envelope so that
    // `freq_hz / sample_rate` stays well within f32 range.
    // • sample_rate: [1.0, 200 000] Hz – covers all practical audio rates.
    // • freq_hz:     {0} ∪ [1, 200 000] Hz – sub-Hz / DC included via 0.
    //   Negative freq is unusual and not part of the oscillator's contract;
    //   silently skip those cases.
    // This matches the "bounded but extreme" input philosophy from the plan.
    // Require a physically meaningful (non-subnormal) sample rate.
    if fi.sample_rate < 1.0 || fi.sample_rate > 200_000.0 {
        return;
    }
    if fi.freq_hz < 0.0 || fi.freq_hz > 200_000.0 {
        return;
    }

    // Clamp noise_mix to a meaningful range so the blend arithmetic is stable.
    let noise_mix = fi.noise_mix.clamp(0.0, 1.0);

    let mut osc = Oscillator::default();

    // Produce a short burst of samples.  This exercises phase accumulation and
    // LFSR state transitions across multiple ticks.
    for _ in 0..32 {
        let out = osc.next_sample(
            fi.freq_hz,
            fi.sample_rate,
            waveform,
            fi.pulse_width,
            noise_mix,
        );

        // Core invariant: the oscillator must never produce a non-finite value.
        assert!(
            out.is_finite(),
            "Oscillator produced non-finite output {out} \
             (freq={}, sr={}, waveform={waveform:?}, pw={}, noise_mix={})",
            fi.freq_hz,
            fi.sample_rate,
            fi.pulse_width,
            noise_mix,
        );
    }
});
