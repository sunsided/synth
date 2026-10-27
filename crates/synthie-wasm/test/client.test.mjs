// The main-thread client (web/client.js) talking to the real worklet through
// a fake AudioWorkletNode. Messages cross the "thread boundary" asynchronously
// and are structured-cloned, as in a browser.

import assert from "node:assert/strict";
import { resolve } from "node:path";
import { before, describe, test } from "node:test";
import { pathToFileURL } from "node:url";
import { PKG, SAMPLE_RATE, loadWorklet } from "./helpers.mjs";

let SynthNode;
let module;
const created = [];

before(async () => {
  let Processor;
  ({ Processor, module } = await loadWorklet());

  // A node whose port is wired to a processor instance, with async delivery.
  globalThis.AudioWorkletNode = class {
    constructor(context, name, options) {
      assert.equal(name, "synthie");
      this.options = options;
      this.connected = [];
      this.processor = new Processor(options);
      const proc = this.processor;
      this.port = {
        onmessage: null,
        postMessage: (msg) => {
          const data = structuredClone(msg);
          setTimeout(() => proc.port.onmessage({ data }));
        },
      };
      const deliver = (msg) => {
        const data = structuredClone(msg);
        setTimeout(() => this.port.onmessage?.({ data }));
      };
      proc.port.posted.forEach(deliver); // sent during construction (ready)
      proc.port.onpost = deliver;
      created.push(this);
    }
    connect(destination) {
      this.connected.push(destination);
      return destination;
    }
    disconnect() {
      this.connected = [];
    }
  };

  ({ SynthNode } = await import(pathToFileURL(resolve(PKG, "web/client.js"))));
});

/** A fake AudioContext that records which worklet modules were added. */
function context() {
  return {
    sampleRate: SAMPLE_RATE,
    added: [],
    audioWorklet: {
      async addModule(url) {
        this.owner.added.push(url);
      },
    },
    init() {
      this.audioWorklet.owner = this;
      return this;
    },
  }.init();
}

/** Let queued port messages arrive. */
const settle = () => new Promise((r) => setTimeout(r, 5));

/** Render quanta on the processor behind a client. */
function pull(synth, quanta = 1) {
  const node = created.find((n) => n === synth.node);
  const out = [];
  for (let i = 0; i < quanta; i++) {
    const quantum = [new Float32Array(128)];
    node.processor.process([], [quantum]);
    out.push(...quantum[0]);
  }
  return out;
}

describe("SynthNode", () => {
  test("create loads the worklet, builds the node and waits for ready", async () => {
    const ctx = context();
    const synth = await SynthNode.create(ctx, { module, workletUrl: "/synthie/worklet.js", idleAfter: 0.5 });
    assert.deepEqual(ctx.added, ["/synthie/worklet.js"]);
    assert.equal(synth.info.type, "ready");
    assert.equal(synth.info.channels, 4);
    assert.deepEqual(synth.node.options.outputChannelCount, [2]);
    assert.equal(synth.node.options.processorOptions.idleAfter, 0.5);
    const destination = {};
    assert.equal(synth.connect(destination), destination);
  });

  test("queries resolve with their own answers, even when interleaved", async () => {
    const synth = await SynthNode.create(context(), { module });
    const [names, patch, cutoff, gain, info] = await Promise.all([
      synth.presetNames(),
      synth.presetPatch(1),
      synth.params(0).then((p) => p.filter.cutoff),
      synth.get("masterGain"),
      synth.paramInfo(),
    ]);
    assert.deepEqual(names, synth.info.presetNames);
    assert.equal(patch.filter.cutoff, 3500);
    assert.equal(cutoff, 4000);
    assert.equal(gain, 0.5);
    assert.deepEqual(info, synth.info.params);
  });

  test("a failed query rejects its promise, without an error event", async () => {
    const synth = await SynthNode.create(context(), { module });
    let events = 0;
    synth.addEventListener("error", () => events++);
    await assert.rejects(synth.get("nonsense"), /unknown query: nonsense/);
    await assert.rejects(synth.presetPatch(42), /no preset 42/);
    assert.equal(events, 0);
  });

  test("commands reach the synth; failed commands raise error events", async () => {
    const synth = await SynthNode.create(context(), { module });
    const errors = [];
    synth.addEventListener("error", (e) => errors.push(e.detail.message));
    synth.loadPreset(1, { channel: 1 });
    synth.setParam("filter.cutoff", 777, { channel: 1 });
    synth.setParam("osc.waveform", "Sine", { channel: 1 });
    synth.noteOn(60, { channel: 1 });
    synth.setParam("delay.mix", 1, { channel: 1 }); // shared: refused
    synth.noteOn(999);
    await settle();
    pull(synth);
    const p = await synth.params(1);
    assert.equal(p.filter.cutoff, 777);
    assert.equal(p.osc.waveform, "Sine");
    assert.equal(await synth.get("activeVoices"), 1);
    assert.deepEqual(errors, [
      "delay.mix belongs to the shared FX chain: set it on channel 0",
      "note must be a MIDI note number 0..=127",
    ]);
  });

  test("loadParams sends patch objects across the boundary", async () => {
    const synth = await SynthNode.create(context(), { module });
    const patch = await synth.presetPatch(4);
    patch.global.volume = 0.2;
    synth.loadParams(patch, { channel: 3 });
    await settle();
    pull(synth);
    assert.deepEqual(await synth.params(3), patch);
  });

  test("settings and schedule pass through", async () => {
    const synth = await SynthNode.create(context(), { module });
    synth.setMasterGain(0.9);
    synth.setNormalizeVoices(false);
    synth.setSilenceThreshold(0.01);
    synth.schedule([
      { type: "noteOn", note: 60 },
      { type: "noteOn", note: 64, channel: 1 },
    ]);
    await settle();
    pull(synth);
    assert.equal(await synth.get("masterGain"), Math.fround(0.9));
    assert.equal(await synth.get("normalizeVoices"), false);
    assert.equal(await synth.get("silenceThreshold"), Math.fround(0.01));
    assert.equal(await synth.get("activeVoices"), 2);
    synth.panic();
    await settle();
    pull(synth);
    assert.equal(await synth.get("activeVoices"), 0);
  });

  test("idle events arrive once a sound has died away", async () => {
    const synth = await SynthNode.create(context(), { module, idleAfter: 0.05 });
    let idles = 0;
    synth.addEventListener("idle", () => idles++);
    synth.drum("hihat-closed");
    await settle();
    pull(synth, 1500);
    await settle();
    assert.equal(idles, 1);
  });
});
