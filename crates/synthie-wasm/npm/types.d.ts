// Generated from synthie's parameter registry by `cargo test -p synthie-wasm`;
// rerun it with SYNTHIE_UPDATE_TYPES=1 to update. Do not edit by hand.

/**
 * A complete patch, as returned by `presetPatch` and `params` and accepted by
 * `loadParams`. Values outside a parameter's range are clamped on load.
 */
export interface SynthParams {
  osc: {
    /** `osc.waveform`: Choice. Default "Pulse". */
    waveform: "Pulse" | "Sawtooth" | "Triangle" | "Noise" | "PulseSaw" | "Sine";
    /** `osc.pulse_width`: 0.05 to 0.95. Default 0.5. */
    pulse_width: number;
    /** `osc.detune`: -100 to 100. Default 0.0. */
    detune: number;
    /** `osc.noise_mix`: 0 to 1. Default 0.0. */
    noise_mix: number;
  };
  env: {
    /** `env.attack`: 0.001 to 10. Default 0.01. */
    attack: number;
    /** `env.decay`: 0.001 to 10. Default 0.1. */
    decay: number;
    /** `env.sustain`: 0 to 1. Default 0.8. */
    sustain: number;
    /** `env.release`: 0.001 to 20. Default 0.3. */
    release: number;
    /** `env.env_reverse`: On or off. Default false. */
    env_reverse: boolean;
  };
  filter: {
    /** `filter.filter_mode`: Choice. Default "LowPass". */
    filter_mode: "LowPass" | "BandPass" | "HighPass";
    /** `filter.cutoff`: 20 to 18000. Default 4000.0. */
    cutoff: number;
    /** `filter.resonance`: 0 to 0.99. Default 0.3. */
    resonance: number;
    /** `filter.drive`: 0 to 1. Default 0.0. */
    drive: number;
  };
  lfo: {
    /** `lfo.lfo_rate`: 0 to 50. Default 3.0. */
    lfo_rate: number;
    /** `lfo.lfo_depth`: 0 to 1. Default 0.0. */
    lfo_depth: number;
    /** `lfo.lfo_target`: Choice. Default "Pitch". */
    lfo_target: "Pitch" | "PulseWidth" | "Cutoff" | "Volume";
    /** `lfo.lfo_shape`: Choice. Default "Sine". */
    lfo_shape: "Sine" | "Square" | "Sawtooth" | "SampleHold";
  };
  fx: {
    /** `fx.reverb_mix`: 0 to 1. Default 0.15. Shared FX: only channel 0's value is used. */
    reverb_mix: number;
    /** `fx.reverb_size`: 0 to 1. Default 0.5. Shared FX: only channel 0's value is used. */
    reverb_size: number;
    /** `fx.reverb_damping`: 0 to 1. Default 0.5. Shared FX: only channel 0's value is used. */
    reverb_damping: number;
  };
  crusher: {
    /** `crusher.bits`: 1 to 16. Default 16.0. */
    bits: number;
    /** `crusher.rate`: 1 to 16. Default 1.0. */
    rate: number;
  };
  chorus: {
    /** `chorus.rate`: 0.1 to 5. Default 0.5. Shared FX: only channel 0's value is used. */
    rate: number;
    /** `chorus.depth_ms`: 0 to 10. Default 3.0. Shared FX: only channel 0's value is used. */
    depth_ms: number;
    /** `chorus.mix`: 0 to 1. Default 0.0. Shared FX: only channel 0's value is used. */
    mix: number;
  };
  delay: {
    /** `delay.time_ms`: 1 to 2000. Default 375.0. Shared FX: only channel 0's value is used. */
    time_ms: number;
    /** `delay.feedback`: 0 to 0.95. Default 0.3. Shared FX: only channel 0's value is used. */
    feedback: number;
    /** `delay.mix`: 0 to 1. Default 0.0. Shared FX: only channel 0's value is used. */
    mix: number;
  };
  osc2: {
    /** `osc2.waveform`: Choice. Default "Sawtooth". */
    waveform: "Pulse" | "Sawtooth" | "Triangle" | "Noise" | "PulseSaw" | "Sine";
    /** `osc2.detune`: -100 to 100. Default 7.0. */
    detune: number;
    /** `osc2.osc2_mix`: 0 to 1. Default 0.0. */
    osc2_mix: number;
    /** `osc2.hard_sync`: On or off. Default false. */
    hard_sync: boolean;
    /** `osc2.ring_mod`: Choice. Default "Off". */
    ring_mod: "Off" | "Osc2ByOsc1Sign" | "Osc1ByOsc2Sign" | "Analog";
  };
  lfo2: {
    /** `lfo2.lfo_rate`: 0 to 50. Default 3.0. */
    lfo_rate: number;
    /** `lfo2.lfo_depth`: 0 to 1. Default 0.0. */
    lfo_depth: number;
    /** `lfo2.lfo_target`: Choice. Default "Pitch". */
    lfo_target: "Pitch" | "PulseWidth" | "Cutoff" | "Volume";
    /** `lfo2.lfo_shape`: Choice. Default "Sine". */
    lfo_shape: "Sine" | "Square" | "Sawtooth" | "SampleHold";
  };
  filter_env: {
    /** `filter_env.attack`: 0.001 to 10. Default 0.001. */
    attack: number;
    /** `filter_env.decay`: 0.001 to 10. Default 0.1. */
    decay: number;
    /** `filter_env.sustain`: 0 to 1. Default 0.0. */
    sustain: number;
    /** `filter_env.release`: 0.001 to 20. Default 0.1. */
    release: number;
    /** `filter_env.depth`: -1 to 1. Default 0.0. */
    depth: number;
  };
  pitch_env: {
    /** `pitch_env.attack`: 0.001 to 10. Default 0.001. */
    attack: number;
    /** `pitch_env.decay`: 0.001 to 10. Default 0.1. */
    decay: number;
    /** `pitch_env.sustain`: 0 to 1. Default 0.0. */
    sustain: number;
    /** `pitch_env.release`: 0.001 to 20. Default 0.1. */
    release: number;
    /** `pitch_env.depth`: -1 to 1. Default 0.0. */
    depth: number;
  };
  arp: {
    /** `arp.enabled`: On or off. Default false. */
    enabled: boolean;
    /** `arp.rate`: 0 to 50. Default 10.0. */
    rate: number;
    /** `arp.gate`: 0 to 1. Default 0.8. */
    gate: number;
    /** `arp.mode`: Choice. Default "Up". */
    mode: "Up" | "Down" | "UpDown" | "Random";
    /** Held notes (runtime state, MIDI numbers); only the first `count` are used. */
    notes: [number, number, number, number];
    /** Number of held notes in `notes`, 0 to 4 (runtime state). */
    count: number;
  };
  global: {
    /** `global.volume`: 0 to 1. Default 0.7. */
    volume: number;
    /** `global.glide_time`: 0 to 10. Default 0.05. */
    glide_time: number;
  };
}

/** Path of a settable parameter, for `setParam`. */
export type ParamPath =
  | "osc.waveform"
  | "osc.pulse_width"
  | "osc.detune"
  | "osc.noise_mix"
  | "env.attack"
  | "env.decay"
  | "env.sustain"
  | "env.release"
  | "env.env_reverse"
  | "filter.filter_mode"
  | "filter.cutoff"
  | "filter.resonance"
  | "filter.drive"
  | "lfo.lfo_rate"
  | "lfo.lfo_depth"
  | "lfo.lfo_target"
  | "lfo.lfo_shape"
  | "fx.reverb_mix"
  | "fx.reverb_size"
  | "fx.reverb_damping"
  | "crusher.bits"
  | "crusher.rate"
  | "chorus.rate"
  | "chorus.depth_ms"
  | "chorus.mix"
  | "delay.time_ms"
  | "delay.feedback"
  | "delay.mix"
  | "osc2.waveform"
  | "osc2.detune"
  | "osc2.osc2_mix"
  | "osc2.hard_sync"
  | "osc2.ring_mod"
  | "lfo2.lfo_rate"
  | "lfo2.lfo_depth"
  | "lfo2.lfo_target"
  | "lfo2.lfo_shape"
  | "filter_env.attack"
  | "filter_env.decay"
  | "filter_env.sustain"
  | "filter_env.release"
  | "filter_env.depth"
  | "pitch_env.attack"
  | "pitch_env.decay"
  | "pitch_env.sustain"
  | "pitch_env.release"
  | "pitch_env.depth"
  | "arp.enabled"
  | "arp.rate"
  | "arp.gate"
  | "arp.mode"
  | "global.volume"
  | "global.glide_time";

/** One parameter, as described by `paramInfo()`. */
export interface ParamInfo {
  path: ParamPath;
  kind: "float" | "choice" | "toggle";
  /** Smallest value (floats only). */
  min?: number;
  /** Largest value (floats only). */
  max?: number;
  /** The options, in index order (choices only). */
  choices?: string[];
  /** Value in the default patch. */
  default: number | string | boolean;
  /** Part of the shared FX chain: only channel 0's value is used. */
  shared: boolean;
}
