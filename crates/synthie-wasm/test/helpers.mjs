// Shared helpers for the npm package tests. They run against the built
// package (`task build:npm`), not the sources, so they cover exactly what
// ships: the wasm binary, the wasm-bindgen glue and the worklet.

import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

/** Root of the built package; override with SYNTHIE_NPM_DIR. */
export const PKG = resolve(process.env.SYNTHIE_NPM_DIR ?? resolve(here, "../../../target/npm"));

if (!existsSync(resolve(PKG, "package.json"))) {
  throw new Error(`no built package at ${PKG}; run \`task build:npm\` first`);
}

/** The Node.js (CommonJS) build, as `require("@sunsided/synthie")` resolves it. */
export const { Synth, Drum } = createRequire(import.meta.url)(resolve(PKG, "node/synthie.js"));

export const SAMPLE_RATE = 48_000;

/** Gain whose output, scaled back up by 1024, is the exact pre-gain signal. */
export const PROBE_GAIN = 2 ** -10;

/** The engine's f32 multiply-then-clamp, reproduced exactly in JS. */
export function amplify(preGain, gain) {
  return Math.min(1, Math.max(-1, Math.fround(preGain * Math.fround(gain))));
}

/** Compare two sample buffers bit for bit; on mismatch, name the first sample. */
export function assertSameSamples(assert, actual, expected, what) {
  assert.equal(actual.length, expected.length, `${what}: length`);
  const a = new Uint32Array(Float32Array.from(actual).buffer);
  const e = new Uint32Array(Float32Array.from(expected).buffer);
  const i = a.findIndex((bits, idx) => bits !== e[idx]);
  assert.ok(i < 0, `${what}: sample ${i} is ${actual[i]}, expected ${expected[i]}`);
}

export function peak(buf) {
  return buf.reduce((m, s) => Math.max(m, Math.abs(s)), 0);
}
