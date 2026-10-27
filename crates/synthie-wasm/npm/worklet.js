// AudioWorklet entry point: registers the "synthie" processor.
//
//   await ctx.audioWorklet.addModule(workletUrl);
//   const module = await WebAssembly.compileStreaming(fetch(wasmUrl));
//   const node = new AudioWorkletNode(ctx, "synthie", {
//     outputChannelCount: [2],
//     processorOptions: { module },
//   });
//   node.connect(ctx.destination);
//   node.port.postMessage({ type: "noteOn", note: 69 });
//
// Messages mirror the Synth methods: noteOn / noteOff { note, channel? },
// drum { hit }, panic, cancelScheduled, loadPreset { index, channel? },
// loadParams { params, channel? } (a patch object or JSON text; `json` is
// accepted too), setParam { path, value, channel? }, masterGain { gain },
// normalizeVoices { enabled }, silenceThreshold { value }. noteOn, noteOff,
// drum, loadPreset, loadParams and setParam take an optional `time` in
// AudioContext seconds (the scale of `ctx.currentTime`) and then take effect
// at exactly that frame; without it, or once it has passed, they take effect
// at the next render quantum. schedule { events: [...] } delivers many
// messages at once.
//
// get { what, args?, id } asks for a value and is answered with
// { type: "result", id, value }: presetNames, presetPatch (args: [index]),
// params (args: [channel]), paramInfo, or one of the Synth properties
// sampleRate, channels, currentFrame, pendingEvents, activeVoices, isIdle,
// silentFrames, silenceThreshold, masterGain, normalizeVoices.
//
// Once loaded, the processor posts { type: "ready", sampleRate, channels,
// presetNames, params } (params: the paramInfo() registry). Failures come
// back as { type: "error", message, id? }, with the `id` of the message that
// failed if it had one; check `type` before reading other fields.
//
// With processorOptions.idleAfter (seconds), the processor also posts
// { type: "idle" } once the synth has been idle and silent for that long
// after making sound, e.g. to suspend the AudioContext.

import "./text-polyfill.js";
import { initSync, Synth } from "./synthie.js";

// Queries answered by calling a Synth method with the message's `args`.
const METHODS = {
  presetNames: (synth) => synth.presetNames(),
  presetPatch: (synth, index) => synth.presetPatch(index),
  params: (synth, channel) => synth.params(channel),
  paramInfo: (synth) => synth.paramInfo(),
};

// Queries answered by reading a Synth property.
const PROPERTIES = new Set([
  "sampleRate", "channels", "currentFrame", "pendingEvents", "activeVoices",
  "isIdle", "silentFrames", "silenceThreshold", "masterGain", "normalizeVoices",
]);

class SynthieProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const { module, idleAfter } = options.processorOptions;
    initSync({ module });
    this.synth = new Synth(sampleRate);
    this.mono = new Float32Array(128);
    // Context frame minus synth frame. Refreshed in process(), where both
    // clocks point at the start of the same render quantum.
    this.offset = currentFrame;
    this.idleFrames = idleAfter === undefined ? undefined : Math.round(idleAfter * sampleRate);
    this.idleArmed = false;
    this.port.onmessage = ({ data }) => this.#handle(data);
    if (this.idleFrames !== undefined && !(this.idleFrames >= 0)) {
      this.idleFrames = undefined;
      this.#error("idleAfter must be a number of seconds >= 0");
    }
    this.port.postMessage({
      type: "ready",
      sampleRate,
      channels: this.synth.channels,
      presetNames: this.synth.presetNames(),
      params: this.synth.paramInfo(),
    });
  }

  // Synth frame for an AudioContext time in seconds, or undefined for "now".
  #at(time) {
    if (time === undefined) return undefined;
    if (!Number.isFinite(time)) throw new Error("time must be a finite number of seconds");
    return Math.max(0, Math.round(time * sampleRate) - this.offset);
  }

  #error(e, id) {
    const message = String(e?.message ?? e);
    this.port.postMessage(id === undefined ? { type: "error", message } : { type: "error", message, id });
  }

  #query(what, args) {
    if (Object.hasOwn(METHODS, what)) return METHODS[what](this.synth, ...args);
    if (PROPERTIES.has(what)) return this.synth[what];
    throw new Error(`unknown query: ${what}`);
  }

  #handle(msg) {
    try {
      const at = this.#at(msg.time);
      switch (msg.type) {
        case "noteOn": this.synth.noteOn(msg.note, msg.channel, at); break;
        case "noteOff": this.synth.noteOff(msg.note, msg.channel, at); break;
        case "drum": this.synth.drum(msg.hit, at); break;
        case "panic": this.synth.panic(); break;
        case "cancelScheduled": this.synth.cancelScheduled(); break;
        case "loadPreset": this.synth.loadPreset(msg.index, msg.channel, at); break;
        case "loadParams": this.synth.loadParams(msg.params ?? msg.json, msg.channel, at); break;
        case "setParam": this.synth.setParam(msg.path, msg.value, msg.channel, at); break;
        case "masterGain": this.synth.masterGain = msg.gain; break;
        case "normalizeVoices": this.synth.normalizeVoices = msg.enabled; break;
        case "silenceThreshold": this.synth.silenceThreshold = msg.value; break;
        case "get": {
          const value = this.#query(msg.what, msg.args ?? []);
          this.port.postMessage({ type: "result", id: msg.id, value });
          break;
        }
        case "schedule":
          if (!Array.isArray(msg.events)) throw new Error("schedule needs an events array");
          for (const event of msg.events) this.#handle(event);
          break;
        default: throw new Error(`unknown message type: ${msg.type}`);
      }
    } catch (e) {
      this.#error(e, msg?.id);
    }
  }

  process(_inputs, outputs) {
    this.offset = currentFrame - this.synth.currentFrame;
    const output = outputs[0];
    const frames = output[0].length;
    if (this.mono.length !== frames) this.mono = new Float32Array(frames);
    this.synth.render(this.mono, 1);
    for (const channel of output) channel.set(this.mono);

    if (this.idleFrames !== undefined) {
      const silent = this.synth.silentFrames;
      if (silent === 0) {
        this.idleArmed = true;
      } else if (this.idleArmed && silent >= this.idleFrames) {
        this.idleArmed = false;
        this.port.postMessage({ type: "idle" });
      }
    }
    return true;
  }
}

registerProcessor("synthie", SynthieProcessor);
