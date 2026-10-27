import type { Drum, ParamInfo, ParamPath, SynthParams } from "./synthie.js";

export type { Drum, ParamInfo, ParamPath, SynthParams };

/** Where and when a command takes effect. */
export interface Timing {
  /** Synth channel, `0..channels` (default 0). */
  channel?: number;
  /** `AudioContext` time in seconds; omitted or past: the next render quantum. */
  time?: number;
}

/** What the processor reports once it has loaded. */
export interface ReadyInfo {
  type: "ready";
  sampleRate: number;
  channels: number;
  presetNames: string[];
  params: ParamInfo[];
}

export interface SynthNodeOptions extends Omit<AudioWorkletNodeOptions, "processorOptions"> {
  /** The compiled `synthie_bg.wasm`. */
  module: WebAssembly.Module;
  /** URL of `worklet.js`; when given, `create` adds the module to the context first. */
  workletUrl?: string | URL;
  /** Post "idle" events after this many seconds of idle silence. */
  idleAfter?: number;
}

/** Values `get` can ask for, and the arguments each takes. */
export interface Queries {
  presetNames: [[], string[]];
  presetPatch: [[index: number], SynthParams];
  params: [[channel?: number], SynthParams];
  paramInfo: [[], ParamInfo[]];
  sampleRate: [[], number];
  channels: [[], number];
  currentFrame: [[], number];
  pendingEvents: [[], number];
  activeVoices: [[], number];
  isIdle: [[], boolean];
  silentFrames: [[], number];
  silenceThreshold: [[], number];
  masterGain: [[], number];
  normalizeVoices: [[], boolean];
}

/** A message for `schedule`. */
export type Message =
  | { type: "noteOn" | "noteOff"; note: number; channel?: number; time?: number }
  | { type: "drum"; hit: Drum; time?: number }
  | { type: "panic" | "cancelScheduled" }
  | { type: "loadPreset"; index: number; channel?: number; time?: number }
  | { type: "loadParams"; params: SynthParams | string; channel?: number; time?: number }
  | { type: "setParam"; path: ParamPath; value: number | string | boolean; channel?: number; time?: number }
  | { type: "masterGain"; gain: number }
  | { type: "normalizeVoices"; enabled: boolean }
  | { type: "silenceThreshold"; value: number }
  | { type: "schedule"; events: Message[] };

export interface SynthNodeEventMap {
  /** A command failed; queries reject their promise instead. */
  error: CustomEvent<{ message: string }>;
  /** The synth has been idle and silent for `idleAfter` seconds. */
  idle: Event;
}

/** Main-thread client for the "synthie" AudioWorklet processor. */
export class SynthNode extends EventTarget {
  /** Load the worklet (if `workletUrl` is given), create the node, wait until ready. */
  static create(context: BaseAudioContext, options: SynthNodeOptions): Promise<SynthNode>;

  /** Wrap an existing "synthie" `AudioWorkletNode`. */
  constructor(node: AudioWorkletNode);

  readonly node: AudioWorkletNode;
  /** Resolves once the processor has loaded. */
  readonly ready: Promise<ReadyInfo>;
  /** The ready report, once it has arrived. */
  readonly info: ReadyInfo | undefined;

  connect(destination: AudioNode, output?: number, input?: number): AudioNode;
  connect(destination: AudioParam, output?: number): void;
  disconnect(): void;

  noteOn(note: number, timing?: Timing): void;
  noteOff(note: number, timing?: Timing): void;
  drum(hit: Drum, timing?: Pick<Timing, "time">): void;
  /** Silence everything and drop all pending events. */
  panic(): void;
  /** Drop all pending events; sounding notes ring on. */
  cancelScheduled(): void;
  loadPreset(index: number, timing?: Timing): void;
  /** Load a full patch; out-of-range values are clamped. */
  loadParams(params: SynthParams | string, timing?: Timing): void;
  /** Change one parameter; numbers are clamped to its range. */
  setParam(path: ParamPath, value: number | string | boolean, timing?: Timing): void;
  setMasterGain(gain: number): void;
  setNormalizeVoices(enabled: boolean): void;
  setSilenceThreshold(value: number): void;
  /** Send many messages at once, e.g. a whole bar with `time`s. */
  schedule(events: Message[]): void;

  get<K extends keyof Queries>(what: K, ...args: Queries[K][0]): Promise<Queries[K][1]>;
  presetNames(): Promise<string[]>;
  presetPatch(index: number): Promise<SynthParams>;
  /** The patch a channel is playing right now. */
  params(channel?: number): Promise<SynthParams>;
  paramInfo(): Promise<ParamInfo[]>;

  addEventListener<K extends keyof SynthNodeEventMap>(
    type: K,
    listener: (event: SynthNodeEventMap[K]) => void,
    options?: boolean | AddEventListenerOptions,
  ): void;
  addEventListener(
    type: string,
    listener: EventListenerOrEventListenerObject | null,
    options?: boolean | AddEventListenerOptions,
  ): void;
}
