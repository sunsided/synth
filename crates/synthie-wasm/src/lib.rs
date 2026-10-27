//! WebAssembly bindings for the synthie engine, published to npm as
//! `@sunsided/synthie`.
//!
//! The package does no audio I/O of its own: the host pulls samples with
//! [`Synth::render`], from an `AudioWorkletProcessor` in the browser or in a
//! loop that writes a WAV file in Node.  Events queued with [`Synth::note_on`]
//! and friends apply at the start of the next `render` call, so render in
//! small blocks (a worklet's 128 frames) for tight timing.

use synthie::audio::processor::{NUM_CHANNELS, SynthProcessor};
use synthie::params::{AudioEvent, ChannelNo, DrumHit, MidiNote, Patch, SynthParams};
use synthie::presets::sid::default_patches;
use wasm_bindgen::prelude::*;

/// Capacity of the event queue; it is drained on every `render`.
const EVENT_CAPACITY: usize = 256;

/// A synthesized drum hit.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drum {
    Kick = "kick",
    HiHatClosed = "hihat-closed",
    HiHatOpen = "hihat-open",
}

/// A four-channel synthesizer with the built-in SID-style preset bank and a
/// drum voice.
#[wasm_bindgen]
pub struct Synth {
    processor: SynthProcessor<NUM_CHANNELS>,
    events: Vec<AudioEvent>,
    presets: Vec<Patch>,
    sample_rate: f32,
}

#[wasm_bindgen]
impl Synth {
    /// Create a synth rendering at `sample_rate` Hz (e.g. the
    /// `AudioContext`'s `sampleRate`).
    ///
    /// # Errors
    ///
    /// Fails if `sample_rate` is not a positive, finite number.
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: f32) -> Result<Synth, JsError> {
        if !(sample_rate.is_finite() && sample_rate > 0.0) {
            return Err(JsError::new("sampleRate must be a positive number"));
        }
        Ok(Self {
            processor: SynthProcessor::new(sample_rate),
            events: Vec::with_capacity(EVENT_CAPACITY),
            presets: default_patches(),
            sample_rate,
        })
    }

    /// Sample rate the synth renders at, in Hz.
    #[wasm_bindgen(getter, js_name = sampleRate)]
    #[must_use]
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Number of independent synth channels (`0..channels`).
    #[wasm_bindgen(getter)]
    #[must_use]
    pub fn channels(&self) -> u8 {
        #[allow(clippy::cast_possible_truncation)] // NUM_CHANNELS is a small constant
        {
            NUM_CHANNELS as u8
        }
    }

    /// Start a note (MIDI number `0..=127`) on `channel` (default 0).
    ///
    /// # Errors
    ///
    /// Fails for an out-of-range note or channel.
    #[wasm_bindgen(js_name = noteOn)]
    pub fn note_on(&mut self, note: u32, channel: Option<u32>) -> Result<(), JsError> {
        let event = AudioEvent::NoteOnChannel(channel_no(channel)?, midi_note(note)?);
        self.events.push(event);
        Ok(())
    }

    /// Release a note on `channel` (default 0).
    ///
    /// # Errors
    ///
    /// Fails for an out-of-range note or channel.
    #[wasm_bindgen(js_name = noteOff)]
    pub fn note_off(&mut self, note: u32, channel: Option<u32>) -> Result<(), JsError> {
        let event = AudioEvent::NoteOffChannel(channel_no(channel)?, midi_note(note)?);
        self.events.push(event);
        Ok(())
    }

    /// Trigger a one-shot drum hit.
    pub fn drum(&mut self, hit: Drum) {
        let hit = match hit {
            Drum::Kick => DrumHit::Kick,
            Drum::HiHatClosed => DrumHit::HiHatClosed,
            Drum::HiHatOpen => DrumHit::HiHatOpen,
            Drum::__Invalid => return,
        };
        self.events.push(AudioEvent::Drum(hit));
    }

    /// Silence every voice on every channel immediately.
    pub fn panic(&mut self) {
        self.events.push(AudioEvent::Panic);
    }

    /// Names of the built-in presets, in index order.
    #[wasm_bindgen(js_name = presetNames)]
    #[must_use]
    pub fn preset_names(&self) -> Vec<String> {
        self.presets.iter().map(|p| p.name.clone()).collect()
    }

    /// Load built-in preset `index` onto `channel` (default 0).
    ///
    /// # Errors
    ///
    /// Fails for an unknown preset or an out-of-range channel.
    #[wasm_bindgen(js_name = loadPreset)]
    pub fn load_preset(&mut self, index: usize, channel: Option<u32>) -> Result<(), JsError> {
        let params = self
            .presets
            .get(index)
            .ok_or_else(|| JsError::new(&format!("no preset {index}")))?
            .params
            .clone();
        self.load(params, channel)
    }

    /// Parameters of built-in preset `index` as JSON, as a starting point
    /// for [`Synth::load_params`].
    ///
    /// # Errors
    ///
    /// Fails for an unknown preset.
    #[wasm_bindgen(js_name = presetParams)]
    pub fn preset_params(&self, index: usize) -> Result<String, JsError> {
        let preset = self
            .presets
            .get(index)
            .ok_or_else(|| JsError::new(&format!("no preset {index}")))?;
        Ok(serde_json::to_string(&preset.params)?)
    }

    /// Load a full parameter set (JSON, see [`Synth::preset_params`]) onto
    /// `channel` (default 0).
    ///
    /// # Errors
    ///
    /// Fails for invalid JSON or an out-of-range channel.
    #[wasm_bindgen(js_name = loadParams)]
    pub fn load_params(&mut self, json: &str, channel: Option<u32>) -> Result<(), JsError> {
        let params: SynthParams = serde_json::from_str(json)?;
        self.load(params, channel)
    }

    /// Render `output.length / outputChannels` frames into `output`, applying
    /// all events queued since the last call first.  Frames are interleaved;
    /// the mono mix is written to every output channel.
    ///
    /// # Errors
    ///
    /// Fails if `outputChannels` is 0 or does not divide `output.length`.
    pub fn render(&mut self, output: &mut [f32], output_channels: usize) -> Result<(), JsError> {
        if output_channels == 0 || !output.len().is_multiple_of(output_channels) {
            return Err(JsError::new(
                "output.length must be a multiple of outputChannels (> 0)",
            ));
        }
        self.processor
            .process_block(&self.events, output, output_channels);
        self.events.clear();
        Ok(())
    }
}

impl Synth {
    fn load(&mut self, params: SynthParams, channel: Option<u32>) -> Result<(), JsError> {
        let event = AudioEvent::LoadPatchChannel(channel_no(channel)?, Box::new(params));
        self.events.push(event);
        Ok(())
    }
}

// Numbers arrive as u32: wasm-bindgen would silently wrap a JS number into a
// u8 (300 → 44), hiding out-of-range input.
fn midi_note(note: u32) -> Result<MidiNote, JsError> {
    u8::try_from(note)
        .ok()
        .filter(|&n| n <= 127)
        .map(MidiNote)
        .ok_or_else(|| JsError::new("note must be a MIDI note number 0..=127"))
}

fn channel_no(channel: Option<u32>) -> Result<ChannelNo, JsError> {
    u8::try_from(channel.unwrap_or(0))
        .ok()
        .filter(|&c| usize::from(c) < NUM_CHANNELS)
        .map(ChannelNo)
        .ok_or_else(|| JsError::new(&format!("channel must be 0..{NUM_CHANNELS}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn renders_silence_then_a_note_into_every_output_channel() {
        let mut synth = Synth::new(48_000.0).unwrap();
        let mut buf = vec![0.0_f32; 2 * 4_800]; // 100 ms, stereo
        synth.render(&mut buf, 2).unwrap();
        assert_eq!(peak(&buf), 0.0);

        synth.load_preset(1, None).unwrap();
        synth.note_on(69, None).unwrap();
        synth.render(&mut buf, 2).unwrap();
        assert!(peak(&buf) > 0.01);
        assert!(buf.chunks(2).all(|f| f[0].to_bits() == f[1].to_bits())); // copied verbatim
    }

    /// Dominant pitch of `window` as the nearest MIDI note, by autocorrelation.
    fn pitch(window: &[f32], sample_rate: f32) -> u8 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let lags = (sample_rate / 1000.0) as usize..(sample_rate / 100.0) as usize;
        let lag = lags
            .max_by(|&a, &b| {
                let corr = |lag: usize| -> f32 {
                    window.iter().zip(&window[lag..]).map(|(x, y)| x * y).sum()
                };
                corr(a).total_cmp(&corr(b))
            })
            .unwrap();
        #[allow(clippy::cast_precision_loss)]
        let hz = sample_rate / lag as f32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            (69.0 + 12.0 * (hz / 440.0).log2()).round() as u8
        }
    }

    #[test]
    fn arpeggiator_steps_through_a_held_chord() {
        const SR: f32 = 48_000.0;
        let mut synth = Synth::new(SR).unwrap();
        let mut params: serde_json::Value =
            serde_json::from_str(&synth.preset_params(1).unwrap()).unwrap();
        params["arp"]["enabled"] = true.into();
        params["arp"]["rate"] = 8.0.into();
        params["arp"]["mode"] = "Up".into();
        params["fx"]["reverb_mix"] = 0.0.into();
        synth.load_params(&params.to_string(), None).unwrap();
        for note in [60, 64, 67] {
            synth.note_on(note, None).unwrap(); // C4 E4 G4, held
        }

        let mut out = vec![0.0_f32; 48_000]; // one second
        for block in out.chunks_mut(128) {
            synth.render(block, 1).unwrap();
        }

        // One pitch per 30 ms, collapsed to the sequence of distinct notes.
        let mut sequence: Vec<u8> = out
            .as_chunks::<1_440>()
            .0
            .iter()
            .map(|w| pitch(w, SR))
            .collect();
        sequence.dedup();
        assert_eq!(sequence[..6], [60, 64, 67, 60, 64, 67]);
    }

    #[test]
    fn preset_params_round_trip_through_json() {
        let mut synth = Synth::new(44_100.0).unwrap();
        assert_eq!(synth.preset_names().len(), default_patches().len());
        let json = synth.preset_params(0).unwrap();
        synth.load_params(&json, Some(3)).unwrap();
    }
}
