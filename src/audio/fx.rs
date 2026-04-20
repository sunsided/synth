//! Freeverb-inspired Schroeder reverb: 4 parallel comb filters → 2 serial allpass sections.
//!
//! All delay buffers are allocated once at construction; the audio callback
//! performs no heap allocation.

/// Comb filter delay lengths tuned for 44100 Hz (Freeverb defaults, slightly reduced).
const COMB_DELAYS: [usize; 4] = [1116, 1188, 1277, 1356];

/// Allpass filter delay lengths tuned for 44100 Hz.
const ALLPASS_DELAYS: [usize; 2] = [556, 441];

/// Feedback comb filter with first-order low-pass damping (Schroeder/Freeverb style).
///
/// The LP on the feedback path simulates high-frequency air absorption in a room.
struct CombFilter {
    /// Delay line ring buffer.
    buf: Vec<f32>,
    /// Current write/read index into `buf`.
    idx: usize,
    /// Feedback gain (controls room decay time).
    feedback: f32,
    /// Damping coefficient for the LP on the feedback path.
    damp: f32,
    /// State variable for the one-pole LP.
    damp_state: f32,
}

impl CombFilter {
    /// Construct a comb filter with the given delay length (in samples).
    fn new(delay: usize) -> Self {
        Self {
            buf: vec![0.0; delay],
            idx: 0,
            feedback: 0.84,
            damp: 0.5,
            damp_state: 0.0,
        }
    }

    /// Update feedback gain and damping without reallocating the delay line.
    fn set_params(&mut self, feedback: f32, damp: f32) {
        self.feedback = feedback;
        self.damp = damp.clamp(0.0, 0.999);
    }

    /// Process one sample through the comb filter.
    fn process(&mut self, input: f32) -> f32 {
        let out = self.buf[self.idx];
        // First-order low-pass on feedback path (simulates air absorption)
        self.damp_state = out * (1.0 - self.damp) + self.damp_state * self.damp;
        self.buf[self.idx] = input + self.damp_state * self.feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

/// Schroeder allpass section (fixed gain of ±0.5).
struct AllpassFilter {
    /// Delay line ring buffer.
    buf: Vec<f32>,
    /// Current write/read index into `buf`.
    idx: usize,
}

impl AllpassFilter {
    /// Construct an allpass filter with the given delay length (in samples).
    fn new(delay: usize) -> Self {
        Self {
            buf: vec![0.0; delay],
            idx: 0,
        }
    }

    /// Process one sample through the allpass section.
    fn process(&mut self, input: f32) -> f32 {
        let buf_out = self.buf[self.idx];
        let out = buf_out - input;
        self.buf[self.idx] = input + buf_out * 0.5;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

/// Freeverb-style plate reverb unit.
///
/// Four parallel comb filters feed into two serial allpass sections.
/// Room size and damping are adjustable at runtime via `set_params`.
pub struct Reverb {
    /// The four parallel feedback comb filters.
    combs: [CombFilter; 4],
    /// The two serial allpass diffusion stages.
    allpasses: [AllpassFilter; 2],
}

impl Default for Reverb {
    /// Create a reverb with default room size and damping.
    fn default() -> Self {
        Self::new()
    }
}

impl Reverb {
    /// Construct a new reverb unit with pre-allocated delay buffers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            combs: [
                CombFilter::new(COMB_DELAYS[0]),
                CombFilter::new(COMB_DELAYS[1]),
                CombFilter::new(COMB_DELAYS[2]),
                CombFilter::new(COMB_DELAYS[3]),
            ],
            allpasses: [
                AllpassFilter::new(ALLPASS_DELAYS[0]),
                AllpassFilter::new(ALLPASS_DELAYS[1]),
            ],
        }
    }

    /// Update room size and damping without reallocating delay buffers.
    pub fn set_params(&mut self, size: f32, damping: f32) {
        let feedback = 0.70 + size.clamp(0.0, 1.0) * 0.25; // 0.70 .. 0.95
        let damp = damping.clamp(0.0, 0.999);
        for c in &mut self.combs {
            c.set_params(feedback, damp);
        }
    }

    /// Process a mono sample and return only the wet reverb tail.
    #[must_use]
    pub fn process_wet(&mut self, input: f32) -> f32 {
        // Sum 4 parallel combs (scale input to prevent overload)
        let scaled = input * 0.015;
        let mut wet = 0.0_f32;
        for c in &mut self.combs {
            wet += c.process(scaled);
        }

        // Two serial allpasses
        for ap in &mut self.allpasses {
            wet = ap.process(wet);
        }

        wet
    }

    /// Process a mono sample.  Returns the wet+dry mix.
    ///
    /// `mix` – 0.0 (dry) .. 1.0 (full wet).
    pub fn process(&mut self, input: f32, mix: f32) -> f32 {
        if mix < 1e-4 {
            return input;
        }

        let wet = self.process_wet(input);
        input * (1.0 - mix) + wet * mix * 5.0 // compensate comb scale-down
    }
}
