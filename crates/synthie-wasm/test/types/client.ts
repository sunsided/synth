// Type-checked against the built package by `task test:npm`: the browser
// client API. Lines marked @ts-expect-error must stay type errors.

import { SynthNode, type ReadyInfo } from "@sunsided/synthie/client";
import type { ParamInfo, ParamPath, SynthParams } from "@sunsided/synthie/client";

export async function demo(ctx: AudioContext, module: WebAssembly.Module): Promise<number> {
  const synth = await SynthNode.create(ctx, { module, workletUrl: "/worklet.js", idleAfter: 1 });
  synth.connect(ctx.destination);
  const ready: ReadyInfo = await synth.ready;
  synth.noteOn(60, { channel: 1, time: ctx.currentTime + 0.1 });
  synth.drum("kick", { time: 1 });
  synth.setParam("filter.cutoff", 1200);
  synth.setParam("osc.waveform", "Sine", { channel: 2 });
  synth.schedule([{ type: "noteOn", note: 64, time: 2 }, { type: "drum", hit: "hihat-open" }]);
  const patch: SynthParams = await synth.presetPatch(1);
  patch.osc.waveform = "Triangle";
  patch.filter.cutoff = 900;
  synth.loadParams(patch, { channel: 3 });
  const info: ParamInfo[] = await synth.paramInfo();
  const paths: ParamPath[] = info.map((p) => p.path);
  const voices: number = await synth.get("activeVoices");
  const idle: boolean = await synth.get("isIdle");
  const p2: SynthParams = await synth.get("params", 2);
  synth.addEventListener("error", (e) => console.log(e.detail.message));
  synth.addEventListener("idle", () => ctx.suspend());
  return voices + paths.length + ready.channels + (idle ? 1 : 0) + p2.global.volume;
}

// These must NOT type-check:
export function mistakes(synth: SynthNode, patch: SynthParams) {
  // @ts-expect-error unknown path
  synth.setParam("filter.cutof", 1);
  // @ts-expect-error not a waveform
  patch.osc.waveform = "Kazoo";
  // @ts-expect-error unknown query
  void synth.get("secrets");
  // @ts-expect-error params takes a channel number
  void synth.get("params", "two");
  // @ts-expect-error not a drum
  synth.drum("cowbell");
}
