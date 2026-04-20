//! Built-in C64/SID-inspired preset bank.
//!
//! Each patch targets a specific sonic archetype from classic C64 music.
//! The inline sound-design notes next to each preset describe the intent so
//! the parameter choices remain legible without running the synth.

use crate::params::{
    EnvParams, FilterMode, FilterParams, FxParams, GlobalParams, LfoParams, LfoTarget, OscParams,
    Patch, SynthParams, Waveform,
};

/// Return the built-in C64-inspired preset bank.
#[allow(clippy::too_many_lines)] // large static preset table; refactoring would reduce clarity
pub fn default_patches() -> Vec<Patch> {
    vec![
        // 1. Classic C64 Bass – iconic SID pluck bass: sawtooth, short decay, LP resonance
        Patch::new(
            "C64 Bass",
            SynthParams {
                osc: OscParams {
                    waveform: Waveform::Sawtooth,
                    pulse_width: 0.5,
                    detune: 0.0,
                    noise_mix: 0.0,
                },
                env: EnvParams {
                    attack: 0.002,
                    decay: 0.18,
                    sustain: 0.0,
                    release: 0.15,
                    env_reverse: false,
                },
                filter: FilterParams {
                    filter_mode: FilterMode::LowPass,
                    cutoff: 900.0,
                    resonance: 0.55,
                    drive: 0.15,
                },
                lfo: LfoParams {
                    lfo_rate: 0.0,
                    lfo_depth: 0.0,
                    lfo_target: LfoTarget::Pitch,
                },
                fx: FxParams {
                    reverb_mix: 0.05,
                    reverb_size: 0.3,
                    reverb_damping: 0.6,
                },
                global: GlobalParams {
                    volume: 0.75,
                    glide_time: 0.0,
                },
            },
        ),
        // 2. Arpeggiated Lead – fast-attack bright pulse with pitch vibrato
        Patch::new(
            "Arp Lead",
            SynthParams {
                osc: OscParams {
                    waveform: Waveform::Pulse,
                    pulse_width: 0.25,
                    detune: 3.0,
                    noise_mix: 0.0,
                },
                env: EnvParams {
                    attack: 0.003,
                    decay: 0.06,
                    sustain: 0.7,
                    release: 0.12,
                    env_reverse: false,
                },
                filter: FilterParams {
                    filter_mode: FilterMode::LowPass,
                    cutoff: 3500.0,
                    resonance: 0.3,
                    drive: 0.0,
                },
                lfo: LfoParams {
                    lfo_rate: 5.5,
                    lfo_depth: 0.12,
                    lfo_target: LfoTarget::Pitch,
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
            },
        ),
        // 3. Plucky Pulse – short percussive pulse stab, no sustain
        Patch::new(
            "Plucky Pulse",
            SynthParams {
                osc: OscParams {
                    waveform: Waveform::Pulse,
                    pulse_width: 0.5,
                    detune: 0.0,
                    noise_mix: 0.0,
                },
                env: EnvParams {
                    attack: 0.001,
                    decay: 0.25,
                    sustain: 0.0,
                    release: 0.1,
                    env_reverse: false,
                },
                filter: FilterParams {
                    filter_mode: FilterMode::LowPass,
                    cutoff: 2200.0,
                    resonance: 0.6,
                    drive: 0.1,
                },
                lfo: LfoParams {
                    lfo_rate: 0.0,
                    lfo_depth: 0.0,
                    lfo_target: LfoTarget::Pitch,
                },
                fx: FxParams {
                    reverb_mix: 0.12,
                    reverb_size: 0.35,
                    reverb_damping: 0.55,
                },
                global: GlobalParams {
                    volume: 0.75,
                    glide_time: 0.0,
                },
            },
        ),
        // 4. PWM Lead – pulse-width modulation gives flute/cello-like timbre
        Patch::new(
            "PWM Lead",
            SynthParams {
                osc: OscParams {
                    waveform: Waveform::Pulse,
                    pulse_width: 0.5,
                    detune: 0.0,
                    noise_mix: 0.0,
                },
                env: EnvParams {
                    attack: 0.04,
                    decay: 0.1,
                    sustain: 0.9,
                    release: 0.25,
                    env_reverse: false,
                },
                filter: FilterParams {
                    filter_mode: FilterMode::LowPass,
                    cutoff: 4000.0,
                    resonance: 0.2,
                    drive: 0.0,
                },
                lfo: LfoParams {
                    lfo_rate: 2.8,
                    lfo_depth: 0.45,
                    lfo_target: LfoTarget::PulseWidth,
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
            },
        ),
        // 5. Metallic Pulse – thin, harsh, metallic character: narrow PW + band-pass
        Patch::new(
            "Metal Pulse",
            SynthParams {
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
                    env_reverse: false,
                },
                filter: FilterParams {
                    filter_mode: FilterMode::BandPass,
                    cutoff: 1800.0,
                    resonance: 0.65,
                    drive: 0.2,
                },
                lfo: LfoParams {
                    lfo_rate: 0.0,
                    lfo_depth: 0.0,
                    lfo_target: LfoTarget::Pitch,
                },
                fx: FxParams {
                    reverb_mix: 0.08,
                    reverb_size: 0.25,
                    reverb_damping: 0.7,
                },
                global: GlobalParams {
                    volume: 0.65,
                    glide_time: 0.0,
                },
            },
        ),
        // 6. Noise Snare – gated LFSR noise burst through band-pass, snare-like
        Patch::new(
            "Noise Snare",
            SynthParams {
                osc: OscParams {
                    waveform: Waveform::Noise,
                    pulse_width: 0.5,
                    detune: 0.0,
                    noise_mix: 1.0,
                },
                env: EnvParams {
                    attack: 0.001,
                    decay: 0.12,
                    sustain: 0.0,
                    release: 0.05,
                    env_reverse: false,
                },
                filter: FilterParams {
                    filter_mode: FilterMode::BandPass,
                    cutoff: 3500.0,
                    resonance: 0.4,
                    drive: 0.25,
                },
                lfo: LfoParams {
                    lfo_rate: 0.0,
                    lfo_depth: 0.0,
                    lfo_target: LfoTarget::Pitch,
                },
                fx: FxParams {
                    reverb_mix: 0.1,
                    reverb_size: 0.2,
                    reverb_damping: 0.8,
                },
                global: GlobalParams {
                    volume: 0.8,
                    glide_time: 0.0,
                },
            },
        ),
        // 7. Slide Bass – sawtooth bass with legato portamento glide
        Patch::new(
            "Slide Bass",
            SynthParams {
                osc: OscParams {
                    waveform: Waveform::Sawtooth,
                    pulse_width: 0.5,
                    detune: 0.0,
                    noise_mix: 0.0,
                },
                env: EnvParams {
                    attack: 0.004,
                    decay: 0.12,
                    sustain: 0.6,
                    release: 0.2,
                    env_reverse: false,
                },
                filter: FilterParams {
                    filter_mode: FilterMode::LowPass,
                    cutoff: 800.0,
                    resonance: 0.45,
                    drive: 0.2,
                },
                lfo: LfoParams {
                    lfo_rate: 0.0,
                    lfo_depth: 0.0,
                    lfo_target: LfoTarget::Pitch,
                },
                fx: FxParams {
                    reverb_mix: 0.05,
                    reverb_size: 0.3,
                    reverb_damping: 0.6,
                },
                global: GlobalParams {
                    volume: 0.75,
                    glide_time: 0.12,
                },
            },
        ),
        // 8. Tri Pad – soft triangle pad, slow attack, heavy reverb wash
        Patch::new(
            "Tri Pad",
            SynthParams {
                osc: OscParams {
                    waveform: Waveform::Triangle,
                    pulse_width: 0.5,
                    detune: -5.0,
                    noise_mix: 0.0,
                },
                env: EnvParams {
                    attack: 0.35,
                    decay: 0.2,
                    sustain: 0.85,
                    release: 0.8,
                    env_reverse: false,
                },
                filter: FilterParams {
                    filter_mode: FilterMode::LowPass,
                    cutoff: 2800.0,
                    resonance: 0.15,
                    drive: 0.0,
                },
                lfo: LfoParams {
                    lfo_rate: 0.8,
                    lfo_depth: 0.08,
                    lfo_target: LfoTarget::Pitch,
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
            },
        ),
    ]
}
