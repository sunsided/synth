//! Bitcrusher: bit depth reduction and sample rate decimation.
//!
//! Runs before the filter in the voice signal chain so quantization harmonics
//! are shaped by filter resonance (matches SID DAC-before-filter architecture).

/// Per-voice bitcrusher combining bit depth reduction with sample rate decimation.
pub struct Bitcrusher {
    held_sample: f32,
    counter: u32,
}

impl Default for Bitcrusher {
    fn default() -> Self {
        Self {
            held_sample: 0.0,
            counter: 0,
        }
    }
}

impl Bitcrusher {
    /// Reset state — call on voice steal or panic.
    pub fn reset(&mut self) {
        self.held_sample = 0.0;
        self.counter = 0;
    }

    /// Process one sample.
    ///
    /// * `input` - sample in -1.0..1.0
    /// * `bits`  - bit depth 1.0..=16.0; 16.0 = pass-through
    /// * `rate`  - sample rate divider 1.0..=16.0; 1.0 = pass-through
    pub fn process(&mut self, input: f32, bits: f32, rate: f32) -> f32 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let divider = (rate.floor() as u32).max(1);
        if self.counter == 0 {
            self.held_sample = crush(input, bits);
        }
        self.counter = (self.counter + 1) % divider;
        self.held_sample
    }
}

#[inline]
fn crush(input: f32, bits: f32) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let n = bits.floor().clamp(1.0, 16.0) as i32;
    let levels = 2.0f32.powi(n);
    let half = levels * 0.5;
    (input * half).round() / half
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_16bit() {
        let mut c = Bitcrusher::default();
        for &x in &[-1.0_f32, -0.5, 0.0, 0.5, 1.0] {
            let out = c.process(x, 16.0, 1.0);
            assert!(
                (out - x).abs() < 1e-4,
                "passthrough failed: in={x}, out={out}"
            );
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn one_bit_sign_only() {
        let mut c = Bitcrusher::default();
        // At 1-bit: levels=2, half=1; crush = round(x * 1) / 1 = round(x)
        // Inputs clearly above/below the ±0.5 threshold produce ±1.0
        for &(x, expected) in &[(-1.0_f32, -1.0_f32), (-0.6, -1.0), (0.6, 1.0), (1.0, 1.0)] {
            let out = c.process(x, 1.0, 1.0);
            assert_eq!(
                out, expected,
                "1-bit: in={x}, expected={expected}, out={out}"
            );
        }
    }

    #[test]
    fn quantization_grid() {
        let mut c = Bitcrusher::default();
        // At 8-bit: levels=256, half=128; step size = 1/128
        let step = 1.0_f32 / 128.0;
        for i in 0..=200 {
            #[allow(clippy::cast_precision_loss)]
            let x = (i as f32 / 100.0) - 1.0; // -1.0..=1.0
            let out = c.process(x, 8.0, 1.0);
            let remainder = (out / step) - (out / step).round();
            assert!(
                remainder.abs() < 1e-3,
                "not on 8-bit grid: in={x}, out={out}"
            );
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn decimation_holds() {
        let mut c = Bitcrusher::default();
        // rate=4: counter starts at 0, updates on calls where counter==0,
        // then holds for 3 more calls before updating again.
        // At bits=16 (pass-through quantization):
        //   call 0 (x=0.1): counter==0 → update held≈0.1; counter→1; return ≈0.1
        //   call 1 (x=0.2): counter==1 → skip; counter→2; return ≈0.1
        //   call 2 (x=0.3): counter==2 → skip; counter→3; return ≈0.1
        //   call 3 (x=0.4): counter==3 → skip; counter→0; return ≈0.1
        //   call 4 (x=0.5): counter==0 → update held≈0.5; counter→1; return ≈0.5
        let inputs = [0.1_f32, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
        let outputs: Vec<f32> = inputs.iter().map(|&x| c.process(x, 16.0, 4.0)).collect();
        // First group (calls 0-3): all equal to crushed(0.1)
        assert_eq!(outputs[0], outputs[1], "hold failed: call 0 and 1 differ");
        assert_eq!(outputs[0], outputs[2], "hold failed: call 0 and 2 differ");
        assert_eq!(outputs[0], outputs[3], "hold failed: call 0 and 3 differ");
        // Second group (calls 4-7): all equal to crushed(0.5)
        assert_eq!(outputs[4], outputs[5], "hold failed: call 4 and 5 differ");
        assert_eq!(outputs[4], outputs[6], "hold failed: call 4 and 6 differ");
        assert_eq!(outputs[4], outputs[7], "hold failed: call 4 and 7 differ");
        // The two groups must differ (different input values)
        assert_ne!(outputs[0], outputs[4], "groups should differ");
    }

    #[test]
    fn decimation_rate1_tracks() {
        let mut c = Bitcrusher::default();
        // rate=1: every call updates (counter==0 every call since (0+1)%1==0)
        for &x in &[0.1_f32, 0.5, -0.3, 0.8, -0.9] {
            let out = c.process(x, 16.0, 1.0);
            assert!(
                (out - x).abs() < 1e-4,
                "rate=1 should track every sample: in={x}, out={out}"
            );
        }
    }

    #[test]
    fn no_nan_extreme_params() {
        let mut c = Bitcrusher::default();
        for &bits in &[1.0_f32, 8.0, 16.0] {
            for &rate in &[1.0_f32, 8.0, 16.0] {
                for &x in &[-1.0_f32, 0.0, 1.0] {
                    let out = c.process(x, bits, rate);
                    assert!(
                        out.is_finite(),
                        "NaN/Inf: bits={bits}, rate={rate}, in={x}, out={out}"
                    );
                }
            }
        }
    }
}
