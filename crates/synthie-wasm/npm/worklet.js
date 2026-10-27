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
// drum { hit }, panic, loadPreset { index, channel? },
// loadParams { json, channel? }. Failures come back as { type: "error", message }.

import "./text-polyfill.js";
import { initSync, Synth } from "./synthie.js";

class SynthieProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    initSync({ module: options.processorOptions.module });
    this.synth = new Synth(sampleRate);
    this.mono = new Float32Array(128);
    this.port.onmessage = ({ data }) => this.#handle(data);
  }

  #handle(msg) {
    try {
      switch (msg.type) {
        case "noteOn": this.synth.noteOn(msg.note, msg.channel); break;
        case "noteOff": this.synth.noteOff(msg.note, msg.channel); break;
        case "drum": this.synth.drum(msg.hit); break;
        case "panic": this.synth.panic(); break;
        case "loadPreset": this.synth.loadPreset(msg.index, msg.channel); break;
        case "loadParams": this.synth.loadParams(msg.json, msg.channel); break;
        default: throw new Error(`unknown message type: ${msg.type}`);
      }
    } catch (e) {
      this.port.postMessage({ type: "error", message: String(e?.message ?? e) });
    }
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    const frames = output[0].length;
    if (this.mono.length !== frames) this.mono = new Float32Array(frames);
    this.synth.render(this.mono, 1);
    for (const channel of output) channel.set(this.mono);
    return true;
  }
}

registerProcessor("synthie", SynthieProcessor);
