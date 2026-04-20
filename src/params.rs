//! Core parameter and event types shared between the UI thread and audio thread.
//!
//! `SynthParams` is the canonical parameter snapshot.  The UI holds a live copy
//! and sends a boxed clone to the audio thread via `AudioEvent::LoadPatch`
//! whenever a value changes.

use serde::{Deserialize, Serialize};

/// Oscillator waveform shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&w| w == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// Return the previous variant, wrapping around.
    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|&w| w == self).unwrap_or(0);
        let len = Self::ALL.len();
        Self::ALL[(idx + len - 1) % len]
    }
}

/// State-variable filter topology selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    pub fn name(self) -> &'static str {
        match self {
            FilterMode::LowPass => "LP",
            FilterMode::BandPass => "BP",
            FilterMode::HighPass => "HP",
        }
    }

    /// Return the next variant, wrapping around.
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&m| m == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }
}

/// Selects which parameter the LFO modulates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    pub fn name(self) -> &'static str {
        match self {
            LfoTarget::Pitch => "Pitch",
            LfoTarget::PulseWidth => "PW",
            LfoTarget::Cutoff => "Cutoff",
            LfoTarget::Volume => "Volume",
        }
    }

    /// Return the next variant, wrapping around.
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&t| t == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }
}

/// Oscillator section parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LfoParams {
    /// LFO rate in Hz.
    pub lfo_rate: f32,
    /// LFO modulation depth, 0 .. 1.
    pub lfo_depth: f32,
    /// Which parameter the LFO modulates.
    pub lfo_target: LfoTarget,
}

/// FX section parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FxParams {
    /// Reverb wet/dry mix, 0 .. 1.
    pub reverb_mix: f32,
    /// Reverb room size, 0 .. 1.
    pub reverb_size: f32,
    /// Reverb high-frequency damping, 0 .. 1.
    pub reverb_damping: f32,
}

/// Global section parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
            global: GlobalParams {
                volume: 0.7,
                glide_time: 0.05,
            },
        }
    }
}

/// A named preset: a display name paired with a full parameter snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Messages sent from the UI thread to the audio thread over the event channel.
pub enum AudioEvent {
    /// Begin sustaining a note at the given MIDI note number.
    NoteOn(u8),
    /// Release the note at the given MIDI note number.
    NoteOff(u8),
    /// Immediately silence all voices and clear the note stack.
    Panic,
    /// Replace the current parameter set with a new snapshot.
    LoadPatch(Box<SynthParams>),
}
