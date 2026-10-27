// The `Synth` class of the Node.js build: the output-level properties as a
// JavaScript caller sees them, including what happens to values that are
// not f32-shaped numbers on their way across the wasm boundary.

import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { PROBE_GAIN, SAMPLE_RATE, Synth, amplify, assertSameSamples, peak } from "./helpers.mjs";

/** A fresh synth playing an A-minor chord on two channels plus a kick. */
function playing({ masterGain, normalizeVoices } = {}) {
  const synth = new Synth(SAMPLE_RATE);
  if (masterGain !== undefined) synth.masterGain = masterGain;
  if (normalizeVoices !== undefined) synth.normalizeVoices = normalizeVoices;
  synth.loadPreset(1, 0);
  synth.loadPreset(4, 1);
  for (const note of [57, 60, 64]) synth.noteOn(note, 0);
  synth.noteOn(45, 1);
  return synth;
}

function render(synth, frames = 4_800) {
  const buf = new Float32Array(frames);
  synth.render(buf, 1);
  return buf;
}

describe("masterGain", () => {
  test("defaults to 0.5 and round-trips through f32", () => {
    const synth = new Synth(SAMPLE_RATE);
    assert.equal(synth.masterGain, 0.5);
    synth.masterGain = 0.75;
    assert.equal(synth.masterGain, 0.75);
    // The engine stores an f32: JS sees the nearest f32, not the double.
    synth.masterGain = 0.1;
    assert.equal(synth.masterGain, Math.fround(0.1));
    assert.notEqual(synth.masterGain, 0.1);
  });

  for (const bad of [NaN, Infinity, -Infinity, "loud", undefined, {}]) {
    test(`rejects ${String(bad)} and keeps the previous gain`, () => {
      const synth = new Synth(SAMPLE_RATE);
      synth.masterGain = 0.25;
      assert.throws(() => {
        synth.masterGain = bad;
      }, /masterGain must be a finite number/);
      assert.equal(synth.masterGain, 0.25);
    });
  }

  // Numbers beyond f32 range round to ±Infinity on the way in.
  test("rejects doubles that overflow f32", () => {
    const synth = new Synth(SAMPLE_RATE);
    assert.throws(() => {
      synth.masterGain = 1e39;
    }, /finite/);
  });

  test("scales the output exactly, with the clamp as the only nonlinearity", () => {
    const pre = render(playing({ masterGain: PROBE_GAIN })).map((s) => s * 1024);
    for (const gain of [0, 0.5, 1, 1.7, 3, 64, -0.5, -2]) {
      const out = render(playing({ masterGain: gain }));
      assertSameSamples(assert, out, pre.map((s) => amplify(s, gain)), `gain ${gain}`);
    }
  });

  test("loud gains clip at ±1 instead of overflowing", () => {
    const out = render(playing({ masterGain: 1e30 }));
    assert.equal(peak(out), 1);
    assert.ok(out.every((s) => s >= -1 && s <= 1));
  });

  test("can be ridden between render calls without disturbing the voices", () => {
    const ridden = playing();
    const twin = playing({ masterGain: PROBE_GAIN });
    const automation = [0.5, 1, 0.25, 0, 2, -1, 0.5];
    for (let block = 0; block < 40; block++) {
      const gain = automation[block % automation.length];
      ridden.masterGain = gain;
      const out = render(ridden, 128);
      const expected = render(twin, 128).map((s) => amplify(s * 1024, gain));
      assertSameSamples(assert, out, expected, `block ${block} at gain ${gain}`);
    }
  });
});

describe("normalizeVoices", () => {
  test("defaults to true and round-trips", () => {
    const synth = new Synth(SAMPLE_RATE);
    assert.equal(synth.normalizeVoices, true);
    synth.normalizeVoices = false;
    assert.equal(synth.normalizeVoices, false);
    synth.normalizeVoices = true;
    assert.equal(synth.normalizeVoices, true);
  });

  test("turning it off is exactly four times the gain for synth voices", () => {
    const synthOnly = (opts) => {
      const synth = new Synth(SAMPLE_RATE);
      synth.masterGain = opts.masterGain;
      synth.normalizeVoices = opts.normalizeVoices;
      synth.loadPreset(1, 0);
      for (const note of [57, 60, 64]) synth.noteOn(note, 0);
      return render(synth);
    };
    const off = synthOnly({ masterGain: 0.125, normalizeVoices: false });
    const on = synthOnly({ masterGain: 0.5, normalizeVoices: true });
    assert.ok(peak(on) > 0.01, "the chord should be audible");
    assertSameSamples(assert, off, on, "Off at g vs Polyphony at 4g");
  });

  test("leaves drums alone", () => {
    const kick = (normalizeVoices) => {
      const synth = new Synth(SAMPLE_RATE);
      synth.normalizeVoices = normalizeVoices;
      synth.drum("kick");
      return render(synth);
    };
    assert.ok(peak(kick(true)) > 0.01, "the kick should be audible");
    assertSameSamples(assert, kick(false), kick(true), "kick");
  });

  test("lifts a lone note by exactly the polyphony", () => {
    const note = (normalizeVoices) => {
      const synth = new Synth(SAMPLE_RATE);
      synth.masterGain = PROBE_GAIN;
      synth.normalizeVoices = normalizeVoices;
      synth.noteOn(69);
      return render(synth);
    };
    const on = note(true);
    assertSameSamples(assert, note(false), on.map((s) => s * 4), "lone note");
  });
});

describe("scheduling", () => {
  test("currentFrame advances with every render", () => {
    const synth = new Synth(SAMPLE_RATE);
    assert.equal(synth.currentFrame, 0);
    render(synth, 100);
    synth.render(new Float32Array(256), 2);
    assert.equal(synth.currentFrame, 228);
  });

  test("an event lands on its frame regardless of how renders are sliced", () => {
    const performance = (synth) => {
      synth.loadPreset(1, 0);
      synth.noteOn(57, 0, 1_000);
      synth.noteOn(64, 1, 1_000);
      synth.drum("kick", 3_333);
      synth.noteOff(57, 0, 9_000);
    };
    const whole = new Synth(SAMPLE_RATE);
    performance(whole);
    assert.equal(whole.pendingEvents, 5);
    const reference = render(whole, 12_000);
    assert.equal(whole.pendingEvents, 0);

    for (const slice of [1, 7, 128, 1_000, 4_096]) {
      const sliced = new Synth(SAMPLE_RATE);
      performance(sliced);
      const out = new Float32Array(12_000);
      for (let pos = 0; pos < out.length; pos += slice) {
        sliced.render(out.subarray(pos, Math.min(pos + slice, out.length)), 1);
      }
      assertSameSamples(assert, out, reference, `slices of ${slice}`);
    }
    assert.equal(reference.findIndex((s) => s !== 0) >= 1_000, true);
  });

  for (const bad of [-1, NaN, Infinity, "soon", {}, 2 ** 53]) {
    test(`rejects at = ${String(bad)}`, () => {
      const synth = new Synth(SAMPLE_RATE);
      assert.throws(() => synth.noteOn(60, 0, bad), /at must be a frame position/);
      assert.equal(synth.pendingEvents, 0);
    });
  }

  test("the queue holds at most 4096 events, and says so", () => {
    const synth = new Synth(SAMPLE_RATE);
    for (let i = 0; i < 4096; i++) synth.drum("kick", i * 10);
    assert.throws(() => synth.drum("kick"), /too many pending events \(at most 4096\)/);
    synth.cancelScheduled();
    assert.equal(synth.pendingEvents, 0);
    synth.drum("kick");
  });
});

describe("silence", () => {
  test("a fresh synth is idle", () => {
    const synth = new Synth(SAMPLE_RATE);
    assert.equal(synth.isIdle, true);
    assert.equal(synth.activeVoices, 0);
    assert.equal(synth.renderUntilSilent(5).length, 0);
  });

  test("activeVoices follows notes through their release", () => {
    const synth = new Synth(SAMPLE_RATE);
    synth.noteOn(60);
    synth.noteOn(64, 1);
    render(synth, 128);
    assert.equal(synth.activeVoices, 2);
    assert.equal(synth.isIdle, false);
    synth.noteOff(60);
    synth.noteOff(64, 1);
    render(synth, 4 * SAMPLE_RATE);
    assert.equal(synth.activeVoices, 0);
  });

  test("renderUntilSilent returns a one-shot sound effect, tail included, trimmed", () => {
    const synth = new Synth(SAMPLE_RATE);
    synth.loadPreset(1);
    synth.normalizeVoices = false;
    synth.noteOn(69);
    synth.noteOff(69, 0, 0.2 * SAMPLE_RATE);
    synth.drum("hihat-open", 0.1 * SAMPLE_RATE);
    const sfx = synth.renderUntilSilent(20);
    assert.ok(sfx instanceof Float32Array);
    assert.ok(sfx.length > 0.2 * SAMPLE_RATE, "outlasts the held note");
    assert.ok(sfx.length < 20 * SAMPLE_RATE, "ends before the cap");
    assert.ok(Math.abs(sfx.at(-1)) > synth.silenceThreshold, "no trailing silence");
    assert.equal(synth.isIdle, true);
    assert.ok(peak(render(synth, SAMPLE_RATE)) <= synth.silenceThreshold, "nothing was cut off");
  });

  test("renderUntilSilent honours the cap and the hold time", () => {
    const held = new Synth(SAMPLE_RATE);
    held.noteOn(60);
    assert.equal(held.renderUntilSilent(0.5).length, 0.5 * SAMPLE_RATE);

    // Rendering continues for the hold time past the point where the synth
    // fell idle and silent; that overrun is trimmed off again.
    const overrun = (hold) => {
      const synth = new Synth(SAMPLE_RATE);
      synth.drum("kick");
      const sfx = synth.renderUntilSilent(10, hold);
      return { frames: synth.currentFrame - sfx.length, length: sfx.length };
    };
    const short = overrun(0.01);
    const long = overrun(3);
    assert.equal(long.length, short.length, "the hold never changes the sound itself");
    const extra = long.frames - short.frames;
    assert.ok(Math.abs(extra - 2.99 * SAMPLE_RATE) <= 128, `3 s hold rendered ${extra} more frames`);
  });

  test("a louder threshold ends the sound sooner", () => {
    const length = (threshold) => {
      const synth = new Synth(SAMPLE_RATE);
      synth.silenceThreshold = threshold;
      synth.loadPreset(1);
      synth.noteOn(69);
      synth.noteOff(69, 0, 4_800);
      return synth.renderUntilSilent(20, 0.5).length;
    };
    assert.ok(length(0.01) < length(1e-5));
  });

  for (const [what, call] of [
    ["silenceThreshold", (s) => { s.silenceThreshold = -1; }],
    ["silenceThreshold", (s) => { s.silenceThreshold = NaN; }],
    ["maxSeconds", (s) => s.renderUntilSilent(-1)],
    ["maxSeconds", (s) => s.renderUntilSilent(Infinity)],
    ["holdSeconds", (s) => s.renderUntilSilent(1, NaN)],
  ]) {
    test(`rejects a bad ${what}`, () => {
      assert.throws(() => call(new Synth(SAMPLE_RATE)), new RegExp(`${what} must be a finite number >= 0`));
    });
  }
});


describe("untrusted patches", () => {
  test("absurd values from JSON are clamped, and the synth keeps working", () => {
    const synth = new Synth(SAMPLE_RATE);
    const params = JSON.parse(synth.presetParams(1));
    // The patch fuzzing found: it used to silence the engine for good.
    params.chorus = { rate: 1.7e38, depth_ms: 9.2e-41, mix: 5.8e17 };
    params.delay = { time_ms: -5, feedback: 1e30, mix: 1e9 };
    params.filter.cutoff = 1e38;
    params.env.attack = -1;
    params.crusher = { bits: 0, rate: 0 };
    params.global.volume = 3e38;
    synth.loadParams(JSON.stringify(params));
    for (const note of [57, 60, 64]) synth.noteOn(note);
    const loud = render(synth, SAMPLE_RATE);
    assert.ok(loud.every((s) => Number.isFinite(s) && Math.abs(s) <= 1));
    assert.ok(peak(loud) > 0.01, "the clamped patch still plays");

    synth.panic();
    synth.loadPreset(1);
    synth.noteOn(69);
    const fresh = new Synth(SAMPLE_RATE);
    fresh.loadPreset(1);
    fresh.noteOn(69);
    assertSameSamples(assert, render(synth), render(fresh), "after panic and a sane preset");
  });
});
