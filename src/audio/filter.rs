//! Trapezoidal state-variable filter (TPT SVF) with pre-drive saturation.
//!
//! Reference: Andrew Simper, "Solving the Continuous SVF Equations Using
//! Trapezoidal Integration and Equivalent Circuits" (2013).

use crate::params::FilterMode;
use std::f32::consts::PI;

/// Trapezoidal-integrated state-variable filter (Cytomic/Simper TPT SVF).
///
/// Unconditionally stable for any sample rate and cutoff.  Provides LP, BP,
/// and HP simultaneously.  A subtle `tanh` saturation is applied to the
/// band-pass state for SID-style resonance character.
///
/// Pre-drive soft-clipping of the input is also supported (maps to the SID
/// "drive" character control).
pub struct SvFilter {
    ic1eq: f32, // first integrator state
    ic2eq: f32, // second integrator state
}

impl Default for SvFilter {
    /// Create a filter with zeroed integrator states.
    fn default() -> Self {
        Self {
            ic1eq: 0.0,
            ic2eq: 0.0,
        }
    }
}

impl SvFilter {
    /// Zero both integrator states (use after a voice panic or hard reset).
    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    /// Process a single sample.
    ///
    /// * `input`      – input sample (-1..1 range expected)
    /// * `mode`       – LP / BP / HP
    /// * `cutoff`     – cutoff frequency in Hz
    /// * `resonance`  – 0.0 (no resonance) .. 0.99 (very high Q)
    /// * `drive`      – 0.0 .. 1.0, pre-filter saturation
    /// * `sample_rate`– audio sample rate in Hz
    pub fn process(
        &mut self,
        input: f32,
        mode: FilterMode,
        cutoff: f32,
        resonance: f32,
        drive: f32,
        sample_rate: f32,
    ) -> f32 {
        // Pre-drive soft clip (SID "grit")
        let x = if drive > 0.001 {
            let gain = 1.0 + drive * 4.0;
            let x = input * gain;
            // Soft clip: x / sqrt(1 + x²)  (keeps odd harmonics, no hard edge)
            let clipped = x / (1.0 + x * x).sqrt();
            // When gain·input overflows f32 to ±Inf, x²→Inf and we get Inf/Inf=NaN.
            // The analytic limit as |x|→∞ is ±1; recover with copysign.
            if clipped.is_finite() {
                clipped
            } else {
                1.0_f32.copysign(x)
            }
        } else {
            input
        };

        // Filter coefficient calculation
        // Clamp the normalised cutoff ratio to just below 0.5 before computing the
        // bilinear warp coefficient `g`.  Two edge cases are handled here:
        //   1. sample_rate < ~40 Hz: Nyquist would fall below the 20 Hz minimum,
        //      so we raise the Nyquist floor to 20 Hz to keep clamp(min, max) valid.
        //   2. Very small sample_rate: even with fc clamped, PI*fc/sr can overflow
        //      f32 to Inf, causing tan(Inf) = NaN.  Clamping fc_norm prevents this.
        let nyquist = (sample_rate * 0.499).max(20.0);
        let fc = cutoff.clamp(20.0, nyquist);
        // fc_norm ∈ (0, 0.499] ensures PI*fc_norm < PI/2, keeping tan finite.
        let fc_norm = (fc / sample_rate).min(0.499_f32);
        let g = (PI * fc_norm).tan();
        // k = 2 - 2*resonance maps resonance 0→k=2 (Q=0.5) to 0.99→k=0.02 (Q≈50)
        let k = (2.0 - 1.98 * resonance.clamp(0.0, 0.999)).max(0.01);

        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v3 = x - self.ic2eq;
        let v1 = a1 * self.ic1eq + a2 * v3;
        let v2 = self.ic2eq + a2 * self.ic1eq + a3 * v3;

        // Update integrators
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;

        // Clamp integrators to prevent denormals / instability under extreme params
        self.ic1eq = clamp_denormal(self.ic1eq);
        self.ic2eq = clamp_denormal(self.ic2eq);

        let out = match mode {
            FilterMode::LowPass => v2,
            // SID character: subtle tanh saturation on band-pass path
            FilterMode::BandPass => fast_tanh(v1 * (1.0 + resonance * 0.5)),
            FilterMode::HighPass => x - k * v1 - v2,
        };

        // Safety: intermediate overflow (e.g. near-f32::MAX input without soft-clip)
        // can produce a non-finite output on the overflow sample even though
        // clamp_denormal above ensures state recovery on the next sample.
        // Return 0.0 rather than propagating Inf/NaN to the caller.
        if out.is_finite() { out } else { 0.0 }
    }
}

/// Flush denormals to zero and reset any non-finite values to zero.
///
/// Two situations are handled:
/// * **Denormals** (`|x| < 1e-15`): flushed to zero to avoid CPU performance
///   degradation from subnormal float arithmetic.
/// * **Non-finite values**: reset to zero so that an Inf/NaN integrator state
///   (e.g., from an extremely large input sample) does not propagate to all
///   subsequent outputs.  The reset causes a brief transient artefact but
///   restores filter operation immediately on the next sample.
#[inline]
fn clamp_denormal(x: f32) -> f32 {
    if !x.is_finite() || x.abs() < 1e-15 {
        0.0
    } else {
        x
    }
}

/// Fast tanh approximation (Padé 5/4) – accurate to ±0.5% for |x| < 4.
///
/// Returns exactly `±1.0` for `|x| >= 4` so the polynomial intermediate values
/// stay within `f32` range regardless of input magnitude.  Without this guard,
/// `x²` overflows to `f32::INFINITY` for large inputs, causing `Inf / Inf = NaN`
/// to escape the final clamp.
#[inline]
fn fast_tanh(x: f32) -> f32 {
    // tanh(±4) ≈ ±0.9993; clamping here is within the stated ±0.5% accuracy
    // window and prevents intermediate overflow for any finite input.
    if x >= 4.0 {
        return 1.0;
    }
    if x <= -4.0 {
        return -1.0;
    }
    let x2 = x * x;
    let n = x * (135.0 + x2 * (17.0 + x2));
    let d = 135.0 + x2 * (45.0 + x2 * 9.0);
    (n / d).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::cast_precision_loss)] // small sample count; precision loss negligible
    fn rms(samples: &[f32]) -> f32 {
        let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }

    #[test]
    #[allow(clippy::cast_precision_loss)] // test signal generation; index fits well within f32
    fn lp_attenuates_high_freq() {
        // 1 kHz tone, LP at 200 Hz → should be significantly attenuated
        let mut filt = SvFilter::default();
        let sr = 44100.0_f32;
        let tone: Vec<f32> = (0..4096)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr).sin())
            .collect();
        let filtered: Vec<f32> = tone
            .iter()
            .map(|&s| filt.process(s, FilterMode::LowPass, 200.0, 0.1, 0.0, sr))
            .collect();
        let ratio = rms(&filtered) / rms(&tone[2048..]); // skip transient
        assert!(ratio < 0.5, "LP did not attenuate: ratio={ratio}");
    }

    #[test]
    #[allow(clippy::cast_precision_loss)] // test signal generation; index fits well within f32
    fn hp_attenuates_low_freq() {
        // 50 Hz tone, HP at 500 Hz → should be significantly attenuated
        let mut filt = SvFilter::default();
        let sr = 44100.0_f32;
        let tone: Vec<f32> = (0..8192)
            .map(|i| (2.0 * std::f32::consts::PI * 50.0 * i as f32 / sr).sin())
            .collect();
        let filtered: Vec<f32> = tone
            .iter()
            .map(|&s| filt.process(s, FilterMode::HighPass, 500.0, 0.1, 0.0, sr))
            .collect();
        let ratio = rms(&filtered[4096..]) / rms(&tone[4096..]);
        assert!(ratio < 0.5, "HP did not attenuate: ratio={ratio}");
    }

    #[test]
    fn no_nan_under_extreme_params() {
        let mut filt = SvFilter::default();
        let input = 1.0_f32;
        for &res in &[0.0_f32, 0.5, 0.95, 0.999] {
            for &drive in &[0.0_f32, 0.5, 1.0] {
                let out = filt.process(input, FilterMode::LowPass, 100.0, res, drive, 44100.0);
                assert!(out.is_finite(), "NaN/Inf at res={res}, drive={drive}");
            }
        }
    }
}
