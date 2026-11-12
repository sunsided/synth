//! Sound-effect patch generators: a random patch shaped for a category
//! (pickup, laser, explosion, …), plus [`randomize`] and [`mutate`] for any
//! patch.
//!
//! Every function takes a seed, so the same seed always gives the same
//! patch. The patches don't set a pitch; they are tuned to sound right
//! around C4–C6.
//!
//! ```
//! use synthie::sfx::{SfxKind, generate, mutate};
//!
//! let laser = generate(SfxKind::Laser, 42);
//! assert!(laser.sfx.slide < 0.0);
//! let variation = mutate(&laser, 7);
//! assert_ne!(variation, laser);
//! ```

use crate::param::{Param, ParamKind};
use crate::params::{EnvParams, FilterParams, GlobalParams, SynthParams, Waveform};

/// A category of sound effect for [`generate`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SfxKind {
    /// Short blip that jumps up an interval: coins, pickups.
    Pickup,
    /// Fast falling sweep: shots, lasers.
    Laser,
    /// Noise falling in pitch with a long decay.
    Explosion,
    /// Rising sweep, often repeating.
    PowerUp,
    /// Short, harsh falling hit: damage, impacts.
    Hit,
    /// Quick rising tone.
    Jump,
    /// Very short plain tone: menu select, UI feedback.
    Blip,
}

impl SfxKind {
    /// Every category, in display order.
    pub const ALL: &'static [SfxKind] = &[
        SfxKind::Pickup,
        SfxKind::Laser,
        SfxKind::Explosion,
        SfxKind::PowerUp,
        SfxKind::Hit,
        SfxKind::Jump,
        SfxKind::Blip,
    ];

    /// Display name, e.g. `"Pickup"`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            SfxKind::Pickup => "Pickup",
            SfxKind::Laser => "Laser",
            SfxKind::Explosion => "Explosion",
            SfxKind::PowerUp => "PowerUp",
            SfxKind::Hit => "Hit",
            SfxKind::Jump => "Jump",
            SfxKind::Blip => "Blip",
        }
    }
}

/// A random patch of category `kind`, the same for the same `seed`.
#[must_use]
pub fn generate(kind: SfxKind, seed: u64) -> SynthParams {
    let mut r = Rng(seed);
    let mut p = SynthParams {
        // One-shot: the sound ends on its own while the note is held.
        env: EnvParams {
            attack: 0.001,
            decay: 0.2,
            sustain: 0.0,
            release: 0.05,
            env_reverse: false,
        },
        filter: FilterParams {
            cutoff: 18_000.0,
            resonance: 0.0,
            ..FilterParams::default()
        },
        global: GlobalParams {
            glide_time: 0.0,
            ..GlobalParams::default()
        },
        ..SynthParams::default()
    };
    p.osc.pulse_width = r.range(0.2, 0.5);

    match kind {
        SfxKind::Pickup => {
            p.osc.waveform = r.pick(&[Waveform::Pulse, Waveform::Sawtooth, Waveform::Sine]);
            p.env.decay = r.range(0.15, 0.4);
            p.sfx.jump_semitones = r.pick(&[3.0, 4.0, 5.0, 7.0, 12.0]);
            p.sfx.jump_time = r.range(0.03, 0.1);
        }
        SfxKind::Laser => {
            p.osc.waveform = r.pick(&[
                Waveform::Pulse,
                Waveform::Sawtooth,
                Waveform::Sine,
                Waveform::PulseSaw,
            ]);
            p.osc.pulse_width = r.range(0.1, 0.5);
            p.env.decay = r.range(0.1, 0.3);
            p.sfx.slide = -r.range(3.0, 10.0);
            if r.chance(0.5) {
                p.sfx.slide_accel = r.range(0.0, 15.0);
            }
            if r.chance(0.4) {
                p.sfx.pw_sweep = r.range(-2.0, 2.0);
            }
        }
        SfxKind::Explosion => {
            p.osc.waveform = Waveform::Noise;
            p.env.decay = r.range(0.4, 1.2);
            p.filter.cutoff = r.range(1_500.0, 8_000.0);
            p.sfx.slide = -r.range(0.3, 2.0);
            if r.chance(0.3) {
                p.sfx.repeat_rate = r.range(4.0, 12.0);
            }
            if r.chance(0.3) {
                p.crusher.bits = r.range(4.0, 8.0);
            }
        }
        SfxKind::PowerUp => {
            p.osc.waveform = r.pick(&[Waveform::Pulse, Waveform::Sawtooth, Waveform::PulseSaw]);
            p.env.decay = r.range(0.3, 0.7);
            p.sfx.slide = r.range(1.0, 4.0);
            // Half repeat, a quarter sweep the pulse width, the rest neither.
            match r.unit() {
                u if u < 0.5 => p.sfx.repeat_rate = r.range(6.0, 15.0),
                u if u < 0.75 => p.sfx.pw_sweep = r.range(-1.0, 1.0),
                _ => {}
            }
        }
        SfxKind::Hit => {
            p.osc.waveform = r.pick(&[Waveform::Noise, Waveform::Pulse, Waveform::Sawtooth]);
            p.env.decay = r.range(0.05, 0.2);
            p.sfx.slide = -r.range(2.0, 8.0);
            if r.chance(0.5) {
                p.filter.cutoff = r.range(2_000.0, 8_000.0);
            }
        }
        SfxKind::Jump => {
            p.osc.pulse_width = r.range(0.3, 0.5);
            p.env.decay = r.range(0.1, 0.3);
            p.sfx.slide = r.range(1.0, 3.0);
        }
        SfxKind::Blip => {
            p.osc.waveform = r.pick(&[Waveform::Pulse, Waveform::Sine]);
            p.env.decay = r.range(0.03, 0.1);
            if r.chance(0.5) {
                p.filter.cutoff = r.range(2_000.0, 12_000.0);
            }
        }
    }
    p
}

/// A patch with every parameter random within its valid range, except
/// `global.volume`, which keeps its default so the result stays audible.
#[must_use]
pub fn randomize(seed: u64) -> SynthParams {
    let mut r = Rng(seed);
    let mut p = SynthParams::default();
    for &param in Param::ALL.iter().filter(|&&q| q != Param::Volume) {
        #[allow(clippy::cast_precision_loss)] // a handful of choices
        let value = match param.kind() {
            ParamKind::Float { min, max } => r.range(min, max),
            ParamKind::Choice(names) => r.range(0.0, names.len() as f32 - 1.0),
            ParamKind::Toggle => r.unit(),
        };
        param.set(&mut p, value);
    }
    p
}

/// `params` with every float parameter nudged by up to ±5% of its range,
/// staying in range. Choices and toggles are kept.
#[must_use]
pub fn mutate(params: &SynthParams, seed: u64) -> SynthParams {
    let mut r = Rng(seed);
    let mut p = params.clone();
    for &param in Param::ALL {
        if let ParamKind::Float { min, max } = param.kind() {
            let value = param.get(&p) + r.range(-0.05, 0.05) * (max - min);
            param.set(&mut p, value);
        }
    }
    p
}

/// `SplitMix64`: tiny, seedable, and good enough for picking parameters.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0.0..1.0`.
    #[allow(clippy::cast_precision_loss)] // 24 bits fit an f32 exactly
    fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1_u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }

    fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }

    #[allow(clippy::cast_possible_truncation)] // the index is below `items.len()`
    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[(self.next_u64() % items.len() as u64) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::voice::Voice;
    use crate::params::MidiNote;

    /// Hold C5 for 0.3 s, then release, rendering 1.5 s in total.
    fn render(params: &SynthParams) -> Vec<f32> {
        let mut voice = Voice::new();
        voice.note_on(MidiNote(72), params, 44100.0);
        (0..66_150)
            .map(|i| {
                if i == 13_230 {
                    voice.note_off();
                }
                voice.process(params, 44100.0)
            })
            .collect()
    }

    #[test]
    fn every_category_sounds_without_clipping_for_many_seeds() {
        for &kind in SfxKind::ALL {
            for seed in 0..64 {
                let p = generate(kind, seed);
                let s = render(&p);
                let peak = s.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
                assert!(s.iter().all(|x| x.is_finite()), "{kind:?} {seed}");
                assert!(peak > 0.05, "{kind:?} seed {seed} is silent: {peak}");
                assert!(peak < 1.0, "{kind:?} seed {seed} clips: {peak}");
                let mut sane = p.clone();
                sane.sanitize();
                assert_eq!(sane, p, "{kind:?} seed {seed} is out of range");
            }
        }
    }

    #[test]
    fn categories_keep_their_signature() {
        for seed in 0..32 {
            assert!(generate(SfxKind::Laser, seed).sfx.slide < 0.0);
            assert!(generate(SfxKind::Hit, seed).sfx.slide < 0.0);
            assert!(generate(SfxKind::PowerUp, seed).sfx.slide > 0.0);
            assert!(generate(SfxKind::Jump, seed).sfx.slide > 0.0);
            assert!(generate(SfxKind::Pickup, seed).sfx.jump_semitones > 0.0);
            assert_eq!(
                generate(SfxKind::Explosion, seed).osc.waveform,
                Waveform::Noise
            );
        }
    }

    #[test]
    fn same_seed_same_patch() {
        for &kind in SfxKind::ALL {
            assert_eq!(generate(kind, 9), generate(kind, 9));
            assert_ne!(generate(kind, 9), generate(kind, 10), "{kind:?}");
        }
        assert_eq!(randomize(3), randomize(3));
        let p = generate(SfxKind::Laser, 1);
        assert_eq!(mutate(&p, 5), mutate(&p, 5));
    }

    #[test]
    fn randomize_and_mutate_stay_in_range() {
        for seed in 0..64 {
            let p = randomize(seed);
            assert_eq!(p.global.volume, SynthParams::default().global.volume);
            let mut sane = p.clone();
            sane.sanitize();
            assert_eq!(sane, p, "randomize({seed})");

            // Mutating the range edges must clamp, not escape.
            let m = mutate(&p, seed);
            let mut sane = m.clone();
            sane.sanitize();
            assert_eq!(sane, m, "mutate(randomize({seed}))");
            assert_eq!(m.osc.waveform, p.osc.waveform, "choices are kept");
        }
    }
}
