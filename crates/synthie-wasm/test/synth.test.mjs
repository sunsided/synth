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
