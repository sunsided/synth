//! Pure-DSP synthesis processor: N polyphonic channels, shared FX, no I/O.
//!
//! [`SynthProcessor`] is the embedding surface for `no_std` / WASM / game-engine
//! consumers. Call [`SynthProcessor::process_block`] from any audio callback.

use crate::audio::chorus::Chorus;
use crate::audio::delay::Delay;
use crate::audio::drums::DrumMachine;
use crate::audio::fx::Reverb;
use crate::audio::voice::Voice;
use crate::params::{AudioEvent, ChannelNo, MidiNote, SynthParams};

/// Number of simultaneous voices per synthesis channel.
const POLYPHONY: usize = 4;

/// Polyphony as `f32` for normalising the summed voice mix.
const POLYPHONY_F32: f32 = 4.0;

/// Default number of independent synthesis channels in [`SynthProcessor`].
pub const NUM_CHANNELS: usize = 4;

/// Master output gain applied after summing all synthesis channels.
const MASTER_GAIN: f32 = 0.5;

#[derive(Clone, Copy, Default)]
struct VoiceSlot {
    note: Option<MidiNote>,
    age: u64,
}

struct AudioChannel {
    params: SynthParams,
    voices: [Voice; POLYPHONY],
    slots: [VoiceSlot; POLYPHONY],
    age_counter: u64,
}

impl AudioChannel {
    fn new() -> Self {
        Self {
            params: SynthParams::default(),
            voices: std::array::from_fn(|_| Voice::new()),
            slots: std::array::from_fn(|_| VoiceSlot::default()),
            age_counter: 0,
        }
    }

    fn is_voice_idle(&self, idx: usize) -> bool {
        let voice = &self.voices[idx];
        !voice.active && !voice.env.is_active() && self.slots[idx].note.is_none()
    }

    fn allocate_voice_index(&self, midi: MidiNote) -> usize {
        if let Some(idx) = self.slots.iter().position(|s| s.note == Some(midi)) {
            return idx;
        }
        if let Some(idx) = (0..POLYPHONY).find(|&idx| self.is_voice_idle(idx)) {
            return idx;
        }
        self.slots
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| s.age)
            .map_or(0, |(idx, _)| idx)
    }

    fn note_on(&mut self, midi: MidiNote, sample_rate: f32) {
        let idx = self.allocate_voice_index(midi);
        self.age_counter = self.age_counter.saturating_add(1);
        self.slots[idx].note = Some(midi);
        self.slots[idx].age = self.age_counter;
        self.voices[idx].note_on(midi, &self.params, sample_rate);
    }

    fn note_off(&mut self, midi: MidiNote) {
        if let Some(idx) = self.slots.iter().position(|s| s.note == Some(midi)) {
            self.voices[idx].note_off();
            self.slots[idx].note = None;
        }
    }

    fn panic(&mut self) {
        for voice in &mut self.voices {
            voice.panic();
        }
        for slot in &mut self.slots {
            *slot = VoiceSlot::default();
        }
        self.age_counter = 0;
    }

    fn process(&mut self, sample_rate: f32) -> f32 {
        self.voices
            .iter_mut()
            .map(|v| v.process(&self.params, sample_rate))
            .sum::<f32>()
            / POLYPHONY_F32
    }
}

/// Multi-channel DSP processor with shared FX chain.
///
/// `N` is the number of independent synthesis channels, each with its own
/// voice pool and [`SynthParams`] snapshot.  The default is [`NUM_CHANNELS`].
///
/// This type has no I/O dependencies and is the intended embedding surface for
/// game engines, WASM targets, and other non-CPAL consumers.
pub struct SynthProcessor<const N: usize> {
    channels: [AudioChannel; N],
    reverb: Reverb,
    chorus: Chorus,
    delay: Delay,
    drums: DrumMachine,
    sample_rate: f32,
}

impl<const N: usize> SynthProcessor<N> {
    /// Create a new processor for the given sample rate.
    #[must_use]
    pub fn new(sample_rate: f32) -> Self {
        const {
            assert!(
                N >= 1,
                "SynthProcessor requires at least 1 synthesis channel (N >= 1)"
            );
        };
        let channels: [AudioChannel; N] = std::array::from_fn(|_| AudioChannel::new());
        let mut reverb = Reverb::new();
        reverb.set_params(
            channels[0].params.fx.reverb_size,
            channels[0].params.fx.reverb_damping,
        );
        Self {
            channels,
            reverb,
            chorus: Chorus::new(sample_rate),
            delay: Delay::new(sample_rate),
            drums: DrumMachine::new(sample_rate),
            sample_rate,
        }
    }

    fn apply_reverb_params(&mut self) {
        let fx = &self.channels[0].params.fx;
        self.reverb.set_params(fx.reverb_size, fx.reverb_damping);
    }

    fn note_on(&mut self, ch: ChannelNo, midi: MidiNote) {
        if let Some(channel) = self.channels.get_mut(ch.as_usize()) {
            channel.note_on(midi, self.sample_rate);
        }
    }

    fn note_off(&mut self, ch: ChannelNo, midi: MidiNote) {
        if let Some(channel) = self.channels.get_mut(ch.as_usize()) {
            channel.note_off(midi);
        }
    }

    fn panic(&mut self) {
        for channel in &mut self.channels {
            channel.panic();
        }
        self.drums.panic();
    }

    fn apply_events(&mut self, events: &[AudioEvent]) {
        for event in events {
            match event {
                AudioEvent::NoteOn(midi) => self.note_on(ChannelNo::DEFAULT, *midi),
                AudioEvent::NoteOff(midi) => self.note_off(ChannelNo::DEFAULT, *midi),
                AudioEvent::Panic => self.panic(),
                AudioEvent::LoadPatch(p) => {
                    self.channels[0].params = (**p).clone();
                    self.apply_reverb_params();
                }
                AudioEvent::Drum(hit) => self.drums.trigger(*hit),
                AudioEvent::NoteOnChannel(ch, midi) => self.note_on(*ch, *midi),
                AudioEvent::NoteOffChannel(ch, midi) => self.note_off(*ch, *midi),
                AudioEvent::LoadPatchChannel(ch, p) => {
                    if let Some(channel) = self.channels.get_mut(ch.as_usize()) {
                        channel.params = (**p).clone();
                    }
                    if *ch == ChannelNo::DEFAULT {
                        self.apply_reverb_params();
                    }
                }
            }
        }
    }

    /// Apply `events` then render `hw_channels`-interleaved frames into `buf`.
    ///
    /// `buf.len()` must be a multiple of `hw_channels`. Events are applied
    /// once before the entire buffer is rendered (same semantics as a
    /// sample-accurate block boundary).
    ///
    /// # Panics
    ///
    /// Panics if `hw_channels` is zero or if `buf.len()` is not a multiple of
    /// `hw_channels`.
    pub fn process_block(&mut self, events: &[AudioEvent], buf: &mut [f32], hw_channels: usize) {
        assert!(hw_channels > 0, "hw_channels must be > 0");
        assert_eq!(
            buf.len() % hw_channels,
            0,
            "buf.len() must be a multiple of hw_channels"
        );
        self.apply_events(events);

        let sample_rate = self.sample_rate;
        let reverb_mix = self.channels[0].params.fx.reverb_mix;
        let chorus_params = self.channels[0].params.chorus.clone();
        let delay_params = self.channels[0].params.delay.clone();

        for frame in buf.chunks_mut(hw_channels) {
            let mix: f32 = self
                .channels
                .iter_mut()
                .map(|ch| ch.process(sample_rate))
                .sum::<f32>()
                + self.drums.process(sample_rate);
            let mix = self.chorus.process(mix, &chorus_params);
            let mix = self.delay.process(mix, &delay_params);
            let sample = self.reverb.process(mix, reverb_mix) * MASTER_GAIN;

            let sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                core::hint::cold_path();
                0.0
            };

            for ch in frame.iter_mut() {
                *ch = sample;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::DrumHit;

    const SR: f32 = 44100.0;
    const NOTE: MidiNote = MidiNote::MIDDLE_C;

    fn make() -> SynthProcessor<NUM_CHANNELS> {
        SynthProcessor::new(SR)
    }

    #[test]
    fn renders_silence_with_no_events() {
        let mut proc = make();
        let mut buf = vec![1.0_f32; 256];
        proc.process_block(&[], &mut buf, 2);
        assert!(buf.iter().all(|s| s.is_finite()), "non-finite samples");
        assert!(
            buf.iter().all(|s| *s == 0.0),
            "expected silence with no notes active"
        );
    }

    #[test]
    fn note_on_produces_non_zero_output() {
        let mut proc = make();
        let mut buf = vec![0.0_f32; 4096];
        proc.process_block(&[AudioEvent::NoteOn(NOTE)], &mut buf, 1);
        assert!(
            buf.iter().any(|s| s.abs() > 1e-6),
            "note_on produced no audible signal"
        );
    }

    #[test]
    fn note_off_after_note_on_does_not_panic() {
        let mut proc = make();
        let mut buf = vec![0.0_f32; 256];
        proc.process_block(&[AudioEvent::NoteOn(NOTE)], &mut buf, 1);
        proc.process_block(&[AudioEvent::NoteOff(NOTE)], &mut buf, 1);
    }

    #[test]
    fn panic_event_clears_voice_state() {
        let mut proc = make();
        let mut buf = vec![0.0_f32; 8192];
        let n2 = MidiNote::A4;
        proc.process_block(
            &[AudioEvent::NoteOn(NOTE), AudioEvent::NoteOn(n2)],
            &mut buf,
            1,
        );
        proc.process_block(&[AudioEvent::Panic], &mut buf, 1);
        // After panic all voice slots are cleared; output may have a brief
        // release tail, but must never go non-finite.
        let mut after = vec![0.0_f32; 8192];
        proc.process_block(&[], &mut after, 1);
        assert!(
            after.iter().all(|s| s.is_finite()),
            "non-finite after panic"
        );
    }

    #[test]
    fn channel_specific_note_on() {
        let mut proc = make();
        let mut buf = vec![0.0_f32; 4096];
        let ch = ChannelNo::from(1u8);
        proc.process_block(&[AudioEvent::NoteOnChannel(ch, NOTE)], &mut buf, 1);
        assert!(
            buf.iter().any(|s| s.abs() > 1e-6),
            "channel-specific note_on produced no signal"
        );
    }

    #[test]
    fn channel_specific_note_off_does_not_affect_other_channel() {
        let mut proc = make();
        let mut buf = vec![0.0_f32; 4096];
        // Note on channel 0, note off on channel 1 — channel 0 should keep playing.
        proc.process_block(&[AudioEvent::NoteOn(NOTE)], &mut buf, 1);
        let ch1 = ChannelNo::from(1u8);
        proc.process_block(&[AudioEvent::NoteOffChannel(ch1, NOTE)], &mut buf, 1);
        assert!(
            buf.iter().any(|s| s.abs() > 1e-6),
            "wrong channel note_off silenced the wrong channel"
        );
    }

    #[test]
    fn load_patch_channel_0_does_not_panic() {
        let mut proc = make();
        let mut buf = vec![0.0_f32; 256];
        let patch = Box::new(SynthParams::default());
        proc.process_block(&[AudioEvent::LoadPatch(patch)], &mut buf, 1);
    }

    #[test]
    fn load_patch_channel_n_does_not_panic() {
        let mut proc = make();
        let mut buf = vec![0.0_f32; 256];
        let ch = ChannelNo::from(2u8);
        let patch = Box::new(SynthParams::default());
        proc.process_block(&[AudioEvent::LoadPatchChannel(ch, patch)], &mut buf, 1);
    }

    #[test]
    fn drum_event_produces_signal() {
        let mut proc = make();
        let mut buf = vec![0.0_f32; 4096];
        proc.process_block(&[AudioEvent::Drum(DrumHit::Kick)], &mut buf, 1);
        assert!(
            buf.iter().any(|s| s.abs() > 1e-6),
            "drum kick produced no signal"
        );
    }

    #[test]
    fn stereo_frames_have_equal_left_right() {
        let mut proc = make();
        let mut buf = vec![0.0_f32; 512];
        proc.process_block(&[AudioEvent::NoteOn(NOTE)], &mut buf, 2);
        // Left and right channels carry the same mono sample — exact bit equality intended.
        #[allow(clippy::float_cmp)]
        for frame in buf.chunks(2) {
            assert_eq!(frame[0], frame[1], "stereo channels differ within a frame");
        }
    }

    #[test]
    #[should_panic(expected = "hw_channels must be > 0")]
    fn zero_hw_channels_panics() {
        let mut proc = make();
        let mut buf = vec![0.0_f32; 4];
        proc.process_block(&[], &mut buf, 0);
    }

    #[test]
    #[should_panic(expected = "buf.len() must be a multiple of hw_channels")]
    fn misaligned_buf_panics() {
        let mut proc = make();
        let mut buf = vec![0.0_f32; 3];
        proc.process_block(&[], &mut buf, 2);
    }
}
