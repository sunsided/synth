//! Core parameter and event types shared between the UI thread and audio thread.
//!
//! `SynthParams` is the canonical parameter snapshot.  The UI holds a live copy
//! and sends a boxed clone to the audio thread via `AudioEvent::LoadPatch`
//! whenever a value changes.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Oscillator waveform shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Waveform {
    /// Classic square/pulse wave; width controlled by `pulse_width`.
    Pulse,
    /// Band-limited sawtooth.
    Sawtooth,
    /// Triangle wave.
    Triangle,
    /// LFSR-based noise clocked at the oscillator period.
    Noise,
    /// 50/50 mix of pulse and sawtooth for a thicker timbre.
    PulseSaw,
    /// Pure sine wave.
    Sine,
}

impl Waveform {
    /// Ordered slice of all variants, used for cycling.
    pub const ALL: &'static [Waveform] = &[
        Waveform::Pulse,
        Waveform::Sawtooth,
        Waveform::Triangle,
        Waveform::Noise,
        Waveform::PulseSaw,
        Waveform::Sine,
    ];

    /// Short display name shown in the UI.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Waveform::Pulse => "Pulse",
            Waveform::Sawtooth => "Saw",
            Waveform::Triangle => "Tri",
            Waveform::Noise => "Noise",
            Waveform::PulseSaw => "Pls+Saw",
            Waveform::Sine => "Sine",
        }
    }

    /// Return the next variant, wrapping around.
    #[must_use]
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&w| w == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// Return the previous variant, wrapping around.
    #[must_use]
    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|&w| w == self).unwrap_or(0);
        let len = Self::ALL.len();
        Self::ALL[(idx + len - 1) % len]
    }
}

/// State-variable filter topology selector.
#[allow(clippy::enum_variant_names)] // LP/BP/HP suffix is standard audio industry terminology
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FilterMode {
    /// Low-pass output.
    LowPass,
    /// Band-pass output.
    BandPass,
    /// High-pass output.
    HighPass,
}

impl FilterMode {
    /// Ordered slice of all variants, used for cycling.
    pub const ALL: &'static [FilterMode] = &[
        FilterMode::LowPass,
        FilterMode::BandPass,
        FilterMode::HighPass,
    ];

    /// Short display name shown in the UI.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            FilterMode::LowPass => "LP",
            FilterMode::BandPass => "BP",
            FilterMode::HighPass => "HP",
        }
    }

    /// Return the next variant, wrapping around.
    #[must_use]
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&m| m == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }
}

/// Selects which parameter the LFO modulates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LfoTarget {
    /// Pitch modulation (vibrato).
    Pitch,
    /// Pulse-width modulation.
    PulseWidth,
    /// Filter cutoff modulation.
    Cutoff,
    /// Amplitude modulation (tremolo).
    Volume,
}

impl LfoTarget {
    /// Ordered slice of all variants, used for cycling.
    pub const ALL: &'static [LfoTarget] = &[
        LfoTarget::Pitch,
        LfoTarget::PulseWidth,
        LfoTarget::Cutoff,
        LfoTarget::Volume,
    ];

    /// Short display name shown in the UI.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            LfoTarget::Pitch => "Pitch",
            LfoTarget::PulseWidth => "PW",
            LfoTarget::Cutoff => "Cutoff",
            LfoTarget::Volume => "Volume",
        }
    }

    /// Return the next variant, wrapping around.
    #[must_use]
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&t| t == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// Return the previous variant, wrapping around.
    #[must_use]
    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|&t| t == self).unwrap_or(0);
        let len = Self::ALL.len();
        Self::ALL[(idx + len - 1) % len]
    }
}

/// Oscillator section parameters.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct OscParams {
    /// Active waveform shape.
    pub waveform: Waveform,
    /// Pulse width, 0.05 .. 0.95.
    pub pulse_width: f32,
    /// Detune in cents, −100 .. 100.
    pub detune: f32,
    /// Noise blend amount, 0 .. 1.
    pub noise_mix: f32,
}

/// Second oscillator section parameters.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Osc2Params {
    /// Waveform shape for the second oscillator.
    pub waveform: Waveform,
    /// Detune relative to OSC1 in cents, -100 .. 100.
    pub detune: f32,
    /// Blend of OSC2 into the output, 0..1.  At 0.0 OSC2 is bypassed.
    pub osc2_mix: f32,
    /// When true, OSC1 period boundary resets OSC2 phase (hard sync).
    pub hard_sync: bool,
}

impl Default for Osc2Params {
    fn default() -> Self {
        Self {
            waveform: Waveform::Sawtooth,
            detune: 7.0,
            osc2_mix: 0.0,
            hard_sync: false,
        }
    }
}

/// Amplitude envelope section parameters.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EnvParams {
    /// Attack time in seconds.
    pub attack: f32,
    /// Decay time in seconds.
    pub decay: f32,
    /// Sustain level, 0 .. 1.
    pub sustain: f32,
    /// Release time in seconds.
    pub release: f32,
    /// When true, the envelope output is inverted (swell / duck effect).
    pub env_reverse: bool,
}

/// Filter section parameters.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FilterParams {
    /// Filter topology (LP / BP / HP).
    pub filter_mode: FilterMode,
    /// Cutoff frequency in Hz, 20 .. 18000.
    pub cutoff: f32,
    /// Resonance, 0 .. 0.99.
    pub resonance: f32,
    /// Pre-filter drive amount, 0 .. 1.
    pub drive: f32,
}

/// Waveform shape for LFOs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LfoShape {
    /// Sine wave (smooth, classic vibrato/tremolo).
    #[default]
    Sine,
    /// Hard square wave; instant +1/-1 transitions.
    Square,
    /// Rising sawtooth; linear ramp from -1 to +1.
    Sawtooth,
    /// Sample-and-hold: random step value held until next period boundary.
    SampleHold,
}

impl LfoShape {
    /// Ordered slice of all variants, used for cycling.
    pub const ALL: &'static [LfoShape] = &[
        LfoShape::Sine,
        LfoShape::Square,
        LfoShape::Sawtooth,
        LfoShape::SampleHold,
    ];

    /// Short display name shown in the UI.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            LfoShape::Sine => "Sin",
            LfoShape::Square => "Sqr",
            LfoShape::Sawtooth => "Saw",
            LfoShape::SampleHold => "S&H",
        }
    }

    /// Return the next variant, wrapping around.
    #[must_use]
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// Return the previous variant, wrapping around.
    #[must_use]
    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// LFO section parameters.
#[allow(clippy::struct_field_names)] // `lfo_` prefix is intentional for clarity in a flat params struct
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LfoParams {
    /// LFO rate in Hz.
    pub lfo_rate: f32,
    /// LFO modulation depth, 0 .. 1.
    pub lfo_depth: f32,
    /// Which parameter the LFO modulates.
    pub lfo_target: LfoTarget,
    /// LFO waveform shape.
    #[cfg_attr(feature = "serde", serde(default))]
    pub lfo_shape: LfoShape,
}

impl Default for LfoParams {
    fn default() -> Self {
        Self {
            lfo_rate: 3.0,
            lfo_depth: 0.0,
            lfo_target: LfoTarget::Pitch,
            lfo_shape: LfoShape::Sine,
        }
    }
}

/// Modulation envelope parameters (filter cutoff or pitch target).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ModEnvParams {
    /// Attack time in seconds.
    pub attack: f32,
    /// Decay time in seconds.
    pub decay: f32,
    /// Sustain level, 0 .. 1.
    pub sustain: f32,
    /// Release time in seconds.
    pub release: f32,
    /// Modulation depth, -1 .. 1.  At 0.0 the envelope has no effect.
    /// For pitch: +-1 maps to approximately +-1 octave sweep.
    /// For filter cutoff: +-1 maps to approximately +-2 octaves of cutoff.
    pub depth: f32,
}

impl Default for ModEnvParams {
    fn default() -> Self {
        Self {
            attack: 0.001,
            decay: 0.1,
            sustain: 0.0,
            release: 0.1,
            depth: 0.0,
        }
    }
}

/// FX section parameters.
#[allow(clippy::struct_field_names)] // reverb_ prefix is intentional; struct may gain non-reverb fields
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FxParams {
    /// Reverb wet/dry mix, 0 .. 1.
    pub reverb_mix: f32,
    /// Reverb room size, 0 .. 1.
    pub reverb_size: f32,
    /// Reverb high-frequency damping, 0 .. 1.
    pub reverb_damping: f32,
}

/// Bitcrusher section parameters.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CrusherParams {
    /// Bit depth: 1.0..=16.0. At 16.0 with `rate` 1.0, the DSP path is bypassed entirely (exact pass-through).
    pub bits: f32,
    /// Sample rate divider: 1.0..=16.0. At 1.0 with `bits` 16.0, the DSP path is bypassed entirely (exact pass-through).
    pub rate: f32,
}

impl Default for CrusherParams {
    fn default() -> Self {
        Self {
            bits: 16.0,
            rate: 1.0,
        }
    }
}

/// Delay/echo FX parameters.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DelayParams {
    /// Delay time in ms, 1.0..=2000.0.
    pub time_ms: f32,
    /// Feedback gain, 0.0..=0.95.
    pub feedback: f32,
    /// Wet/dry mix, 0.0..=1.0.
    pub mix: f32,
}

impl Default for DelayParams {
    fn default() -> Self {
        Self {
            time_ms: 375.0,
            feedback: 0.3,
            mix: 0.0,
        }
    }
}

/// Chorus FX parameters.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ChorusParams {
    /// LFO modulation rate in Hz, 0.1..=5.0.
    pub rate: f32,
    /// Modulation depth (half-swing around 15 ms center), 0.0..=10.0 ms.
    pub depth_ms: f32,
    /// Wet/dry mix, 0.0..=1.0.
    pub mix: f32,
}

impl Default for ChorusParams {
    fn default() -> Self {
        Self {
            rate: 0.5,
            depth_ms: 3.0,
            mix: 0.0,
        }
    }
}

/// Global section parameters.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GlobalParams {
    /// Master output volume, 0 .. 1.
    pub volume: f32,
    /// Portamento (glide) time in seconds.
    pub glide_time: f32,
}

/// Full parameter snapshot shared between the UI and audio threads.
///
/// The UI owns the authoritative copy; the audio thread receives a boxed clone
/// via `AudioEvent::LoadPatch` on every user edit.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SynthParams {
    /// Oscillator parameters.
    pub osc: OscParams,
    /// Amplitude envelope parameters.
    pub env: EnvParams,
    /// Filter parameters.
    pub filter: FilterParams,
    /// LFO parameters.
    pub lfo: LfoParams,
    /// FX parameters.
    pub fx: FxParams,
    /// Bitcrusher parameters.
    #[cfg_attr(feature = "serde", serde(default))]
    pub crusher: CrusherParams,
    /// Chorus FX parameters.
    #[cfg_attr(feature = "serde", serde(default))]
    pub chorus: ChorusParams,
    /// Delay/echo FX parameters.
    #[cfg_attr(feature = "serde", serde(default))]
    pub delay: DelayParams,
    /// Second oscillator parameters.
    #[cfg_attr(feature = "serde", serde(default))]
    pub osc2: Osc2Params,
    /// Second LFO parameters.
    #[cfg_attr(feature = "serde", serde(default))]
    pub lfo2: LfoParams,
    /// Modulation envelope routed to filter cutoff.
    #[cfg_attr(feature = "serde", serde(default))]
    pub filter_env: ModEnvParams,
    /// Modulation envelope routed to oscillator pitch.
    #[cfg_attr(feature = "serde", serde(default))]
    pub pitch_env: ModEnvParams,
    /// Global parameters.
    pub global: GlobalParams,
}

impl Default for SynthParams {
    /// Sensible starting patch: medium pulse wave, gentle filter, light reverb.
    fn default() -> Self {
        Self {
            osc: OscParams {
                waveform: Waveform::Pulse,
                pulse_width: 0.5,
                detune: 0.0,
                noise_mix: 0.0,
            },
            env: EnvParams {
                attack: 0.01,
                decay: 0.1,
                sustain: 0.8,
                release: 0.3,
                env_reverse: false,
            },
            filter: FilterParams {
                filter_mode: FilterMode::LowPass,
                cutoff: 4000.0,
                resonance: 0.3,
                drive: 0.0,
            },
            lfo: LfoParams {
                lfo_rate: 3.0,
                lfo_depth: 0.0,
                lfo_target: LfoTarget::Pitch,
                lfo_shape: LfoShape::Sine,
            },
            fx: FxParams {
                reverb_mix: 0.15,
                reverb_size: 0.5,
                reverb_damping: 0.5,
            },
            crusher: CrusherParams::default(),
            chorus: ChorusParams::default(),
            delay: DelayParams::default(),
            osc2: Osc2Params::default(),
            lfo2: LfoParams::default(),
            filter_env: ModEnvParams::default(),
            pitch_env: ModEnvParams::default(),
            global: GlobalParams {
                volume: 0.7,
                glide_time: 0.05,
            },
        }
    }
}

/// A named preset: a display name paired with a full parameter snapshot.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Patch {
    /// Human-readable patch name shown in the preset list.
    pub name: String,
    /// Parameter values for this patch.
    pub params: SynthParams,
}

impl Patch {
    /// Construct a new patch from a name and a parameter snapshot.
    pub fn new(name: impl Into<String>, params: SynthParams) -> Self {
        Self {
            name: name.into(),
            params,
        }
    }
}

/// A typed wrapper around a MIDI note byte.
///
/// The inner `u8` field is public for ergonomic construction with numeric
/// literals (`MidiNote(60)`).  No range check is performed; the MIDI spec
/// defines valid values as 0..=127, but values up to 255 are accepted.
/// Use [`MidiNote::new_clamped`] when constructing from untrusted input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MidiNote(pub u8);

impl MidiNote {
    /// Middle C (C4).
    pub const MIDDLE_C: Self = Self(60);
    /// A4 (440 Hz reference pitch).
    pub const A4: Self = Self(69);

    /// Clamp `v` to 0..=127 and wrap in `MidiNote`.
    #[must_use]
    pub const fn new_clamped(v: u8) -> Self {
        Self(if v > 127 { 127 } else { v })
    }

    /// Raw MIDI byte value.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self.0
    }
}

impl From<u8> for MidiNote {
    fn from(v: u8) -> Self {
        Self(v)
    }
}

impl Default for MidiNote {
    fn default() -> Self {
        Self::MIDDLE_C
    }
}

/// Index of an independent synthesis channel (voice pool + parameter set).
///
/// Channel 0 is the default; `NoteOn` / `LoadPatch` without a channel argument
/// implicitly target it.  Values beyond the engine's `NUM_CHANNELS` limit are
/// silently ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ChannelNo(pub u8);

impl ChannelNo {
    /// The implicit default channel used by the channel-less event variants.
    pub const DEFAULT: Self = Self(0);

    /// Convert to `usize` for array indexing.
    #[must_use]
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

impl From<u8> for ChannelNo {
    fn from(v: u8) -> Self {
        Self(v)
    }
}

impl Default for ChannelNo {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Drum one-shot events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrumHit {
    /// Trigger a synthesized kick drum hit.
    Kick,
    /// Trigger a short, bright closed hi-hat hit.
    HiHatClosed,
    /// Trigger a longer, ringing open hi-hat hit.
    HiHatOpen,
}

#[cfg(all(test, feature = "serde"))]
mod tests {
    use super::*;

    #[test]
    fn synth_params_osc2_defaults_when_missing_from_json() {
        // JSON from before `osc2` was added: the field should deserialise to
        // `Osc2Params::default()` rather than failing or panicking.
        let json = r#"{
            "osc":    {"waveform":"Sawtooth","pulse_width":0.5,"detune":0.0,"noise_mix":0.0},
            "env":    {"attack":0.01,"decay":0.1,"sustain":0.8,"release":0.3,"env_reverse":false},
            "filter": {"filter_mode":"LowPass","cutoff":4000.0,"resonance":0.3,"drive":0.0},
            "lfo":    {"lfo_rate":3.0,"lfo_depth":0.0,"lfo_target":"Pitch"},
            "fx":     {"reverb_mix":0.15,"reverb_size":0.5,"reverb_damping":0.5},
            "global": {"volume":0.7,"glide_time":0.05}
        }"#;

        let params: SynthParams =
            serde_json::from_str(json).expect("old-style JSON must deserialise without osc2 key");

        let def = Osc2Params::default();
        assert_eq!(params.osc2.waveform, def.waveform);
        assert!((params.osc2.detune - def.detune).abs() < f32::EPSILON);
        assert!((params.osc2.osc2_mix - def.osc2_mix).abs() < f32::EPSILON);
        assert_eq!(params.osc2.hard_sync, def.hard_sync);
    }

    #[test]
    fn synth_params_mod_fields_default_when_missing_from_json() {
        let json = r#"{
            "osc":    {"waveform":"Sawtooth","pulse_width":0.5,"detune":0.0,"noise_mix":0.0},
            "env":    {"attack":0.01,"decay":0.1,"sustain":0.8,"release":0.3,"env_reverse":false},
            "filter": {"filter_mode":"LowPass","cutoff":4000.0,"resonance":0.3,"drive":0.0},
            "lfo":    {"lfo_rate":3.0,"lfo_depth":0.0,"lfo_target":"Pitch"},
            "fx":     {"reverb_mix":0.15,"reverb_size":0.5,"reverb_damping":0.5},
            "global": {"volume":0.7,"glide_time":0.05}
        }"#;

        let params: SynthParams =
            serde_json::from_str(json).expect("old JSON must deserialise without mod fields");

        assert_eq!(params.lfo.lfo_shape, LfoShape::Sine);
        assert_eq!(params.lfo2.lfo_shape, LfoShape::Sine);
        assert!((params.lfo2.lfo_depth).abs() < f32::EPSILON);
        assert!((params.filter_env.depth).abs() < f32::EPSILON);
        assert!((params.pitch_env.depth).abs() < f32::EPSILON);
    }
}

/// Messages sent from the UI thread to the audio thread over the event channel.
#[derive(Default, Debug, Clone)]
pub enum AudioEvent {
    /// Immediately silence all voices and clear active note routing on all channels.
    #[default]
    Panic,
    /// Begin sustaining a note at the given MIDI note number on channel 0.
    NoteOn(MidiNote),
    /// Release the note at the given MIDI note number on channel 0.
    NoteOff(MidiNote),
    /// Replace the parameter set for channel 0 with a new snapshot.
    LoadPatch(Box<SynthParams>),
    /// Trigger a one-shot synthesized drum hit.
    Drum(DrumHit),
    /// Begin sustaining a note on the given channel at the given MIDI note number.
    NoteOnChannel(ChannelNo, MidiNote),
    /// Release the note on the given channel at the given MIDI note number.
    NoteOffChannel(ChannelNo, MidiNote),
    /// Replace the parameter set for the given channel with a new snapshot.
    LoadPatchChannel(ChannelNo, Box<SynthParams>),
}
