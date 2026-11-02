//! Built-in C64/SID-inspired preset bank.
//!
//! Each patch targets a specific sonic archetype from classic C64 music.
//! The inline sound-design notes next to each preset describe the intent so
//! the parameter choices remain legible without running the synth.
//!
//! Pick a preset by its [`Preset`] variant; the names are for UIs and files:
//!
//! ```
//! use synthie::presets::sid::Preset;
//!
//! let bass = Preset::C64Bass.params();
//! assert_eq!("C64 Bass".parse::<Preset>(), Ok(Preset::C64Bass));
//! assert_eq!(Preset::C64Bass.name(), "C64 Bass");
//! ```

use core::fmt;
use core::str::FromStr;

use crate::params::{
    DelayParams, EnvParams, FilterMode, FilterParams, FxParams, GlobalParams, LfoParams, LfoShape,
    LfoTarget, OscParams, Patch, SynthParams, Waveform,
};

/// A built-in preset of the SID bank.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Preset {
    /// Iconic SID pluck bass: sawtooth, short decay, resonant low-pass.
    C64Bass,
    /// Fast-attack bright pulse with pitch vibrato.
    ArpLead,
    /// Short percussive pulse stab, no sustain.
    PluckyPulse,
    /// Pulse-width modulation for a flute- or cello-like lead.
    PwmLead,
    /// Thin, harsh and metallic: narrow pulse through a band-pass.
    MetalPulse,
    /// Gated noise burst through a band-pass, snare-like.
    NoiseSnare,
    /// Sawtooth bass with portamento glide.
    SlideBass,
    /// Soft triangle pad with a slow attack and a reverb wash.
    TriPad,
    /// Chugging, distorted rhythm guitar; plays power chords well.
    DriveGuitar,
    /// Singing, overdriven pulse lead with a wide vibrato.
    RockLead,
    /// Plucked, bell-like triangle that dies away.
    MusicBox,
    /// NES-style triangle bass, nearly clean.
    TriBass,
    /// Sawtooth horn with a soft attack, wide vibrato and a little glide.
    Brass,
    /// Soft square lead with a single, quieter echo. The delay is a master
    /// effect set by channel 0's patch, so the echo needs channel 0.
    EchoLead,
}

impl Preset {
    /// Every preset, in bank order (the order of [`default_patches`]).
    pub const ALL: &'static [Preset] = &[
        Preset::C64Bass,
        Preset::ArpLead,
        Preset::PluckyPulse,
        Preset::PwmLead,
        Preset::MetalPulse,
        Preset::NoiseSnare,
        Preset::SlideBass,
        Preset::TriPad,
        Preset::DriveGuitar,
        Preset::RockLead,
        Preset::MusicBox,
        Preset::TriBass,
        Preset::Brass,
        Preset::EchoLead,
    ];

    /// The display name, e.g. `"C64 Bass"`; [`FromStr`] parses it back.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Preset::C64Bass => "C64 Bass",
            Preset::ArpLead => "Arp Lead",
            Preset::PluckyPulse => "Plucky Pulse",
            Preset::PwmLead => "PWM Lead",
            Preset::MetalPulse => "Metal Pulse",
            Preset::NoiseSnare => "Noise Snare",
            Preset::SlideBass => "Slide Bass",
            Preset::TriPad => "Tri Pad",
            Preset::DriveGuitar => "Drive Guitar",
            Preset::RockLead => "Rock Lead",
            Preset::MusicBox => "Music Box",
            Preset::TriBass => "Tri Bass",
            Preset::Brass => "Brass",
            Preset::EchoLead => "Echo Lead",
        }
    }

    /// The preset's parameters.
    #[must_use]
    #[allow(clippy::too_many_lines)] // one patch per arm; splitting them up hides the bank
    pub fn params(self) -> SynthParams {
        match self {
            Preset::C64Bass => SynthParams {
                osc: OscParams {
                    waveform: Waveform::Sawtooth,
                    ..OscParams::default()
                },
                env: EnvParams {
                    attack: 0.002,
                    decay: 0.18,
                    sustain: 0.0,
                    release: 0.15,
                    ..EnvParams::default()
                },
                filter: FilterParams {
                    cutoff: 900.0,
                    resonance: 0.55,
                    drive: 0.15,
                    ..FilterParams::default()
                },
                lfo: NO_LFO,
                fx: FxParams {
                    reverb_mix: 0.05,
                    reverb_size: 0.3,
                    reverb_damping: 0.6,
                },
                global: GlobalParams {
                    volume: 0.75,
                    glide_time: 0.0,
                },
                ..SynthParams::default()
            },
            Preset::ArpLead => SynthParams {
                osc: OscParams {
                    pulse_width: 0.25,
                    detune: 3.0,
                    ..OscParams::default()
                },
                env: EnvParams {
                    attack: 0.003,
                    decay: 0.06,
                    sustain: 0.7,
                    release: 0.12,
                    ..EnvParams::default()
                },
                filter: FilterParams {
                    cutoff: 3500.0,
                    ..FilterParams::default()
                },
                lfo: LfoParams {
                    lfo_rate: 5.5,
                    lfo_depth: 0.12,
                    ..LfoParams::default()
                },
                fx: FxParams {
                    reverb_mix: 0.1,
                    reverb_size: 0.4,
                    reverb_damping: 0.5,
                },
                global: GlobalParams {
                    volume: 0.75,
                    glide_time: 0.0,
                },
                ..SynthParams::default()
            },
            Preset::PluckyPulse => SynthParams {
                env: EnvParams {
                    attack: 0.001,
                    decay: 0.25,
                    sustain: 0.0,
                    release: 0.1,
                    ..EnvParams::default()
                },
                filter: FilterParams {
                    cutoff: 2200.0,
                    resonance: 0.6,
                    drive: 0.1,
                    ..FilterParams::default()
                },
                lfo: NO_LFO,
                fx: FxParams {
                    reverb_mix: 0.12,
                    reverb_size: 0.35,
                    reverb_damping: 0.55,
                },
                global: GlobalParams {
                    volume: 0.75,
                    glide_time: 0.0,
                },
                ..SynthParams::default()
            },
            Preset::PwmLead => SynthParams {
                env: EnvParams {
                    attack: 0.04,
                    decay: 0.1,
                    sustain: 0.9,
                    release: 0.25,
                    ..EnvParams::default()
                },
                filter: FilterParams {
                    resonance: 0.2,
                    ..FilterParams::default()
                },
                lfo: LfoParams {
                    lfo_rate: 2.8,
                    lfo_depth: 0.45,
                    lfo_target: LfoTarget::PulseWidth,
                    ..LfoParams::default()
                },
                fx: FxParams {
                    reverb_mix: 0.18,
                    reverb_size: 0.55,
                    reverb_damping: 0.4,
                },
                global: GlobalParams {
                    volume: 0.7,
                    glide_time: 0.03,
                },
                ..SynthParams::default()
            },
            Preset::MetalPulse => SynthParams {
                osc: OscParams {
                    waveform: Waveform::PulseSaw,
                    pulse_width: 0.08,
                    detune: 8.0,
                    noise_mix: 0.05,
                },
                env: EnvParams {
                    attack: 0.001,
                    decay: 0.08,
                    sustain: 0.5,
                    release: 0.08,
                    ..EnvParams::default()
                },
                filter: FilterParams {
                    filter_mode: FilterMode::BandPass,
                    cutoff: 1800.0,
                    resonance: 0.65,
                    drive: 0.2,
                },
                lfo: NO_LFO,
                fx: FxParams {
                    reverb_mix: 0.08,
                    reverb_size: 0.25,
                    reverb_damping: 0.7,
                },
                global: GlobalParams {
                    volume: 0.65,
                    glide_time: 0.0,
                },
                ..SynthParams::default()
            },
            Preset::NoiseSnare => SynthParams {
                osc: OscParams {
                    waveform: Waveform::Noise,
                    noise_mix: 1.0,
                    ..OscParams::default()
                },
                env: EnvParams {
                    attack: 0.001,
                    decay: 0.12,
                    sustain: 0.0,
                    release: 0.05,
                    ..EnvParams::default()
                },
                filter: FilterParams {
                    filter_mode: FilterMode::BandPass,
                    cutoff: 3500.0,
                    resonance: 0.4,
                    drive: 0.25,
                },
                lfo: NO_LFO,
                fx: FxParams {
                    reverb_mix: 0.1,
                    reverb_size: 0.2,
                    reverb_damping: 0.8,
                },
                global: GlobalParams {
                    volume: 0.8,
                    glide_time: 0.0,
                },
                ..SynthParams::default()
            },
            Preset::SlideBass => SynthParams {
                osc: OscParams {
                    waveform: Waveform::Sawtooth,
                    ..OscParams::default()
                },
                env: EnvParams {
                    attack: 0.004,
                    decay: 0.12,
                    sustain: 0.6,
                    release: 0.2,
                    ..EnvParams::default()
                },
                filter: FilterParams {
                    cutoff: 800.0,
                    resonance: 0.45,
                    drive: 0.2,
                    ..FilterParams::default()
                },
                lfo: NO_LFO,
                fx: FxParams {
                    reverb_mix: 0.05,
                    reverb_size: 0.3,
                    reverb_damping: 0.6,
                },
                global: GlobalParams {
                    volume: 0.75,
                    glide_time: 0.12,
                },
                ..SynthParams::default()
            },
            Preset::TriPad => SynthParams {
                osc: OscParams {
                    waveform: Waveform::Triangle,
                    detune: -5.0,
                    ..OscParams::default()
                },
                env: EnvParams {
                    attack: 0.35,
                    decay: 0.2,
                    sustain: 0.85,
                    release: 0.8,
                    ..EnvParams::default()
                },
                filter: FilterParams {
                    cutoff: 2800.0,
                    resonance: 0.15,
                    ..FilterParams::default()
                },
                lfo: LfoParams {
                    lfo_rate: 0.8,
                    lfo_depth: 0.08,
                    ..LfoParams::default()
                },
                fx: FxParams {
                    reverb_mix: 0.45,
                    reverb_size: 0.75,
                    reverb_damping: 0.3,
                },
                global: GlobalParams {
                    volume: 0.65,
                    glide_time: 0.0,
                },
                ..SynthParams::default()
            },
            // From Cheesed!: hard-driven sawtooth with a tight, chugging
            // envelope. Play root, fifth and octave together for power chords.
            Preset::DriveGuitar => SynthParams {
                osc: OscParams {
                    waveform: Waveform::Sawtooth,
                    ..OscParams::default()
                },
                env: EnvParams {
                    attack: 0.004,
                    decay: 0.11,
                    sustain: 0.45,
                    release: 0.025,
                    ..EnvParams::default()
                },
                filter: FilterParams {
                    cutoff: 3200.0,
                    resonance: 0.2,
                    drive: 0.8,
                    ..FilterParams::default()
                },
                lfo: NO_LFO,
                fx: FxParams {
                    reverb_mix: 0.08,
                    reverb_size: 0.3,
                    reverb_damping: 0.6,
                },
                global: GlobalParams {
                    volume: 0.65,
                    glide_time: 0.0,
                },
                ..SynthParams::default()
            },
            // From Cheesed!: overdriven narrow pulse with a slow decay to a
            // high sustain, and a wide 6.5 Hz vibrato (about ±0.45 semitones).
            Preset::RockLead => SynthParams {
                osc: OscParams {
                    pulse_width: 0.3,
                    ..OscParams::default()
                },
                env: EnvParams {
                    attack: 0.01,
                    decay: 0.5,
                    sustain: 0.75,
                    release: 0.1,
                    ..EnvParams::default()
                },
                filter: FilterParams {
                    resonance: 0.25,
                    drive: 0.71,
                    ..FilterParams::default()
                },
                lfo: LfoParams {
                    lfo_rate: 6.5,
                    lfo_depth: 0.375,
                    ..LfoParams::default()
                },
                fx: FxParams {
                    reverb_mix: 0.15,
                    reverb_size: 0.45,
                    reverb_damping: 0.5,
                },
                global: GlobalParams {
                    volume: 0.65,
                    glide_time: 0.0,
                },
                ..SynthParams::default()
            },
            // From Cheesed!: a plucked triangle with no sustain and a faint
            // 5 Hz shimmer, through an open filter and a roomy reverb.
            Preset::MusicBox => SynthParams {
                osc: OscParams {
                    waveform: Waveform::Triangle,
                    ..OscParams::default()
                },
                env: EnvParams {
                    attack: 0.002,
                    decay: 0.18,
                    sustain: 0.0,
                    release: 0.17,
                    ..EnvParams::default()
                },
                filter: FilterParams {
                    cutoff: 6000.0,
                    resonance: 0.1,
                    ..FilterParams::default()
                },
                lfo: LfoParams {
                    lfo_rate: 5.0,
                    lfo_depth: 0.04,
                    ..LfoParams::default()
                },
                fx: FxParams {
                    reverb_mix: 0.25,
                    reverb_size: 0.5,
                    reverb_damping: 0.4,
                },
                global: GlobalParams {
                    volume: 0.75,
                    glide_time: 0.0,
                },
                ..SynthParams::default()
            },
            // From Cheesed! and Cookie D'oh!: a NES-style triangle bass with a
            // short release so lines stay tight. Barely driven, so a game can
            // add its own grit (Cheesed! uses drive 0.5).
            Preset::TriBass => SynthParams {
                osc: OscParams {
                    waveform: Waveform::Triangle,
                    ..OscParams::default()
                },
                env: EnvParams {
                    attack: 0.004,
                    decay: 0.17,
                    sustain: 0.7,
                    release: 0.03,
                    ..EnvParams::default()
                },
                filter: FilterParams {
                    cutoff: 2000.0,
                    resonance: 0.2,
                    drive: 0.1,
                    ..FilterParams::default()
                },
                lfo: NO_LFO,
                fx: FxParams {
                    reverb_mix: 0.05,
                    reverb_size: 0.3,
                    reverb_damping: 0.6,
                },
                global: GlobalParams {
                    volume: 0.8,
                    glide_time: 0.0,
                },
                ..SynthParams::default()
            },
            // From Cookie D'oh!'s sad trombone: a sawtooth horn with a soft
            // attack, a slow decay to a high sustain, a wide 6.5 Hz vibrato
            // and a short glide so it can slide between notes.
            Preset::Brass => SynthParams {
                osc: OscParams {
                    waveform: Waveform::Sawtooth,
                    ..OscParams::default()
                },
                env: EnvParams {
                    attack: 0.03,
                    decay: 0.5,
                    sustain: 0.8,
                    release: 0.125,
                    ..EnvParams::default()
                },
                filter: FilterParams {
                    cutoff: 2400.0,
                    resonance: 0.2,
                    drive: 0.1,
                    ..FilterParams::default()
                },
                lfo: LfoParams {
                    lfo_rate: 6.5,
                    lfo_depth: 0.375,
                    ..LfoParams::default()
                },
                fx: FxParams {
                    reverb_mix: 0.15,
                    reverb_size: 0.5,
                    reverb_damping: 0.5,
                },
                global: GlobalParams {
                    volume: 0.65,
                    glide_time: 0.06,
                },
                ..SynthParams::default()
            },
            // From Cookie D'oh!'s cracktro: a soft square lead with a gentle
            // 5 Hz vibrato and one quieter repeat (delay without feedback).
            // The delay is a master effect taken from channel 0's patch, so
            // play this on channel 0 for the echo; set `delay.time_ms` to
            // suit the tempo.
            Preset::EchoLead => SynthParams {
                env: EnvParams {
                    attack: 0.01,
                    decay: 0.33,
                    sustain: 0.5,
                    release: 0.17,
                    ..EnvParams::default()
                },
                filter: FilterParams {
                    cutoff: 3500.0,
                    resonance: 0.2,
                    ..FilterParams::default()
                },
                lfo: LfoParams {
                    lfo_rate: 5.0,
                    lfo_depth: 0.17,
                    ..LfoParams::default()
                },
                fx: FxParams {
                    reverb_mix: 0.1,
                    reverb_size: 0.4,
                    reverb_damping: 0.5,
                },
                delay: DelayParams {
                    time_ms: 450.0,
                    feedback: 0.0,
                    mix: 0.35,
                },
                global: GlobalParams {
                    volume: 0.7,
                    glide_time: 0.0,
                },
                ..SynthParams::default()
            },
        }
    }

    /// The preset as a named [`Patch`].
    #[must_use]
    pub fn patch(self) -> Patch {
        Patch::new(self.name(), self.params())
    }
}

/// A stopped LFO, for presets without vibrato or PWM.
const NO_LFO: LfoParams = LfoParams {
    lfo_rate: 0.0,
    lfo_depth: 0.0,
    lfo_target: LfoTarget::Pitch,
    lfo_shape: LfoShape::Sine,
};

impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A name that is not one of the built-in presets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownPreset(pub String);

impl fmt::Display for UnknownPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown preset `{}`", self.0)
    }
}

impl core::error::Error for UnknownPreset {}

impl FromStr for Preset {
    type Err = UnknownPreset;

    /// Parse a preset by its exact [display name](Preset::name).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Preset::ALL
            .iter()
            .copied()
            .find(|p| p.name() == s)
            .ok_or_else(|| UnknownPreset(s.into()))
    }
}

/// Return the built-in C64-inspired preset bank, in [`Preset::ALL`] order.
#[must_use]
pub fn default_patches() -> Vec<Patch> {
    Preset::ALL.iter().map(|p| p.patch()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_through_from_str() {
        for &preset in Preset::ALL {
            assert_eq!(preset.name().parse::<Preset>(), Ok(preset));
            assert_eq!(preset.to_string(), preset.name());
        }
        assert_eq!(
            "c64 bass".parse::<Preset>(),
            Err(UnknownPreset("c64 bass".into()))
        );
    }

    #[test]
    fn all_covers_every_variant_once() {
        // Adding a variant without listing it in `ALL` fails to compile here.
        for &preset in Preset::ALL {
            match preset {
                Preset::C64Bass
                | Preset::ArpLead
                | Preset::PluckyPulse
                | Preset::PwmLead
                | Preset::MetalPulse
                | Preset::NoiseSnare
                | Preset::SlideBass
                | Preset::TriPad
                | Preset::DriveGuitar
                | Preset::RockLead
                | Preset::MusicBox
                | Preset::TriBass
                | Preset::Brass
                | Preset::EchoLead => {}
            }
        }
        assert_eq!(Preset::ALL.len(), 14, "a variant is missing from ALL");
        for (i, a) in Preset::ALL.iter().enumerate() {
            assert!(!Preset::ALL[..i].contains(a), "{a} is listed twice");
        }
    }

    #[test]
    fn the_bank_follows_all() {
        let bank = default_patches();
        assert_eq!(bank.len(), Preset::ALL.len());
        for (patch, &preset) in bank.iter().zip(Preset::ALL) {
            assert_eq!(patch.name, preset.name());
            assert_eq!(patch.params, preset.params());
        }
    }

    #[test]
    fn presets_are_already_in_range() {
        for &preset in Preset::ALL {
            let params = preset.params();
            let mut sanitized = params.clone();
            sanitized.sanitize();
            assert_eq!(sanitized, params, "{preset} is out of range");
        }
    }
}
