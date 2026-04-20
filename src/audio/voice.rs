//! Monophonic voice pipeline and MIDI note stack.
//!
//! `NoteStack` implements last-note priority with legato retrigger; `Voice`
//! connects all DSP modules in a single sample-by-sample render path.

use crate::audio::{
    env::{EnvStage, Envelope},
    filter::SvFilter,
    fx::Reverb,
    osc::{Lfo, Oscillator, detune_hz, midi_to_hz},
};
use crate::params::SynthParams;

/// Monophonic note stack with last-note (newest-wins) priority.
///
/// When a key is released, the voice retriggers to the previously held note
/// if one is still depressed, rather than going silent.
pub struct NoteStack {
    /// Ordered list of currently held MIDI notes (oldest first, newest last).
    notes: Vec<u8>,
}

impl NoteStack {
    /// Create an empty note stack.
    pub fn new() -> Self {
        Self {
            notes: Vec::with_capacity(16),
        }
    }

    /// Press a key: remove any existing entry for this note, then push to top.
    pub fn press(&mut self, note: u8) {
        self.notes.retain(|&n| n != note);
        self.notes.push(note);
    }

    /// Release a key.  Returns the new top note (if any notes still held).
    pub fn release(&mut self, note: u8) -> Option<u8> {
        self.notes.retain(|&n| n != note);
        self.notes.last().copied()
    }

    /// Peek at the currently sounding (top) note without modifying the stack.
    #[allow(dead_code)]
    pub fn top(&self) -> Option<u8> {
        self.notes.last().copied()
    }

    /// Returns `true` if no notes are currently held.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// Remove all held notes (used by the panic event).
    pub fn clear(&mut self) {
        self.notes.clear();
    }
}

/// Monophonic synthesiser voice combining oscillator, envelope, filter, LFO, and reverb.
pub struct Voice {
    /// Whether the voice is currently producing sound.
    pub active: bool,
    /// MIDI note number of the current target pitch.
    pub target_note: u8,
    /// Target frequency in Hz (includes detune).
    pub target_freq: f32,
    /// Current glide-smoothed frequency in Hz.
    pub current_freq: f32,
    /// Waveform generator.
    pub osc: Oscillator,
    /// Amplitude envelope.
    pub env: Envelope,
    /// State-variable filter.
    pub filter: SvFilter,
    /// Low-frequency oscillator.
    pub lfo: Lfo,
    /// Plate reverb unit.
    pub reverb: Reverb,
    /// Per-sample portamento smoothing coefficient (0 = instant, ~1 = very slow).
    pub glide_coeff: f32,
}

impl Voice {
    /// Construct a voice with default state at A4 (440 Hz).
    pub fn new() -> Self {
        Self {
            active: false,
            target_note: 69,
            target_freq: 440.0,
            current_freq: 440.0,
            osc: Oscillator::default(),
            env: Envelope::default(),
            filter: SvFilter::default(),
            lfo: Lfo::default(),
            reverb: Reverb::new(),
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
    pub fn note_on(&mut self, note: u8, params: &SynthParams, sample_rate: f32) {
        let legato = self.active;
        self.target_note = note;
        let base = midi_to_hz(note);
        self.target_freq = detune_hz(base, params.osc.detune);
        if !legato {
            // No glide on fresh attacks – snap to pitch immediately.
            self.current_freq = self.target_freq;
        }
        self.active = true;
        self.update_glide(params.global.glide_time, sample_rate);
        self.reverb
            .set_params(params.fx.reverb_size, params.fx.reverb_damping);
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
        use crate::params::LfoTarget;
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

        // Volume & tremolo
        let dry = filtered * env_val * vol_mod * params.global.volume;

        // Reverb
        self.reverb.process(dry, params.fx.reverb_mix)
    }
}
