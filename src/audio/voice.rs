use crate::audio::{
    env::{EnvStage, Envelope},
    filter::SvFilter,
    fx::Reverb,
    osc::{detune_hz, midi_to_hz, Lfo, Oscillator},
};
use crate::params::SynthParams;

// ---------------------------------------------------------------------------
// Note stack – monophonic priority: last-note (newest note wins, releases
// return to the previous note if still held)
// ---------------------------------------------------------------------------

pub struct NoteStack {
    notes: Vec<u8>,
}

impl NoteStack {
    pub fn new() -> Self {
        Self {
            notes: Vec::with_capacity(16),
        }
    }

    /// Press a key (add to stack, promote to top).
    pub fn press(&mut self, note: u8) {
        self.notes.retain(|&n| n != note);
        self.notes.push(note);
    }

    /// Release a key.  Returns the new top note (if any still held).
    pub fn release(&mut self, note: u8) -> Option<u8> {
        self.notes.retain(|&n| n != note);
        self.notes.last().copied()
    }

    #[allow(dead_code)]
    pub fn top(&self) -> Option<u8> {
        self.notes.last().copied()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn clear(&mut self) {
        self.notes.clear();
    }
}

// ---------------------------------------------------------------------------
// Monophonic voice
// ---------------------------------------------------------------------------

pub struct Voice {
    pub active: bool,
    pub target_note: u8,   // MIDI note being played
    pub target_freq: f32,  // Hz of target_note (+ detune)
    pub current_freq: f32, // glide-smoothed frequency
    pub osc: Oscillator,
    pub env: Envelope,
    pub filter: SvFilter,
    pub lfo: Lfo,
    pub reverb: Reverb,
    pub glide_coeff: f32, // per-sample smoothing coefficient
}

impl Voice {
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

    /// Update the glide coefficient when glide_time or sample_rate changes.
    pub fn update_glide(&mut self, glide_time: f32, sample_rate: f32) {
        self.glide_coeff = if glide_time < 1e-4 {
            0.0
        } else {
            (-1.0_f32 / (glide_time * sample_rate)).exp()
        };
    }

    /// Start a new note (or retrigger legato if already active).
    pub fn note_on(&mut self, note: u8, params: &SynthParams, sample_rate: f32) {
        let legato = self.active;
        self.target_note = note;
        let base = midi_to_hz(note);
        self.target_freq = detune_hz(base, params.detune);
        if !legato {
            // No glide on fresh attacks – snap to pitch immediately
            self.current_freq = self.target_freq;
        }
        self.active = true;
        self.update_glide(params.glide_time, sample_rate);
        self.reverb
            .set_params(params.reverb_size, params.reverb_damping);
        self.env.note_on(legato);
    }

    /// Begin the release phase.
    pub fn note_off(&mut self) {
        self.env.note_off();
    }

    /// Silence immediately (all-notes-off / panic).
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

        // Deactivate once envelope reaches Idle
        if self.env.stage == EnvStage::Idle && !self.env.is_active() {
            self.active = false;
        }

        // --- LFO ---
        let lfo_val = self.lfo.next(params.lfo_rate, sample_rate); // -1..1
        let lfo_depth = params.lfo_depth;

        // --- Glide (portamento) ---
        let gc = self.glide_coeff;
        self.current_freq = self.target_freq + (self.current_freq - self.target_freq) * gc;
        let freq = self.current_freq;

        // --- Pitch modulation (vibrato) ---
        use crate::params::LfoTarget;
        let modded_freq = match params.lfo_target {
            LfoTarget::Pitch => freq * 2.0_f32.powf(lfo_val * lfo_depth * 0.1),
            _ => freq,
        };

        // --- Detune (re-apply in case params changed) ---
        let final_freq = detune_hz(modded_freq, 0.0); // detune already baked into target_freq

        // --- Pulse width modulation ---
        let pw = match params.lfo_target {
            LfoTarget::PulseWidth => {
                (params.pulse_width + lfo_val * lfo_depth * 0.4).clamp(0.05, 0.95)
            }
            _ => params.pulse_width,
        };

        // --- Oscillator ---
        let osc_out = self.osc.next_sample(
            final_freq,
            sample_rate,
            params.waveform,
            pw,
            params.noise_mix,
        );

        // --- Envelope ---
        let env_val = self.env.process(
            params.attack,
            params.decay,
            params.sustain,
            params.release,
            params.env_reverse,
            sample_rate,
        );

        // --- Volume LFO (tremolo) ---
        let vol_mod = match params.lfo_target {
            LfoTarget::Volume => 1.0 - lfo_val * lfo_depth * 0.5,
            _ => 1.0,
        };

        // --- Filter cutoff LFO ---
        let cutoff_mod = match params.lfo_target {
            LfoTarget::Cutoff => {
                (params.cutoff * 2.0_f32.powf(lfo_val * lfo_depth * 2.0)).clamp(20.0, 18000.0)
            }
            _ => params.cutoff,
        };

        // --- Filter ---
        let filtered = self.filter.process(
            osc_out * env_val,
            params.filter_mode,
            cutoff_mod,
            params.resonance,
            params.drive,
            sample_rate,
        );

        // --- Volume & tremolo ---
        let dry = filtered * env_val * vol_mod * params.volume;

        // --- Reverb ---
        self.reverb.process(dry, params.reverb_mix)
    }
}
