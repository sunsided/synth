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
// loadParams { json, channel? }, masterGain { gain },
// normalizeVoices { enabled }. noteOn, noteOff, drum, loadPreset and
// loadParams take an optional `time` in AudioContext seconds (the scale of
// `ctx.currentTime`) and then take effect at exactly that frame; without it,
// or once it has passed, they take effect at the next render quantum.
// schedule { events: [...] } delivers many messages at once.
// Failures come back as { type: "error", message }.
//
// With processorOptions.idleAfter (seconds), the processor also posts
// { type: "idle" } once the synth has been idle and silent for that long
// after making sound, e.g. to suspend the AudioContext.

import "./text-polyfill.js";
import { initSync, Synth } from "./synthie.js";

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
  }

  // Synth frame for an AudioContext time in seconds, or undefined for "now".
  #at(time) {
    if (time === undefined) return undefined;
    if (!Number.isFinite(time)) throw new Error("time must be a finite number of seconds");
    return Math.max(0, Math.round(time * sampleRate) - this.offset);
  }

  #error(e) {
    this.port.postMessage({ type: "error", message: String(e?.message ?? e) });
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
        case "loadParams": this.synth.loadParams(msg.json, msg.channel, at); break;
        case "masterGain": this.synth.masterGain = msg.gain; break;
        case "normalizeVoices": this.synth.normalizeVoices = msg.enabled; break;
        case "schedule":
          if (!Array.isArray(msg.events)) throw new Error("schedule needs an events array");
          for (const event of msg.events) this.#handle(event);
          break;
        default: throw new Error(`unknown message type: ${msg.type}`);
      }
    } catch (e) {
      this.#error(e);
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
