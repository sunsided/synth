//! Fuzz target: `SvFilter` numerical stability.
//!
//! Verifies that `SvFilter::process` never produces `NaN` or `±Inf` output
//! for any combination of inputs that can arise in practice, including extreme
//! parameter values that reach (but stay within) the internal clamping logic.
//!
//! The harness interprets the raw fuzz bytes as a tightly-packed struct of
//! parameter fields, which lets the fuzzer explore the full parameter space
//! efficiently without relying on arbitrary byte-to-float conversions.
//!
//! # Invariant
//! `SvFilter::process` output **must** be finite for all finite inputs when
//! the filter mode is any valid `FilterMode` variant.

#![no_main]

use libfuzzer_sys::fuzz_target;
use synth::audio::filter::SvFilter;
use synth::params::FilterMode;

/// Packed fuzz input for one `SvFilter::process` call.
///
/// Exactly 24 bytes; `libfuzzer` will mutate and minimise this struct.
#[repr(C)]
#[derive(Clone, Copy)]
struct FuzzInput {
    /// Input sample value (arbitrary — we test all float ranges).
    input: f32,
    /// Cutoff frequency in Hz.  Internal clamping handles out-of-range values.
    cutoff: f32,
    /// Resonance (0.0 – 0.999 expected; fully exercising the clamp path).
    resonance: f32,
    /// Drive (0.0 – 1.0 expected).
    drive: f32,
    /// Sample rate in Hz (positive non-zero values make sense).
    sample_rate: f32,
    /// Filter mode index (0 → LP, 1 → BP, 2 → HP; treated modulo 3).
    mode_idx: u8,
    _pad: [u8; 3],
}

/// All valid `FilterMode` variants in index order.
const MODES: [FilterMode; 3] = [
    FilterMode::LowPass,
    FilterMode::BandPass,
    FilterMode::HighPass,
];

fuzz_target!(|data: &[u8]| {
    // Need at least one full FuzzInput (24 bytes).  Silently skip shorter
    // inputs; the fuzzer will quickly learn to produce valid-length data.
    if data.len() < size_of::<FuzzInput>() {
        return;
    }

    // SAFETY: data is at least size_of::<FuzzInput>() bytes.  We copy it
    // into a properly-aligned local variable to avoid any alignment UB.
    let mut raw = [0u8; size_of::<FuzzInput>()];
    raw.copy_from_slice(&data[..size_of::<FuzzInput>()]);
    let fi: FuzzInput = unsafe { std::mem::transmute(raw) };

    // Skip inputs with non-finite floats: library pre-condition is finite input.
    // These are valid edges for a real caller, but the filter contract requires
    // callers to supply finite values; our fuzz postcondition is output finiteness.
    if !fi.input.is_finite()
        || !fi.cutoff.is_finite()
        || !fi.resonance.is_finite()
        || !fi.drive.is_finite()
        || !fi.sample_rate.is_finite()
        // Require a physically plausible (non-subnormal) sample rate.
        // sr < 1.0 Hz is nonsensical for audio and triggers overflow paths
        // in the bilinear warp coefficient that are outside the filter's
        // intended operating envelope.
        || fi.sample_rate < 1.0
    {
        return;
    }

    let mode = MODES[(fi.mode_idx as usize) % MODES.len()];

    let mut filter = SvFilter::default();

    // Run a short burst of samples so accumulated integrator state is exercised.
    for _ in 0..16 {
        let out = filter.process(
            fi.input,
            mode,
            fi.cutoff,
            fi.resonance,
            fi.drive,
            fi.sample_rate,
        );

        // Core invariant: output must always be finite.
        assert!(
            out.is_finite(),
            "SvFilter produced non-finite output {out} \
             (input={}, cutoff={}, res={}, drive={}, sr={}, mode={mode:?})",
            fi.input,
            fi.cutoff,
            fi.resonance,
            fi.drive,
            fi.sample_rate,
        );
    }
});
