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

Messages: `noteOn` / `noteOff` `{ note, channel?, time? }`, `drum` `{ hit, time? }`, `panic`, `cancelScheduled`, `loadPreset` `{ index, channel?, time? }`, `loadParams` `{ json, channel?, time? }`, `masterGain` `{ gain }`, `normalizeVoices` `{ enabled }`, and `schedule` `{ events }` to send many at once.

### Timing

A message without `time` takes effect at the next render quantum, so its timing depends on when the main thread gets to send it: fine for playing live, too loose for a sequence. For sequences, schedule a little ahead with `time` in `AudioContext` seconds (`ctx.currentTime`'s clock); the processor starts each event on exactly that frame, even in the middle of a render quantum:

```js
const t0 = ctx.currentTime + 0.1;
const step = 0.125; // seconds per sixteenth at 120 BPM
synth.port.postMessage({
  type: "schedule",
  events: [0, 4, 8, 12].map((i) => ({ type: "drum", hit: "kick", time: t0 + i * step })),
});
```

For an endless pattern, use the usual lookahead loop: every ~25 ms, send whatever falls into the next ~100 ms. `cancelScheduled` drops everything not yet played (sounding notes ring on); `panic` also silences what is playing. Times in the past play at once.

Pass `idleAfter` (seconds) in `processorOptions` to receive a `{ type: "idle" }` message once the synth has made sound and then been idle and silent that long, e.g. to suspend the `AudioContext`.

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

For a one-shot sound effect you usually don't know the length up front. Schedule the whole thing by frame position, then let `renderUntilSilent` render until the notes, drums and FX tails have died away; the result is trimmed of trailing silence:

```js
const sfx = new Synth(44100);
sfx.normalizeVoices = false;       // full level for a single voice
sfx.loadPreset(1);
sfx.noteOn(76);
sfx.noteOn(81, 0, 0.08 * 44100);   // at 80 ms
sfx.noteOff(76, 0, 0.08 * 44100);
sfx.noteOff(81, 0, 0.25 * 44100);  // at 250 ms
const samples = sfx.renderUntilSilent(5); // Float32Array, at most 5 s
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
| `noteOn(note, channel = 0, at?)` / `noteOff(note, channel = 0, at?)` | MIDI note `0..=127` on channel `0..channels`. |
| `drum(hit, at?)` | `"kick"`, `"hihat-closed"` or `"hihat-open"`. |
| `panic()` | Silence every voice and drop all pending events. |
| `cancelScheduled()` | Drop all pending events; sounding notes ring on. |
| `presetNames()` | Names of the built-in presets, by index. |
| `loadPreset(index, channel = 0, at?)` | Load a built-in preset onto a channel. |
| `presetParams(index)` / `loadParams(json, channel = 0, at?)` | A preset's parameters as JSON; load edited parameters. |
| `render(output, outputChannels)` | Fill an interleaved `Float32Array`; the mono mix goes to every channel. |
| `renderUntilSilent(maxSeconds, holdSeconds = 2.5)` | Render mono until idle and silent for `holdSeconds` (or `maxSeconds`); returns a `Float32Array` without trailing silence. |
| `currentFrame` | Frames rendered so far: the timeline `at` refers to. |
| `pendingEvents` | Events that have not taken effect yet (at most 4096). |
| `activeVoices` | Synth voices still held or releasing. |
| `isIdle` | No voice, drum, arpeggiator or pending event left to make sound (FX tails may still ring). |
| `silenceThreshold`, `silentFrames` | Level counted as silence (default `1e-4`, -80 dBFS); frames the synth has been idle and silent in a row. |
| `masterGain` | Output gain after the FX chain (default `0.5`); the mix is clamped to `-1..=1`. |
| `normalizeVoices` | Divide each channel's voice sum by its polyphony (default `true`); turn off for louder single notes. |
| `sampleRate`, `channels` | Read-only properties. |

`at` is a frame position on the synth's own timeline (`currentFrame`): the event takes effect exactly there, wherever it falls within a `render()` call. Without `at`, or once it has passed, an event takes effect at the start of the next `render()`. Events for the same frame apply in the order they were added. Invalid arguments throw an `Error`.

## License

EUPL-1.2
