//! Addressable patch parameters.
//!
//! [`Param`] names every settable field of a [`SynthParams`] by its path in
//! the patch JSON (`"filter.cutoff"`), together with its kind and valid range.
//! It is the single source of truth for those ranges: [`SynthParams::sanitize`]
//! is built on it, and [`AudioEvent::SetParam`](crate::params::AudioEvent::SetParam)
//! uses it to change one field of a playing patch.
//!
//! Every value travels as an `f32`: floats as themselves, choices as the index
//! into [`ParamKind::Choice`]'s names, toggles as `0.0` or `1.0`.

#[cfg(feature = "arp")]
use crate::params::ArpMode;
use crate::params::{FilterMode, LfoShape, LfoTarget, RingModMode, SynthParams, Waveform};

/// What kind of value a [`Param`] takes, and which values are valid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamKind {
    /// A number in `min..=max`.
    Float {
        /// Smallest valid value.
        min: f32,
        /// Largest valid value.
        max: f32,
    },
    /// One of these named options, addressed by index.
    Choice(&'static [&'static str]),
    /// Off (`0.0`) or on (`1.0`).
    Toggle,
}

const fn float(min: f32, max: f32) -> ParamKind {
    ParamKind::Float { min, max }
}

const TOGGLE: ParamKind = ParamKind::Toggle;

/// Index of `value` in `all`, as a parameter value.
#[allow(clippy::cast_precision_loss)] // a handful of variants
fn index_of<T: PartialEq>(all: &[T], value: &T) -> f32 {
    all.iter().position(|v| v == value).unwrap_or(0) as f32
}

/// The variant at index `v` (already rounded and clamped by [`Param::set`]).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn pick<T: Copy>(all: &[T], v: f32) -> T {
    all[(v as usize).min(all.len() - 1)]
}

fn toggle(on: bool) -> f32 {
    if on { 1.0 } else { 0.0 }
}

macro_rules! params {
    ($(
        $(#[$meta:meta])*
        $variant:ident = $path:literal, $kind:expr,
            get |$gp:ident| $get:expr,
            set |$sp:ident, $sv:ident| $set:expr;
    )*) => {
        /// A settable patch parameter; see the [module docs](self).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Param {
            $( $(#[$meta])* #[doc = concat!("`", $path, "`")] $variant, )*
        }

        impl Param {
            /// Every parameter, in patch order.
            pub const ALL: &'static [Param] = &[ $( $(#[$meta])* Param::$variant, )* ];

            /// The parameter's path in the patch JSON, e.g. `"filter.cutoff"`.
            #[must_use]
            pub fn path(self) -> &'static str {
                match self { $( $(#[$meta])* Param::$variant => $path, )* }
            }

            /// The kind of value the parameter takes, and its valid range.
            #[must_use]
            pub fn kind(self) -> ParamKind {
                match self { $( $(#[$meta])* Param::$variant => $kind, )* }
            }

            /// The parameter's current value in `params`.
            #[must_use]
            pub fn get(self, params: &SynthParams) -> f32 {
                match self { $( $(#[$meta])* Param::$variant => { let $gp = params; $get } )* }
            }

            fn set_raw(self, params: &mut SynthParams, value: f32) {
                match self {
                    $( $(#[$meta])* Param::$variant => {
                        let ($sp, $sv) = (params, value);
                        $set;
                    } )*
                }
            }
        }
    };
}

params! {
    OscWaveform = "osc.waveform", ParamKind::Choice(Waveform::NAMES),
        get |p| index_of(Waveform::ALL, &p.osc.waveform),
        set |p, v| p.osc.waveform = pick(Waveform::ALL, v);
    OscPulseWidth = "osc.pulse_width", float(0.05, 0.95),
        get |p| p.osc.pulse_width, set |p, v| p.osc.pulse_width = v;
    OscDetune = "osc.detune", float(-100.0, 100.0),
        get |p| p.osc.detune, set |p, v| p.osc.detune = v;
    OscNoiseMix = "osc.noise_mix", float(0.0, 1.0),
        get |p| p.osc.noise_mix, set |p, v| p.osc.noise_mix = v;

    EnvAttack = "env.attack", float(0.001, 10.0),
        get |p| p.env.attack, set |p, v| p.env.attack = v;
    EnvDecay = "env.decay", float(0.001, 10.0),
        get |p| p.env.decay, set |p, v| p.env.decay = v;
    EnvSustain = "env.sustain", float(0.0, 1.0),
        get |p| p.env.sustain, set |p, v| p.env.sustain = v;
    EnvRelease = "env.release", float(0.001, 20.0),
        get |p| p.env.release, set |p, v| p.env.release = v;
    EnvReverse = "env.env_reverse", TOGGLE,
        get |p| toggle(p.env.env_reverse), set |p, v| p.env.env_reverse = v >= 0.5;

    FilterMode = "filter.filter_mode", ParamKind::Choice(FilterMode::NAMES),
        get |p| index_of(FilterMode::ALL, &p.filter.filter_mode),
        set |p, v| p.filter.filter_mode = pick(FilterMode::ALL, v);
    FilterCutoff = "filter.cutoff", float(20.0, 18_000.0),
        get |p| p.filter.cutoff, set |p, v| p.filter.cutoff = v;
    FilterResonance = "filter.resonance", float(0.0, 0.99),
        get |p| p.filter.resonance, set |p, v| p.filter.resonance = v;
    FilterDrive = "filter.drive", float(0.0, 1.0),
        get |p| p.filter.drive, set |p, v| p.filter.drive = v;

    LfoRate = "lfo.lfo_rate", float(0.0, 50.0),
        get |p| p.lfo.lfo_rate, set |p, v| p.lfo.lfo_rate = v;
    LfoDepth = "lfo.lfo_depth", float(0.0, 1.0),
        get |p| p.lfo.lfo_depth, set |p, v| p.lfo.lfo_depth = v;
    LfoTarget = "lfo.lfo_target", ParamKind::Choice(LfoTarget::NAMES),
        get |p| index_of(LfoTarget::ALL, &p.lfo.lfo_target),
        set |p, v| p.lfo.lfo_target = pick(LfoTarget::ALL, v);
    LfoShape = "lfo.lfo_shape", ParamKind::Choice(LfoShape::NAMES),
        get |p| index_of(LfoShape::ALL, &p.lfo.lfo_shape),
        set |p, v| p.lfo.lfo_shape = pick(LfoShape::ALL, v);

    ReverbMix = "fx.reverb_mix", float(0.0, 1.0),
        get |p| p.fx.reverb_mix, set |p, v| p.fx.reverb_mix = v;
    ReverbSize = "fx.reverb_size", float(0.0, 1.0),
        get |p| p.fx.reverb_size, set |p, v| p.fx.reverb_size = v;
    ReverbDamping = "fx.reverb_damping", float(0.0, 1.0),
        get |p| p.fx.reverb_damping, set |p, v| p.fx.reverb_damping = v;

    CrusherBits = "crusher.bits", float(1.0, 16.0),
        get |p| p.crusher.bits, set |p, v| p.crusher.bits = v;
    CrusherRate = "crusher.rate", float(1.0, 16.0),
        get |p| p.crusher.rate, set |p, v| p.crusher.rate = v;

    ChorusRate = "chorus.rate", float(0.1, 5.0),
        get |p| p.chorus.rate, set |p, v| p.chorus.rate = v;
    ChorusDepth = "chorus.depth_ms", float(0.0, 10.0),
        get |p| p.chorus.depth_ms, set |p, v| p.chorus.depth_ms = v;
    ChorusMix = "chorus.mix", float(0.0, 1.0),
        get |p| p.chorus.mix, set |p, v| p.chorus.mix = v;

    DelayTime = "delay.time_ms", float(1.0, 2_000.0),
        get |p| p.delay.time_ms, set |p, v| p.delay.time_ms = v;
    DelayFeedback = "delay.feedback", float(0.0, 0.95),
        get |p| p.delay.feedback, set |p, v| p.delay.feedback = v;
    DelayMix = "delay.mix", float(0.0, 1.0),
        get |p| p.delay.mix, set |p, v| p.delay.mix = v;

    Osc2Waveform = "osc2.waveform", ParamKind::Choice(Waveform::NAMES),
        get |p| index_of(Waveform::ALL, &p.osc2.waveform),
        set |p, v| p.osc2.waveform = pick(Waveform::ALL, v);
    Osc2Detune = "osc2.detune", float(-100.0, 100.0),
        get |p| p.osc2.detune, set |p, v| p.osc2.detune = v;
    Osc2Mix = "osc2.osc2_mix", float(0.0, 1.0),
        get |p| p.osc2.osc2_mix, set |p, v| p.osc2.osc2_mix = v;
    Osc2HardSync = "osc2.hard_sync", TOGGLE,
        get |p| toggle(p.osc2.hard_sync), set |p, v| p.osc2.hard_sync = v >= 0.5;
    Osc2RingMod = "osc2.ring_mod", ParamKind::Choice(RingModMode::NAMES),
        get |p| index_of(RingModMode::ALL, &p.osc2.ring_mod),
        set |p, v| p.osc2.ring_mod = pick(RingModMode::ALL, v);

    Lfo2Rate = "lfo2.lfo_rate", float(0.0, 50.0),
        get |p| p.lfo2.lfo_rate, set |p, v| p.lfo2.lfo_rate = v;
    Lfo2Depth = "lfo2.lfo_depth", float(0.0, 1.0),
        get |p| p.lfo2.lfo_depth, set |p, v| p.lfo2.lfo_depth = v;
    Lfo2Target = "lfo2.lfo_target", ParamKind::Choice(LfoTarget::NAMES),
        get |p| index_of(LfoTarget::ALL, &p.lfo2.lfo_target),
        set |p, v| p.lfo2.lfo_target = pick(LfoTarget::ALL, v);
    Lfo2Shape = "lfo2.lfo_shape", ParamKind::Choice(LfoShape::NAMES),
        get |p| index_of(LfoShape::ALL, &p.lfo2.lfo_shape),
        set |p, v| p.lfo2.lfo_shape = pick(LfoShape::ALL, v);

    FilterEnvAttack = "filter_env.attack", float(0.001, 10.0),
        get |p| p.filter_env.attack, set |p, v| p.filter_env.attack = v;
    FilterEnvDecay = "filter_env.decay", float(0.001, 10.0),
        get |p| p.filter_env.decay, set |p, v| p.filter_env.decay = v;
    FilterEnvSustain = "filter_env.sustain", float(0.0, 1.0),
        get |p| p.filter_env.sustain, set |p, v| p.filter_env.sustain = v;
    FilterEnvRelease = "filter_env.release", float(0.001, 20.0),
        get |p| p.filter_env.release, set |p, v| p.filter_env.release = v;
    FilterEnvDepth = "filter_env.depth", float(-1.0, 1.0),
        get |p| p.filter_env.depth, set |p, v| p.filter_env.depth = v;

    PitchEnvAttack = "pitch_env.attack", float(0.001, 10.0),
        get |p| p.pitch_env.attack, set |p, v| p.pitch_env.attack = v;
    PitchEnvDecay = "pitch_env.decay", float(0.001, 10.0),
        get |p| p.pitch_env.decay, set |p, v| p.pitch_env.decay = v;
    PitchEnvSustain = "pitch_env.sustain", float(0.0, 1.0),
        get |p| p.pitch_env.sustain, set |p, v| p.pitch_env.sustain = v;
    PitchEnvRelease = "pitch_env.release", float(0.001, 20.0),
        get |p| p.pitch_env.release, set |p, v| p.pitch_env.release = v;
    PitchEnvDepth = "pitch_env.depth", float(-1.0, 1.0),
        get |p| p.pitch_env.depth, set |p, v| p.pitch_env.depth = v;

    #[cfg(feature = "arp")]
    ArpEnabled = "arp.enabled", TOGGLE,
        get |p| toggle(p.arp.enabled), set |p, v| p.arp.enabled = v >= 0.5;
    #[cfg(feature = "arp")]
    ArpRate = "arp.rate", float(0.0, 50.0),
        get |p| p.arp.rate, set |p, v| p.arp.rate = v;
    #[cfg(feature = "arp")]
    ArpGate = "arp.gate", float(0.0, 1.0),
        get |p| p.arp.gate, set |p, v| p.arp.gate = v;
    #[cfg(feature = "arp")]
    ArpMode = "arp.mode", ParamKind::Choice(ArpMode::NAMES),
        get |p| index_of(ArpMode::ALL, &p.arp.mode),
        set |p, v| p.arp.mode = pick(ArpMode::ALL, v);

    Volume = "global.volume", float(0.0, 1.0),
        get |p| p.global.volume, set |p, v| p.global.volume = v;
    GlideTime = "global.glide_time", float(0.0, 10.0),
        get |p| p.global.glide_time, set |p, v| p.global.glide_time = v;
}

impl Param {
    /// The parameter with this patch path, e.g. `"filter.cutoff"`.
    #[must_use]
    pub fn from_path(path: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|p| p.path() == path)
    }

    /// Set the parameter in `params`, brought into range: floats are
    /// clamped, choice indices rounded and clamped, toggles are on from
    /// `0.5`. A non-finite `value` is ignored.
    pub fn set(self, params: &mut SynthParams, value: f32) {
        if !value.is_finite() {
            return;
        }
        let value = match self.kind() {
            ParamKind::Float { min, max } => value.clamp(min, max),
            #[allow(clippy::cast_precision_loss)] // a handful of variants
            ParamKind::Choice(names) => value.round().clamp(0.0, (names.len() - 1) as f32),
            ParamKind::Toggle => toggle(value >= 0.5),
        };
        self.set_raw(params, value);
    }

    /// The parameter's value in the default patch.
    #[must_use]
    pub fn default_value(self) -> f32 {
        self.get(&SynthParams::default())
    }

    /// Whether the parameter belongs to the shared FX chain (reverb, chorus,
    /// delay). The processor reads those from channel 0's patch only; the
    /// same fields in other channels' patches have no effect.
    #[must_use]
    pub fn is_shared(self) -> bool {
        let path = self.path();
        path.starts_with("fx.") || path.starts_with("chorus.") || path.starts_with("delay.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_unique_and_resolve() {
        for &param in Param::ALL {
            assert_eq!(Param::from_path(param.path()), Some(param));
        }
        assert_eq!(Param::from_path("filter.nope"), None);
    }

    #[test]
    fn get_after_set_round_trips_every_valid_value() {
        let mut p = SynthParams::default();
        for &param in Param::ALL {
            let values: Vec<f32> = match param.kind() {
                ParamKind::Float { min, max } => vec![min, max, f32::midpoint(min, max)],
                #[allow(clippy::cast_precision_loss)]
                ParamKind::Choice(names) => (0..names.len()).map(|i| i as f32).collect(),
                ParamKind::Toggle => vec![0.0, 1.0],
            };
            for v in values {
                param.set(&mut p, v);
                assert_eq!(param.get(&p).to_bits(), v.to_bits(), "{}", param.path());
            }
        }
    }

    #[test]
    fn set_brings_values_into_range() {
        let mut p = SynthParams::default();
        Param::FilterCutoff.set(&mut p, 1e30);
        assert_eq!(p.filter.cutoff, 18_000.0);
        Param::OscWaveform.set(&mut p, 99.0);
        assert_eq!(p.osc.waveform, Waveform::Sine);
        Param::OscWaveform.set(&mut p, 0.6);
        assert_eq!(p.osc.waveform, Waveform::Sawtooth);
        Param::EnvReverse.set(&mut p, 0.7);
        assert!(p.env.env_reverse);
        Param::FilterCutoff.set(&mut p, f32::NAN);
        assert_eq!(p.filter.cutoff, 18_000.0, "NaN is ignored");
    }

    #[test]
    fn setting_one_parameter_changes_nothing_else() {
        for &param in Param::ALL {
            let mut p = SynthParams::default();
            let other = |p: &SynthParams| -> Vec<u32> {
                Param::ALL
                    .iter()
                    .filter(|&&q| q != param)
                    .map(|q| q.get(p).to_bits())
                    .collect()
            };
            let before = other(&p);
            let target = match param.kind() {
                ParamKind::Float { max, .. } => max,
                #[allow(clippy::cast_precision_loss)]
                ParamKind::Choice(names) => (names.len() - 1) as f32,
                ParamKind::Toggle => 1.0 - param.get(&p),
            };
            param.set(&mut p, target);
            assert_eq!(other(&p), before, "{} touched another field", param.path());
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn paths_and_choice_names_match_the_patch_json() {
        let json = serde_json::to_value(SynthParams::default()).unwrap();
        for &param in Param::ALL {
            let (section, field) = param.path().split_once('.').unwrap();
            let value = &json[section][field];
            match param.kind() {
                ParamKind::Float { .. } => assert!(value.is_number(), "{}", param.path()),
                ParamKind::Toggle => assert!(value.is_boolean(), "{}", param.path()),
                ParamKind::Choice(names) => {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let current = names[param.get(&SynthParams::default()) as usize];
                    assert_eq!(value, current, "{}", param.path());
                    // Every name must deserialize: set each and compare JSON.
                    let mut p = SynthParams::default();
                    for (i, name) in names.iter().enumerate() {
                        #[allow(clippy::cast_precision_loss)]
                        param.set(&mut p, i as f32);
                        let json = serde_json::to_value(&p).unwrap();
                        assert_eq!(&json[section][field], name, "{}", param.path());
                    }
                }
            }
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn every_scalar_patch_field_has_a_param() {
        let json = serde_json::to_value(SynthParams::default()).unwrap();
        for (section, fields) in json.as_object().unwrap() {
            for (field, value) in fields.as_object().unwrap() {
                let path = format!("{section}.{field}");
                // Runtime arpeggiator state, not a setting.
                if path == "arp.notes" || path == "arp.count" {
                    continue;
                }
                assert!(value.is_number() || value.is_boolean() || value.is_string());
                assert!(Param::from_path(&path).is_some(), "no Param for {path}");
            }
        }
    }
}
