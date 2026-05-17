//! Single-tap digital delay with feedback and wet/dry mix.
//!
//! Ring buffer allocated once at construction. No heap allocation in `process()`.

use crate::params::DelayParams;

/// Maximum supported delay time in milliseconds. Determines ring buffer allocation.
const MAX_DELAY_MS: f32 = 2000.0;

/// Single-tap feedback delay line.
pub struct Delay {
    buf: Vec<f32>,
    write_idx: usize,
    sample_rate: f32,
}

impl Delay {
    /// Allocate a delay line for the given sample rate.
    ///
    /// Buffer length = `ceil(MAX_DELAY_MS` / 1000 * `sample_rate)` samples.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let len = ((MAX_DELAY_MS / 1000.0) * sample_rate).ceil() as usize;
        Self {
            buf: vec![0.0; len.max(1)],
            write_idx: 0,
            sample_rate,
        }
    }

    /// Process one sample.
    ///
    /// Returns `input` unchanged when `mix < 1e-4` (fast bypass).
    pub fn process(&mut self, input: f32, params: &DelayParams) -> f32 {
        if params.mix < 1e-4 {
            return input;
        }

        let buf_len = self.buf.len();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let delay_samples =
            ((params.time_ms / 1000.0 * self.sample_rate).round() as usize).clamp(1, buf_len);
        let feedback = params.feedback.clamp(0.0, 0.999);

        let read_idx = (self.write_idx + buf_len - delay_samples) % buf_len;
        let wet = self.buf[read_idx];

        self.buf[self.write_idx] = input + wet * feedback;
        self.write_idx = (self.write_idx + 1) % buf_len;

        input * (1.0 - params.mix) + wet * params.mix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(time_ms: f32, feedback: f32, mix: f32) -> DelayParams {
        DelayParams {
            time_ms,
            feedback,
            mix,
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn bypass_at_zero_mix() {
        let mut d = Delay::new(44100.0);
        let p = params(100.0, 0.5, 0.0);
        for &x in &[-1.0_f32, -0.5, 0.0, 0.5, 1.0] {
            assert_eq!(d.process(x, &p), x, "zero-mix bypass failed for {x}");
        }
    }

    #[test]
    fn delayed_signal_appears_at_correct_offset() {
        let sr = 44100.0_f32;
        let time_ms = 10.0_f32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let delay_samples = (time_ms / 1000.0 * sr).round() as usize; // 441

        let mut d = Delay::new(sr);
        let p = params(time_ms, 0.0, 1.0);

        // Inject impulse; wet output is 0 (buffer was silent).
        let out0 = d.process(1.0, &p);
        assert!(out0.abs() < 1e-4, "expected ~0 immediately, got {out0}");

        // Feed silence for delay_samples-1 more calls.
        for _ in 0..(delay_samples - 1) {
            d.process(0.0, &p);
        }

        // On the delay_samples-th call, the impulse should come back.
        let out_delayed = d.process(0.0, &p);
        assert!(
            (out_delayed - 1.0).abs() < 1e-4,
            "impulse not found at delay offset: got {out_delayed}"
        );
    }

    #[test]
    fn feedback_energy_decays() {
        let mut d = Delay::new(44100.0);
        // 1 ms delay so the echo period is short and we can measure many cycles fast.
        let p = params(1.0, 0.5, 1.0);

        d.process(1.0, &p); // inject impulse

        let outputs: Vec<f32> = (0..200).map(|_| d.process(0.0, &p)).collect();
        let first: f32 = outputs[..100].iter().map(|x| x * x).sum();
        let second: f32 = outputs[100..].iter().map(|x| x * x).sum();
        assert!(
            second < first,
            "feedback should decay: first={first}, second={second}"
        );
    }

    #[test]
    fn no_nan_at_boundary_params() {
        let mut d = Delay::new(44100.0);
        for &time_ms in &[1.0_f32, 1000.0, 2000.0] {
            for &feedback in &[0.0_f32, 0.5, 0.95] {
                for &mix in &[0.0_f32, 0.5, 1.0] {
                    let p = params(time_ms, feedback, mix);
                    for &x in &[-1.0_f32, 0.0, 1.0] {
                        let out = d.process(x, &p);
                        assert!(
                            out.is_finite(),
                            "NaN/Inf: time={time_ms}, fb={feedback}, mix={mix}, in={x}, out={out}"
                        );
                    }
                }
            }
        }
    }
}
