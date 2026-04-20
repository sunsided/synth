//! ADSR envelope generator with optional reverse (swell/duck) mode.
//!
//! Legato behaviour: when a note-on arrives while the envelope is still active
//! (not `Idle`), the attack continues from the current level rather than
//! restarting from zero, avoiding audible clicks on legato note changes.

/// Current phase of the ADSR envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvStage {
    /// Envelope is silent; no output.
    Idle,
    /// Rising from the current level toward 1.0.
    Attack,
    /// Falling from 1.0 toward the sustain level.
    Decay,
    /// Holding at the sustain level until note-off.
    Sustain,
    /// Falling from the current level toward silence.
    Release,
}

impl EnvStage {
    /// Short display name for the current stage.
    #[allow(dead_code)]
    pub fn name(self) -> &'static str {
        match self {
            EnvStage::Idle => "Idle",
            EnvStage::Attack => "Attack",
            EnvStage::Decay => "Decay",
            EnvStage::Sustain => "Sustain",
            EnvStage::Release => "Release",
        }
    }
}

/// ADSR envelope state.
#[derive(Debug, Clone)]
pub struct Envelope {
    /// Current phase of the envelope.
    pub stage: EnvStage,
    /// Instantaneous amplitude level (0.0 .. 1.0).
    pub level: f32,
}

impl Default for Envelope {
    /// Create an envelope in the `Idle` stage at zero level.
    fn default() -> Self {
        Self {
            stage: EnvStage::Idle,
            level: 0.0,
        }
    }
}

impl Envelope {
    /// Trigger note-on.
    ///
    /// If `legato` is true and the envelope is already active, the attack
    /// continues from the current level (no click on legato note changes).
    pub fn note_on(&mut self, legato: bool) {
        if legato && self.stage != EnvStage::Idle {
            // Continue from current level into attack phase.
            self.stage = EnvStage::Attack;
        } else {
            self.level = 0.0;
            self.stage = EnvStage::Attack;
        }
    }

    /// Trigger note-off (start release phase).
    pub fn note_off(&mut self) {
        if self.stage != EnvStage::Idle {
            self.stage = EnvStage::Release;
        }
    }

    /// Immediately silence the envelope.
    pub fn reset(&mut self) {
        self.level = 0.0;
        self.stage = EnvStage::Idle;
    }

    /// Returns `true` if the envelope is producing non-zero output.
    pub fn is_active(&self) -> bool {
        self.stage != EnvStage::Idle
    }

    /// Advance the envelope by one sample and return the current amplitude (0..1).
    ///
    /// Parameters:
    /// * `attack`  – attack time in seconds
    /// * `decay`   – decay time in seconds
    /// * `sustain` – sustain level 0..1
    /// * `release` – release time in seconds
    /// * `reverse` – if true, output is `1.0 - level` (reverse/swell mode)
    pub fn process(
        &mut self,
        attack: f32,
        decay: f32,
        sustain: f32,
        release: f32,
        reverse: bool,
        sample_rate: f32,
    ) -> f32 {
        match self.stage {
            EnvStage::Idle => {
                self.level = 0.0;
            }
            EnvStage::Attack => {
                let rate = 1.0 / (attack * sample_rate).max(1.0);
                self.level += rate;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvStage::Decay;
                }
            }
            EnvStage::Decay => {
                // Linear decay toward sustain level.
                let rate = (1.0 - sustain) / (decay * sample_rate).max(1.0);
                self.level -= rate;
                if self.level <= sustain {
                    self.level = sustain;
                    self.stage = if sustain > 0.0 {
                        EnvStage::Sustain
                    } else {
                        EnvStage::Idle
                    };
                }
            }
            EnvStage::Sustain => {
                self.level = sustain;
            }
            EnvStage::Release => {
                if release <= 0.001 {
                    self.level = 0.0;
                    self.stage = EnvStage::Idle;
                } else {
                    // Exponential decay from wherever the level is now.
                    let coeff = (-1.0_f32 / (release * sample_rate)).exp();
                    self.level *= coeff;
                    if self.level < 1e-3 {
                        self.level = 0.0;
                        self.stage = EnvStage::Idle;
                    }
                }
            }
        }

        let out = self.level.clamp(0.0, 1.0);
        if reverse { 1.0 - out } else { out }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_cmp)] // idle envelope is exactly 0.0 by construction
    fn idle_gives_zero() {
        let mut env = Envelope::default();
        let v = env.process(0.01, 0.1, 0.5, 0.2, false, 44100.0);
        assert_eq!(v, 0.0);
    }

    #[test]
    fn attack_rises_to_one() {
        let mut env = Envelope::default();
        env.note_on(false);
        // 44100 samples at attack=1.0 s
        let mut peak = 0.0_f32;
        for _ in 0..44100 {
            peak = peak.max(env.process(1.0, 0.1, 0.5, 0.2, false, 44100.0));
        }
        assert!(peak >= 0.999, "peak={peak}");
    }

    #[test]
    fn release_reaches_idle() {
        let mut env = Envelope::default();
        env.note_on(false);
        // Skip through attack + decay + sustain (very fast params)
        for _ in 0..44100 {
            env.process(0.001, 0.001, 0.5, 0.1, false, 44100.0);
        }
        env.note_off();
        // Release at 0.1 s: after ~50k samples the level should be well below 1e-3
        for _ in 0..50_000 {
            env.process(0.001, 0.001, 0.5, 0.1, false, 44100.0);
        }
        assert_eq!(env.stage, EnvStage::Idle, "stage={:?}", env.stage);
    }

    #[test]
    #[allow(clippy::float_cmp)] // reversed idle envelope is exactly 1.0 by construction
    fn reverse_mode() {
        let mut env = Envelope::default();
        // In idle with reverse=true the output should be 1.0 - 0.0 = 1.0
        let v = env.process(0.01, 0.1, 0.5, 0.2, true, 44100.0);
        assert_eq!(v, 1.0);
    }

    #[test]
    fn legato_no_click() {
        // Envelope at sustain; pressing a new note legato should NOT reset to 0
        let mut env = Envelope::default();
        env.note_on(false);
        // Run to sustain
        for _ in 0..88200 {
            env.process(0.001, 0.001, 0.7, 0.3, false, 44100.0);
        }
        let level_before = env.level;
        env.note_on(true); // legato
        let level_after = env.level;
        // Level must not have jumped to zero
        assert!(
            (level_after - level_before).abs() < 0.01,
            "legato caused click: before={level_before}, after={level_after}"
        );
    }
}
