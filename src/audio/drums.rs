//! Lightweight synthesized drum machine (kick + hi-hats).
//!
//! This runs alongside the polyphonic voice pool so drum hits do not consume
//! melodic voice slots.

use crate::params::DrumHit;
use std::f32::consts::TAU;

const DRUM_GAIN: f32 = 0.85;

const KICK_START_HZ: f32 = 140.0;
const KICK_END_HZ: f32 = 50.0;
const KICK_PITCH_DECAY_SECONDS: f32 = 0.06;
const KICK_AMP_DECAY_SECONDS: f32 = 0.25;
const KICK_AMP_CUTOFF: f32 = 1.0e-4;

const HAT_CLOSED_DECAY_SECONDS: f32 = 0.05;
const HAT_OPEN_DECAY_SECONDS: f32 = 0.30;
const HAT_CLOSED_AMP_CUTOFF: f32 = 1.0e-4;
const HAT_OPEN_AMP_CUTOFF: f32 = 1.0e-4;

// One-pole HPF coefficient for ~7 kHz at 44.1 kHz.
const HAT_HPF_ALPHA: f32 = 0.864;

struct KickVoice {
    phase: f32,
    amp: f32,
    pitch: f32,
}

impl Default for KickVoice {
    fn default() -> Self {
        Self {
            phase: 0.0,
            amp: 0.0,
            pitch: KICK_START_HZ,
        }
    }
}

impl KickVoice {
    fn trigger(&mut self) {
        self.phase = 0.0;
        self.amp = 1.0;
        self.pitch = KICK_START_HZ;
    }

    fn panic(&mut self) {
        self.amp = 0.0;
    }

    fn process(&mut self, sample_rate: f32) -> f32 {
        if self.amp <= KICK_AMP_CUTOFF {
            self.amp = 0.0;
            return 0.0;
        }

        let pitch_coeff = (-1.0 / (KICK_PITCH_DECAY_SECONDS * sample_rate)).exp();
        let amp_coeff = (-1.0 / (KICK_AMP_DECAY_SECONDS * sample_rate)).exp();

        self.pitch = KICK_END_HZ + (self.pitch - KICK_END_HZ) * pitch_coeff;
        self.amp *= amp_coeff;

        self.phase += self.pitch / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }

        (TAU * self.phase).sin() * self.amp
    }
}

struct HatVoice {
    amp_closed: f32,
    amp_open: f32,
    noise_lfsr: u32,
    hpf_prev_in: f32,
    hpf_prev_out: f32,
}

impl Default for HatVoice {
    fn default() -> Self {
        Self {
            amp_closed: 0.0,
            amp_open: 0.0,
            noise_lfsr: 0xACE1_FEED,
            hpf_prev_in: 0.0,
            hpf_prev_out: 0.0,
        }
    }
}

impl HatVoice {
    fn trigger_closed(&mut self) {
        self.amp_open = 0.0;
        self.amp_closed = 1.0;
    }

    fn trigger_open(&mut self) {
        self.amp_closed = 0.0;
        self.amp_open = 1.0;
    }

    fn panic(&mut self) {
        self.amp_closed = 0.0;
        self.amp_open = 0.0;
    }

    #[allow(clippy::cast_precision_loss)]
    fn tick_lfsr(&mut self) -> f32 {
        let bit = self.noise_lfsr & 1;
        self.noise_lfsr >>= 1;
        if bit != 0 {
            self.noise_lfsr ^= 0xB4BC_D35C;
        }
        self.noise_lfsr.cast_signed() as f32 / 2_147_483_648.0
    }

    fn process(&mut self, sample_rate: f32) -> f32 {
        let active_closed = self.amp_closed > HAT_CLOSED_AMP_CUTOFF;
        let active_open = self.amp_open > HAT_OPEN_AMP_CUTOFF;
        if !active_closed && !active_open {
            self.amp_closed = 0.0;
            self.amp_open = 0.0;
            return 0.0;
        }

        let closed_coeff = (-1.0 / (HAT_CLOSED_DECAY_SECONDS * sample_rate)).exp();
        let open_coeff = (-1.0 / (HAT_OPEN_DECAY_SECONDS * sample_rate)).exp();

        self.amp_closed *= closed_coeff;
        self.amp_open *= open_coeff;

        if self.amp_closed <= HAT_CLOSED_AMP_CUTOFF {
            self.amp_closed = 0.0;
        }
        if self.amp_open <= HAT_OPEN_AMP_CUTOFF {
            self.amp_open = 0.0;
        }

        let noise = self.tick_lfsr();
        let filtered = HAT_HPF_ALPHA * (self.hpf_prev_out + noise - self.hpf_prev_in);
        self.hpf_prev_in = noise;
        self.hpf_prev_out = filtered;

        (self.amp_closed + self.amp_open) * filtered
    }
}

/// Drum one-shot synth, mixed as a parallel engine.
#[derive(Default)]
pub struct DrumMachine {
    kick: KickVoice,
    hats: HatVoice,
}

impl DrumMachine {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn trigger(&mut self, hit: DrumHit) {
        match hit {
            DrumHit::Kick => self.kick.trigger(),
            DrumHit::HiHatClosed => self.hats.trigger_closed(),
            DrumHit::HiHatOpen => self.hats.trigger_open(),
        }
    }

    #[must_use]
    pub fn process(&mut self, sample_rate: f32) -> f32 {
        let kick = self.kick.process(sample_rate);
        let hats = self.hats.process(sample_rate);
        (kick + hats) * DRUM_GAIN
    }

    pub fn panic(&mut self) {
        self.kick.panic();
        self.hats.panic();
    }
}
