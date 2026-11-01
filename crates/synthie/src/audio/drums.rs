//! Lightweight synthesized drum machine: kick, snare, hi-hats, crash, toms and
//! clap.
//!
//! This runs alongside the polyphonic voice pool so drum hits do not consume
//! melodic voice slots. Every voice is synthesized from oscillators and noise;
//! there are no samples.

use crate::params::DrumHit;
use core::f32::consts::TAU;

/// Global drum bus gain applied after summing all drum voices.
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

/// Amplitude below which a voice is considered idle.
const AMP_CUTOFF: f32 = 1.0e-4;

/// Snare body start frequency in Hz.
const SNARE_BODY_START_HZ: f32 = 200.0;
/// Snare body end frequency in Hz.
const SNARE_BODY_END_HZ: f32 = 180.0;
/// Snare body pitch envelope decay time in seconds.
const SNARE_BODY_PITCH_DECAY_SECONDS: f32 = 0.03;
/// Snare body (tone) amplitude decay time in seconds.
const SNARE_BODY_DECAY_SECONDS: f32 = 0.05;
/// Snare rattle (noise) amplitude decay time in seconds.
const SNARE_NOISE_DECAY_SECONDS: f32 = 0.09;
/// Snare body level.
const SNARE_BODY_LEVEL: f32 = 0.6;
/// Snare noise level.
const SNARE_NOISE_LEVEL: f32 = 0.6;
/// Cutoff of the high-pass that takes the mud out of the snare noise, in Hz.
const SNARE_NOISE_HPF_HZ: f32 = 1_200.0;

/// Crash amplitude decay time in seconds (inaudible after about 2 s).
const CRASH_DECAY_SECONDS: f32 = 0.42;
/// Crash level; a crash is long and dense, so it sits well below the kick.
const CRASH_LEVEL: f32 = 0.5;
/// Base clock of the crash's sample-and-hold noise in Hz.
const CRASH_NOISE_CLOCK_HZ: f32 = 17_000.0;
/// Rate of the shimmer that wobbles the crash's noise clock, in Hz.
const CRASH_SHIMMER_HZ: f32 = 6.0;
/// Depth of the shimmer as a fraction of the noise clock.
const CRASH_SHIMMER_DEPTH: f32 = 0.3;
/// Cutoff of the high-pass that keeps the crash airy, in Hz.
const CRASH_HPF_HZ: f32 = 3_000.0;

/// Tom pitch envelope decay time in seconds.
const TOM_PITCH_DECAY_SECONDS: f32 = 0.08;
/// Tom amplitude decay time in seconds.
const TOM_AMP_DECAY_SECONDS: f32 = 0.22;
/// Tom level.
const TOM_LEVEL: f32 = 0.85;
/// Ratio between a tom's start and end frequency.
const TOM_SWEEP_RATIO: f32 = 1.6;
/// End frequency of the low tom in Hz.
const TOM_LOW_HZ: f32 = 90.0;
/// End frequency of the mid tom in Hz.
const TOM_MID_HZ: f32 = 130.0;
/// End frequency of the high tom in Hz.
const TOM_HIGH_HZ: f32 = 180.0;

/// Number of rapid noise bursts at the start of a clap.
const CLAP_BURSTS: u32 = 3;
/// Spacing between clap bursts in seconds.
const CLAP_BURST_SPACING_SECONDS: f32 = 0.011;
/// Decay of one clap burst in seconds.
const CLAP_BURST_DECAY_SECONDS: f32 = 0.006;
/// Decay of the clap's reverberant tail in seconds.
const CLAP_TAIL_DECAY_SECONDS: f32 = 0.09;
/// Clap level at 44.1 kHz; the narrow band-pass removes most of the noise
/// energy, so the gain makes up for it.
const CLAP_LEVEL: f32 = 3.2;
/// Centre of the clap's band-pass in Hz.
const CLAP_BPF_HZ: f32 = 1_400.0;
/// Q of the clap's band-pass.
const CLAP_BPF_Q: f32 = 1.2;

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
        }
    }
}

impl KickVoice {
    /// Start a new kick hit by resetting phase and envelopes.
    fn trigger(&mut self) {
        self.phase = 0.0;
        self.amp = 1.0;
        self.pitch = KICK_START_HZ;
    }

    /// Immediately silence the kick voice.
    fn panic(&mut self) {
        self.amp = 0.0;
    }

    /// Recompute per-sample decay coefficients for a given sample rate.
    fn update_coefficients(&mut self, sample_rate: f32) {
        self.pitch_coeff = crate::math::expf(-1.0 / (KICK_PITCH_DECAY_SECONDS * sample_rate));
        self.amp_coeff = crate::math::expf(-1.0 / (KICK_AMP_DECAY_SECONDS * sample_rate));
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

        crate::math::sinf(TAU * self.phase) * self.amp
    }
}

/// Shared noise hi-hat voice with mutually exclusive closed/open envelopes.
struct HatVoice {
    /// Closed hat envelope amplitude.
    amp_closed: f32,
    /// Open hat envelope amplitude.
    amp_open: f32,
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
    fn trigger_closed(&mut self) {
        self.amp_open = 0.0;
        self.amp_closed = 1.0;
    }

    /// Trigger an open hat and choke any currently sounding closed hat.
    fn trigger_open(&mut self) {
        self.amp_closed = 0.0;
        self.amp_open = 1.0;
    }

    /// Immediately silence both hi-hat envelopes.
    fn panic(&mut self) {
        self.amp_closed = 0.0;
        self.amp_open = 0.0;
    }

    /// Recompute per-sample decay coefficients for a given sample rate.
    fn update_coefficients(&mut self, sample_rate: f32) {
        self.closed_coeff = crate::math::expf(-1.0 / (HAT_CLOSED_DECAY_SECONDS * sample_rate));
        self.open_coeff = crate::math::expf(-1.0 / (HAT_OPEN_DECAY_SECONDS * sample_rate));
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

/// Per-sample decay multiplier for an exponential decay with time constant
/// `seconds`.
fn decay_coeff(seconds: f32, sample_rate: f32) -> f32 {
    crate::math::expf(-1.0 / (seconds * sample_rate))
}

/// Coefficient of a one-pole high-pass with cutoff `hz`.
fn hpf_alpha(hz: f32, sample_rate: f32) -> f32 {
    let rc = 1.0 / (TAU * hz);
    let dt = 1.0 / sample_rate;
    rc / (rc + dt)
}

/// 32-bit Galois LFSR mapped to -1.0..1.0, the noise source of every voice.
struct Lfsr(u32);

impl Lfsr {
    /// Advance one step and map the state to -1.0..1.0.
    #[allow(clippy::cast_precision_loss)]
    fn next(&mut self) -> f32 {
        let bit = self.0 & 1;
        self.0 >>= 1;
        if bit != 0 {
            self.0 ^= 0xB4BC_D35C;
        }
        self.0.cast_signed() as f32 / 2_147_483_648.0
    }
}

/// One-pole high-pass filter.
#[derive(Default)]
struct OnePoleHpf {
    alpha: f32,
    prev_in: f32,
    prev_out: f32,
}

impl OnePoleHpf {
    fn process(&mut self, x: f32) -> f32 {
        let y = self.alpha * (self.prev_out + x - self.prev_in);
        self.prev_in = x;
        self.prev_out = y;
        y
    }

    fn reset(&mut self) {
        self.prev_in = 0.0;
        self.prev_out = 0.0;
    }
}

/// Snare: a short decaying tone body around 190 Hz plus a high-passed noise
/// rattle that lingers slightly longer.
struct SnareVoice {
    phase: f32,
    pitch: f32,
    body_amp: f32,
    noise_amp: f32,
    pitch_coeff: f32,
    body_coeff: f32,
    noise_coeff: f32,
    noise: Lfsr,
    hpf: OnePoleHpf,
}

impl Default for SnareVoice {
    fn default() -> Self {
        Self {
            phase: 0.0,
            pitch: SNARE_BODY_START_HZ,
            body_amp: 0.0,
            noise_amp: 0.0,
            pitch_coeff: 1.0,
            body_coeff: 1.0,
            noise_coeff: 1.0,
            noise: Lfsr(0x5EED_0001),
            hpf: OnePoleHpf::default(),
        }
    }
}

impl SnareVoice {
    fn trigger(&mut self) {
        self.phase = 0.0;
        self.pitch = SNARE_BODY_START_HZ;
        self.body_amp = 1.0;
        self.noise_amp = 1.0;
    }

    fn panic(&mut self) {
        self.body_amp = 0.0;
        self.noise_amp = 0.0;
        self.hpf.reset();
    }

    fn update_coefficients(&mut self, sample_rate: f32) {
        self.pitch_coeff = decay_coeff(SNARE_BODY_PITCH_DECAY_SECONDS, sample_rate);
        self.body_coeff = decay_coeff(SNARE_BODY_DECAY_SECONDS, sample_rate);
        self.noise_coeff = decay_coeff(SNARE_NOISE_DECAY_SECONDS, sample_rate);
        self.hpf.alpha = hpf_alpha(SNARE_NOISE_HPF_HZ, sample_rate);
    }

    fn is_active(&self) -> bool {
        self.body_amp > 0.0 || self.noise_amp > 0.0
    }

    fn process(&mut self, sample_rate: f32) -> f32 {
        if !self.is_active() {
            return 0.0;
        }

        self.pitch = SNARE_BODY_END_HZ + (self.pitch - SNARE_BODY_END_HZ) * self.pitch_coeff;
        self.phase += self.pitch / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }
        let body = crate::math::sinf(TAU * self.phase) * self.body_amp;
        let rattle = self.hpf.process(self.noise.next()) * self.noise_amp;

        self.body_amp *= self.body_coeff;
        self.noise_amp *= self.noise_coeff;
        if self.body_amp <= AMP_CUTOFF {
            self.body_amp = 0.0;
        }
        if self.noise_amp <= AMP_CUTOFF {
            self.noise_amp = 0.0;
        }

        body * SNARE_BODY_LEVEL + rattle * SNARE_NOISE_LEVEL
    }
}

/// Crash cymbal: a long, washy decay of sample-and-hold noise whose clock is
/// wobbled by a slow triangle, so the wash shimmers instead of hissing
/// statically.
struct CrashVoice {
    amp: f32,
    amp_coeff: f32,
    noise: Lfsr,
    /// Fraction of a noise-clock tick owed.
    clock_acc: f32,
    /// Currently held noise value.
    held: f32,
    /// Shimmer LFO phase in 0.0..1.0.
    shimmer_phase: f32,
    hpf: OnePoleHpf,
}

impl Default for CrashVoice {
    fn default() -> Self {
        Self {
            amp: 0.0,
            amp_coeff: 1.0,
            noise: Lfsr(0xC0FF_EE11),
            clock_acc: 0.0,
            held: 0.0,
            shimmer_phase: 0.0,
            hpf: OnePoleHpf::default(),
        }
    }
}

impl CrashVoice {
    fn trigger(&mut self) {
        self.amp = 1.0;
        self.shimmer_phase = 0.0;
    }

    fn panic(&mut self) {
        self.amp = 0.0;
        self.hpf.reset();
    }

    fn update_coefficients(&mut self, sample_rate: f32) {
        self.amp_coeff = decay_coeff(CRASH_DECAY_SECONDS, sample_rate);
        self.hpf.alpha = hpf_alpha(CRASH_HPF_HZ, sample_rate);
    }

    fn process(&mut self, sample_rate: f32) -> f32 {
        if self.amp <= 0.0 {
            return 0.0;
        }

        // Triangle LFO in -1..1.
        self.shimmer_phase += CRASH_SHIMMER_HZ / sample_rate;
        if self.shimmer_phase >= 1.0 {
            self.shimmer_phase -= 1.0;
        }
        let tri = 4.0 * (self.shimmer_phase - 0.5).abs() - 1.0;
        let clock = CRASH_NOISE_CLOCK_HZ * (1.0 + CRASH_SHIMMER_DEPTH * tri);

        self.clock_acc += clock / sample_rate;
        while self.clock_acc >= 1.0 {
            self.clock_acc -= 1.0;
            self.held = self.noise.next();
        }

        let out = self.hpf.process(self.held) * self.amp;
        self.amp *= self.amp_coeff;
        if self.amp <= AMP_CUTOFF {
            self.amp = 0.0;
        }
        out * CRASH_LEVEL
    }
}

/// Tom: a kick-like sine with a downward pitch sweep, retuned per hit.
/// Monophonic, so a fast fill chokes the previous tom like a real kit would
/// not, but like most drum machines do.
struct TomVoice {
    phase: f32,
    amp: f32,
    pitch: f32,
    end_hz: f32,
    pitch_coeff: f32,
    amp_coeff: f32,
}

impl Default for TomVoice {
    fn default() -> Self {
        Self {
            phase: 0.0,
            amp: 0.0,
            pitch: TOM_MID_HZ,
            end_hz: TOM_MID_HZ,
            pitch_coeff: 1.0,
            amp_coeff: 1.0,
        }
    }
}

impl TomVoice {
    fn trigger(&mut self, end_hz: f32) {
        self.phase = 0.0;
        self.amp = 1.0;
        self.end_hz = end_hz;
        self.pitch = end_hz * TOM_SWEEP_RATIO;
    }

    fn panic(&mut self) {
        self.amp = 0.0;
    }

    fn update_coefficients(&mut self, sample_rate: f32) {
        self.pitch_coeff = decay_coeff(TOM_PITCH_DECAY_SECONDS, sample_rate);
        self.amp_coeff = decay_coeff(TOM_AMP_DECAY_SECONDS, sample_rate);
    }

    fn process(&mut self, sample_rate: f32) -> f32 {
        if self.amp <= 0.0 {
            return 0.0;
        }

        self.pitch = self.end_hz + (self.pitch - self.end_hz) * self.pitch_coeff;
        self.phase += self.pitch / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }
        let out = crate::math::sinf(TAU * self.phase) * self.amp;

        self.amp *= self.amp_coeff;
        if self.amp <= AMP_CUTOFF {
            self.amp = 0.0;
        }
        out * TOM_LEVEL
    }
}

/// Clap: a few rapid band-passed noise bursts followed by a short tail.
struct ClapVoice {
    /// Samples since the hit, or `None` when idle.
    elapsed: Option<u32>,
    /// Spacing between bursts in samples.
    spacing: u32,
    amp: f32,
    burst_coeff: f32,
    tail_coeff: f32,
    noise: Lfsr,
    /// Band-pass (TPT state-variable filter) coefficients and state.
    svf_g: f32,
    svf_k: f32,
    svf_ic1: f32,
    svf_ic2: f32,
    /// Output gain: [`CLAP_LEVEL`] corrected for the sample rate (white noise
    /// spreads its power over `0..sr/2`, so a fixed band carries more of it at
    /// low sample rates).
    gain: f32,
}

impl Default for ClapVoice {
    fn default() -> Self {
        Self {
            elapsed: None,
            spacing: 1,
            amp: 0.0,
            burst_coeff: 1.0,
            tail_coeff: 1.0,
            noise: Lfsr(0xC1A9_0042),
            svf_g: 0.0,
            svf_k: 1.0 / CLAP_BPF_Q,
            svf_ic1: 0.0,
            svf_ic2: 0.0,
            gain: CLAP_LEVEL,
        }
    }
}

impl ClapVoice {
    fn trigger(&mut self) {
        self.elapsed = Some(0);
    }

    fn panic(&mut self) {
        self.elapsed = None;
        self.amp = 0.0;
        self.svf_ic1 = 0.0;
        self.svf_ic2 = 0.0;
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "spacing is a small positive sample count"
    )]
    fn update_coefficients(&mut self, sample_rate: f32) {
        self.spacing = ((CLAP_BURST_SPACING_SECONDS * sample_rate) as u32).max(1);
        self.burst_coeff = decay_coeff(CLAP_BURST_DECAY_SECONDS, sample_rate);
        self.tail_coeff = decay_coeff(CLAP_TAIL_DECAY_SECONDS, sample_rate);
        // Topology-preserving SVF: stable at any cutoff; keep it below
        // Nyquist for low sample rates.
        let hz = CLAP_BPF_HZ.min(sample_rate * 0.45);
        self.svf_g = crate::math::tanf(core::f32::consts::PI * hz / sample_rate);
        self.gain = CLAP_LEVEL * crate::math::sqrtf(sample_rate / 44_100.0);
    }

    fn process(&mut self) -> f32 {
        let Some(elapsed) = self.elapsed else {
            return 0.0;
        };

        // Retrigger the envelope at each burst; the last one decays slowly.
        let in_bursts = elapsed < CLAP_BURSTS * self.spacing;
        if elapsed % self.spacing == 0 && elapsed <= CLAP_BURSTS * self.spacing {
            self.amp = 1.0;
        }
        let coeff = if in_bursts {
            self.burst_coeff
        } else {
            self.tail_coeff
        };

        let x = self.noise.next();
        let (g, k) = (self.svf_g, self.svf_k);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let v1 = a1 * (self.svf_ic1 + g * (x - self.svf_ic2));
        let v2 = self.svf_ic2 + g * v1;
        self.svf_ic1 = 2.0 * v1 - self.svf_ic1;
        self.svf_ic2 = 2.0 * v2 - self.svf_ic2;
        // `k * v1` is the unity-gain band-pass.
        let out = k * v1 * self.amp;

        self.amp *= coeff;
        if !in_bursts && self.amp <= AMP_CUTOFF {
            self.panic();
        } else {
            self.elapsed = Some(elapsed + 1);
        }
        out * self.gain
    }
}

/// Drum one-shot synth, mixed as a parallel engine.
#[derive(Default)]
pub struct DrumMachine {
    kick: KickVoice,
    snare: SnareVoice,
    hats: HatVoice,
    crash: CrashVoice,
    tom: TomVoice,
    clap: ClapVoice,
}

impl DrumMachine {
    /// Create a new drum machine and precompute decay coefficients.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        let mut machine = Self::default();
        machine.kick.update_coefficients(sample_rate);
        machine.snare.update_coefficients(sample_rate);
        machine.hats.update_coefficients(sample_rate);
        machine.crash.update_coefficients(sample_rate);
        machine.tom.update_coefficients(sample_rate);
        machine.clap.update_coefficients(sample_rate);
        machine
    }

    /// Trigger a drum one-shot event.
    pub fn trigger(&mut self, hit: DrumHit) {
        match hit {
            DrumHit::Kick => self.kick.trigger(),
            DrumHit::Snare => self.snare.trigger(),
            DrumHit::HiHatClosed => self.hats.trigger_closed(),
            DrumHit::HiHatOpen => self.hats.trigger_open(),
            DrumHit::Crash => self.crash.trigger(),
            DrumHit::TomLow => self.tom.trigger(TOM_LOW_HZ),
            DrumHit::TomMid => self.tom.trigger(TOM_MID_HZ),
            DrumHit::TomHigh => self.tom.trigger(TOM_HIGH_HZ),
            DrumHit::Clap => self.clap.trigger(),
        }
    }

    /// Render one drum sample at the given sample rate.
    #[must_use]
    pub fn process(&mut self, sample_rate: f32) -> f32 {
        let sum = self.kick.process(sample_rate)
            + self.snare.process(sample_rate)
            + self.hats.process()
            + self.crash.process(sample_rate)
            + self.tom.process(sample_rate)
            + self.clap.process();
        sum * DRUM_GAIN
    }

    /// Whether any drum hit is still ringing.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.kick.amp > 0.0
            || self.snare.is_active()
            || self.hats.amp_closed > 0.0
            || self.hats.amp_open > 0.0
            || self.crash.amp > 0.0
            || self.tom.amp > 0.0
            || self.clap.elapsed.is_some()
    }

    /// Immediately silence all active drum voices.
    pub fn panic(&mut self) {
        self.kick.panic();
        self.snare.panic();
        self.hats.panic();
        self.crash.panic();
        self.tom.panic();
        self.clap.panic();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render `hit` until the machine goes idle; returns the samples.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn render(hit: DrumHit, sample_rate: f32) -> Vec<f32> {
        let mut machine = DrumMachine::new(sample_rate);
        machine.trigger(hit);
        let limit = (10.0 * sample_rate) as usize;
        let mut out = Vec::new();
        while machine.is_active() {
            out.push(machine.process(sample_rate));
            assert!(out.len() < limit, "{hit:?} still ringing after 10 s");
        }
        out
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0, |m, s| m.max(s.abs()))
    }

    #[allow(clippy::cast_precision_loss)]
    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn every_hit_is_audible_finite_and_below_full_scale() {
        for sample_rate in [8_000.0, 22_050.0, 44_100.0, 48_000.0, 96_000.0] {
            for hit in DrumHit::ALL {
                let out = render(hit, sample_rate);
                assert!(out.iter().all(|s| s.is_finite()), "{hit:?} @ {sample_rate}");
                let p = peak(&out);
                assert!(p > 0.2, "{hit:?} @ {sample_rate} too quiet: {p}");
                assert!(p < 1.0, "{hit:?} @ {sample_rate} clips the bus: {p}");
            }
        }
    }

    /// The kick anchors the kit; nothing else should be louder over its first
    /// 100 ms, and the snare must cut through the hats on the backbeat.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    #[test]
    fn levels_sit_around_the_kick() {
        const SR: f32 = 44_100.0;
        let window = (0.1 * SR) as usize;
        let level = |hit| {
            let out = render(hit, SR);
            rms(&out[..window.min(out.len())])
        };
        let kick = level(DrumHit::Kick);
        for hit in DrumHit::ALL {
            let l = level(hit);
            assert!(l <= kick, "{hit:?} ({l}) louder than the kick ({kick})");
            assert!(
                l > kick * 0.25,
                "{hit:?} ({l}) buried under the kick ({kick})"
            );
        }
        assert!(level(DrumHit::Snare) > level(DrumHit::HiHatClosed));
    }

    #[allow(clippy::cast_precision_loss)]
    #[test]
    fn the_crash_rings_for_about_two_seconds() {
        const SR: f32 = 44_100.0;
        let out = render(DrumHit::Crash, SR);
        // Last sample above -40 dBFS.
        let tail = out.iter().rposition(|s| s.abs() > 0.01).unwrap() as f32 / SR;
        assert!((1.2..=2.5).contains(&tail), "crash tail {tail} s");
        assert!(
            render(DrumHit::Snare, SR).len() < out.len() / 2,
            "snare should be short next to the crash"
        );
    }

    #[test]
    fn toms_are_pitched_low_to_high() {
        // Count zero crossings in the first 150 ms as a pitch proxy.
        let crossings = |hit| {
            let out = render(hit, 44_100.0);
            out[..6_615]
                .windows(2)
                .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
                .count()
        };
        let (low, mid, high) = (
            crossings(DrumHit::TomLow),
            crossings(DrumHit::TomMid),
            crossings(DrumHit::TomHigh),
        );
        assert!(low < mid && mid < high, "{low} {mid} {high}");
    }

    #[test]
    fn repeated_hits_sound_identical_after_panic() {
        for hit in DrumHit::ALL {
            let mut machine = DrumMachine::new(44_100.0);
            let take = |m: &mut DrumMachine| {
                m.trigger(hit);
                (0..2_048).map(|_| m.process(44_100.0)).collect::<Vec<_>>()
            };
            let first = take(&mut machine);
            machine.panic();
            assert!(!machine.is_active());
            let second = take(&mut machine);
            // Noise sources keep running, so compare energy, not samples.
            let (a, b) = (rms(&first), rms(&second));
            assert!((a - b).abs() < a * 0.2, "{hit:?}: {a} vs {b}");
        }
    }
}
