use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Waveform
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Waveform {
    Pulse,
    Sawtooth,
    Triangle,
    Noise,
    PulseSaw,
}

impl Waveform {
    pub const ALL: &'static [Waveform] = &[
        Waveform::Pulse,
        Waveform::Sawtooth,
        Waveform::Triangle,
        Waveform::Noise,
        Waveform::PulseSaw,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Waveform::Pulse => "Pulse",
            Waveform::Sawtooth => "Saw",
            Waveform::Triangle => "Tri",
            Waveform::Noise => "Noise",
            Waveform::PulseSaw => "Pls+Saw",
        }
    }

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&w| w == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|&w| w == self).unwrap_or(0);
        let len = Self::ALL.len();
        Self::ALL[(idx + len - 1) % len]
    }
}

// ---------------------------------------------------------------------------
// Filter mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterMode {
    LowPass,
    BandPass,
    HighPass,
}

impl FilterMode {
    pub const ALL: &'static [FilterMode] = &[
        FilterMode::LowPass,
        FilterMode::BandPass,
        FilterMode::HighPass,
    ];

    pub fn name(self) -> &'static str {
        match self {
            FilterMode::LowPass => "LP",
            FilterMode::BandPass => "BP",
            FilterMode::HighPass => "HP",
        }
    }

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&m| m == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }
}

// ---------------------------------------------------------------------------
// LFO target
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LfoTarget {
    Pitch,
    PulseWidth,
    Cutoff,
    Volume,
}

impl LfoTarget {
    pub const ALL: &'static [LfoTarget] = &[
        LfoTarget::Pitch,
        LfoTarget::PulseWidth,
        LfoTarget::Cutoff,
        LfoTarget::Volume,
    ];

    pub fn name(self) -> &'static str {
        match self {
            LfoTarget::Pitch => "Pitch",
            LfoTarget::PulseWidth => "PW",
            LfoTarget::Cutoff => "Cutoff",
            LfoTarget::Volume => "Volume",
        }
    }

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&t| t == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }
}

// ---------------------------------------------------------------------------
// SynthParams – the full parameter set shared between UI and audio
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthParams {
    // Oscillator
    pub waveform: Waveform,
    pub pulse_width: f32, // 0.05 .. 0.95
    pub detune: f32,      // cents, -100 .. 100
    pub noise_mix: f32,   // 0 .. 1

    // Amplitude envelope
    pub attack: f32,  // seconds
    pub decay: f32,   // seconds
    pub sustain: f32, // 0 .. 1
    pub release: f32, // seconds
    pub env_reverse: bool,

    // Filter
    pub filter_mode: FilterMode,
    pub cutoff: f32,    // Hz, 20 .. 18000
    pub resonance: f32, // 0 .. 0.99
    pub drive: f32,     // 0 .. 1

    // LFO
    pub lfo_rate: f32,  // Hz
    pub lfo_depth: f32, // 0 .. 1
    pub lfo_target: LfoTarget,

    // FX
    pub reverb_mix: f32,
    pub reverb_size: f32,
    pub reverb_damping: f32,

    // Global
    pub volume: f32,
    pub glide_time: f32, // seconds
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            waveform: Waveform::Pulse,
            pulse_width: 0.5,
            detune: 0.0,
            noise_mix: 0.0,
            attack: 0.01,
            decay: 0.1,
            sustain: 0.8,
            release: 0.3,
            env_reverse: false,
            filter_mode: FilterMode::LowPass,
            cutoff: 4000.0,
            resonance: 0.3,
            drive: 0.0,
            lfo_rate: 3.0,
            lfo_depth: 0.0,
            lfo_target: LfoTarget::Pitch,
            reverb_mix: 0.15,
            reverb_size: 0.5,
            reverb_damping: 0.5,
            volume: 0.7,
            glide_time: 0.05,
        }
    }
}

// ---------------------------------------------------------------------------
// Patch
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Patch {
    pub name: String,
    pub params: SynthParams,
}

impl Patch {
    pub fn new(name: impl Into<String>, params: SynthParams) -> Self {
        Self {
            name: name.into(),
            params,
        }
    }
}

// ---------------------------------------------------------------------------
// AudioEvent – messages sent from UI thread to audio thread
// ---------------------------------------------------------------------------

pub enum AudioEvent {
    NoteOn(u8),
    NoteOff(u8),
    Panic,
    LoadPatch(Box<SynthParams>),
}
