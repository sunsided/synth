# @sunsided/synthie

[![npm](https://img.shields.io/npm/v/@sunsided/synthie.svg)](https://www.npmjs.com/package/@sunsided/synthie)
[![crates.io](https://img.shields.io/crates/v/synthie.svg)](https://crates.io/crates/synthie)
[![Live demo](https://img.shields.io/badge/demo-sunsided.github.io%2Fsynth-ff2eaa.svg)](https://sunsided.github.io/synth/)

A chiptune synthesizer engine compiled to WebAssembly: SID-style oscillators, a resonant filter, envelopes, LFOs, an arpeggiator, reverb/delay/chorus/bitcrusher FX and a drum voice, with four independent channels and the built-in preset bank. It is the engine behind the [browser demos](https://sunsided.github.io/synth/), written in Rust ([`synthie`](https://crates.io/crates/synthie)).

The package does no audio I/O itself. You pull samples with `render()`: from an AudioWorklet in the browser (a ready-made one is included), or in a loop in Node.js, e.g. to write a WAV file.

## Install

```sh
npm install @sunsided/synthie
```

Tagged releases are also published to GitHub Packages, which needs a token even for public packages: a personal access token (classic) with `read:packages`, and in `.npmrc`:

```ini
@sunsided:registry=https://npm.pkg.github.com
//npm.pkg.github.com/:_authToken=${GITHUB_TOKEN}
```

## Browser: AudioWorklet

`web/worklet.js` registers an `AudioWorkletProcessor` named `"synthie"`. Worklet modules and the wasm binary are loaded by URL, so serve the package's `web/` directory (`worklet.js`, `synthie.js`, `text-polyfill.js`, `synthie_bg.wasm`) as static assets: copy it into your public folder, or have your bundler emit it. Then hand the processor the compiled WebAssembly module and drive it through its port:

```js
const ctx = new AudioContext(); // create or resume it from a user gesture
await ctx.audioWorklet.addModule("/synthie/worklet.js");
const module = await WebAssembly.compileStreaming(fetch("/synthie/synthie_bg.wasm"));

const synth = new AudioWorkletNode(ctx, "synthie", {
  outputChannelCount: [2],
  processorOptions: { module },
});
synth.connect(ctx.destination);
synth.port.onmessage = ({ data }) => console.error(data.message); // { type: "error" }

synth.port.postMessage({ type: "loadPreset", index: 1 }); // "Arp Lead"
synth.port.postMessage({ type: "noteOn", note: 69 });     // A4
synth.port.postMessage({ type: "drum", hit: "kick" });
```

Messages: `noteOn` / `noteOff` `{ note, channel? }`, `drum` `{ hit }`, `panic`, `loadPreset` `{ index, channel? }`, `loadParams` `{ json, channel? }`.

To use the `Synth` class directly on the main thread instead, initialise the module first:

```js
import init, { Synth } from "@sunsided/synthie";
await init();
const synth = new Synth(48000);
```

## Node.js: render offline

In Node the module loads synchronously:

```js
import { Synth } from "@sunsided/synthie";

const synth = new Synth(44100);
synth.loadPreset(1);
synth.noteOn(69);

const samples = new Float32Array(44100); // one second, mono
for (let i = 0; i < samples.length; i += 128) {
  synth.render(samples.subarray(i, i + 128), 1);
}
```

## Arpeggiator

Each channel has an arpeggiator, configured through the parameter JSON. Enable it, then hold a chord: every held note joins the pattern.

```js
const params = JSON.parse(synth.presetParams(1)); // start from "Arp Lead"
params.arp = { ...params.arp, enabled: true, rate: 8, gate: 0.8, mode: "UpDown" };
synth.loadParams(JSON.stringify(params));

for (const note of [57, 60, 64]) synth.noteOn(note); // A minor, held
```

`rate` is in steps per second, `gate` the fraction of a step a note sounds (`0..=1`), and `mode` one of `"Up"`, `"Down"`, `"UpDown"`, `"Random"`. Up to four held notes take part.

## API

| Member | |
|---|---|
| `new Synth(sampleRate)` | Four-channel synth at `sampleRate` Hz. |
| `noteOn(note, channel = 0)` / `noteOff(note, channel = 0)` | MIDI note `0..=127` on channel `0..channels`. |
| `drum(hit)` | `"kick"`, `"hihat-closed"` or `"hihat-open"`. |
| `panic()` | Silence every voice. |
| `presetNames()` | Names of the built-in presets, by index. |
| `loadPreset(index, channel = 0)` | Load a built-in preset onto a channel. |
| `presetParams(index)` / `loadParams(json, channel = 0)` | A preset's parameters as JSON; load edited parameters. |
| `render(output, outputChannels)` | Fill an interleaved `Float32Array`; the mono mix goes to every channel. |
| `sampleRate`, `channels` | Read-only properties. |

Events apply at the start of the next `render()` call, so render in small blocks (an AudioWorklet's 128 frames) for tight timing. Invalid arguments throw an `Error`.

## License

EUPL-1.2
