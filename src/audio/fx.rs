/// Freeverb-inspired Schroeder reverb: 4 parallel comb filters → 2 serial allpass.
///
/// All delay buffers are allocated once at construction; the audio callback
/// does no heap allocation.

// Comb delay lengths tuned for 44100 Hz (Freeverb defaults, slightly reduced)
const COMB_DELAYS: [usize; 4] = [1116, 1188, 1277, 1356];
const ALLPASS_DELAYS: [usize; 2] = [556, 441];

// ---------------------------------------------------------------------------
// Comb filter (with first-order LP damping – Schroeder/Freeverb style)
// ---------------------------------------------------------------------------

struct CombFilter {
    buf: Vec<f32>,
    idx: usize,
    feedback: f32,
    damp: f32,
    damp_state: f32,
}

impl CombFilter {
    fn new(delay: usize) -> Self {
        Self {
            buf: vec![0.0; delay],
            idx: 0,
            feedback: 0.84,
            damp: 0.5,
            damp_state: 0.0,
        }
    }

    fn set_params(&mut self, feedback: f32, damp: f32) {
        self.feedback = feedback;
        self.damp = damp.clamp(0.0, 0.999);
    }

    fn process(&mut self, input: f32) -> f32 {
        let out = self.buf[self.idx];
        // First-order low-pass on feedback path (simulates air absorption)
        self.damp_state = out * (1.0 - self.damp) + self.damp_state * self.damp;
        self.buf[self.idx] = input + self.damp_state * self.feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

// ---------------------------------------------------------------------------
// Allpass filter (Schroeder allpass section)
// ---------------------------------------------------------------------------

struct AllpassFilter {
    buf: Vec<f32>,
    idx: usize,
}

impl AllpassFilter {
    fn new(delay: usize) -> Self {
        Self {
            buf: vec![0.0; delay],
            idx: 0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buf_out = self.buf[self.idx];
        let out = buf_out - input;
        self.buf[self.idx] = input + buf_out * 0.5;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

// ---------------------------------------------------------------------------
// Public Reverb unit
// ---------------------------------------------------------------------------

pub struct Reverb {
    combs: [CombFilter; 4],
    allpasses: [AllpassFilter; 2],
}

impl Default for Reverb {
    fn default() -> Self {
        Self::new()
    }
}

impl Reverb {
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

    /// Update room size and damping without reallocating.
    pub fn set_params(&mut self, size: f32, damping: f32) {
        let feedback = 0.70 + size.clamp(0.0, 1.0) * 0.25; // 0.70 .. 0.95
        let damp = damping.clamp(0.0, 0.999);
        for c in &mut self.combs {
            c.set_params(feedback, damp);
        }
    }

    /// Process a mono sample.  Returns wet+dry mix.
    /// `mix` – 0.0 (dry) .. 1.0 (full wet).
    pub fn process(&mut self, input: f32, mix: f32) -> f32 {
        if mix < 1e-4 {
            return input;
        }

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

        input * (1.0 - mix) + wet * mix * 5.0 // compensate comb scale-down
    }
}
