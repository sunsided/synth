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
use crate::params::{MidiNote, SynthParams};

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
        }
    }

    /// Recompute the portamento smoothing coefficient from glide time and sample rate.
    pub fn update_glide(&mut self, glide_time: f32, sample_rate: f32) {
        self.glide_coeff = if glide_time < 1e-4 {
            0.0
        } else {
            (-1.0_f32 / (glide_time * sample_rate)).exp()
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
        self.active = false;
        self.env.reset();
        self.filter.reset();
        self.filter_env.reset();
        self.pitch_env.reset();
        self.crusher.reset();
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

        // --- Signal chain ---

        // Glide (portamento)
        let gc = self.glide_coeff;
        self.current_freq = self.target_freq + (self.current_freq - self.target_freq) * gc;
        let freq = self.current_freq;

        // Pitch modulation (vibrato + pitch env)
        let modded_freq = freq * 2.0_f32.powf(bus.pitch * 0.1);
        let final_freq = modded_freq;

        // Pulse width modulation
        let pw = (params.osc.pulse_width + bus.pw * 0.4).clamp(0.05, 0.95);

        // Oscillator 1
        let osc_out = self.osc.next_sample(
            final_freq,
            sample_rate,
            params.osc.waveform,
            pw,
            params.osc.noise_mix,
        );

        // Oscillator 2 (unchanged)
        let osc_out = if params.osc2.osc2_mix > 0.001 {
            if params.osc2.hard_sync && self.osc.just_wrapped() {
                self.osc2.reset();
            }
            let osc2_freq = detune_hz(final_freq, params.osc2.detune);
            let secondary =
                self.osc2
                    .next_sample(osc2_freq, sample_rate, params.osc2.waveform, pw, 0.0);
            osc_out * (1.0 - params.osc2.osc2_mix) + secondary * params.osc2.osc2_mix
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
        let cutoff_mod =
            (params.filter.cutoff * 2.0_f32.powf(bus.cutoff * 2.0)).clamp(20.0, 18000.0);

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
    use crate::params::{MidiNote, SynthParams, Waveform};

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
        params.osc2.detune = 700.0; // OSC2 ~5.3× faster than OSC1
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
}
