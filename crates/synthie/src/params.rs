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
}

impl Waveform {
    /// Ordered slice of all variants, used for cycling.
    pub const ALL: &'static [Waveform] = &[
        Waveform::Pulse,
        Waveform::Sawtooth,
        Waveform::Triangle,
        Waveform::Noise,
        Waveform::PulseSaw,
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

/// LFO section parameters.
#[allow(clippy::struct_field_names)] // lfo_ prefix is intentional for clarity in a flat params struct
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LfoParams {
    /// LFO rate in Hz.
    pub lfo_rate: f32,
    /// LFO modulation depth, 0 .. 1.
    pub lfo_depth: f32,
    /// Which parameter the LFO modulates.
    pub lfo_target: LfoTarget,
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
            },
            fx: FxParams {
                reverb_mix: 0.15,
                reverb_size: 0.5,
                reverb_damping: 0.5,
            },
            crusher: CrusherParams::default(),
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
