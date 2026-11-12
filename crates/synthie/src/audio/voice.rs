//! Monophonic voice pipeline.
//!
//! `Voice` connects oscillator, envelope, filter, and LFO in a single
//! sample-by-sample render path.

use crate::audio::{
    crusher::Bitcrusher,
    env::{EnvStage, Envelope},
    filter::SvFilter,
    osc::{Lfo, Oscillator, detune_hz, midi_to_hz},
};
use crate::params::{MidiNote, RingModMode, SfxParams, SynthParams};

/// Furthest a slide can take a note, in octaves either way. Keeps a long
/// slide from overflowing; the final frequency is clamped below Nyquist anyway.
const MAX_SLIDE_OCTAVES: f32 = 10.0;

/// Per-sample modulation accumulator.
///
/// All modulation sources (LFOs, envelopes) write additively into these slots.
/// `Voice::process()` applies them in one pass, keeping the signal chain readable.
#[derive(Default)]
struct ModBus {
    /// Pitch modulation, applied as `freq *= 2^(pitch * 0.1)`.
    /// LFO contribution: `lfo_val * depth`. Env contribution: `env_out * depth * 10`.
    pitch: f32,
    /// Pulse-width modulation, applied as `pw += pw_mod * 0.4` (clamped 0.05..0.95).
    pw: f32,
    /// Filter cutoff modulation, applied as `cutoff *= 2^(bus.cutoff * 2.0)`.
    cutoff: f32,
    /// Volume/tremolo modulation, applied as `amp *= (1 - volume * 0.5)`.
    volume: f32,
}

impl ModBus {
    /// Add an LFO contribution to the appropriate slot.
    fn add_lfo(&mut self, lfo_val: f32, depth: f32, target: crate::params::LfoTarget) {
        use crate::params::LfoTarget;
        let c = lfo_val * depth;
        match target {
            LfoTarget::Pitch => self.pitch += c,
            LfoTarget::PulseWidth => self.pw += c,
            LfoTarget::Cutoff => self.cutoff += c,
            LfoTarget::Volume => self.volume += c,
        }
    }
}

/// Whether a falling slide has taken `freq` below `min_freq` (0 = never),
/// which ends the note.
fn slid_below(freq: f32, slide_oct: f32, min_freq: f32) -> bool {
    min_freq > 0.0 && slide_oct < 0.0 && freq * crate::math::powf(2.0_f32, slide_oct) < min_freq
}

/// Monophonic synthesiser voice combining oscillator, envelope, filter, and LFO.
pub struct Voice {
    /// Whether the voice is currently producing sound.
    pub active: bool,
    /// MIDI note number of the current target pitch.
    pub target_note: MidiNote,
    /// Target frequency in Hz (includes detune).
    pub target_freq: f32,
    /// Current glide-smoothed frequency in Hz.
    pub current_freq: f32,
    /// Waveform generator.
    pub osc: Oscillator,
    /// Second oscillator for unison/detune/hard-sync effects.
    pub osc2: Oscillator,
    /// Bitcrusher (bit depth and sample rate reduction).
    pub crusher: Bitcrusher,
    /// Amplitude envelope.
    pub env: Envelope,
    /// State-variable filter.
    pub filter: SvFilter,
    /// Low-frequency oscillator.
    pub lfo: Lfo,
    /// Second low-frequency oscillator.
    pub lfo2: Lfo,
    /// Modulation envelope for filter cutoff.
    pub filter_env: Envelope,
    /// Modulation envelope for oscillator pitch.
    pub pitch_env: Envelope,
    /// Per-sample portamento smoothing coefficient (0 = instant, ~1 = very slow).
    pub glide_coeff: f32,
    /// Samples since note-on or the last sound-effect repeat; drives the
    /// slide, pitch jump and pulse-width sweep in `params.sfx`.
    sfx_samples: u32,
}

impl Default for Voice {
    fn default() -> Self {
        Self::new()
    }
}

impl Voice {
    /// Construct a voice with default state at A4 (440 Hz).
    #[must_use]
    pub fn new() -> Self {
        Self {
            active: false,
            target_note: MidiNote::A4,
            target_freq: 440.0,
            current_freq: 440.0,
            osc: Oscillator::default(),
            osc2: Oscillator::default(),
            crusher: Bitcrusher::default(),
            env: Envelope::default(),
            filter: SvFilter::default(),
            lfo: Lfo::default(),
            lfo2: Lfo::seeded(0xDEAD_BEEF),
            filter_env: Envelope::default(),
            pitch_env: Envelope::default(),
            glide_coeff: 0.0,
            sfx_samples: 0,
        }
    }

    /// Recompute the portamento smoothing coefficient from glide time and sample rate.
    pub fn update_glide(&mut self, glide_time: f32, sample_rate: f32) {
        self.glide_coeff = if glide_time < 1e-4 {
            0.0
        } else {
            crate::math::expf(-1.0_f32 / (glide_time * sample_rate))
        };
    }

    /// Start a new note (or retrigger legato if the voice is already active).
    pub fn note_on(&mut self, note: impl Into<MidiNote>, params: &SynthParams, sample_rate: f32) {
        let note = note.into();
        let legato = self.active;
        self.target_note = note;
        let base = midi_to_hz(note);
        self.target_freq = detune_hz(base, params.osc.detune);
        if !legato {
            // No glide on fresh attacks – snap to pitch immediately.
            self.current_freq = self.target_freq;
            self.crusher.reset();
        }
        self.active = true;
        self.sfx_samples = 0;
        self.update_glide(params.global.glide_time, sample_rate);
        self.env.note_on(legato);
        self.filter_env.note_on(legato);
        self.pitch_env.note_on(legato);
    }

    /// Begin the envelope release phase.
    pub fn note_off(&mut self) {
        self.env.note_off();
        self.filter_env.note_off();
        self.pitch_env.note_off();
    }

    /// Immediately silence the voice and reset DSP state (all-notes-off / panic).
    pub fn panic(&mut self) {
        // A full reset, not just the envelopes: glide frequency, oscillator
        // and LFO phases carry over into the next note, so a non-finite
        // value left in any of them would never clear. `Voice` holds no heap
        // data, so this does not allocate.
        *self = Self::new();
    }

    /// Advance the sound-effect clock by one sample, restarting it on each
    /// repeat. Returns the seconds since note-on or the last repeat, the
    /// slide's pitch offset and the pitch jump's offset, both in octaves.
    fn sfx_step(&mut self, sfx: &SfxParams, sample_rate: f32) -> (f32, f32, f32) {
        #[allow(clippy::cast_precision_loss)] // exact for ~6 minutes, then coarser
        let mut t = self.sfx_samples as f32 / sample_rate;
        if sfx.repeat_rate > 0.0 && t * sfx.repeat_rate >= 1.0 {
            t = 0.0;
            self.sfx_samples = 0;
        }
        self.sfx_samples = self.sfx_samples.saturating_add(1);
        // Closed form of a slide whose rate changes linearly over time.
        let slide_oct = (sfx.slide * t + 0.5 * sfx.slide_accel * t * t)
            .clamp(-MAX_SLIDE_OCTAVES, MAX_SLIDE_OCTAVES);
        let jump_oct = if t >= sfx.jump_time {
            sfx.jump_semitones / 12.0
        } else {
            0.0
        };
        (t, slide_oct, jump_oct)
    }

    /// Render one sample.  Called from the audio callback – no allocation.
    pub fn process(&mut self, params: &SynthParams, sample_rate: f32) -> f32 {
        if !self.active && !self.env.is_active() {
            return 0.0;
        }

        if self.env.stage == EnvStage::Idle && !self.env.is_active() {
            self.active = false;
        }

        // --- Modulation bus: fill from all sources, then read once ---

        let mut bus = ModBus::default();

        // LFO 1
        let lfo1 = self
            .lfo
            .next(params.lfo.lfo_rate, params.lfo.lfo_shape, sample_rate);
        bus.add_lfo(lfo1, params.lfo.lfo_depth, params.lfo.lfo_target);

        // LFO 2
        let lfo2 = self
            .lfo2
            .next(params.lfo2.lfo_rate, params.lfo2.lfo_shape, sample_rate);
        bus.add_lfo(lfo2, params.lfo2.lfo_depth, params.lfo2.lfo_target);

        // Filter envelope (0..1 output, scaled by depth)
        let fenv = self.filter_env.process(
            params.filter_env.attack,
            params.filter_env.decay,
            params.filter_env.sustain,
            params.filter_env.release,
            false,
            sample_rate,
        );
        bus.cutoff += fenv * params.filter_env.depth;

        // Pitch envelope (scaled ×10 so depth=1 ≈ ±1 octave vs. the 0.1 LFO scale)
        let penv = self.pitch_env.process(
            params.pitch_env.attack,
            params.pitch_env.decay,
            params.pitch_env.sustain,
            params.pitch_env.release,
            false,
            sample_rate,
        );
        bus.pitch += penv * params.pitch_env.depth * 10.0;

        let sfx = &params.sfx;
        let (t, slide_oct, jump_oct) = self.sfx_step(sfx, sample_rate);

        // Glide (portamento)
        let gc = self.glide_coeff;
        self.current_freq = self.target_freq + (self.current_freq - self.target_freq) * gc;
        let freq = self.current_freq;

        if slid_below(freq, slide_oct, sfx.min_freq) {
            self.env.reset();
            self.active = false;
            return 0.0;
        }

        // Pitch modulation (vibrato, pitch env, slide, jump), kept below
        // Nyquist so the oscillator's phase wraps at most once per sample.
        let final_freq = (freq
            * crate::math::powf(2.0_f32, bus.pitch * 0.1 + slide_oct + jump_oct))
        .min(sample_rate * 0.45);

        // Pulse width modulation and sweep
        let pw = (params.osc.pulse_width + sfx.pw_sweep * t + bus.pw * 0.4).clamp(0.05, 0.95);

        // Oscillator 1
        let osc_out = self.osc.next_sample(
            final_freq,
            sample_rate,
            params.osc.waveform,
            pw,
            params.osc.noise_mix,
        );

        // Oscillator 2 with optional ring modulation
        let osc_out = if params.osc2.osc2_mix > 0.001 {
            if params.osc2.hard_sync && self.osc.just_wrapped() {
                self.osc2.reset();
            }
            let osc2_freq = detune_hz(
                final_freq,
                params.osc2.detune + params.osc2.semitones * 100.0,
            )
            .min(sample_rate * 0.45);
            let secondary =
                self.osc2
                    .next_sample(osc2_freq, sample_rate, params.osc2.waveform, pw, 0.0);

            let modulated = match params.osc2.ring_mod {
                RingModMode::Off => secondary,
                RingModMode::Osc2ByOsc1Sign => secondary * self.osc.phase_sign(),
                RingModMode::Osc1ByOsc2Sign => osc_out * self.osc2.phase_sign(),
                RingModMode::Analog => osc_out * secondary,
            };

            osc_out * (1.0 - params.osc2.osc2_mix) + modulated * params.osc2.osc2_mix
        } else {
            osc_out
        };

        // Bitcrusher (pre-filter)
        let osc_out = self
            .crusher
            .process(osc_out, params.crusher.bits, params.crusher.rate);

        // Amplitude envelope
        let env_val = self.env.process(
            params.env.attack,
            params.env.decay,
            params.env.sustain,
            params.env.release,
            params.env.env_reverse,
            sample_rate,
        );

        // Volume modulation (tremolo)
        let vol_mod = (1.0 - bus.volume * 0.5).max(0.0);

        // Filter cutoff modulation
        let cutoff_mod = (params.filter.cutoff * crate::math::powf(2.0_f32, bus.cutoff * 2.0))
            .clamp(20.0, 18000.0);

        // Filter
        let filtered = self.filter.process(
            osc_out * env_val,
            params.filter.filter_mode,
            cutoff_mod,
            params.filter.resonance,
            params.filter.drive,
            sample_rate,
        );

        filtered * env_val * vol_mod * params.global.volume
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{MidiNote, RingModMode, SynthParams, Waveform};

    #[test]
    fn voice_osc2_mix_zero_is_finite() {
        let mut voice = Voice::new();
        let params = SynthParams::default(); // osc2_mix = 0.0
        voice.note_on(MidiNote::A4, &params, 44100.0);
        for _ in 0..1000 {
            let s = voice.process(&params, 44100.0);
            assert!(s.is_finite(), "non-finite sample with osc2 off: {s}");
        }
    }

    #[test]
    fn voice_osc2_hard_sync_is_finite() {
        let mut voice = Voice::new();
        let mut params = SynthParams::default();
        params.osc2.osc2_mix = 0.5;
        params.osc2.hard_sync = true;
        params.osc2.waveform = Waveform::Sawtooth;
        params.osc2.detune = 7.0;
        voice.note_on(MidiNote::A4, &params, 44100.0);
        for _ in 0..1000 {
            let s = voice.process(&params, 44100.0);
            assert!(s.is_finite(), "non-finite sample with hard sync on: {s}");
        }
    }

    #[test]
    fn voice_hard_sync_resets_osc2_phase() {
        // With hard sync, OSC2 resets to phase 0 each time OSC1 wraps.
        // OSC2 (sawtooth) immediately after reset starts at phase ≈ 0, giving
        // output ≈ -1.  This is observable even through the envelope and filter
        // because OSC2 is the sole source (osc2_mix = 1.0) and the filter is
        // transparent at low resonance.  Without sync the same OSC2 would be at
        // an arbitrary phase after 5+ free cycles, frequently yielding positive
        // values; with sync the output must be strongly negative at every wrap.
        use crate::params::{EnvParams, FilterMode, FilterParams, GlobalParams};
        let mut params = SynthParams::default();
        params.osc.waveform = Waveform::Sawtooth;
        params.osc2.waveform = Waveform::Sawtooth;
        params.osc2.detune = 0.0;
        params.osc2.semitones = 7.0; // OSC2 a fifth (1.5×) above OSC1
        params.osc2.osc2_mix = 1.0; // 100% OSC2 so the reset is the sole signal
        params.osc2.hard_sync = true;
        params.env = EnvParams {
            attack: 0.0,
            decay: 0.0,
            sustain: 1.0,
            release: 0.0,
            env_reverse: false,
        };
        params.filter = FilterParams {
            filter_mode: FilterMode::LowPass,
            cutoff: 20000.0,
            resonance: 0.0,
            drive: 0.0,
        };
        params.global = GlobalParams {
            volume: 1.0,
            glide_time: 0.0,
        };

        let mut voice = Voice::new();
        voice.note_on(MidiNote::A4, &params, 44100.0);

        // 440 Hz at 44100 Hz → wrap every ~100 samples; 600 samples covers ≥5 wraps.
        let mut wrap_samples: Vec<f32> = Vec::new();
        for _ in 0..600 {
            let s = voice.process(&params, 44100.0);
            if voice.osc.just_wrapped() {
                wrap_samples.push(s);
            }
        }

        assert!(
            wrap_samples.len() >= 4,
            "expected ≥4 OSC1 wraps in 600 samples, got {}",
            wrap_samples.len()
        );

        // After the first wrap (envelope may still settle), each wrap-point
        // must produce strongly negative output (OSC2 sawtooth just off phase 0).
        // Phase step ≈ 659 Hz / 44100 Hz ≈ 0.015 → sawtooth ≈ -0.97.
        // Allow headroom for filter state and slight envelope drift.
        for (i, &s) in wrap_samples[1..].iter().enumerate() {
            assert!(
                s < -0.5,
                "wrap sample {}: expected output near -1 (OSC2 reset to phase 0), got {s}",
                i + 1
            );
        }

        // Contrast: without hard sync, OSC2 runs freely and after 5+ free
        // cycles will often be at a positive phase.  Collect and verify at least
        // one wrap-point sample is clearly positive (proving the sync test is
        // discriminating rather than vacuously satisfied).
        let mut params_free = params.clone();
        params_free.osc2.hard_sync = false;
        let mut voice_free = Voice::new();
        voice_free.note_on(MidiNote::A4, &params_free, 44100.0);

        let mut any_positive = false;
        for _ in 0..600 {
            let s = voice_free.process(&params_free, 44100.0);
            if voice_free.osc.just_wrapped() && s > 0.0 {
                any_positive = true;
                break;
            }
        }
        assert!(
            any_positive,
            "unsynced OSC2 should produce positive values at some OSC1 wrap-points"
        );
    }

    #[test]
    fn filter_env_opens_cutoff() {
        use crate::params::{
            EnvParams, FilterMode, FilterParams, GlobalParams, ModEnvParams, Waveform,
        };

        // Start with a very low cutoff so most signal is blocked.
        // Fast-attack filter env at full positive depth (opens toward higher cutoff).
        let mut params = SynthParams {
            filter: FilterParams {
                filter_mode: FilterMode::LowPass,
                cutoff: 200.0,
                resonance: 0.0,
                drive: 0.0,
            },
            filter_env: ModEnvParams {
                attack: 0.001,
                decay: 4.0,
                sustain: 1.0,
                release: 4.0,
                depth: 1.0,
            },
            env: EnvParams {
                attack: 0.001,
                decay: 0.0,
                sustain: 1.0,
                release: 4.0,
                env_reverse: false,
            },
            global: GlobalParams {
                volume: 1.0,
                glide_time: 0.0,
            },
            ..SynthParams::default()
        };
        params.osc.waveform = Waveform::Sawtooth;
        params.lfo.lfo_depth = 0.0;
        params.lfo2.lfo_depth = 0.0;
        params.pitch_env.depth = 0.0;

        // With filter env open (depth = 1.0).
        let mut v_open = Voice::new();
        v_open.note_on(MidiNote::A4, &params, 44100.0);
        let rms_open: f32 = (0..4410)
            .map(|_| v_open.process(&params, 44100.0).powi(2))
            .sum::<f32>()
            .sqrt();

        // With filter env off (depth = 0.0) — filter stays closed at 200 Hz.
        let mut params_closed = params.clone();
        params_closed.filter_env.depth = 0.0;
        let mut v_closed = Voice::new();
        v_closed.note_on(MidiNote::A4, &params_closed, 44100.0);
        let rms_closed: f32 = (0..4410)
            .map(|_| v_closed.process(&params_closed, 44100.0).powi(2))
            .sum::<f32>()
            .sqrt();

        assert!(
            rms_open > rms_closed * 1.5,
            "filter env depth=1 should pass more signal than depth=0: open={rms_open:.4}, closed={rms_closed:.4}"
        );
    }

    #[test]
    fn pitch_env_is_finite() {
        use crate::params::ModEnvParams;

        let params = SynthParams {
            pitch_env: ModEnvParams {
                attack: 0.001,
                decay: 0.5,
                sustain: 0.0,
                release: 0.1,
                depth: 1.0,
            },
            ..SynthParams::default()
        };
        let mut voice = Voice::new();
        voice.note_on(MidiNote::A4, &params, 44100.0);
        for i in 0..4410 {
            let s = voice.process(&params, 44100.0);
            assert!(
                s.is_finite(),
                "non-finite sample at {i} with pitch env: {s}"
            );
        }
    }

    #[test]
    fn lfo2_and_lfo1_both_active_is_finite() {
        use crate::params::LfoShape;

        let mut params = SynthParams::default();
        params.lfo.lfo_depth = 0.5;
        params.lfo.lfo_shape = LfoShape::Square;
        params.lfo.lfo_target = crate::params::LfoTarget::Pitch;
        params.lfo2.lfo_depth = 0.5;
        params.lfo2.lfo_shape = LfoShape::Sawtooth;
        params.lfo2.lfo_target = crate::params::LfoTarget::Pitch;
        let mut voice = Voice::new();
        voice.note_on(MidiNote::A4, &params, 44100.0);
        for i in 0..4410 {
            let s = voice.process(&params, 44100.0);
            assert!(s.is_finite(), "non-finite sample at {i} with two LFOs: {s}");
        }
    }

    #[test]
    fn ring_mod_modes_alter_output() {
        // Each active RingModMode must produce output different from Off.
        // Sawtooth (non-symmetric) avoids signed-sum cancellation.
        // Comparison uses cumulative absolute per-sample difference (robust to phase).
        use crate::params::EnvParams;

        let base_params = {
            let mut p = SynthParams::default();
            p.osc.waveform = Waveform::Sawtooth;
            p.osc2.osc2_mix = 1.0;
            p.osc2.waveform = Waveform::Sawtooth;
            p.osc2.detune = 0.0;
            p.osc2.semitones = 7.0; // a fifth above OSC1, ensures phase difference
            p.env = EnvParams {
                attack: 0.0,
                decay: 0.0,
                sustain: 1.0,
                release: 0.0,
                env_reverse: false,
            };
            p
        };

        // Collect Off-mode samples as baseline.
        let mut params_off = base_params.clone();
        params_off.osc2.ring_mod = RingModMode::Off;
        let mut voice_off = Voice::new();
        voice_off.note_on(MidiNote::A4, &params_off, 44100.0);
        let samples_off: Vec<f32> = (0..500)
            .map(|_| voice_off.process(&params_off, 44100.0))
            .collect();

        for mode in [
            RingModMode::Osc2ByOsc1Sign,
            RingModMode::Osc1ByOsc2Sign,
            RingModMode::Analog,
        ] {
            let mut params_rm = base_params.clone();
            params_rm.osc2.ring_mod = mode;
            let mut voice_rm = Voice::new();
            voice_rm.note_on(MidiNote::A4, &params_rm, 44100.0);
            let abs_diff: f32 = samples_off
                .iter()
                .map(|&off| {
                    let rm = voice_rm.process(&params_rm, 44100.0);
                    (rm - off).abs()
                })
                .sum();
            assert!(
                abs_diff > 0.1,
                "RingModMode::{mode:?}: cumulative |diff| vs Off = {abs_diff:.6} (expected > 0.1)"
            );
        }
    }

    #[test]
    fn ring_mod_analog_output_is_finite() {
        // Analog mode multiplies osc1 × osc2. Both oscillators are in -1..1,
        // so the product is bounded; verify no NaN/Inf escapes through the
        // voice pipeline regardless of waveform or detune ratio.
        use crate::params::EnvParams;

        let mut params = SynthParams::default();
        params.osc2.osc2_mix = 1.0;
        params.osc2.waveform = Waveform::Sawtooth;
        params.osc2.detune = 0.0;
        params.osc2.semitones = 7.0;
        params.osc2.ring_mod = RingModMode::Analog;
        params.env = EnvParams {
            attack: 0.0,
            decay: 0.0,
            sustain: 1.0,
            release: 0.0,
            env_reverse: false,
        };

        let mut voice = Voice::new();
        voice.note_on(MidiNote::A4, &params, 44100.0);
        for i in 0..1000 {
            let s = voice.process(&params, 44100.0);
            assert!(s.is_finite(), "sample {i}: non-finite output: {s}");
        }
    }

    #[test]
    fn ring_mod_off_is_deterministic() {
        // Two voices with identical params (ring_mod = Off) must produce
        // bit-identical output — guards against any stochastic side effects.
        let mut params_a = SynthParams::default();
        params_a.osc2.osc2_mix = 0.5;
        params_a.osc2.waveform = Waveform::Sawtooth;
        params_a.osc2.detune = 7.0;
        params_a.osc2.ring_mod = RingModMode::Off;

        let params_b = params_a.clone();

        let mut voice_a = Voice::new();
        voice_a.note_on(MidiNote::A4, &params_a, 44100.0);
        let mut voice_b = Voice::new();
        voice_b.note_on(MidiNote::A4, &params_b, 44100.0);

        for i in 0..200 {
            let a = voice_a.process(&params_a, 44100.0);
            let b = voice_b.process(&params_b, 44100.0);
            assert!(
                a.to_bits() == b.to_bits(),
                "sample {i}: Off mode is non-deterministic: {a} != {b}"
            );
        }
    }

    /// Fundamental of `params` playing A4, estimated from upward zero
    /// crossings over one second.
    #[allow(clippy::cast_precision_loss)]
    fn measured_hz(params: &SynthParams) -> f32 {
        let mut voice = Voice::new();
        voice.note_on(MidiNote::A4, params, 44100.0);
        // Skip the attack, then count crossings over one second.
        for _ in 0..4410 {
            voice.process(params, 44100.0);
        }
        let mut prev = voice.process(params, 44100.0);
        let mut crossings = 0;
        for _ in 0..44100 {
            let s = voice.process(params, 44100.0);
            if prev < 0.0 && s >= 0.0 {
                crossings += 1;
            }
            prev = s;
        }
        crossings as f32
    }

    #[test]
    fn osc2_semitones_tune_osc2_by_an_interval() {
        use crate::params::{EnvParams, FilterMode, FilterParams};
        // OSC2 alone (mix 1.0), sine, open filter: the output's pitch is OSC2's.
        let mut params = SynthParams::default();
        params.osc2.waveform = Waveform::Sine;
        params.osc2.osc2_mix = 1.0;
        params.osc2.detune = 0.0;
        params.env = EnvParams {
            attack: 0.001,
            decay: 0.001,
            sustain: 1.0,
            release: 0.001,
            env_reverse: false,
        };
        params.filter = FilterParams {
            filter_mode: FilterMode::LowPass,
            cutoff: 18_000.0,
            resonance: 0.0,
            drive: 0.0,
        };
        params.global.glide_time = 0.0;
        params.lfo.lfo_depth = 0.0;

        for (semitones, expected) in [(0.0, 440.0), (7.0, 659.26), (12.0, 880.0), (-12.0, 220.0)] {
            params.osc2.semitones = semitones;
            let hz = measured_hz(&params);
            assert!(
                (hz - expected).abs() <= 2.0,
                "semitones {semitones}: expected ~{expected} Hz, measured {hz} Hz"
            );
        }

        // Detune adds on top of the coarse offset: +7 st and +100 ct is +8 st.
        params.osc2.semitones = 7.0;
        params.osc2.detune = 100.0;
        let hz = measured_hz(&params);
        assert!(
            (hz - 698.46).abs() <= 2.0,
            "+7 st +100 ct: measured {hz} Hz"
        );
    }

    /// Plain sine at full sustain through an open filter, so zero crossings
    /// give the pitch and the output follows the oscillator closely.
    fn sfx_base() -> SynthParams {
        use crate::params::{EnvParams, FilterMode, FilterParams};
        let mut p = SynthParams::default();
        p.osc.waveform = Waveform::Sine;
        p.env = EnvParams {
            attack: 0.001,
            decay: 0.001,
            sustain: 1.0,
            release: 0.001,
            env_reverse: false,
        };
        p.filter = FilterParams {
            filter_mode: FilterMode::LowPass,
            cutoff: 18_000.0,
            resonance: 0.0,
            drive: 0.0,
        };
        p.global.glide_time = 0.0;
        p.global.volume = 1.0;
        p.lfo.lfo_depth = 0.0;
        p
    }

    /// Render `seconds` of A4 with `params`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn render(params: &SynthParams, seconds: f32) -> Vec<f32> {
        let mut voice = Voice::new();
        voice.note_on(MidiNote::A4, params, 44100.0);
        (0..(seconds * 44100.0) as usize)
            .map(|_| voice.process(params, 44100.0))
            .collect()
    }

    /// Pitch in Hz of `samples[from..to]` (seconds), from upward zero crossings.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn hz_between(samples: &[f32], from: f32, to: f32) -> f32 {
        let window = &samples[(from * 44100.0) as usize..(to * 44100.0) as usize];
        let crossings = window
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count();
        crossings as f32 / (to - from)
    }

    #[test]
    fn slide_falls_and_accel_turns_it_around() {
        let mut p = sfx_base();
        p.sfx.slide = -1.0; // an octave down per second
        let s = render(&p, 1.0);
        let early = hz_between(&s, 0.0, 0.1);
        let late = hz_between(&s, 0.9, 1.0);
        assert!((early - 425.0).abs() < 15.0, "early {early}");
        assert!((late - 226.0).abs() < 10.0, "late {late}");

        // -2 oct/s plus +4 oct/s²: bottom at 0.5 s, back at A4 by 1.0 s.
        p.sfx.slide = -2.0;
        p.sfx.slide_accel = 4.0;
        let s = render(&p, 1.1);
        let bottom = hz_between(&s, 0.45, 0.55);
        let back = hz_between(&s, 1.0, 1.1);
        assert!(bottom < 330.0, "bottom {bottom}");
        assert!(back > 430.0, "back {back}");
    }

    #[test]
    fn min_freq_ends_a_falling_slide() {
        let mut p = sfx_base();
        p.sfx.slide = -4.0;
        p.sfx.min_freq = 220.0; // one octave below A4: reached after 0.25 s
        let mut voice = Voice::new();
        voice.note_on(MidiNote::A4, &p, 44100.0);
        let s: Vec<f32> = (0..22050).map(|_| voice.process(&p, 44100.0)).collect();
        assert!(s[..10_000].iter().any(|x| x.abs() > 0.5), "sounds at first");
        assert!(s[11_100..].iter().all(|&x| x == 0.0), "silent past 220 Hz");
        assert!(!voice.active && !voice.env.is_active());

        // A rising slide never trips it, even from below the cutoff.
        p.sfx.slide = 1.0;
        p.sfx.min_freq = 2000.0;
        let s = render(&p, 0.5);
        assert!(s[20_000..].iter().any(|x| x.abs() > 0.5));
    }

    #[test]
    fn extreme_sfx_settings_stay_finite() {
        for (slide, accel) in [(20.0, 100.0), (-20.0, -100.0), (20.0, -100.0)] {
            let mut p = sfx_base();
            p.osc.waveform = Waveform::Pulse;
            p.sfx.slide = slide;
            p.sfx.slide_accel = accel;
            p.sfx.jump_semitones = 24.0;
            p.sfx.pw_sweep = 4.0;
            p.sfx.repeat_rate = 0.3;
            let s = render(&p, 5.0);
            assert!(s.iter().all(|x| x.is_finite()), "{slide} {accel}");
        }
    }

    #[test]
    fn pitch_jump_happens_once_at_jump_time() {
        let mut p = sfx_base();
        p.sfx.jump_semitones = 12.0;
        p.sfx.jump_time = 0.1;
        let s = render(&p, 0.5);
        let before = hz_between(&s, 0.0, 0.1);
        let after = hz_between(&s, 0.1, 0.5);
        assert!((before - 440.0).abs() < 15.0, "before {before}");
        assert!((after - 880.0).abs() < 10.0, "after {after}");
    }

    #[test]
    #[allow(clippy::cast_precision_loss)]
    fn pw_sweep_moves_the_duty_cycle_and_stops_at_the_limit() {
        let mut p = sfx_base();
        p.osc.waveform = Waveform::Pulse;
        p.osc.pulse_width = 0.5;
        p.sfx.pw_sweep = 0.8;
        let s = render(&p, 2.0);
        let duty = |from: usize, to: usize| {
            s[from..to].iter().filter(|&&x| x > 0.0).count() as f32 / (to - from) as f32
        };
        let early = duty(500, 2_000);
        let mid = duty(17_000, 19_000); // ~0.4 s: pw ~0.82
        let late = duty(80_000, 88_000); // clamped at 0.95
        assert!((early - 0.5).abs() < 0.1, "early {early}");
        assert!(mid > early + 0.2, "mid {mid}");
        assert!(late > 0.85 && late < 1.0, "late {late}, never stuck high");
    }

    #[test]
    fn repeat_restarts_the_slide_without_clicks() {
        let mut p = sfx_base();
        p.sfx.slide = 1.0;
        p.sfx.repeat_rate = 4.0;
        let s = render(&p, 1.0);
        let first = hz_between(&s, 0.0, 0.2);
        let third = hz_between(&s, 0.5, 0.7);
        assert!((first - third).abs() < 10.0, "{first} vs {third}");
        assert!(hz_between(&s, 0.2, 0.25) > first, "rises within a cycle");
        // Phase and envelope run on across repeats: a sine never jumps.
        let max_step = s
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0, f32::max);
        assert!(max_step < 0.15, "step {max_step}");
    }
}
