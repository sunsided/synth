//! Pure-DSP synthesis processor: N polyphonic channels, shared FX, no I/O.
//!
//! [`SynthProcessor`] is the embedding surface for WASM and game-engine
//! consumers. Call [`SynthProcessor::process_block`] from any audio callback.
//! Full `no_std` support is planned but not yet implemented.

use crate::audio::chorus::Chorus;
use crate::audio::delay::Delay;
use crate::audio::drums::DrumMachine;
use crate::audio::fx::Reverb;
use crate::audio::voice::Voice;
use crate::param::Param;
use crate::params::{AudioEvent, ChannelNo, MidiNote, SynthParams};

/// Number of simultaneous voices per synthesis channel.
const POLYPHONY: usize = 4;

/// Polyphony as `f32` for normalising the summed voice mix.
const POLYPHONY_F32: f32 = 4.0;

/// How each synthesis channel scales the sum of its voices.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VoiceNormalization {
    /// Divide each channel's voice sum by its polyphony (4).
    ///
    /// Keeps headroom constant when every voice sounds at once, at the cost
    /// of a lone note playing at a quarter of a voice's level.
    #[default]
    Polyphony,
    /// Pass the voice sum through unscaled.
    ///
    /// A lone note plays at full level, which suits one-shot sound effects
    /// and monophonic use; chords can clip at the final clamp unless the
    /// master gain is lowered.
    Off,
}

impl VoiceNormalization {
    fn scale(self) -> f32 {
        match self {
            Self::Polyphony => 1.0 / POLYPHONY_F32,
            Self::Off => 1.0,
        }
    }
}

/// Default number of independent synthesis channels in [`SynthProcessor`].
pub const NUM_CHANNELS: usize = 4;

/// Default master output gain of a new [`SynthProcessor`].
///
/// Applied after the FX chain and before the final clamp to `[-1.0, 1.0]`.
/// With the default [`VoiceNormalization::Polyphony`] each channel's voice sum
/// is additionally divided by its polyphony (4), so a single note peaks at
/// roughly `DEFAULT_MASTER_GAIN / 4 × volume` of full scale. Use
/// [`SynthProcessor::set_master_gain`] and
/// [`SynthProcessor::set_voice_normalization`] to trade headroom for level.
pub const DEFAULT_MASTER_GAIN: f32 = 0.5;

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
    #[cfg(feature = "arp")]
    arp: crate::audio::arp::Arpeggiator,
}

impl AudioChannel {
    fn new() -> Self {
        Self {
            params: SynthParams::default(),
            voices: std::array::from_fn(|_| Voice::new()),
            slots: std::array::from_fn(|_| VoiceSlot::default()),
            age_counter: 0,
            #[cfg(feature = "arp")]
            arp: crate::audio::arp::Arpeggiator::new(),
        }
    }

    fn active_voices(&self) -> usize {
        self.voices
            .iter()
            .filter(|v| v.active || v.env.is_active())
            .count()
    }

    fn is_idle(&self) -> bool {
        #[cfg(feature = "arp")]
        if self.params.arp.enabled && self.params.arp.count > 0 {
            return false;
        }
        self.active_voices() == 0
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
        #[cfg(feature = "arp")]
        if self.params.arp.enabled {
            self.arp.add_note(&mut self.params.arp, midi);
            return;
        }
        self.voice_note_on(midi, sample_rate);
    }

    fn note_off(&mut self, midi: MidiNote) {
        #[cfg(feature = "arp")]
        if self.params.arp.enabled {
            self.arp.remove_note(&mut self.params.arp, midi);
            // If the last note was removed, release any voice the arp was holding
            // so it doesn't sustain indefinitely (arp won't tick while count == 0).
            if self.params.arp.count == 0
                && let Some(stuck) = self.arp.sounding.take()
            {
                self.voice_note_off(stuck);
            }
            return;
        }
        self.voice_note_off(midi);
    }

    fn voice_note_on(&mut self, midi: MidiNote, sample_rate: f32) {
        let idx = self.allocate_voice_index(midi);
        self.age_counter = self.age_counter.saturating_add(1);
        self.slots[idx].note = Some(midi);
        self.slots[idx].age = self.age_counter;
        self.voices[idx].note_on(midi, &self.params, sample_rate);
    }

    fn voice_note_off(&mut self, midi: MidiNote) {
        if let Some(idx) = self.slots.iter().position(|s| s.note == Some(midi)) {
            self.voices[idx].note_off();
            self.slots[idx].note = None;
        }
    }

    fn panic(&mut self) {
        #[cfg(feature = "arp")]
        self.arp.panic(&mut self.params.arp);

        for voice in &mut self.voices {
            voice.panic();
        }
        for slot in &mut self.slots {
            *slot = VoiceSlot::default();
        }
        self.age_counter = 0;
    }

    fn process(&mut self, sample_rate: f32) -> f32 {
        #[cfg(feature = "arp")]
        if self.params.arp.enabled && self.params.arp.count > 0 {
            let events = self.arp.tick(sample_rate, &self.params.arp);
            if let Some(note) = events.off {
                self.voice_note_off(note);
            }
            if let Some(note) = events.on {
                self.voice_note_on(note, sample_rate);
            }
        }

        self.voices
            .iter_mut()
            .map(|v| v.process(&self.params, sample_rate))
            .sum::<f32>()
    }
}

/// A copy of `params` brought into range, see [`SynthParams::sanitize`].
fn sanitized(params: &SynthParams) -> SynthParams {
    let mut params = params.clone();
    params.sanitize();
    params
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
    master_gain: f32,
    voice_normalization: VoiceNormalization,
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
            master_gain: DEFAULT_MASTER_GAIN,
            voice_normalization: VoiceNormalization::default(),
        }
    }

    /// Master output gain applied after the FX chain.
    ///
    /// Defaults to [`DEFAULT_MASTER_GAIN`].
    #[must_use]
    pub fn master_gain(&self) -> f32 {
        self.master_gain
    }

    /// Set the master output gain applied after the FX chain.
    ///
    /// The output is still clamped to `[-1.0, 1.0]`, so gains above the
    /// default reduce headroom and can clip when many voices sound at once.
    /// Non-finite values are ignored.
    pub fn set_master_gain(&mut self, gain: f32) {
        if gain.is_finite() {
            self.master_gain = gain;
        }
    }

    /// The patch channel `ch` is playing, or `None` for an unknown channel.
    #[must_use]
    pub fn params(&self, ch: ChannelNo) -> Option<&SynthParams> {
        self.channels.get(ch.as_usize()).map(|c| &c.params)
    }

    /// Number of synth voices still sounding, held or releasing, across all
    /// channels. Drum hits are not counted; see [`Self::is_idle`].
    #[must_use]
    pub fn active_voices(&self) -> usize {
        self.channels.iter().map(AudioChannel::active_voices).sum()
    }

    /// Whether nothing will produce new sound: no synth voice is held or
    /// releasing, no drum hit is ringing, and no arpeggiator is running.
    ///
    /// Reverb, delay and chorus tails can still be audible after the
    /// processor becomes idle; check the rendered level to wait them out.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.channels.iter().all(AudioChannel::is_idle) && !self.drums.is_active()
    }

    /// How each synthesis channel scales the sum of its voices.
    ///
    /// Defaults to [`VoiceNormalization::Polyphony`].
    #[must_use]
    pub fn voice_normalization(&self) -> VoiceNormalization {
        self.voice_normalization
    }

    /// Set how each synthesis channel scales the sum of its voices.
    ///
    /// Drums are not affected.
    pub fn set_voice_normalization(&mut self, normalization: VoiceNormalization) {
        self.voice_normalization = normalization;
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

    /// Silence everything at once: voices, drums and the FX tails.
    fn panic(&mut self) {
        for channel in &mut self.channels {
            channel.panic();
        }
        self.drums.panic();
        self.chorus.reset();
        self.delay.reset();
        self.reverb.reset();
    }

    fn apply_events(&mut self, events: &[AudioEvent]) {
        for event in events {
            match event {
                AudioEvent::NoteOn(midi) => self.note_on(ChannelNo::DEFAULT, *midi),
                AudioEvent::NoteOff(midi) => self.note_off(ChannelNo::DEFAULT, *midi),
                AudioEvent::Panic => self.panic(),
                AudioEvent::LoadPatch(p) => {
                    self.channels[0].params = sanitized(p);
                    self.apply_reverb_params();
                }
                AudioEvent::Drum(hit) => self.drums.trigger(*hit),
                AudioEvent::NoteOnChannel(ch, midi) => self.note_on(*ch, *midi),
                AudioEvent::NoteOffChannel(ch, midi) => self.note_off(*ch, *midi),
                AudioEvent::LoadPatchChannel(ch, p) => {
                    if let Some(channel) = self.channels.get_mut(ch.as_usize()) {
                        channel.params = sanitized(p);
                    }
                    if *ch == ChannelNo::DEFAULT {
                        self.apply_reverb_params();
                    }
                }
                AudioEvent::SetParam(ch, param, value) => {
                    if let Some(channel) = self.channels.get_mut(ch.as_usize()) {
                        param.set(&mut channel.params, *value);
                    }
                    if *ch == ChannelNo::DEFAULT
                        && matches!(param, Param::ReverbSize | Param::ReverbDamping)
                    {
                        self.apply_reverb_params();
                    }
                }
                #[cfg(feature = "arp")]
                AudioEvent::ArpSetNotes(ch, notes, count) => {
                    if let Some(channel) = self.channels.get_mut(ch.as_usize()) {
                        channel
                            .arp
                            .set_notes(&mut channel.params.arp, &notes[..(*count as usize).min(4)]);
                    }
                }
                #[cfg(feature = "arp")]
                AudioEvent::ArpEnabled(ch, enabled) => {
                    if let Some(channel) = self.channels.get_mut(ch.as_usize()) {
                        channel.params.arp.enabled = *enabled;
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
        let master_gain = self.master_gain;
        let voice_scale = self.voice_normalization.scale();
        let reverb_mix = self.channels[0].params.fx.reverb_mix;
        let chorus_params = self.channels[0].params.chorus.clone();
        let delay_params = self.channels[0].params.delay.clone();

        for frame in buf.chunks_mut(hw_channels) {
            let mix: f32 = self
                .channels
                .iter_mut()
                .map(|ch| ch.process(sample_rate))
                .sum::<f32>()
                * voice_scale
                + self.drums.process(sample_rate);
            let mix = self.chorus.process(mix, &chorus_params);
            let mix = self.delay.process(mix, &delay_params);
            let sample = self.reverb.process(mix, reverb_mix) * master_gain;

            let sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                // Something overflowed despite sanitized patches. A NaN or
                // infinity would otherwise circulate in the voice and FX
                // feedback paths forever, silencing the engine for good:
                // drop all state and start over from silence.
                core::hint::cold_path();
                self.panic();
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
    fn master_gain_defaults_and_ignores_non_finite() {
        let mut proc = make();
        assert!((proc.master_gain() - DEFAULT_MASTER_GAIN).abs() < f32::EPSILON);
        proc.set_master_gain(1.5);
        assert!((proc.master_gain() - 1.5).abs() < f32::EPSILON);
        proc.set_master_gain(f32::NAN);
        proc.set_master_gain(f32::INFINITY);
        assert!((proc.master_gain() - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn master_gain_scales_output() {
        let render = |gain: f32| {
            let mut proc = make();
            proc.set_master_gain(gain);
            let mut buf = vec![0.0_f32; 4096];
            proc.process_block(&[AudioEvent::NoteOn(NOTE)], &mut buf, 1);
            buf
        };
        let base = render(DEFAULT_MASTER_GAIN);
        let doubled = render(DEFAULT_MASTER_GAIN * 2.0);
        let silent = render(0.0);
        assert!(silent.iter().all(|s| *s == 0.0), "zero gain must be silent");
        for (b, d) in base.iter().zip(&doubled) {
            assert!(
                (d - (b * 2.0).clamp(-1.0, 1.0)).abs() < 1e-6,
                "doubling the gain must double the (unclipped) output"
            );
        }
    }

    #[test]
    fn voice_normalization_off_raises_a_lone_note_by_the_polyphony() {
        let render = |normalization: VoiceNormalization| {
            let mut proc = make();
            proc.set_voice_normalization(normalization);
            // Low gain keeps the unnormalised output clear of the clamp.
            proc.set_master_gain(0.1);
            let mut buf = vec![0.0_f32; 4096];
            proc.process_block(&[AudioEvent::NoteOn(NOTE)], &mut buf, 1);
            buf
        };
        assert_eq!(make().voice_normalization(), VoiceNormalization::Polyphony);
        let normalized = render(VoiceNormalization::Polyphony);
        let raw = render(VoiceNormalization::Off);
        assert!(normalized.iter().any(|s| s.abs() > 1e-6));
        for (n, r) in normalized.iter().zip(&raw) {
            assert!(
                (r - n * POLYPHONY_F32).abs() < 1e-5,
                "disabling normalisation must scale the voice sum by the polyphony"
            );
        }
    }

    /// Render `frames` mono frames and return them.
    fn run(
        proc: &mut SynthProcessor<NUM_CHANNELS>,
        events: &[AudioEvent],
        frames: usize,
    ) -> Vec<f32> {
        let mut buf = vec![0.0_f32; frames];
        proc.process_block(events, &mut buf, 1);
        buf
    }

    /// Render in small blocks until `done` holds; return the frames it took.
    fn frames_until(
        proc: &mut SynthProcessor<NUM_CHANNELS>,
        done: impl Fn(&SynthProcessor<NUM_CHANNELS>) -> bool,
    ) -> usize {
        let mut frames = 0;
        while !done(proc) {
            run(proc, &[], 64);
            frames += 64;
            assert!(frames < 10 * 44_100, "still not done after 10 s");
        }
        frames
    }

    #[test]
    fn a_fresh_processor_is_idle() {
        let proc = make();
        assert!(proc.is_idle());
        assert_eq!(proc.active_voices(), 0);
    }

    #[test]
    fn voices_count_while_held_and_releasing() {
        let mut proc = make();
        let chord = [MidiNote(60), MidiNote(64), MidiNote(67)];
        let on: Vec<_> = chord.iter().map(|&n| AudioEvent::NoteOn(n)).collect();
        run(&mut proc, &on, 4096);
        assert_eq!(proc.active_voices(), 3);
        assert!(!proc.is_idle());

        let off: Vec<_> = chord.iter().map(|&n| AudioEvent::NoteOff(n)).collect();
        run(&mut proc, &off, 1);
        assert_eq!(proc.active_voices(), 3, "released voices still sound");
        let release = frames_until(&mut proc, SynthProcessor::is_idle);
        assert!(release > 0, "the release should take time");
        assert_eq!(proc.active_voices(), 0);
    }

    #[test]
    fn voices_count_across_channels() {
        let mut proc = make();
        let events: Vec<_> = (0..4u8)
            .map(|ch| AudioEvent::NoteOnChannel(ChannelNo(ch), NOTE))
            .collect();
        run(&mut proc, &events, 256);
        assert_eq!(proc.active_voices(), 4);
    }

    #[test]
    fn drums_keep_the_processor_busy_until_they_decay() {
        for hit in [DrumHit::Kick, DrumHit::HiHatClosed, DrumHit::HiHatOpen] {
            let mut proc = make();
            run(&mut proc, &[AudioEvent::Drum(hit)], 64);
            assert!(!proc.is_idle(), "{hit:?} should keep the processor busy");
            assert_eq!(proc.active_voices(), 0, "drums are not synth voices");
            frames_until(&mut proc, SynthProcessor::is_idle);
        }
    }

    #[test]
    fn panic_makes_the_processor_idle_at_once() {
        let mut proc = make();
        run(
            &mut proc,
            &[
                AudioEvent::NoteOn(NOTE),
                AudioEvent::Drum(DrumHit::HiHatOpen),
            ],
            256,
        );
        run(&mut proc, &[AudioEvent::Panic], 1);
        assert!(proc.is_idle());
    }

    #[cfg(feature = "arp")]
    #[test]
    fn a_running_arpeggiator_is_never_idle() {
        let mut proc = make();
        let notes = [MidiNote::MIDDLE_C; 4];
        run(
            &mut proc,
            &[
                AudioEvent::ArpEnabled(ChannelNo::DEFAULT, true),
                AudioEvent::ArpSetNotes(ChannelNo::DEFAULT, notes, 1),
            ],
            64,
        );
        // Between steps every voice may be silent, but more notes are coming.
        for _ in 0..200 {
            run(&mut proc, &[], 441);
            assert!(!proc.is_idle());
        }
        run(&mut proc, &[AudioEvent::Panic], 1);
        assert!(proc.is_idle());
    }

    /// Belt and braces: even if a non-finite value gets past the patch
    /// sanitizing, the engine drops all state instead of staying silent.
    #[test]
    fn a_non_finite_sample_resets_the_engine_instead_of_poisoning_it() {
        let mut proc = make();
        // Bypass sanitizing: this rate overflows the chorus LFO to NaN.
        proc.channels[0].params.chorus.rate = f32::MAX;
        proc.channels[0].params.chorus.mix = 0.5;
        proc.channels[0].params.delay.mix = 0.5;
        let out = run(&mut proc, &[AudioEvent::NoteOn(NOTE)], 4096);
        assert!(out.iter().all(|s| s.is_finite()));

        let events = [
            AudioEvent::LoadPatch(Box::default()),
            AudioEvent::NoteOn(NOTE),
        ];
        let healed = run(&mut proc, &events, 4096);
        let reference = run(&mut make(), &events, 4096);
        assert_eq!(
            healed.iter().map(|s| s.to_bits()).collect::<Vec<_>>(),
            reference.iter().map(|s| s.to_bits()).collect::<Vec<_>>(),
            "after the reset the engine should play like a new one"
        );
    }

    #[test]
    fn set_param_changes_one_field_and_keeps_the_rest() {
        let mut proc = make();
        let ch = ChannelNo(2);
        run(
            &mut proc,
            &[
                AudioEvent::LoadPatchChannel(
                    ch,
                    Box::new(crate::presets::sid::default_patches()[1].params.clone()),
                ),
                AudioEvent::SetParam(ch, Param::FilterCutoff, 1e9),
                AudioEvent::SetParam(ch, Param::OscWaveform, 2.0),
            ],
            64,
        );
        let mut expected = crate::presets::sid::default_patches()[1].params.clone();
        expected.filter.cutoff = 18_000.0;
        expected.osc.waveform = crate::params::Waveform::Triangle;
        assert_eq!(proc.params(ch), Some(&expected));
        assert_eq!(proc.params(ChannelNo(0)), Some(&SynthParams::default()));
        assert_eq!(proc.params(ChannelNo(9)), None);
    }

    #[test]
    fn set_param_reaches_the_shared_reverb() {
        // Same final patch, built two ways, must sound identical.
        let mut patch = SynthParams::default();
        patch.fx.reverb_mix = 1.0;
        patch.fx.reverb_size = 0.95;
        patch.fx.reverb_damping = 0.05;
        let mut loaded = make();
        let a = run(
            &mut loaded,
            &[
                AudioEvent::LoadPatch(Box::new(patch)),
                AudioEvent::NoteOn(NOTE),
            ],
            8192,
        );
        let mut tweaked = make();
        let b = run(
            &mut tweaked,
            &[
                AudioEvent::SetParam(ChannelNo::DEFAULT, Param::ReverbMix, 1.0),
                AudioEvent::SetParam(ChannelNo::DEFAULT, Param::ReverbSize, 0.95),
                AudioEvent::SetParam(ChannelNo::DEFAULT, Param::ReverbDamping, 0.05),
                AudioEvent::NoteOn(NOTE),
            ],
            8192,
        );
        assert_eq!(
            a.iter().map(|s| s.to_bits()).collect::<Vec<_>>(),
            b.iter().map(|s| s.to_bits()).collect::<Vec<_>>()
        );
    }

    #[cfg(feature = "arp")]
    #[test]
    fn set_param_keeps_the_arpeggiator_running() {
        let mut proc = make();
        let notes = [MidiNote::MIDDLE_C; 4];
        run(
            &mut proc,
            &[
                AudioEvent::ArpEnabled(ChannelNo::DEFAULT, true),
                AudioEvent::ArpSetNotes(ChannelNo::DEFAULT, notes, 1),
                AudioEvent::SetParam(ChannelNo::DEFAULT, Param::ArpRate, 20.0),
            ],
            64,
        );
        assert_eq!(proc.params(ChannelNo::DEFAULT).unwrap().arp.count, 1);
        assert!(!proc.is_idle(), "the arpeggiator should still be running");
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

    #[cfg(feature = "arp")]
    #[test]
    fn arp_set_notes_and_enable_produces_signal() {
        let mut proc = make();
        // Phase 1: arp enabled but no notes loaded (count=0) -> silence
        let mut buf = vec![0.0_f32; 4096];
        proc.process_block(
            &[AudioEvent::ArpEnabled(ChannelNo::DEFAULT, true)],
            &mut buf,
            1,
        );
        assert!(
            buf.iter().all(|s| s.abs() < 1e-6),
            "arp with count=0 should be silent"
        );
        // Phase 2: set 1 note via ArpSetNotes -> arp now has count=1 -> produces signal
        let notes = [MidiNote::MIDDLE_C; 4];
        let mut buf2 = vec![0.0_f32; 8192];
        proc.process_block(
            &[AudioEvent::ArpSetNotes(ChannelNo::DEFAULT, notes, 1)],
            &mut buf2,
            1,
        );
        assert!(
            buf2.iter().any(|s| s.abs() > 1e-6),
            "arp with 1 note loaded should produce signal"
        );
    }

    #[cfg(feature = "arp")]
    #[test]
    fn arp_disabled_after_enable_allows_direct_notes() {
        let mut proc = make();
        let notes = [MidiNote::MIDDLE_C; 4];
        // Phase 1: enable arp with 1 note - arp drives audio without any NoteOn event
        let setup = vec![
            AudioEvent::ArpEnabled(ChannelNo::DEFAULT, true),
            AudioEvent::ArpSetNotes(ChannelNo::DEFAULT, notes, 1),
        ];
        let mut buf = vec![0.0_f32; 8192];
        proc.process_block(&setup, &mut buf, 1);
        assert!(
            buf.iter().any(|s| s.abs() > 1e-6),
            "arp-driven audio should appear without a direct NoteOn"
        );
        // Phase 2: disable arp and panic to clear any ringing voices
        let mut silence_buf = vec![0.0_f32; 256];
        proc.process_block(
            &[
                AudioEvent::ArpEnabled(ChannelNo::DEFAULT, false),
                AudioEvent::Panic,
            ],
            &mut silence_buf,
            1,
        );
        // Phase 3: direct NoteOn now works
        let mut buf2 = vec![0.0_f32; 4096];
        proc.process_block(&[AudioEvent::NoteOn(MidiNote::MIDDLE_C)], &mut buf2, 1);
        assert!(
            buf2.iter().any(|s| s.abs() > 1e-6),
            "direct NoteOn after arp disable should produce signal"
        );
    }

    #[cfg(feature = "arp")]
    #[test]
    fn arp_panic_leaves_processor_in_finite_state() {
        let mut proc = make();
        let notes = [MidiNote::MIDDLE_C; 4];
        let setup = vec![
            AudioEvent::ArpSetNotes(ChannelNo::DEFAULT, notes, 1),
            AudioEvent::ArpEnabled(ChannelNo::DEFAULT, true),
        ];
        let mut buf = vec![0.0_f32; 4096];
        proc.process_block(&setup, &mut buf, 1);
        proc.process_block(&[AudioEvent::Panic], &mut buf, 1);
        let mut after = vec![0.0_f32; 4096];
        proc.process_block(&[], &mut after, 1);
        assert!(
            after.iter().all(|s| s.is_finite()),
            "non-finite output after arp panic"
        );
    }
}
