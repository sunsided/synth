use crate::params::{FilterMode, LfoTarget, Patch, SynthParams, Waveform};

/// Return the built-in C64-inspired preset bank.
pub fn default_patches() -> Vec<Patch> {
    vec![
        // ------------------------------------------------------------------
        // 1. Classic C64 Bass – the iconic SID pluck bass
        // ------------------------------------------------------------------
        Patch::new(
            "C64 Bass",
            SynthParams {
                waveform: Waveform::Sawtooth,
                pulse_width: 0.5,
                detune: 0.0,
                noise_mix: 0.0,
                attack: 0.002,
                decay: 0.18,
                sustain: 0.0,
                release: 0.15,
                env_reverse: false,
                filter_mode: FilterMode::LowPass,
                cutoff: 900.0,
                resonance: 0.55,
                drive: 0.15,
                lfo_rate: 0.0,
                lfo_depth: 0.0,
                lfo_target: LfoTarget::Pitch,
                reverb_mix: 0.05,
                reverb_size: 0.3,
                reverb_damping: 0.6,
                volume: 0.75,
                glide_time: 0.0,
            },
        ),
        // ------------------------------------------------------------------
        // 2. Arpeggiated Lead – fast-attack bright pulse with vibrato
        // ------------------------------------------------------------------
        Patch::new(
            "Arp Lead",
            SynthParams {
                waveform: Waveform::Pulse,
                pulse_width: 0.25,
                detune: 3.0,
                noise_mix: 0.0,
                attack: 0.003,
                decay: 0.06,
                sustain: 0.7,
                release: 0.12,
                env_reverse: false,
                filter_mode: FilterMode::LowPass,
                cutoff: 3500.0,
                resonance: 0.3,
                drive: 0.0,
                lfo_rate: 5.5,
                lfo_depth: 0.12,
                lfo_target: LfoTarget::Pitch,
                reverb_mix: 0.1,
                reverb_size: 0.4,
                reverb_damping: 0.5,
                volume: 0.75,
                glide_time: 0.0,
            },
        ),
        // ------------------------------------------------------------------
        // 3. Plucky Pulse – short percussive pulse stab
        // ------------------------------------------------------------------
        Patch::new(
            "Plucky Pulse",
            SynthParams {
                waveform: Waveform::Pulse,
                pulse_width: 0.5,
                detune: 0.0,
                noise_mix: 0.0,
                attack: 0.001,
                decay: 0.25,
                sustain: 0.0,
                release: 0.1,
                env_reverse: false,
                filter_mode: FilterMode::LowPass,
                cutoff: 2200.0,
                resonance: 0.6,
                drive: 0.1,
                lfo_rate: 0.0,
                lfo_depth: 0.0,
                lfo_target: LfoTarget::Pitch,
                reverb_mix: 0.12,
                reverb_size: 0.35,
                reverb_damping: 0.55,
                volume: 0.75,
                glide_time: 0.0,
            },
        ),
        // ------------------------------------------------------------------
        // 4. PWM Lead – pulse-width modulated flute/string character
        // ------------------------------------------------------------------
        Patch::new(
            "PWM Lead",
            SynthParams {
                waveform: Waveform::Pulse,
                pulse_width: 0.5,
                detune: 0.0,
                noise_mix: 0.0,
                attack: 0.04,
                decay: 0.1,
                sustain: 0.9,
                release: 0.25,
                env_reverse: false,
                filter_mode: FilterMode::LowPass,
                cutoff: 4000.0,
                resonance: 0.2,
                drive: 0.0,
                lfo_rate: 2.8,
                lfo_depth: 0.45,
                lfo_target: LfoTarget::PulseWidth,
                reverb_mix: 0.18,
                reverb_size: 0.55,
                reverb_damping: 0.4,
                volume: 0.7,
                glide_time: 0.03,
            },
        ),
        // ------------------------------------------------------------------
        // 5. Metallic Pulse – thin, harsh, metallic timbre
        // ------------------------------------------------------------------
        Patch::new(
            "Metal Pulse",
            SynthParams {
                waveform: Waveform::PulseSaw,
                pulse_width: 0.08,
                detune: 8.0,
                noise_mix: 0.05,
                attack: 0.001,
                decay: 0.08,
                sustain: 0.5,
                release: 0.08,
                env_reverse: false,
                filter_mode: FilterMode::BandPass,
                cutoff: 1800.0,
                resonance: 0.65,
                drive: 0.2,
                lfo_rate: 0.0,
                lfo_depth: 0.0,
                lfo_target: LfoTarget::Pitch,
                reverb_mix: 0.08,
                reverb_size: 0.25,
                reverb_damping: 0.7,
                volume: 0.65,
                glide_time: 0.0,
            },
        ),
        // ------------------------------------------------------------------
        // 6. Noise Snare – gated noise burst, snare-like
        // ------------------------------------------------------------------
        Patch::new(
            "Noise Snare",
            SynthParams {
                waveform: Waveform::Noise,
                pulse_width: 0.5,
                detune: 0.0,
                noise_mix: 1.0,
                attack: 0.001,
                decay: 0.12,
                sustain: 0.0,
                release: 0.05,
                env_reverse: false,
                filter_mode: FilterMode::BandPass,
                cutoff: 3500.0,
                resonance: 0.4,
                drive: 0.25,
                lfo_rate: 0.0,
                lfo_depth: 0.0,
                lfo_target: LfoTarget::Pitch,
                reverb_mix: 0.1,
                reverb_size: 0.2,
                reverb_damping: 0.8,
                volume: 0.8,
                glide_time: 0.0,
            },
        ),
        // ------------------------------------------------------------------
        // 7. Slide Bass – saw bass with legato glide
        // ------------------------------------------------------------------
        Patch::new(
            "Slide Bass",
            SynthParams {
                waveform: Waveform::Sawtooth,
                pulse_width: 0.5,
                detune: 0.0,
                noise_mix: 0.0,
                attack: 0.004,
                decay: 0.12,
                sustain: 0.6,
                release: 0.2,
                env_reverse: false,
                filter_mode: FilterMode::LowPass,
                cutoff: 800.0,
                resonance: 0.45,
                drive: 0.2,
                lfo_rate: 0.0,
                lfo_depth: 0.0,
                lfo_target: LfoTarget::Pitch,
                reverb_mix: 0.05,
                reverb_size: 0.3,
                reverb_damping: 0.6,
                volume: 0.75,
                glide_time: 0.12,
            },
        ),
        // ------------------------------------------------------------------
        // 8. Tri Pad – soft triangle pad with reverb
        // ------------------------------------------------------------------
        Patch::new(
            "Tri Pad",
            SynthParams {
                waveform: Waveform::Triangle,
                pulse_width: 0.5,
                detune: -5.0,
                noise_mix: 0.0,
                attack: 0.35,
                decay: 0.2,
                sustain: 0.85,
                release: 0.8,
                env_reverse: false,
                filter_mode: FilterMode::LowPass,
                cutoff: 2800.0,
                resonance: 0.15,
                drive: 0.0,
                lfo_rate: 0.8,
                lfo_depth: 0.08,
                lfo_target: LfoTarget::Pitch,
                reverb_mix: 0.45,
                reverb_size: 0.75,
                reverb_damping: 0.3,
                volume: 0.65,
                glide_time: 0.0,
            },
        ),
    ]
}
