//! Oscillator, LFO, and supporting functions for pitch conversion.
//!
//! The `Oscillator` implements five waveform shapes with an integrated LFSR
//! noise source clocked at the oscillator period boundary (SID-style behaviour).

use crate::params::Waveform;
use std::f32::consts::TAU;

/// Single oscillator with LFSR-based noise (SID-style: LFSR clocked at osc frequency).
pub struct Oscillator {
    /// Current oscillator phase, normalised to 0.0 .. 1.0.
    phase: f32,
    /// 32-bit Galois LFSR state (feedback polynomial 0xB4BCD35C).
    noise_lfsr: u32,
    /// Most recent LFSR output, held between period boundaries.
    last_noise: f32,
}

impl Default for Oscillator {
    /// Create an oscillator with a non-zero LFSR seed to avoid the zero-lock state.
    fn default() -> Self {
        Self {
            phase: 0.0,
            noise_lfsr: 0xACE1_FEED,
            last_noise: 0.0,
        }
    }
}

impl Oscillator {
    /// Reset the phase accumulator to zero (useful for hard-sync effects).
    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Returns the next sample in the range -1.0 .. 1.0.
    ///
    /// * `freq_hz`     – instantaneous frequency (already LFO-modulated if needed)
    /// * `pulse_width` – 0.05 .. 0.95 (only relevant for Pulse / `PulseSaw`)
    /// * `noise_mix`   – blend pure oscillator with raw LFSR noise
    pub fn next_sample(
        &mut self,
        freq_hz: f32,
        sample_rate: f32,
        waveform: Waveform,
        pulse_width: f32,
        noise_mix: f32,
    ) -> f32 {
        let inc = freq_hz / sample_rate;
        self.phase += inc;

        // Phase wrap – tick LFSR on oscillator period boundary (SID behaviour)
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.last_noise = self.tick_lfsr();
        }

        let p = self.phase;

        let osc = match waveform {
            Waveform::Pulse => {
                if p < pulse_width {
                    1.0_f32
                } else {
                    -1.0_f32
                }
            }
            Waveform::Sawtooth => 2.0 * p - 1.0,
            Waveform::Triangle => {
                if p < 0.5 {
                    4.0 * p - 1.0
                } else {
                    3.0 - 4.0 * p
                }
            }
            Waveform::Noise => self.last_noise,
            Waveform::PulseSaw => {
                let pulse = if p < pulse_width { 1.0_f32 } else { -1.0_f32 };
                let saw = 2.0 * p - 1.0;
                (pulse + saw) * 0.5
            }
        };

        // Blend oscillator output with raw noise
        if noise_mix > 0.001 {
            osc * (1.0 - noise_mix) + self.last_noise * noise_mix
        } else {
            osc
        }
    }

    /// Advance the 32-bit Galois LFSR by one step and return a sample in -1..1.
    ///
    /// Feedback polynomial: 0xB4BCD35C.  The LFSR is clocked once per oscillator
    /// period (phase wrap), matching the SID chip's noise behaviour.
    #[allow(clippy::cast_precision_loss)] // deliberate DSP normalisation; precision loss is acceptable
    fn tick_lfsr(&mut self) -> f32 {
        let bit = self.noise_lfsr & 1;
        self.noise_lfsr >>= 1;
        if bit != 0 {
            self.noise_lfsr ^= 0xB4BC_D35C;
        }
        // Map u32 → -1..1 via signed reinterpretation (intentional wrapping cast)
        self.noise_lfsr.cast_signed() as f32 / 2_147_483_648.0
    }
}

/// Convert a MIDI note number to Hz (A4 = 69 = 440 Hz).
#[inline]
pub fn midi_to_hz(midi: u8) -> f32 {
    440.0 * 2.0_f32.powf((f32::from(midi) - 69.0) / 12.0)
}

/// Apply detune in cents to a base frequency.
#[inline]
pub fn detune_hz(base_hz: f32, cents: f32) -> f32 {
    base_hz * 2.0_f32.powf(cents / 1200.0)
}

/// Simple sine-wave LFO.
pub struct Lfo {
    /// Current LFO phase, normalised to 0.0 .. 1.0.
    phase: f32,
}

impl Default for Lfo {
    /// Create an LFO starting at phase zero.
    fn default() -> Self {
        Self { phase: 0.0 }
    }
}

impl Lfo {
    /// Advance the LFO by one sample and return a value in -1.0 .. 1.0.
    pub fn next(&mut self, rate_hz: f32, sample_rate: f32) -> f32 {
        self.phase += rate_hz / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        (TAU * self.phase).sin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc_sawtooth_bounds() {
        let mut osc = Oscillator::default();
        for _ in 0..4410 {
            let s = osc.next_sample(440.0, 44100.0, Waveform::Sawtooth, 0.5, 0.0);
            assert!((-1.0..=1.0).contains(&s), "sawtooth out of bounds: {s}");
        }
    }

    #[test]
    #[allow(clippy::float_cmp)] // pulse wave output is exactly ±1.0 by construction
    fn osc_pulse_bounds() {
        let mut osc = Oscillator::default();
        for _ in 0..4410 {
            let s = osc.next_sample(440.0, 44100.0, Waveform::Pulse, 0.5, 0.0);
            assert!(s == 1.0 || s == -1.0);
        }
    }

    #[test]
    fn midi_to_hz_a4() {
        let hz = midi_to_hz(69);
        assert!((hz - 440.0).abs() < 0.01);
    }

    #[test]
    fn midi_to_hz_c4() {
        let hz = midi_to_hz(60);
        assert!((hz - 261.626).abs() < 0.1);
    }
}
