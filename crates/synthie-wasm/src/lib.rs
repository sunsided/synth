//! WebAssembly bindings for the synthie engine, published to npm as
//! `@sunsided/synthie`.
//!
//! The package does no audio I/O of its own: the host pulls samples with
//! [`Synth::render`], from an `AudioWorkletProcessor` in the browser or in a
//! loop that writes a WAV file in Node.  Events from [`Synth::note_on`] and
//! friends apply at the start of the next `render` call, or, given a frame
//! position, at exactly that frame of the synth's render timeline.

use std::collections::VecDeque;

use synthie::audio::processor::{NUM_CHANNELS, SynthProcessor, VoiceNormalization};
use synthie::params::{AudioEvent, ChannelNo, DrumHit, MidiNote, Patch, SynthParams};
use synthie::presets::sid::default_patches;
use wasm_bindgen::prelude::*;

/// Most events that can be pending at once. The queue never grows past this,
/// so scheduling never allocates on the audio thread.
const SCHEDULE_CAPACITY: usize = 4096;

/// Default level at or below which the output counts as silent (-80 dBFS).
const DEFAULT_SILENCE_THRESHOLD: f32 = 1e-4;

/// Default time the output must stay silent before `renderUntilSilent`
/// stops. Longer than the longest delay time (2 s), so a late echo is not
/// mistaken for the end of the sound.
const DEFAULT_SILENCE_HOLD_SECONDS: f64 = 2.5;

/// Block size of `renderUntilSilent`, matching a worklet's render quantum.
const SILENCE_BLOCK: usize = 128;

/// An event waiting for its frame.
struct Scheduled {
    frame: u64,
    event: AudioEvent,
}

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
    /// Pending events, ordered by frame, then by the order they were added.
    queue: VecDeque<Scheduled>,
    /// Scratch buffer for the events due in one render segment.
    due: Vec<AudioEvent>,
    presets: Vec<Patch>,
    sample_rate: f32,
    /// Frames rendered so far: the synth's own timeline.
    frame: u64,
    silence_threshold: f32,
    /// Frames the output has been silent and the synth idle, in a row.
    silent_frames: u64,
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
            queue: VecDeque::with_capacity(SCHEDULE_CAPACITY),
            due: Vec::with_capacity(SCHEDULE_CAPACITY),
            presets: default_patches(),
            sample_rate,
            frame: 0,
            silence_threshold: DEFAULT_SILENCE_THRESHOLD,
            silent_frames: 0,
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

    /// Output gain applied after the FX chain (default 0.5).  The mix is
    /// still clamped to `-1..=1`, so higher values trade headroom for level.
    #[wasm_bindgen(getter, js_name = masterGain)]
    #[must_use]
    pub fn master_gain(&self) -> f32 {
        self.processor.master_gain()
    }

    /// Set the output gain applied after the FX chain.
    ///
    /// # Errors
    ///
    /// Fails if `gain` is not a finite number.
    #[wasm_bindgen(setter, js_name = masterGain)]
    pub fn set_master_gain(&mut self, gain: f32) -> Result<(), JsError> {
        if !gain.is_finite() {
            return Err(JsError::new("masterGain must be a finite number"));
        }
        self.processor.set_master_gain(gain);
        Ok(())
    }

    /// Whether each channel divides its voice sum by its polyphony (default
    /// `true`).  Turn off for louder single notes, e.g. sound effects.
    #[wasm_bindgen(getter, js_name = normalizeVoices)]
    #[must_use]
    pub fn normalize_voices(&self) -> bool {
        self.processor.voice_normalization() == VoiceNormalization::Polyphony
    }

    /// Enable or disable per-channel voice normalisation.
    #[wasm_bindgen(setter, js_name = normalizeVoices)]
    pub fn set_normalize_voices(&mut self, enabled: bool) {
        self.processor.set_voice_normalization(if enabled {
            VoiceNormalization::Polyphony
        } else {
            VoiceNormalization::Off
        });
    }

    /// Frames rendered so far. Event methods take an optional `at` on this
    /// timeline: the frame at which the event takes effect.
    #[wasm_bindgen(getter, js_name = currentFrame)]
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // exact below 2^53 frames (6000 years at 48 kHz)
    pub fn current_frame(&self) -> f64 {
        self.frame as f64
    }

    /// Number of events that have not taken effect yet.
    #[wasm_bindgen(getter, js_name = pendingEvents)]
    #[must_use]
    pub fn pending_events(&self) -> usize {
        self.queue.len()
    }

    /// Start a note (MIDI number `0..=127`) on `channel` (default 0), at frame
    /// `at` (default: the start of the next `render`).
    ///
    /// Events without `at`, or with an `at` that has already passed, take
    /// effect at the start of the next `render`; events for the same frame
    /// take effect in the order they were added.
    ///
    /// # Errors
    ///
    /// Fails for an out-of-range note, channel or frame, or when
    /// 4096 events are already pending.
    #[wasm_bindgen(js_name = noteOn)]
    pub fn note_on(
        &mut self,
        note: u32,
        channel: Option<u32>,
        at: Option<f64>,
    ) -> Result<(), JsError> {
        let event = AudioEvent::NoteOnChannel(channel_no(channel)?, midi_note(note)?);
        self.schedule(event, at)
    }

    /// Release a note on `channel` (default 0), at frame `at` (default: the
    /// start of the next `render`).
    ///
    /// # Errors
    ///
    /// Fails for an out-of-range note, channel or frame, or when
    /// 4096 events are already pending.
    #[wasm_bindgen(js_name = noteOff)]
    pub fn note_off(
        &mut self,
        note: u32,
        channel: Option<u32>,
        at: Option<f64>,
    ) -> Result<(), JsError> {
        let event = AudioEvent::NoteOffChannel(channel_no(channel)?, midi_note(note)?);
        self.schedule(event, at)
    }

    /// Trigger a one-shot drum hit at frame `at` (default: the start of the
    /// next `render`).
    ///
    /// # Errors
    ///
    /// Fails for an out-of-range frame, or when 4096 events are already
    /// pending.
    pub fn drum(&mut self, hit: Drum, at: Option<f64>) -> Result<(), JsError> {
        let hit = match hit {
            Drum::Kick => DrumHit::Kick,
            Drum::HiHatClosed => DrumHit::HiHatClosed,
            Drum::HiHatOpen => DrumHit::HiHatOpen,
            Drum::__Invalid => return Ok(()),
        };
        self.schedule(AudioEvent::Drum(hit), at)
    }

    /// Silence every voice on every channel at the start of the next
    /// `render`, and drop every pending event so nothing scheduled plays on.
    pub fn panic(&mut self) {
        self.queue.clear();
        self.queue.push_back(Scheduled {
            frame: self.frame,
            event: AudioEvent::Panic,
        });
    }

    /// Drop every event that has not taken effect yet, e.g. to stop a
    /// sequence. Sounding notes keep playing; follow with `noteOff` or
    /// `panic` to end them.
    #[wasm_bindgen(js_name = cancelScheduled)]
    pub fn cancel_scheduled(&mut self) {
        self.queue.clear();
    }

    /// Number of synth voices still sounding, held or releasing, across all
    /// channels. Drum hits are not counted.
    #[wasm_bindgen(getter, js_name = activeVoices)]
    #[must_use]
    pub fn active_voices(&self) -> usize {
        self.processor.active_voices()
    }

    /// Whether nothing will make new sound: no voice held or releasing, no
    /// drum ringing, no arpeggiator running and no event pending. Reverb
    /// and delay tails may still be audible; see `silentFrames`.
    #[wasm_bindgen(getter, js_name = isIdle)]
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.processor.is_idle() && self.queue.is_empty()
    }

    /// Level at or below which output counts as silent (default `1e-4`,
    /// -80 dBFS).
    #[wasm_bindgen(getter, js_name = silenceThreshold)]
    #[must_use]
    pub fn silence_threshold(&self) -> f32 {
        self.silence_threshold
    }

    /// Set the level at or below which output counts as silent.
    ///
    /// # Errors
    ///
    /// Fails unless `threshold` is a finite number `>= 0`.
    #[wasm_bindgen(setter, js_name = silenceThreshold)]
    pub fn set_silence_threshold(&mut self, threshold: f32) -> Result<(), JsError> {
        if !(threshold.is_finite() && threshold >= 0.0) {
            return Err(JsError::new(
                "silenceThreshold must be a finite number >= 0",
            ));
        }
        self.silence_threshold = threshold;
        Ok(())
    }

    /// Frames in a row, up to the last `render`, for which the synth was
    /// idle and its output at or below `silenceThreshold`. Counted per
    /// `render` call: a call that is not silent throughout resets it to 0.
    #[wasm_bindgen(getter, js_name = silentFrames)]
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // exact below 2^53 frames
    pub fn silent_frames(&self) -> f64 {
        self.silent_frames as f64
    }

    /// Names of the built-in presets, in index order.
    #[wasm_bindgen(js_name = presetNames)]
    #[must_use]
    pub fn preset_names(&self) -> Vec<String> {
        self.presets.iter().map(|p| p.name.clone()).collect()
    }

    /// Load built-in preset `index` onto `channel` (default 0), at frame `at`
    /// (default: the start of the next `render`).
    ///
    /// # Errors
    ///
    /// Fails for an unknown preset, an out-of-range channel or frame, or
    /// when 4096 events are already pending.
    #[wasm_bindgen(js_name = loadPreset)]
    pub fn load_preset(
        &mut self,
        index: usize,
        channel: Option<u32>,
        at: Option<f64>,
    ) -> Result<(), JsError> {
        let params = self
            .presets
            .get(index)
            .ok_or_else(|| JsError::new(&format!("no preset {index}")))?
            .params
            .clone();
        self.load(params, channel, at)
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
    /// `channel` (default 0), at frame `at` (default: the start of the next
    /// `render`).
    ///
    /// # Errors
    ///
    /// Fails for invalid JSON, an out-of-range channel or frame, or when
    /// 4096 events are already pending.
    #[wasm_bindgen(js_name = loadParams)]
    pub fn load_params(
        &mut self,
        json: &str,
        channel: Option<u32>,
        at: Option<f64>,
    ) -> Result<(), JsError> {
        let params: SynthParams = serde_json::from_str(json)?;
        self.load(params, channel, at)
    }

    /// Render `output.length / outputChannels` frames into `output`.  Each
    /// pending event takes effect at its frame (events that are already due
    /// at the start).  Frames are interleaved; the mono mix is written to
    /// every output channel.
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
        self.render_frames(output, output_channels);
        Ok(())
    }

    /// Render mono audio until the sound has died away, and return it with
    /// trailing silence trimmed.
    ///
    /// Rendering stops once the synth has been idle with its output at or
    /// below `silenceThreshold` for `holdSeconds` (default 2.5, longer than
    /// the longest delay echo), or after `maxSeconds`.  Pending events play
    /// as scheduled, so a whole sequence can be rendered in one call.
    ///
    /// # Errors
    ///
    /// Fails unless `maxSeconds` and `holdSeconds` are finite numbers `>= 0`.
    #[wasm_bindgen(js_name = renderUntilSilent)]
    pub fn render_until_silent(
        &mut self,
        max_seconds: f64,
        hold_seconds: Option<f64>,
    ) -> Result<Vec<f32>, JsError> {
        let max_frames = self.seconds_to_frames(max_seconds, "maxSeconds")?;
        let hold_frames = self.seconds_to_frames(
            hold_seconds.unwrap_or(DEFAULT_SILENCE_HOLD_SECONDS),
            "holdSeconds",
        )?;
        Ok(self.render_until_silent_frames(max_frames, hold_frames))
    }
}

impl Synth {
    fn load(
        &mut self,
        params: SynthParams,
        channel: Option<u32>,
        at: Option<f64>,
    ) -> Result<(), JsError> {
        let event = AudioEvent::LoadPatchChannel(channel_no(channel)?, Box::new(params));
        self.schedule(event, at)
    }

    fn schedule(&mut self, event: AudioEvent, at: Option<f64>) -> Result<(), JsError> {
        let frame = at.map(frame_position).transpose()?;
        self.enqueue(event, frame).map_err(JsError::new)
    }

    /// Queue `event` for `frame` (`None` or past: the next render's start),
    /// after any event already queued for the same frame.
    fn enqueue(&mut self, event: AudioEvent, frame: Option<u64>) -> Result<(), &'static str> {
        if self.queue.len() >= SCHEDULE_CAPACITY {
            return Err("too many pending events (at most 4096)");
        }
        let frame = frame.unwrap_or(self.frame).max(self.frame);
        let idx = self.queue.partition_point(|s| s.frame <= frame);
        self.queue.insert(idx, Scheduled { frame, event });
        Ok(())
    }

    /// Render `output`, splitting it at every pending event's frame.
    fn render_frames(&mut self, output: &mut [f32], output_channels: usize) {
        let frames = output.len() / output_channels;
        let mut pos = 0;
        while pos < frames {
            let now = self.frame + pos as u64;
            self.due.clear();
            while let Some(s) = self.queue.pop_front_if(|s| s.frame <= now) {
                self.due.push(s.event);
            }
            let end = self.queue.front().map_or(frames, |s| {
                usize::try_from(s.frame - self.frame).map_or(frames, |f| f.min(frames))
            });
            self.processor.process_block(
                &self.due,
                &mut output[pos * output_channels..end * output_channels],
                output_channels,
            );
            pos = end;
        }
        self.due.clear();
        self.frame += frames as u64;

        let peak = output.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        if self.is_idle() && peak <= self.silence_threshold {
            self.silent_frames += frames as u64;
        } else {
            self.silent_frames = 0;
        }
    }

    fn render_until_silent_frames(&mut self, max_frames: usize, hold_frames: usize) -> Vec<f32> {
        let mut out = Vec::new();
        let mut block = [0.0_f32; SILENCE_BLOCK];
        // Length of `out` up to its last sample above the threshold.
        let mut audible_len = 0;
        while out.len() < max_frames {
            let block = &mut block[..(max_frames - out.len()).min(SILENCE_BLOCK)];
            self.render_frames(block, 1);
            if let Some(i) = block.iter().rposition(|s| s.abs() > self.silence_threshold) {
                audible_len = out.len() + i + 1;
            }
            out.extend_from_slice(block);
            if self.silent_frames >= hold_frames as u64 {
                break;
            }
        }
        out.truncate(audible_len);
        out
    }

    fn seconds_to_frames(&self, seconds: f64, name: &str) -> Result<usize, JsError> {
        if !(seconds.is_finite() && seconds >= 0.0) {
            return Err(JsError::new(&format!(
                "{name} must be a finite number >= 0"
            )));
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // checked above
        Ok((seconds * f64::from(self.sample_rate)).round() as usize)
    }
}

/// A frame position on the synth's timeline, from a JS number.
fn frame_position(at: f64) -> Result<u64, JsError> {
    if !(at.is_finite() && (0.0..9_007_199_254_740_992.0).contains(&at)) {
        return Err(JsError::new("at must be a frame position >= 0"));
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // checked above
    Ok(at.round() as u64)
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

        synth.load_preset(1, None, None).unwrap();
        synth.note_on(69, None, None).unwrap();
        synth.render(&mut buf, 2).unwrap();
        assert!(peak(&buf) > 0.01);
        assert!(buf.chunks(2).all(|f| f[0].to_bits() == f[1].to_bits())); // copied verbatim
    }

    #[test]
    fn master_gain_and_voice_normalization_raise_a_lone_note() {
        let render = |configure: &dyn Fn(&mut Synth)| {
            let mut synth = Synth::new(48_000.0).unwrap();
            configure(&mut synth);
            synth.load_preset(1, None, None).unwrap();
            synth.note_on(69, None, None).unwrap();
            let mut buf = vec![0.0_f32; 4_800];
            synth.render(&mut buf, 1).unwrap();
            peak(&buf)
        };
        let base = render(&|_| {});
        let louder = render(&|s| {
            s.set_master_gain(1.0).unwrap();
            s.set_normalize_voices(false);
        });
        assert!(louder > base * 2.0, "{louder} vs {base}");

        let synth = Synth::new(48_000.0).unwrap();
        assert!((synth.master_gain() - 0.5).abs() < f32::EPSILON);
        assert!(synth.normalize_voices());
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
        synth.load_params(&params.to_string(), None, None).unwrap();
        for note in [60, 64, 67] {
            synth.note_on(note, None, None).unwrap(); // C4 E4 G4, held
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
        synth.load_params(&json, Some(3), None).unwrap();
    }

    // --- Scheduling ---------------------------------------------------------

    const SR: f32 = 48_000.0;

    fn bits(buf: &[f32]) -> Vec<u32> {
        buf.iter().map(|s| s.to_bits()).collect()
    }

    /// Render `frames` mono frames in chunks of the given sizes (cycled).
    fn render_chunked(synth: &mut Synth, frames: usize, chunks: &[usize]) -> Vec<f32> {
        let mut out = vec![0.0_f32; frames];
        let mut pos = 0;
        for &chunk in chunks.iter().cycle() {
            if pos == frames {
                break;
            }
            let end = (pos + chunk.max(1)).min(frames);
            synth.render(&mut out[pos..end], 1).unwrap();
            pos = end;
        }
        out
    }

    #[test]
    fn a_scheduled_note_starts_on_its_exact_frame() {
        for at in [0_u32, 1, 127, 128, 1000, 4095] {
            let mut synth = Synth::new(SR).unwrap();
            synth.load_preset(1, None, None).unwrap();
            synth.note_on(69, None, Some(f64::from(at))).unwrap();
            let out = render_chunked(&mut synth, 4096 + 512, &[128]);
            let first = out.iter().position(|s| *s != 0.0).unwrap();
            // The attack starts from zero, so allow the first sample to be 0.
            let at = usize::try_from(at).unwrap();
            assert!(
                (at..=at + 1).contains(&first),
                "at {at}: first sound at {first}"
            );
        }
    }

    #[test]
    fn scheduling_matches_splitting_the_render_by_hand() {
        let events = [(300_u32, 57_u32), (300, 60), (1_000, 64), (2_345, 69)];
        let mut scheduled = Synth::new(SR).unwrap();
        scheduled.load_preset(4, None, None).unwrap();
        for &(at, note) in &events {
            scheduled.note_on(note, None, Some(f64::from(at))).unwrap();
        }
        scheduled.note_off(57, None, Some(1_500.0)).unwrap();
        let out = render_chunked(&mut scheduled, 4096, &[4096]);

        // The same performance, rendered up to each event and sent "now".
        let mut by_hand = Synth::new(SR).unwrap();
        by_hand.load_preset(4, None, None).unwrap();
        let mut expected = Vec::new();
        for (until, action) in [
            (300, &[(true, 57), (true, 60)][..]),
            (1_000, &[(true, 64)][..]),
            (1_500, &[(false, 57)][..]),
            (2_345, &[(true, 69)][..]),
            (4_096, &[][..]),
        ] {
            expected.extend(render_chunked(
                &mut by_hand,
                until - expected.len(),
                &[usize::MAX],
            ));
            for &(on, note) in action {
                if on {
                    by_hand.note_on(note, None, None).unwrap();
                } else {
                    by_hand.note_off(note, None, None).unwrap();
                }
            }
        }
        assert_eq!(bits(&out), bits(&expected));
    }

    #[test]
    fn late_events_apply_at_the_start_of_the_next_render() {
        let mut late = Synth::new(SR).unwrap();
        render_chunked(&mut late, 1_000, &[1_000]);
        late.note_on(69, None, Some(10.0)).unwrap();
        let mut now = Synth::new(SR).unwrap();
        render_chunked(&mut now, 1_000, &[1_000]);
        now.note_on(69, None, None).unwrap();
        assert_eq!(
            bits(&render_chunked(&mut late, 2_048, &[128])),
            bits(&render_chunked(&mut now, 2_048, &[128]))
        );
    }

    #[test]
    fn events_for_the_same_frame_keep_their_order() {
        // On then off at the same frame leaves nothing held; off then on
        // leaves the note playing.
        let held = |first_on: bool| {
            let mut synth = Synth::new(SR).unwrap();
            let (a, b) = if first_on {
                (true, false)
            } else {
                (false, true)
            };
            for on in [a, b] {
                if on {
                    synth.note_on(60, None, Some(256.0)).unwrap();
                } else {
                    synth.note_off(60, None, Some(256.0)).unwrap();
                }
            }
            render_chunked(&mut synth, 48_000, &[128]);
            synth.active_voices()
        };
        assert_eq!(held(true), 0);
        assert_eq!(held(false), 1);
    }

    #[test]
    fn the_queue_is_bounded() {
        let mut synth = Synth::new(SR).unwrap();
        for i in 0..SCHEDULE_CAPACITY {
            synth
                .enqueue(AudioEvent::Drum(DrumHit::Kick), Some(i as u64))
                .unwrap();
        }
        assert_eq!(synth.pending_events(), SCHEDULE_CAPACITY);
        assert!(synth.enqueue(AudioEvent::Panic, None).is_err());
        let capacity = synth.queue.capacity();
        render_chunked(&mut synth, 8192, &[128]);
        assert_eq!(synth.pending_events(), 0);
        assert_eq!(
            synth.queue.capacity(),
            capacity,
            "the queue must not reallocate"
        );
    }

    #[test]
    fn panic_and_cancel_drop_pending_events() {
        for panic in [true, false] {
            let mut synth = Synth::new(SR).unwrap();
            synth.note_on(60, None, None).unwrap();
            synth.note_on(64, None, Some(2_000.0)).unwrap();
            render_chunked(&mut synth, 1_000, &[128]);
            if panic {
                synth.panic();
                assert_eq!(synth.pending_events(), 1, "only the panic itself");
            } else {
                synth.cancel_scheduled();
                assert_eq!(synth.pending_events(), 0);
            }
            render_chunked(&mut synth, 4_000, &[128]);
            let expected = usize::from(!panic); // cancel leaves the first note held
            assert_eq!(synth.active_voices(), expected, "panic = {panic}");
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(48))]

        /// However the host slices its `render` calls, a schedule renders to
        /// the same samples, bit for bit.
        #[test]
        fn output_does_not_depend_on_render_block_sizes(
            events in proptest::collection::vec((0_u32..6_000, 0_u8..5, 36_u32..84), 1..24),
            chunks in proptest::collection::vec(1_usize..700, 1..8),
            preset in 0_usize..8,
        ) {
            let render_with = |chunks: &[usize]| {
                let mut synth = Synth::new(SR).unwrap();
                for ch in 0_u8..4 {
                    let preset = (preset + usize::from(ch)) % default_patches().len();
                    synth.load_preset(preset, Some(u32::from(ch)), None).unwrap();
                }
                for &(at, kind, note) in &events {
                    let at = Some(f64::from(at));
                    match kind {
                        0 | 1 => synth.note_on(note, Some(note % 4), at).unwrap(),
                        2 | 3 => synth.note_off(note, Some(note % 4), at).unwrap(),
                        _ => synth.drum(Drum::HiHatClosed, at).unwrap(),
                    }
                }
                render_chunked(&mut synth, 8_000, chunks)
            };
            proptest::prop_assert_eq!(bits(&render_with(&chunks)), bits(&render_with(&[8_000])));
        }
    }

    // --- Silence ------------------------------------------------------------

    #[test]
    fn a_fresh_synth_is_idle_and_renders_nothing_until_silent() {
        let mut synth = Synth::new(SR).unwrap();
        assert!(synth.is_idle());
        assert_eq!(synth.render_until_silent_frames(48_000, 4_800).len(), 0);
    }

    #[test]
    fn render_until_silent_captures_a_hit_with_its_whole_tail() {
        let mut synth = Synth::new(SR).unwrap();
        synth.drum(Drum::Kick, None).unwrap();
        let hold = 4_800;
        let out = synth.render_until_silent_frames(10 * 48_000, hold);
        let threshold = synth.silence_threshold();
        assert!(out.len() > 1_000, "the kick should last a while");
        assert!(
            out.last().unwrap().abs() > threshold,
            "trailing silence is trimmed"
        );
        assert!(synth.is_idle());
        // Nothing audible was cut off: the synth stays silent from here on.
        let mut after = vec![0.0_f32; 48_000];
        synth.render(&mut after, 1).unwrap();
        assert!(after.iter().all(|s| s.abs() <= threshold));
    }

    #[test]
    fn render_until_silent_waits_for_a_released_note_and_its_reverb() {
        let mut synth = Synth::new(SR).unwrap();
        synth.load_preset(1, None, None).unwrap();
        synth.note_on(69, None, None).unwrap();
        synth.note_off(69, None, Some(4_800.0)).unwrap();
        let out = synth.render_until_silent_frames(30 * 48_000, 24_000);
        assert!(out.len() > 4_800, "the release outlasts the held part");
        assert!(out.len() < 30 * 48_000, "it should end well before the cap");
        assert_eq!(synth.active_voices(), 0);
    }

    #[test]
    fn render_until_silent_plays_a_whole_scheduled_sequence() {
        let mut synth = Synth::new(SR).unwrap();
        // A long idle, silent gap (far longer than the hold) before a late
        // hit: the pending event must keep the render going through it.
        synth.drum(Drum::HiHatClosed, None).unwrap();
        synth.drum(Drum::HiHatOpen, Some(240_000.0)).unwrap();
        let out = synth.render_until_silent_frames(10 * 48_000, 4_800);
        assert!(
            out.len() > 240_000,
            "stopped before the scheduled hit: {}",
            out.len()
        );
        assert!(synth.is_idle());
    }

    #[test]
    fn render_until_silent_stops_at_the_cap() {
        let mut synth = Synth::new(SR).unwrap();
        synth.load_preset(1, None, None).unwrap();
        synth.note_on(69, None, None).unwrap(); // held forever
        let out = synth.render_until_silent_frames(12_345, 4_800);
        assert_eq!(out.len(), 12_345);
        assert!(!synth.is_idle());
    }

    #[test]
    fn silent_frames_counts_only_idle_silence() {
        let mut synth = Synth::new(SR).unwrap();
        let mut buf = vec![0.0_f32; 128];
        synth.render(&mut buf, 1).unwrap();
        synth.render(&mut buf, 1).unwrap();
        assert!((synth.silent_frames() - 256.0).abs() < f64::EPSILON);
        // A note scheduled for later is not silence yet, even though nothing
        // is audible so far.
        synth.note_on(60, None, Some(100_000.0)).unwrap();
        synth.render(&mut buf, 1).unwrap();
        assert_eq!(synth.silent_frames().to_bits(), 0.0_f64.to_bits());
    }
}
