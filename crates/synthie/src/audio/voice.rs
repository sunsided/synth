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
use crate::params::{LfoTarget, MidiNote, SynthParams};

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
    }

    /// Begin the envelope release phase.
    pub fn note_off(&mut self) {
        self.env.note_off();
    }

    /// Immediately silence the voice and reset DSP state (all-notes-off / panic).
    pub fn panic(&mut self) {
        self.active = false;
        self.env.reset();
        self.filter.reset();
        self.crusher.reset();
    }

    /// Render one sample.  Called from the audio callback – no allocation.
    pub fn process(&mut self, params: &SynthParams, sample_rate: f32) -> f32 {
        if !self.active && !self.env.is_active() {
            return 0.0;
        }

        // Deactivate once envelope reaches Idle.
        if self.env.stage == EnvStage::Idle && !self.env.is_active() {
            self.active = false;
        }

        // LFO
        let lfo_val = self.lfo.next(params.lfo.lfo_rate, sample_rate); // -1..1
        let lfo_depth = params.lfo.lfo_depth;

        // Glide (portamento)
        let gc = self.glide_coeff;
        self.current_freq = self.target_freq + (self.current_freq - self.target_freq) * gc;
        let freq = self.current_freq;

        // Pitch modulation (vibrato)
        let modded_freq = match params.lfo.lfo_target {
            LfoTarget::Pitch => freq * 2.0_f32.powf(lfo_val * lfo_depth * 0.1),
            _ => freq,
        };

        // Detune (re-apply in case params changed)
        let final_freq = detune_hz(modded_freq, 0.0); // detune already baked into target_freq

        // Pulse width modulation
        let pw = match params.lfo.lfo_target {
            LfoTarget::PulseWidth => {
                (params.osc.pulse_width + lfo_val * lfo_depth * 0.4).clamp(0.05, 0.95)
            }
            _ => params.osc.pulse_width,
        };

        // Oscillator
        let osc_out = self.osc.next_sample(
            final_freq,
            sample_rate,
            params.osc.waveform,
            pw,
            params.osc.noise_mix,
        );

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

        // Bitcrusher (pre-filter: quantization harmonics shaped by filter resonance)
        let osc_out = self
            .crusher
            .process(osc_out, params.crusher.bits, params.crusher.rate);

        // Envelope
        let env_val = self.env.process(
            params.env.attack,
            params.env.decay,
            params.env.sustain,
            params.env.release,
            params.env.env_reverse,
            sample_rate,
        );

        // Volume LFO (tremolo)
        let vol_mod = match params.lfo.lfo_target {
            LfoTarget::Volume => 1.0 - lfo_val * lfo_depth * 0.5,
            _ => 1.0,
        };

        // Filter cutoff LFO
        let cutoff_mod = match params.lfo.lfo_target {
            LfoTarget::Cutoff => (params.filter.cutoff * 2.0_f32.powf(lfo_val * lfo_depth * 2.0))
                .clamp(20.0, 18000.0),
            _ => params.filter.cutoff,
        };

        // Filter stage
        let filtered = self.filter.process(
            osc_out * env_val,
            params.filter.filter_mode,
            cutoff_mod,
            params.filter.resonance,
            params.filter.drive,
            sample_rate,
        );

        // Volume & tremolo (dry; post-mix reverb is handled in engine)
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
}
