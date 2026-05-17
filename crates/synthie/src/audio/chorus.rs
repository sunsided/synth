//! BBD-style chorus: 3 modulated taps from a shared delay line.
//!
//! Three sine LFOs at 0/120/240-degree phase spread modulate the read position
//! around a 15 ms center delay. Linear interpolation between samples.
//!
//! Hot path uses sin/cos rotation (no `sin()` calls per sample) to stay within
//! real-time audio budget at high LFO rates.

use std::f32::consts::TAU;

use crate::params::ChorusParams;

/// Maximum chorus delay line length in milliseconds.
const MAX_CHORUS_DELAY_MS: f32 = 50.0;

/// Center delay offset in milliseconds (read position when depth = 0).
const CENTER_DELAY_MS: f32 = 15.0;

/// Number of modulated taps.
const NUM_TAPS: usize = 3;

/// BBD-style 3-tap chorus with sine LFO modulation.
pub struct Chorus {
    buf: Vec<f32>,
    write_idx: usize,
    /// Per-tap LFO state as (sin, cos) pairs — advanced via rotation each sample.
    lfo: [(f32, f32); NUM_TAPS],
    /// Cached rotation step recomputed only when `params.rate` changes.
    sin_inc: f32,
    cos_inc: f32,
    cached_rate: f32,
    sample_rate: f32,
}

impl Chorus {
    /// Allocate chorus delay line for the given sample rate.
    ///
    /// LFO phases are initialised at 0°, 120°, 240°.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let len = ((MAX_CHORUS_DELAY_MS / 1000.0) * sample_rate).ceil() as usize;
        let phases = [0.0_f32, TAU / 3.0, 2.0 * TAU / 3.0];
        let lfo = phases.map(f32::sin_cos);
        Self {
            buf: vec![0.0; len.max(1)],
            write_idx: 0,
            lfo,
            sin_inc: 0.0,
            cos_inc: 1.0,
            cached_rate: -1.0,
            sample_rate,
        }
    }

    /// Process one sample.
    ///
    /// Returns `input` unchanged when `mix < 1e-4` (fast bypass).
    #[allow(clippy::cast_precision_loss)]
    pub fn process(&mut self, input: f32, params: &ChorusParams) -> f32 {
        if params.mix < 1e-4 {
            return input;
        }

        debug_assert!(
            params.depth_ms <= CENTER_DELAY_MS,
            "depth_ms ({}) exceeds center delay ({}ms) — read offset would wrap to zero-latency tap",
            params.depth_ms,
            CENTER_DELAY_MS
        );

        // Recompute rotation coefficients only when rate changes.
        // Intentional exact-bits comparison: cached_rate stores the f32 we last used.
        #[allow(clippy::float_cmp)]
        if params.rate != self.cached_rate {
            let phase_inc = TAU * params.rate / self.sample_rate;
            (self.sin_inc, self.cos_inc) = phase_inc.sin_cos();
            self.cached_rate = params.rate;
        }

        let buf_len = self.buf.len();
        let buf_len_f = buf_len as f32;
        self.buf[self.write_idx] = input;

        let center_samples = CENTER_DELAY_MS / 1000.0 * self.sample_rate;
        let depth_samples = params.depth_ms.clamp(0.0, CENTER_DELAY_MS) / 1000.0 * self.sample_rate;
        let write_idx_f = self.write_idx as f32;

        let mut tap_sum = 0.0_f32;
        for (s, c) in &mut self.lfo {
            let offset = (center_samples + depth_samples * *s).clamp(0.0, buf_len_f - 1.001); // -1.001: keep ceil_idx in-bounds after floor+1

            // Rotate LFO state — no sin() call.
            let new_s = *s * self.cos_inc + *c * self.sin_inc;
            let new_c = *c * self.cos_inc - *s * self.sin_inc;
            *s = new_s;
            *c = new_c;

            // Fractional read position (backwards from write head).
            // raw is in [buf_len_f - (buf_len_f - 1.0), buf_len_f + write_idx_f]
            // i.e. always in (0, 2*buf_len_f), so one conditional subtract suffices.
            let raw = write_idx_f + buf_len_f - offset;
            let read_pos = if raw >= buf_len_f {
                raw - buf_len_f
            } else {
                raw
            };

            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let floor_idx = read_pos.floor() as usize % buf_len;
            let ceil_idx = (floor_idx + 1) % buf_len;
            let frac = read_pos.fract();
            tap_sum += self.buf[floor_idx] * (1.0 - frac) + self.buf[ceil_idx] * frac;
        }

        self.write_idx = (self.write_idx + 1) % buf_len;

        #[allow(clippy::cast_precision_loss)]
        let wet = tap_sum / NUM_TAPS as f32;
        input * (1.0 - params.mix) + wet * params.mix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(rate: f32, depth_ms: f32, mix: f32) -> ChorusParams {
        ChorusParams {
            rate,
            depth_ms,
            mix,
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn bypass_at_zero_mix() {
        let mut c = Chorus::new(44100.0);
        let p = params(0.5, 3.0, 0.0);
        for &x in &[-1.0_f32, -0.5, 0.0, 0.5, 1.0] {
            assert_eq!(c.process(x, &p), x, "zero-mix bypass failed for {x}");
        }
    }

    #[test]
    fn output_finite_at_boundary_params() {
        let mut c = Chorus::new(44100.0);
        for &rate in &[0.1_f32, 2.5, 5.0] {
            for &depth_ms in &[0.0_f32, 5.0, 10.0] {
                for &mix in &[0.0_f32, 0.5, 1.0] {
                    let p = params(rate, depth_ms, mix);
                    for &x in &[-1.0_f32, 0.0, 1.0] {
                        let out = c.process(x, &p);
                        assert!(
                            out.is_finite(),
                            "NaN/Inf: rate={rate}, depth={depth_ms}, mix={mix}, in={x}, out={out}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn modulation_active_after_warmup() {
        // After filling the delay line, output with mix=1.0 and depth>0 must be non-zero.
        let mut c = Chorus::new(44100.0);
        let p = params(1.0, 5.0, 1.0);
        // Warm up: more than MAX_CHORUS_DELAY_MS worth of samples (50ms @ 44100 = 2205 samples).
        for _ in 0..3000 {
            c.process(1.0, &p);
        }
        let out = c.process(1.0, &p);
        assert!(
            out.abs() > 0.01,
            "chorus output should be non-zero after warmup: {out}"
        );
    }

    #[test]
    fn dc_input_steady_state_near_unity() {
        // With depth=0 all three taps read from the same center-delay position.
        // After the buffer fills with 1.0, tap_sum/3 = 1.0, so wet = 1.0.
        let mut c = Chorus::new(44100.0);
        let p = params(0.1, 0.0, 1.0); // depth=0 → no modulation
        for _ in 0..3000 {
            c.process(1.0, &p);
        }
        let out = c.process(1.0, &p);
        assert!(
            (out - 1.0).abs() < 0.02,
            "expected ~1.0 for DC+depth=0, got {out}"
        );
    }
}
