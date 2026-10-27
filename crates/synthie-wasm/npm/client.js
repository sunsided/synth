// Main-thread client for the "synthie" AudioWorklet: typed methods instead
// of hand-built messages, and promises for anything the processor answers.
//
//   import { SynthNode } from "@sunsided/synthie/client";
//   const module = await WebAssembly.compileStreaming(fetch(wasmUrl));
//   const synth = await SynthNode.create(ctx, { module, workletUrl });
//   synth.connect(ctx.destination);
//   synth.noteOn(69, { time: ctx.currentTime + 0.1 });
//   const names = await synth.presetNames();
//
// Errors from queries reject their promise; errors from commands are
// dispatched as "error" events (CustomEvent, detail: { message }). With
// `idleAfter`, "idle" events report that the synth has gone quiet.

export class SynthNode extends EventTarget {
  #port;
  #pending = new Map();
  #nextId = 1;
  #resolveReady;

  /**
   * Load the worklet module (if `workletUrl` is given), create the node and
   * wait until the processor is ready.
   */
  static async create(context, { module, workletUrl, idleAfter, outputChannelCount = [2], ...options } = {}) {
    if (workletUrl !== undefined) await context.audioWorklet.addModule(workletUrl);
    const node = new AudioWorkletNode(context, "synthie", {
      ...options,
      outputChannelCount,
      processorOptions: { module, idleAfter },
    });
    const synth = new SynthNode(node);
    await synth.ready;
    return synth;
  }

  /** Wrap an existing "synthie" AudioWorkletNode. */
  constructor(node) {
    super();
    this.node = node;
    this.info = undefined;
    this.ready = new Promise((resolve) => {
      this.#resolveReady = resolve;
    });
    this.#port = node.port;
    this.#port.onmessage = ({ data }) => this.#receive(data);
  }

  #receive(msg) {
    switch (msg?.type) {
      case "ready":
        this.info = msg;
        this.#resolveReady(msg);
        break;
      case "result":
        this.#settle(msg.id)?.resolve(msg.value);
        break;
      case "error": {
        const pending = this.#settle(msg.id);
        if (pending) pending.reject(new Error(msg.message));
        else this.dispatchEvent(new CustomEvent("error", { detail: { message: msg.message } }));
        break;
      }
      case "idle":
        this.dispatchEvent(new Event("idle"));
        break;
    }
  }

  #settle(id) {
    const pending = this.#pending.get(id);
    this.#pending.delete(id);
    return pending;
  }

  #send(msg) {
    this.#port.postMessage(msg);
  }

  connect(...args) {
    return this.node.connect(...args);
  }

  disconnect(...args) {
    return this.node.disconnect(...args);
  }

  noteOn(note, { channel, time } = {}) {
    this.#send({ type: "noteOn", note, channel, time });
  }

  noteOff(note, { channel, time } = {}) {
    this.#send({ type: "noteOff", note, channel, time });
  }

  drum(hit, { time } = {}) {
    this.#send({ type: "drum", hit, time });
  }

  panic() {
    this.#send({ type: "panic" });
  }

  cancelScheduled() {
    this.#send({ type: "cancelScheduled" });
  }

  loadPreset(index, { channel, time } = {}) {
    this.#send({ type: "loadPreset", index, channel, time });
  }

  loadParams(params, { channel, time } = {}) {
    this.#send({ type: "loadParams", params, channel, time });
  }

  setParam(path, value, { channel, time } = {}) {
    this.#send({ type: "setParam", path, value, channel, time });
  }

  setMasterGain(gain) {
    this.#send({ type: "masterGain", gain });
  }

  setNormalizeVoices(enabled) {
    this.#send({ type: "normalizeVoices", enabled });
  }

  setSilenceThreshold(value) {
    this.#send({ type: "silenceThreshold", value });
  }

  /** Send many messages at once, e.g. a whole bar with `time`s. */
  schedule(events) {
    this.#send({ type: "schedule", events });
  }

  /** Ask the processor for a value; see the worklet's query list. */
  get(what, ...args) {
    const id = this.#nextId++;
    return new Promise((resolve, reject) => {
      this.#pending.set(id, { resolve, reject });
      this.#send({ type: "get", what, args, id });
    });
  }

  presetNames() {
    return this.get("presetNames");
  }

  presetPatch(index) {
    return this.get("presetPatch", index);
  }

  params(channel = 0) {
    return this.get("params", channel);
  }

  paramInfo() {
    return this.get("paramInfo");
  }
}
