// Type-checked against the built package by `task test:npm`: the Synth class
// as Node.js sees it.

import { Synth } from "@sunsided/synthie";
const synth = new Synth(48000);
synth.setParam("env.release", 0.5, 0, 1000);
synth.loadParams(synth.presetPatch(1));
const p = synth.params(0);
const n: number = p.filter.cutoff + synth.paramInfo().length;
// @ts-expect-error unknown path
synth.setParam("nope", 1);
export default n;
