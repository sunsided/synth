// The shipped AudioWorklet entry point (web/worklet.js), loaded into a stub
// AudioWorkletGlobalScope: the real glue, the real wasm, a fake audio thread.
// This is the only coverage the worklet's message routing gets.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { before, describe, test } from "node:test";
import { pathToFileURL } from "node:url";
import { PKG, PROBE_GAIN, SAMPLE_RATE, amplify, assertSameSamples, peak } from "./helpers.mjs";

const QUANTUM = 128;
let Processor;
let module;

before(async () => {
  // Just enough of AudioWorkletGlobalScope for worklet.js.
  globalThis.sampleRate = SAMPLE_RATE;
  globalThis.AudioWorkletProcessor = class {
    constructor() {
      this.port = { posted: [], postMessage(msg) { this.posted.push(msg); } };
    }
  };
  globalThis.registerProcessor = (name, cls) => {
    assert.equal(name, "synthie");
    Processor = cls;
  };
  await import(pathToFileURL(resolve(PKG, "web/worklet.js")));
  module = await WebAssembly.compile(await readFile(resolve(PKG, "web/synthie_bg.wasm")));
});

/** A processor node as `new AudioWorkletNode(ctx, "synthie", …)` creates it. */
function node() {
  const proc = new Processor({ processorOptions: { module } });
  return {
    send: (...msgs) => msgs.forEach((data) => proc.port.onmessage({ data })),
    errors: () => proc.port.posted.filter((m) => m.type === "error").map((m) => m.message),
    /** Run `quanta` render quanta into a stereo output; return both channels. */
    pull(quanta = 1) {
      const left = [];
      const right = [];
      for (let i = 0; i < quanta; i++) {
        const out = [new Float32Array(QUANTUM), new Float32Array(QUANTUM)];
        assert.equal(proc.process([], [out]), true, "processor must stay alive");
        left.push(...out[0]);
        right.push(...out[1]);
      }
      return { left, right };
    },
  };
}

const chord = [
  { type: "loadPreset", index: 1 },
  ...[57, 60, 64].map((note) => ({ type: "noteOn", note })),
];

describe("worklet", () => {
  test("registers a processor that plays the same mix on every channel", () => {
    const n = node();
    n.send(...chord);
    const { left, right } = n.pull(20);
    assert.ok(peak(left) > 0.01, "the chord should be audible");
    assertSameSamples(assert, right, left, "right vs left");
    assert.deepEqual(n.errors(), []);
  });

  test("masterGain messages scale the output exactly", () => {
    const probe = node();
    probe.send({ type: "masterGain", gain: PROBE_GAIN }, ...chord);
    const pre = probe.pull(20).left.map((s) => s * 1024);
    for (const gain of [1, 0.25, -0.5, 8]) {
      const n = node();
      n.send({ type: "masterGain", gain }, ...chord);
      assertSameSamples(assert, n.pull(20).left, pre.map((s) => amplify(s, gain)), `gain ${gain}`);
      assert.deepEqual(n.errors(), []);
    }
  });

  test("normalizeVoices messages lift the voices by the polyphony", () => {
    const on = node();
    on.send({ type: "masterGain", gain: PROBE_GAIN }, ...chord);
    const off = node();
    off.send(
      { type: "masterGain", gain: PROBE_GAIN },
      { type: "normalizeVoices", enabled: false },
      ...chord,
    );
    const expected = on.pull(20).left.map((s) => s * 4);
    assertSameSamples(assert, off.pull(20).left, expected, "normalizeVoices off");
  });

  test("a bad gain is reported over the port, and the audio keeps going unchanged", () => {
    const n = node();
    const twin = node();
    n.send(...chord);
    twin.send(...chord);
    for (const gain of [NaN, Infinity, "loud", undefined]) {
      n.send({ type: "masterGain", gain });
    }
    assert.deepEqual(n.errors(), Array(4).fill("masterGain must be a finite number"));
    assertSameSamples(assert, n.pull(20).left, twin.pull(20).left, "after rejected gains");
  });

  test("unknown messages are reported, not swallowed", () => {
    const n = node();
    n.send({ type: "volume", gain: 1 });
    assert.deepEqual(n.errors(), ["unknown message type: volume"]);
  });
});
