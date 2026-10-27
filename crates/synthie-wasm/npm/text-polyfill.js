// Minimal UTF-8 TextDecoder / TextEncoder for AudioWorkletGlobalScope, which
// lacks both in some browsers; the wasm-bindgen glue needs them at load time.
// Installed only where missing.

if (typeof globalThis.TextDecoder === "undefined") {
  globalThis.TextDecoder = class TextDecoder {
    decode(bytes) {
      if (!bytes) return "";
      const u8 = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes.buffer ?? bytes);
      let out = "";
      for (let i = 0; i < u8.length; ) {
        const b = u8[i++];
        let cp;
        if (b < 0x80) cp = b;
        else if (b < 0xe0) cp = ((b & 0x1f) << 6) | (u8[i++] & 0x3f);
        else if (b < 0xf0) cp = ((b & 0x0f) << 12) | ((u8[i++] & 0x3f) << 6) | (u8[i++] & 0x3f);
        else cp = ((b & 0x07) << 18) | ((u8[i++] & 0x3f) << 12) | ((u8[i++] & 0x3f) << 6) | (u8[i++] & 0x3f);
        out += String.fromCodePoint(cp);
      }
      return out;
    }
  };
}

if (typeof globalThis.TextEncoder === "undefined") {
  globalThis.TextEncoder = class TextEncoder {
    encode(text = "") {
      const bytes = [];
      for (const ch of text) {
        const cp = ch.codePointAt(0);
        if (cp < 0x80) bytes.push(cp);
        else if (cp < 0x800) bytes.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f));
        else if (cp < 0x10000) bytes.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
        else bytes.push(0xf0 | (cp >> 18), 0x80 | ((cp >> 12) & 0x3f), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
      }
      return new Uint8Array(bytes);
    }
    encodeInto(text, view) {
      const bytes = this.encode(text);
      view.set(bytes);
      return { read: text.length, written: bytes.length };
    }
  };
}
