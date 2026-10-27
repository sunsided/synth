// The shipped AudioWorklet entry point (web/worklet.js), loaded into a stub
// AudioWorkletGlobalScope: the real glue, the real wasm, a fake audio thread.
// This is the only coverage the worklet's message routing gets.

import assert from "node:assert/strict";
import { before, describe, test } from "node:test";
import { PROBE_GAIN, SAMPLE_RATE, amplify, assertSameSamples, loadWorklet, peak } from "./helpers.mjs";

const QUANTUM = 128;
let Processor;
let module;

before(async () => {
  let registered;
  ({ registered, Processor, module } = await loadWorklet());
  assert.deepEqual(Object.keys(registered), ["synthie"]);
});

/**
 * A processor node as `new AudioWorkletNode(ctx, "synthie", …)` creates it,
 * in a context whose clock already stands at `startFrame` (the context has
 * been running before the node was created).
 */
function node({ startFrame = 12_345 * QUANTUM, ...options } = {}) {
  let clock = startFrame;
  const tick = () => {
    globalThis.currentFrame = clock;
  };
  tick();
  const proc = new Processor({ processorOptions: { module, ...options } });
  const posted = (type) => proc.port.posted.filter((m) => m.type === type);
  return {
    proc,
    posted,
    /** Send one message and return the replies it produced. */
    ask(msg) {
      const before = proc.port.posted.length;
      this.send(msg);
      return proc.port.posted.slice(before);
    },
    /** AudioContext time of the next render quantum, in seconds. */
    get currentTime() {
      return clock / SAMPLE_RATE;
    },
    send: (...msgs) => {
      tick();
      msgs.forEach((data) => proc.port.onmessage({ data }));
    },
    errors: () => posted("error").map((m) => m.message),
    idles: () => posted("idle").length,
    /** Run `quanta` render quanta into a stereo output; return both channels. */
    pull(quanta = 1) {
      const left = [];
      const right = [];
      for (let i = 0; i < quanta; i++) {
        tick();
        const out = [new Float32Array(QUANTUM), new Float32Array(QUANTUM)];
        assert.equal(proc.process([], [out]), true, "processor must stay alive");
        left.push(...out[0]);
        right.push(...out[1]);
        clock += QUANTUM;
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

/** Index of the first non-zero sample. */
const onset = (buf) => buf.findIndex((s) => s !== 0);

describe("worklet scheduling", () => {
  test("a timed note starts on its exact frame, wherever the context clock stands", () => {
    for (const startFrame of [0, 128, 99 * QUANTUM, 1e6 * QUANTUM]) {
      for (const delay of [0, 1, 127, 128, 777, 2_000]) {
        const n = node({ startFrame });
        n.pull(3); // the node has been running for a while
        const t0 = n.currentTime;
        n.send({ type: "noteOn", note: 69, time: t0 + delay / SAMPLE_RATE });
        const first = onset(n.pull(24).left);
        // The attack starts from zero, so the first sample may still be 0.
        assert.ok(first === delay || first === delay + 1, `start ${startFrame}, delay ${delay}: ${first}`);
      }
    }
  });

  test("a schedule renders the same as messages sent just in time", () => {
    const beat = 0.05; // seconds
    const pattern = [0, 1, 2, 3, 4, 5, 6, 7].map((step) => ({
      type: step % 4 === 0 ? "drum" : "noteOn",
      ...(step % 4 === 0 ? { hit: "kick" } : { note: 60 + step }),
      step,
    }));
    const quantaPerBeat = (beat * SAMPLE_RATE) / QUANTUM; // 18.75: steps fall mid-quantum

    const scheduled = node();
    const t0 = scheduled.currentTime;
    scheduled.send({
      type: "schedule",
      events: pattern.map(({ step, ...msg }) => ({ ...msg, time: t0 + step * beat })),
    });
    const ahead = scheduled.pull(Math.ceil(quantaPerBeat * 8) + 8).left;
    assert.deepEqual(scheduled.errors(), []);

    // Each hit lands on its own frame, not on a quantum boundary.
    for (const { step, type } of pattern) {
      const frame = Math.round(step * beat * SAMPLE_RATE);
      if (type === "drum") assert.notEqual(ahead[frame + 1], 0, `kick at step ${step}`);
    }
    // Just-in-time messages snap to quantum boundaries: audibly different.
    const late = node();
    const lateOut = [];
    for (let q = 0; q < Math.ceil(quantaPerBeat * 8) + 8; q++) {
      for (const { step, ...msg } of pattern) {
        if (Math.floor((step * beat * SAMPLE_RATE) / QUANTUM) === q) late.send(msg);
      }
      lateOut.push(...late.pull(1).left);
    }
    assert.notDeepEqual(lateOut, ahead, "scheduling should beat just-in-time messages");
  });

  test("past times play at once; bad times are reported", () => {
    const n = node();
    n.send({ type: "drum", hit: "kick", time: 0 }); // long gone
    assert.ok(onset(n.pull(1).left) <= 1);
    n.send({ type: "noteOn", note: 60, time: "soon" }, { type: "noteOn", note: 60, time: NaN });
    assert.deepEqual(n.errors(), Array(2).fill("time must be a finite number of seconds"));
  });

  test("schedule reports each bad event and still plays the good ones", () => {
    const n = node();
    n.send({
      type: "schedule",
      events: [{ type: "noteOn", note: 300 }, { type: "drum", hit: "kick" }, { type: "nope" }],
    });
    assert.equal(n.errors().length, 2);
    assert.ok(peak(n.pull(10).left) > 0.01, "the kick should still play");
    n.send({ type: "schedule" });
    assert.deepEqual(n.errors().at(-1), "schedule needs an events array");
  });

  test("cancelScheduled and panic stop a pattern that is already queued", () => {
    for (const type of ["cancelScheduled", "panic"]) {
      const n = node();
      const t0 = n.currentTime;
      n.send({
        type: "schedule",
        events: [1, 2, 3].map((s) => ({ type: "drum", hit: "kick", time: t0 + s * 0.1 })),
      });
      n.pull(10);
      n.send({ type });
      assert.equal(peak(n.pull(200).left), 0, `${type} should silence the pattern`);
    }
  });
});

describe("worklet idle notification", () => {
  test("posts idle once after a sound has died away", () => {
    const n = node({ idleAfter: 0.1 });
    n.pull(100);
    assert.equal(n.idles(), 0, "no idle before anything has played");
    n.send({ type: "drum", hit: "kick" });
    n.pull(5);
    assert.equal(n.idles(), 0, "still ringing");
    n.pull(1000);
    assert.equal(n.idles(), 1);
    n.pull(1000);
    assert.equal(n.idles(), 1, "posted only once per sound");
    n.send({ type: "drum", hit: "hihat-closed" });
    n.pull(1000);
    assert.equal(n.idles(), 2, "re-armed by the next sound");
  });

  test("a pending event is not idle", () => {
    const n = node({ idleAfter: 0.05 });
    n.send({ type: "drum", hit: "kick" });
    n.send({ type: "drum", hit: "kick", time: n.currentTime + 2 });
    n.pull(Math.floor((1.5 * SAMPLE_RATE) / QUANTUM));
    assert.equal(n.idles(), 0, "the second kick is still to come");
  });

  test("is off unless asked for, and rejects nonsense", () => {
    const n = node();
    n.send({ type: "drum", hit: "kick" });
    n.pull(2000);
    assert.equal(n.idles(), 0);
    assert.deepEqual(node({ idleAfter: -1 }).errors(), ["idleAfter must be a number of seconds >= 0"]);
  });
});

describe("worklet ready report and queries", () => {
  test("reports ready, once, with the preset names and the parameter registry", () => {
    const n = node();
    const ready = n.posted("ready");
    assert.equal(ready.length, 1);
    assert.equal(n.proc.port.posted[0].type, "ready", "ready comes first");
    const [info] = ready;
    assert.equal(info.sampleRate, SAMPLE_RATE);
    assert.equal(info.channels, 4);
    assert.ok(info.presetNames.length >= 8);
    const cutoff = info.params.find((p) => p.path === "filter.cutoff");
    assert.deepEqual(cutoff, { path: "filter.cutoff", default: 4000, shared: false, kind: "float", min: 20, max: 18000 });
  });

  test("answers each query with its id", () => {
    const n = node();
    const ask = (what, ...args) => {
      const replies = n.ask({ type: "get", what, args, id: `q-${what}` });
      assert.equal(replies.length, 1, what);
      assert.equal(replies[0].type, "result", `${what}: ${replies[0].message}`);
      assert.equal(replies[0].id, `q-${what}`);
      return replies[0].value;
    };
    assert.deepEqual(ask("presetNames"), n.posted("ready")[0].presetNames);
    assert.equal(ask("presetPatch", 1).filter.cutoff, 3500);
    assert.equal(ask("params", 0).filter.cutoff, 4000);
    assert.equal(ask("paramInfo").length, n.posted("ready")[0].params.length);
    assert.equal(ask("sampleRate"), SAMPLE_RATE);
    assert.equal(ask("masterGain"), 0.5);
    assert.equal(ask("isIdle"), true);
    n.send({ type: "noteOn", note: 60 });
    n.pull(1);
    assert.equal(ask("activeVoices"), 1);
    assert.equal(ask("currentFrame"), QUANTUM);
  });

  test("errors carry the id of the message that failed", () => {
    const n = node();
    assert.deepEqual(n.ask({ type: "get", what: "secrets", id: 7 }), [
      { type: "error", message: "unknown query: secrets", id: 7 },
    ]);
    assert.deepEqual(n.ask({ type: "noteOn", note: 300, id: "bad-note" }), [
      { type: "error", message: "note must be a MIDI note number 0..=127", id: "bad-note" },
    ]);
    assert.deepEqual(n.ask({ type: "noteOn", note: 300 }), [
      { type: "error", message: "note must be a MIDI note number 0..=127" },
    ]);
  });

  test("queries cannot reach anything but the listed methods and properties", () => {
    const n = node();
    for (const what of ["render", "free", "constructor", "__proto__", "toString", "panic"]) {
      const [reply] = n.ask({ type: "get", what, id: 1 });
      assert.equal(reply.type, "error", what);
    }
  });
});

describe("worklet patch messages", () => {
  test("setParam changes one parameter, at its time", () => {
    const n = node();
    const params = () => n.ask({ type: "get", what: "params", id: 0 })[0].value;
    n.send({ type: "loadPreset", index: 1 });
    n.pull(1);
    const t = n.currentTime + 1000 / SAMPLE_RATE;
    n.send(
      { type: "setParam", path: "filter.cutoff", value: 1234, time: t },
      { type: "setParam", path: "osc.waveform", value: "Sine", time: t },
    );
    n.pull(5); // 640 frames: not yet
    assert.equal(params().filter.cutoff, 3500);
    n.pull(5);
    const after = params();
    assert.equal(after.filter.cutoff, 1234);
    assert.equal(after.osc.waveform, "Sine");
    assert.equal(after.env.attack, 0.003, "the rest of the preset stays");
    assert.deepEqual(n.errors(), []);
  });

  test("setParam rejects nonsense with a reason", () => {
    const n = node();
    n.send(
      { type: "setParam", path: "filter.cutof", value: 1 },
      { type: "setParam", path: "osc.waveform", value: "Kazoo" },
      { type: "setParam", path: "delay.mix", value: 0.5, channel: 1 },
    );
    assert.deepEqual(n.errors(), [
      "unknown parameter: filter.cutof",
      'osc.waveform must be one of "Pulse", "Sawtooth", "Triangle", "Noise", "PulseSaw", "Sine"',
      "delay.mix belongs to the shared FX chain: set it on channel 0",
    ]);
  });

  test("loadParams takes a patch object as well as JSON text", () => {
    const n = node();
    const patch = n.ask({ type: "get", what: "presetPatch", args: [2], id: 0 })[0].value;
    patch.global.volume = 0.25;
    n.send({ type: "loadParams", params: patch, channel: 1 });
    n.send({ type: "loadParams", json: JSON.stringify(patch), channel: 2 });
    n.pull(1);
    for (const channel of [1, 2]) {
      assert.deepEqual(n.ask({ type: "get", what: "params", args: [channel], id: 0 })[0].value, patch);
    }
    assert.deepEqual(n.errors(), []);
  });
});

