//! Lightweight synthesized drum machine (kick + hi-hats).
//!
//! This runs alongside the polyphonic voice pool so drum hits do not consume
//! melodic voice slots.

use crate::params::DrumHit;
use std::f32::consts::{FRAC_1_SQRT_2, FRAC_PI_4, TAU};

/// Global drum bus gain applied after summing kick and hats.
const DRUM_GAIN: f32 = 0.85;

/// Kick oscillator start frequency in Hz.
const KICK_START_HZ: f32 = 140.0;
/// Kick oscillator target end frequency in Hz.
const KICK_END_HZ: f32 = 50.0;
/// Kick pitch envelope decay time in seconds.
const KICK_PITCH_DECAY_SECONDS: f32 = 0.06;
/// Kick amplitude envelope decay time in seconds.
const KICK_AMP_DECAY_SECONDS: f32 = 0.25;
/// Kick amplitude threshold below which the voice is considered idle.
const KICK_AMP_CUTOFF: f32 = 1.0e-4;

/// Closed hat amplitude envelope decay time in seconds.
const HAT_CLOSED_DECAY_SECONDS: f32 = 0.05;
/// Open hat amplitude envelope decay time in seconds.
const HAT_OPEN_DECAY_SECONDS: f32 = 0.30;
/// Closed hat amplitude threshold below which the voice is considered idle.
const HAT_CLOSED_AMP_CUTOFF: f32 = 1.0e-4;
/// Open hat amplitude threshold below which the voice is considered idle.
const HAT_OPEN_AMP_CUTOFF: f32 = 1.0e-4;

/// One-pole HPF coefficient for approximately 7 kHz at 44.1 kHz.
const HAT_CLOSED_HPF_ALPHA: f32 = 0.864;
/// One-pole HPF coefficient for approximately 5 kHz at 44.1 kHz.
const HAT_OPEN_HPF_ALPHA: f32 = 0.737;

fn sanitize_pan(pan: f32) -> f32 {
    if pan.is_finite() {
        pan.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

fn pan_gains(pan: f32) -> (f32, f32) {
    debug_assert!(pan.is_finite());
    debug_assert!((-1.0..=1.0).contains(&pan));
    let angle = (pan + 1.0) * FRAC_PI_4;
    (angle.cos(), angle.sin())
}

/// Monophonic kick drum voice with pitch and amplitude decays.
struct KickVoice {
    /// Oscillator phase in normalized 0.0..1.0 units.
    phase: f32,
    /// Current amplitude envelope value.
    amp: f32,
    /// Current oscillator frequency in Hz.
    pitch: f32,
    /// Precomputed per-sample pitch decay multiplier.
    pitch_coeff: f32,
    /// Precomputed per-sample amplitude decay multiplier.
    amp_coeff: f32,
    /// Pan position in -1.0..=1.0.
    pan: f32,
    /// Cached left gain for equal-power pan.
    l_gain: f32,
    /// Cached right gain for equal-power pan.
    r_gain: f32,
}

impl Default for KickVoice {
    /// Create a silent kick voice with neutral decay coefficients.
    fn default() -> Self {
        Self {
            phase: 0.0,
            amp: 0.0,
            pitch: KICK_START_HZ,
            pitch_coeff: 1.0,
            amp_coeff: 1.0,
            pan: 0.0,
            l_gain: FRAC_1_SQRT_2,
            r_gain: FRAC_1_SQRT_2,
        }
    }
}

impl KickVoice {
    /// Start a new kick hit by resetting phase and envelopes.
    fn trigger(&mut self, pan: f32) {
        self.phase = 0.0;
        self.amp = 1.0;
        self.pitch = KICK_START_HZ;
        self.pan = sanitize_pan(pan);
        (self.l_gain, self.r_gain) = pan_gains(self.pan);
    }

    /// Immediately silence the kick voice.
    fn panic(&mut self) {
        self.amp = 0.0;
    }

    /// Recompute per-sample decay coefficients for a given sample rate.
    fn update_coefficients(&mut self, sample_rate: f32) {
        self.pitch_coeff = (-1.0 / (KICK_PITCH_DECAY_SECONDS * sample_rate)).exp();
        self.amp_coeff = (-1.0 / (KICK_AMP_DECAY_SECONDS * sample_rate)).exp();
    }

    /// Render one kick sample.
    fn process(&mut self, sample_rate: f32) -> f32 {
        if self.amp <= KICK_AMP_CUTOFF {
            self.amp = 0.0;
            return 0.0;
        }

        self.pitch = KICK_END_HZ + (self.pitch - KICK_END_HZ) * self.pitch_coeff;
        self.amp *= self.amp_coeff;

        self.phase += self.pitch / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }

        (TAU * self.phase).sin() * self.amp
    }
}

/// Shared noise hi-hat voice with mutually exclusive closed/open envelopes.
struct HatVoice {
    /// Closed hat envelope amplitude.
    amp_closed: f32,
    /// Open hat envelope amplitude.
    amp_open: f32,
    /// Hi-hat pan in -1.0..=1.0.
    pan: f32,
    /// Cached left gain for equal-power pan.
    l_gain: f32,
    /// Cached right gain for equal-power pan.
    r_gain: f32,
    /// 32-bit Galois LFSR state used as white-ish noise source.
    noise_lfsr: u32,
    /// Precomputed per-sample closed hat decay multiplier.
    closed_coeff: f32,
    /// Precomputed per-sample open hat decay multiplier.
    open_coeff: f32,
    /// Previous input sample for closed-hat HPF.
    hpf_closed_prev_in: f32,
    /// Previous output sample for closed-hat HPF.
    hpf_closed_prev_out: f32,
    /// Previous input sample for open-hat HPF.
    hpf_open_prev_in: f32,
    /// Previous output sample for open-hat HPF.
    hpf_open_prev_out: f32,
}

impl Default for HatVoice {
    /// Create a silent hi-hat voice with reset filter states.
    fn default() -> Self {
        Self {
            amp_closed: 0.0,
            amp_open: 0.0,
            pan: 0.0,
            l_gain: FRAC_1_SQRT_2,
            r_gain: FRAC_1_SQRT_2,
            noise_lfsr: 0xACE1_FEED,
            closed_coeff: 1.0,
            open_coeff: 1.0,
            hpf_closed_prev_in: 0.0,
            hpf_closed_prev_out: 0.0,
            hpf_open_prev_in: 0.0,
            hpf_open_prev_out: 0.0,
        }
    }
}

impl HatVoice {
    /// Trigger a closed hat and choke any currently ringing open hat.
    fn trigger_closed(&mut self, pan: f32) {
        self.amp_open = 0.0;
        self.amp_closed = 1.0;
        self.pan = sanitize_pan(pan);
        (self.l_gain, self.r_gain) = pan_gains(self.pan);
    }

    /// Trigger an open hat and choke any currently sounding closed hat.
    fn trigger_open(&mut self, pan: f32) {
        self.amp_closed = 0.0;
        self.amp_open = 1.0;
        self.pan = sanitize_pan(pan);
        (self.l_gain, self.r_gain) = pan_gains(self.pan);
    }

    /// Immediately silence both hi-hat envelopes.
    fn panic(&mut self) {
        self.amp_closed = 0.0;
        self.amp_open = 0.0;
    }

    /// Recompute per-sample decay coefficients for a given sample rate.
    fn update_coefficients(&mut self, sample_rate: f32) {
        self.closed_coeff = (-1.0 / (HAT_CLOSED_DECAY_SECONDS * sample_rate)).exp();
        self.open_coeff = (-1.0 / (HAT_OPEN_DECAY_SECONDS * sample_rate)).exp();
    }

    /// Advance the LFSR one step and map the value to the -1.0..1.0 range.
    #[allow(clippy::cast_precision_loss)]
    fn tick_lfsr(&mut self) -> f32 {
        let bit = self.noise_lfsr & 1;
        self.noise_lfsr >>= 1;
        if bit != 0 {
            self.noise_lfsr ^= 0xB4BC_D35C;
        }
        self.noise_lfsr.cast_signed() as f32 / 2_147_483_648.0
    }

    /// Render one hi-hat sample by combining closed/open paths.
    fn process(&mut self) -> f32 {
        let active_closed = self.amp_closed > HAT_CLOSED_AMP_CUTOFF;
        let active_open = self.amp_open > HAT_OPEN_AMP_CUTOFF;
        if !active_closed && !active_open {
            self.amp_closed = 0.0;
            self.amp_open = 0.0;
            return 0.0;
        }

        self.amp_closed *= self.closed_coeff;
        self.amp_open *= self.open_coeff;

        if self.amp_closed <= HAT_CLOSED_AMP_CUTOFF {
            self.amp_closed = 0.0;
        }
        if self.amp_open <= HAT_OPEN_AMP_CUTOFF {
            self.amp_open = 0.0;
        }

        let noise = self.tick_lfsr();

        let filtered_closed =
            HAT_CLOSED_HPF_ALPHA * (self.hpf_closed_prev_out + noise - self.hpf_closed_prev_in);
        self.hpf_closed_prev_in = noise;
        self.hpf_closed_prev_out = filtered_closed;

        let filtered_open =
            HAT_OPEN_HPF_ALPHA * (self.hpf_open_prev_out + noise - self.hpf_open_prev_in);
        self.hpf_open_prev_in = noise;
        self.hpf_open_prev_out = filtered_open;

        self.amp_closed * filtered_closed + self.amp_open * filtered_open
    }
}

/// Drum one-shot synth, mixed as a parallel engine.
#[derive(Default)]
pub struct DrumMachine {
    kick: KickVoice,
    hats: HatVoice,
}

impl DrumMachine {
    /// Create a new drum machine and precompute decay coefficients.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        let mut machine = Self::default();
        machine.kick.update_coefficients(sample_rate);
        machine.hats.update_coefficients(sample_rate);
        machine
    }

    /// Trigger a drum one-shot event.
    pub fn trigger(&mut self, hit: DrumHit, pan: f32) {
        let pan = sanitize_pan(pan);
        match hit {
            DrumHit::Kick => self.kick.trigger(pan),
            DrumHit::HiHatClosed => self.hats.trigger_closed(pan),
            DrumHit::HiHatOpen => self.hats.trigger_open(pan),
        }
    }

    /// Render one drum sample at the given sample rate.
    #[must_use]
    pub fn process(&mut self, sample_rate: f32) -> f32 {
        let (kick, hats) = self.process_components(sample_rate);
        kick.0 + hats.0
    }

    /// Render per-type drum components and cached pan gains.
    #[must_use]
    pub fn process_components(&mut self, sample_rate: f32) -> ((f32, f32, f32), (f32, f32, f32)) {
        let kick = self.kick.process(sample_rate);
        let hats = self.hats.process();
        (
            (kick * DRUM_GAIN, self.kick.l_gain, self.kick.r_gain),
            (hats * DRUM_GAIN, self.hats.l_gain, self.hats.r_gain),
        )
    }

    /// Immediately silence all active drum voices.
    pub fn panic(&mut self) {
        self.kick.panic();
        self.hats.panic();
    }
}
