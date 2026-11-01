//! synthie as a [kira](https://crates.io/crates/kira) sound source.
//!
//! Games that already play their audio through kira can't open a second
//! output stream for the synth: on the web and on some platforms two
//! backends fight over the device, and kira's mixer, tracks, effects,
//! volume and pause would not apply to the synth. With the `kira` feature,
//! synthie plays *inside* kira instead:
//!
//! ```no_run
//! use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Tween};
//! use synthie::kira::SynthSoundData;
//! use synthie::params::{AudioEvent, DrumHit};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())?;
//! let mut synth = manager.play(SynthSoundData::new())?;
//! synth.send(AudioEvent::Drum(DrumHit::Kick))?;
//! synth.set_volume(-6.0, Tween::default());
//! # Ok(()) }
//! ```
//!
//! [`SynthSoundData`] is the kira [`SoundData`]: the patches, the processor
//! settings and an optional [`Sequencer`]. Playing it returns a
//! [`SynthHandle`] for the game thread, which queues [`AudioEvent`]s through
//! a lock-free ring buffer and controls volume, pause and stop like kira's
//! own sound handles.
//!
//! # Timing
//!
//! Events sent through the handle take effect at the start of the next
//! render block kira asks for. Anything rhythmic belongs in the sequencer
//! ([`SynthSoundData::with_sequencer`]): its events land on the exact frame
//! it asks for, whatever block size kira renders with.
//!
//! # Real-time safety
//!
//! kira does not tell sounds the sample rate up front, so the processor is
//! built on the audio thread on the first render (and rebuilt if the sample
//! rate changes), which allocates once. After that the sound only allocates
//! when a sequencer tick pushes more than
//! [`SEQUENCER_EVENT_CAPACITY`](crate::audio::sequencer::SEQUENCER_EVENT_CAPACITY)
//! events, and drops the boxed patch of a `LoadPatch` event once applied.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use ::kira::command::{
    CommandReader, CommandWriter, ValueChangeCommand, command_writer_and_reader,
};
use ::kira::info::Info;
use ::kira::sound::{PlaybackState, Sound, SoundData};
use ::kira::{Decibels, Frame, Parameter, Tween, Value};

use crate::audio::processor::{NUM_CHANNELS, SynthProcessor, VoiceNormalization};
use crate::audio::sequencer::{SequencedProcessor, Sequencer};
use crate::params::{AudioEvent, ChannelNo, SynthParams};

/// Default capacity of the event queue from a [`SynthHandle`] to the sound.
pub const DEFAULT_EVENT_CAPACITY: usize = 1024;

/// Frames rendered per processor call; kira's blocks are split into these.
const CHUNK: usize = 256;

/// A synth ready to be played by a kira `AudioManager`.
///
/// `N` is the number of synthesis channels, `S` an optional [`Sequencer`].
pub struct SynthSoundData<S = (), const N: usize = NUM_CHANNELS> {
    patches: Vec<SynthParams>,
    sequencer: S,
    volume: Value<Decibels>,
    master_gain: Option<f32>,
    voice_normalization: VoiceNormalization,
    event_capacity: usize,
}

impl SynthSoundData {
    /// A synth with [`NUM_CHANNELS`] channels on the default patch and no
    /// sequencer.
    #[must_use]
    pub fn new() -> Self {
        Self::with_channels()
    }
}

impl Default for SynthSoundData {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> SynthSoundData<(), N> {
    /// A synth with `N` channels on the default patch and no sequencer.
    #[must_use]
    pub fn with_channels() -> Self {
        Self {
            patches: Vec::new(),
            sequencer: (),
            volume: Value::Fixed(Decibels::IDENTITY),
            master_gain: None,
            voice_normalization: VoiceNormalization::default(),
            event_capacity: DEFAULT_EVENT_CAPACITY,
        }
    }
}

impl<S, const N: usize> SynthSoundData<S, N> {
    /// Start channel `i` on `patches[i]`; extra patches are ignored and
    /// channels without one keep the default patch.
    #[must_use]
    pub fn with_patches(mut self, patches: impl IntoIterator<Item = SynthParams>) -> Self {
        self.patches = patches.into_iter().take(N).collect();
        self
    }

    /// Drive the synth from `sequencer`, sample-accurately.
    #[must_use]
    pub fn with_sequencer<T: Sequencer + Send + 'static>(
        self,
        sequencer: T,
    ) -> SynthSoundData<T, N> {
        SynthSoundData {
            patches: self.patches,
            sequencer,
            volume: self.volume,
            master_gain: self.master_gain,
            voice_normalization: self.voice_normalization,
            event_capacity: self.event_capacity,
        }
    }

    /// Start at `volume` (default: 0 dB). Change it later with
    /// [`SynthHandle::set_volume`].
    #[must_use]
    pub fn with_volume(mut self, volume: impl Into<Value<Decibels>>) -> Self {
        self.volume = volume.into();
        self
    }

    /// See [`SynthProcessor::set_master_gain`].
    #[must_use]
    pub fn with_master_gain(mut self, gain: f32) -> Self {
        self.master_gain = Some(gain);
        self
    }

    /// See [`SynthProcessor::set_voice_normalization`].
    #[must_use]
    pub fn with_voice_normalization(mut self, normalization: VoiceNormalization) -> Self {
        self.voice_normalization = normalization;
        self
    }

    /// How many events the handle can queue between two render blocks
    /// (default: [`DEFAULT_EVENT_CAPACITY`]).
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is zero.
    #[must_use]
    pub fn with_event_capacity(mut self, capacity: usize) -> Self {
        assert!(capacity > 0, "event capacity must be > 0");
        self.event_capacity = capacity;
        self
    }
}

/// Errors from [`SynthSoundData::into_sound`]; it cannot fail.
pub type SynthSoundError = core::convert::Infallible;

impl<S: Sequencer + Send + 'static, const N: usize> SoundData for SynthSoundData<S, N> {
    type Error = SynthSoundError;
    type Handle = SynthHandle;

    fn into_sound(self) -> Result<(Box<dyn Sound>, Self::Handle), Self::Error> {
        let (events_tx, events_rx) = rtrb::RingBuffer::new(self.event_capacity);
        let (volume_tx, volume_rx) = command_writer_and_reader();
        let (playback_tx, playback_rx) = command_writer_and_reader();
        let shared = Arc::new(Shared {
            state: AtomicU8::new(encode_state(PlaybackState::Playing)),
            handle_dropped: AtomicBool::new(false),
            idle: AtomicBool::new(true),
        });

        let sound = SynthSound::<S, N> {
            synth: None,
            setup: Some(Setup {
                patches: self.patches,
                sequencer: self.sequencer,
            }),
            master_gain: self.master_gain,
            voice_normalization: self.voice_normalization,
            events: events_rx,
            drained: Vec::with_capacity(self.event_capacity),
            scratch: [0.0; CHUNK],
            volume: Parameter::new(self.volume, Decibels::IDENTITY),
            volume_rx,
            fade: Parameter::new(Value::Fixed(Decibels::IDENTITY), Decibels::IDENTITY),
            playback_rx,
            state: PlaybackState::Playing,
            shared: Arc::clone(&shared),
        };
        let handle = SynthHandle {
            events: events_tx,
            volume: volume_tx,
            playback: playback_tx,
            shared,
        };
        Ok((Box::new(sound), handle))
    }
}

/// State shared between the handle and the sound.
#[derive(Debug)]
struct Shared {
    state: AtomicU8,
    handle_dropped: AtomicBool,
    idle: AtomicBool,
}

fn encode_state(state: PlaybackState) -> u8 {
    match state {
        PlaybackState::Playing => 0,
        PlaybackState::Pausing => 1,
        PlaybackState::Paused => 2,
        PlaybackState::WaitingToResume => 3,
        PlaybackState::Resuming => 4,
        PlaybackState::Stopping => 5,
        PlaybackState::Stopped => 6,
    }
}

fn decode_state(state: u8) -> PlaybackState {
    match state {
        0 => PlaybackState::Playing,
        1 => PlaybackState::Pausing,
        2 => PlaybackState::Paused,
        3 => PlaybackState::WaitingToResume,
        4 => PlaybackState::Resuming,
        5 => PlaybackState::Stopping,
        _ => PlaybackState::Stopped,
    }
}

/// A pause, resume or stop request.
#[derive(Debug, Clone, Copy)]
enum PlaybackCommand {
    Pause(Tween),
    Resume(Tween),
    Stop(Tween),
}

/// Controls a playing synth from the game thread.
///
/// Dropping the handle stops the sound, like kira's handles for endless
/// sounds.
#[derive(Debug)]
pub struct SynthHandle {
    events: rtrb::Producer<AudioEvent>,
    volume: CommandWriter<ValueChangeCommand<Decibels>>,
    playback: CommandWriter<PlaybackCommand>,
    shared: Arc<Shared>,
}

/// The event queue to the audio thread is full; the event comes back.
#[derive(Debug)]
pub struct QueueFull(pub AudioEvent);

impl core::fmt::Display for QueueFull {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("synth event queue is full")
    }
}

impl core::error::Error for QueueFull {}

impl SynthHandle {
    /// Queue `event`; it takes effect at the start of the next render
    /// block. While the synth is paused, events wait in the queue.
    ///
    /// # Errors
    ///
    /// Fails with the event when the queue is full, e.g. because the synth
    /// is paused or the audio thread has stalled.
    pub fn send(&mut self, event: AudioEvent) -> Result<(), QueueFull> {
        self.events
            .push(event)
            .map_err(|rtrb::PushError::Full(event)| QueueFull(event))
    }

    /// Note on for `note` (MIDI) on `channel`.
    ///
    /// # Errors
    ///
    /// See [`send`](Self::send).
    pub fn note_on(
        &mut self,
        channel: ChannelNo,
        note: crate::params::MidiNote,
    ) -> Result<(), QueueFull> {
        self.send(AudioEvent::NoteOnChannel(channel, note))
    }

    /// Note off for `note` (MIDI) on `channel`.
    ///
    /// # Errors
    ///
    /// See [`send`](Self::send).
    pub fn note_off(
        &mut self,
        channel: ChannelNo,
        note: crate::params::MidiNote,
    ) -> Result<(), QueueFull> {
        self.send(AudioEvent::NoteOffChannel(channel, note))
    }

    /// Fire a drum hit.
    ///
    /// # Errors
    ///
    /// See [`send`](Self::send).
    pub fn drum(&mut self, hit: crate::params::DrumHit) -> Result<(), QueueFull> {
        self.send(AudioEvent::Drum(hit))
    }

    /// Number of events that can be queued right now.
    #[must_use]
    pub fn free_slots(&self) -> usize {
        self.events.slots()
    }

    /// Set the synth's volume.
    pub fn set_volume(&mut self, volume: impl Into<Value<Decibels>>, tween: Tween) {
        self.volume.write(ValueChangeCommand {
            target: volume.into(),
            tween,
        });
    }

    /// Fade out and pause. The synth (voices, sequencer) stops advancing
    /// while paused.
    pub fn pause(&mut self, tween: Tween) {
        self.playback.write(PlaybackCommand::Pause(tween));
    }

    /// Resume a paused synth, fading in.
    pub fn resume(&mut self, tween: Tween) {
        self.playback.write(PlaybackCommand::Resume(tween));
    }

    /// Fade out and stop for good; kira then unloads the sound.
    pub fn stop(&mut self, tween: Tween) {
        self.playback.write(PlaybackCommand::Stop(tween));
    }

    /// The playback state as of the last render block.
    #[must_use]
    pub fn state(&self) -> PlaybackState {
        decode_state(self.shared.state.load(Ordering::Acquire))
    }

    /// Whether, as of the last render block, nothing would produce new
    /// sound (see [`SynthProcessor::is_idle`]). A running sequencer may
    /// still start notes.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.shared.idle.load(Ordering::Acquire)
    }
}

impl Drop for SynthHandle {
    fn drop(&mut self) {
        self.shared.handle_dropped.store(true, Ordering::Release);
    }
}

/// What the sound needs to build its processor once the sample rate is
/// known.
struct Setup<S> {
    patches: Vec<SynthParams>,
    sequencer: S,
}

/// The kira [`Sound`] that renders the synth.
struct SynthSound<S, const N: usize> {
    synth: Option<SequencedProcessor<N, S>>,
    setup: Option<Setup<S>>,
    master_gain: Option<f32>,
    voice_normalization: VoiceNormalization,
    events: rtrb::Consumer<AudioEvent>,
    drained: Vec<AudioEvent>,
    scratch: [f32; CHUNK],
    volume: Parameter<Decibels>,
    volume_rx: CommandReader<ValueChangeCommand<Decibels>>,
    /// Pause/resume/stop fade.
    fade: Parameter<Decibels>,
    playback_rx: CommandReader<PlaybackCommand>,
    state: PlaybackState,
    shared: Arc<Shared>,
}

impl<S: Sequencer + Send + 'static, const N: usize> SynthSound<S, N> {
    /// Build (or, after a sample-rate change, rebuild) the processor.
    fn synth_for(&mut self, sample_rate: f32) -> &mut SequencedProcessor<N, S> {
        let stale = self
            .synth
            .as_ref()
            .is_some_and(|s| (s.processor().sample_rate() - sample_rate).abs() > 0.5);
        if stale && let Some(old) = self.synth.take() {
            // Keep the patches and the sequencer; voices and FX tails do not
            // survive a sample-rate change.
            let (processor, sequencer) = old.into_parts();
            let patches = (0..N)
                .filter_map(|i| processor.params(channel(i)).cloned())
                .collect();
            self.setup = Some(Setup { patches, sequencer });
        }
        if let Some(setup) = self.setup.take() {
            let mut processor = SynthProcessor::<N>::new(sample_rate);
            if let Some(gain) = self.master_gain {
                processor.set_master_gain(gain);
            }
            processor.set_voice_normalization(self.voice_normalization);
            let loads: Vec<_> = setup
                .patches
                .into_iter()
                .enumerate()
                .map(|(i, p)| AudioEvent::LoadPatchChannel(channel(i), Box::new(p)))
                .collect();
            processor.process_block(&loads, &mut [], 1);
            self.synth = Some(SequencedProcessor::new(processor, setup.sequencer));
        }
        self.synth.as_mut().expect("synth was just built")
    }

    fn read_commands(&mut self) {
        self.volume.read_command(&mut self.volume_rx);
        if matches!(self.state, PlaybackState::Stopping | PlaybackState::Stopped) {
            return;
        }
        match self.playback_rx.read() {
            Some(PlaybackCommand::Pause(tween)) => {
                self.state = PlaybackState::Pausing;
                self.fade.set(Value::Fixed(Decibels::SILENCE), tween);
            }
            Some(PlaybackCommand::Resume(tween))
                if matches!(self.state, PlaybackState::Pausing | PlaybackState::Paused) =>
            {
                self.state = PlaybackState::Resuming;
                self.fade.set(Value::Fixed(Decibels::IDENTITY), tween);
            }
            Some(PlaybackCommand::Stop(tween)) => {
                self.state = PlaybackState::Stopping;
                self.fade.set(Value::Fixed(Decibels::SILENCE), tween);
            }
            _ => {}
        }
    }

    /// Advance the volume and fade by `dt` seconds and settle finished
    /// fades.
    fn update_parameters(&mut self, dt: f64, info: &Info) {
        self.volume.update(dt, info);
        if self.fade.update(dt, info) {
            self.state = match self.state {
                PlaybackState::Pausing => PlaybackState::Paused,
                PlaybackState::Stopping => PlaybackState::Stopped,
                PlaybackState::Resuming => PlaybackState::Playing,
                state => state,
            };
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "channel indices are < N, which is small"
)]
fn channel(i: usize) -> ChannelNo {
    ChannelNo(i as u8)
}

impl<S: Sequencer + Send + 'static, const N: usize> Sound for SynthSound<S, N> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn process(&mut self, out: &mut [Frame], dt: f64, info: &Info) {
        self.read_commands();
        if !self.state.is_advancing() {
            out.fill(Frame::ZERO);
            self.publish();
            return;
        }

        let prev_gain = (self.volume.value(), self.fade.value());
        self.update_parameters(dt * out.len() as f64, info);
        let gain = (self.volume.value(), self.fade.value());
        let len = out.len() as f32;

        self.drained.clear();
        while self.drained.len() < self.drained.capacity()
            && let Ok(event) = self.events.pop()
        {
            self.drained.push(event);
        }

        let sample_rate = (1.0 / dt) as f32;
        let mut events = core::mem::take(&mut self.drained);
        let mut scratch = self.scratch;
        let synth = self.synth_for(sample_rate);
        for (chunk_index, frames) in out.chunks_mut(CHUNK).enumerate() {
            let buf = &mut scratch[..frames.len()];
            let pending: &[AudioEvent] = if chunk_index == 0 { &events } else { &[] };
            synth.process_block(pending, buf, 1);
            for (i, (frame, &sample)) in frames.iter_mut().zip(buf.iter()).enumerate() {
                // Ramp the gain linearly across the block, like kira's sounds.
                let t = (chunk_index * CHUNK + i) as f32 / len;
                let volume = Decibels(lerp(prev_gain.0.0, gain.0.0, t)).as_amplitude();
                let fade = Decibels(lerp(prev_gain.1.0, gain.1.0, t)).as_amplitude();
                *frame = Frame::from_mono(sample * volume * fade);
            }
        }
        events.clear();
        self.drained = events;
        self.publish();
    }

    fn finished(&self) -> bool {
        self.state == PlaybackState::Stopped || self.shared.handle_dropped.load(Ordering::Acquire)
    }
}

impl<S, const N: usize> SynthSound<S, N> {
    fn publish(&self) {
        self.shared
            .state
            .store(encode_state(self.state), Ordering::Release);
        let idle = self.synth.as_ref().is_none_or(|s| s.processor().is_idle());
        self.shared.idle.store(idle, Ordering::Release);
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use ::kira::info::MockInfoBuilder;

    use super::*;
    use crate::params::{DrumHit, MidiNote};
    use crate::presets::sid::default_patches;

    const SR: f64 = 44_100.0;

    fn tween(ms: u64) -> Tween {
        Tween {
            duration: Duration::from_millis(ms),
            ..Tween::default()
        }
    }

    /// Render `frames` frames in kira blocks of `block` frames; returns the
    /// left channel and checks the right one matches.
    fn render(sound: &mut dyn Sound, frames: usize, block: usize) -> Vec<f32> {
        let info = MockInfoBuilder::new().build();
        let mut out = vec![Frame::ZERO; frames];
        for chunk in out.chunks_mut(block) {
            sound.process(chunk, 1.0 / SR, &info);
        }
        out.iter()
            .map(|f| {
                assert_eq!(f.left.to_bits(), f.right.to_bits());
                f.left
            })
            .collect()
    }

    /// A kick every 1 000 frames and a held note on channel 1.
    struct Beat(usize);

    impl Sequencer for Beat {
        fn tick(&mut self, _sample_rate: f32, out: &mut Vec<AudioEvent>) -> usize {
            if self.0 == 0 {
                out.push(AudioEvent::NoteOnChannel(ChannelNo(1), MidiNote(57)));
            }
            out.push(AudioEvent::Drum(DrumHit::ALL[self.0 % DrumHit::ALL.len()]));
            self.0 += 1;
            1_000
        }
    }

    fn patches() -> Vec<SynthParams> {
        default_patches().into_iter().map(|p| p.params).collect()
    }

    /// What the plain processor renders for the same setup.
    #[allow(clippy::cast_possible_truncation)]
    fn reference(frames: usize) -> Vec<f32> {
        let mut processor = SynthProcessor::<NUM_CHANNELS>::new(SR as f32);
        let loads: Vec<_> = patches()
            .into_iter()
            .take(NUM_CHANNELS)
            .enumerate()
            .map(|(i, p)| AudioEvent::LoadPatchChannel(channel(i), Box::new(p)))
            .collect();
        processor.process_block(&loads, &mut [], 1);
        let mut synth = SequencedProcessor::new(processor, Beat(0));
        let mut out = vec![0.0; frames];
        synth.process_block(&[], &mut out, 1);
        out
    }

    fn sequenced() -> (Box<dyn Sound>, SynthHandle) {
        SynthSoundData::new()
            .with_patches(patches())
            .with_sequencer(Beat(0))
            .into_sound()
            .unwrap()
    }

    fn bits(samples: &[f32]) -> Vec<u32> {
        samples.iter().map(|s| s.to_bits()).collect()
    }

    #[test]
    fn plays_the_sequencer_sample_accurately_in_any_block_size() {
        const FRAMES: usize = 20_000;
        let expected = reference(FRAMES);
        assert!(expected.iter().any(|s| s.abs() > 0.01));
        for block in [512, 1, 100, 1_000, 4_096] {
            let (mut sound, _handle) = sequenced();
            let out = render(sound.as_mut(), FRAMES, block);
            assert_eq!(bits(&out), bits(&expected), "kira block of {block}");
        }
    }

    #[test]
    fn handle_events_apply_at_the_next_block() {
        let (mut sound, mut handle) = SynthSoundData::new().into_sound().unwrap();
        assert!(render(sound.as_mut(), 512, 512).iter().all(|&s| s == 0.0));
        assert!(handle.is_idle());
        handle.drum(DrumHit::Snare).unwrap();
        let out = render(sound.as_mut(), 512, 512);
        assert!(
            out[0] != 0.0 || out[1] != 0.0,
            "snare missing at block start"
        );
        assert!(!handle.is_idle());
    }

    #[test]
    fn volume_scales_the_output() {
        const FRAMES: usize = 8_000;
        let (mut sound, _handle) = SynthSoundData::new()
            .with_patches(patches())
            .with_sequencer(Beat(0))
            .with_volume(Decibels(-6.0))
            .into_sound()
            .unwrap();
        let quiet = render(sound.as_mut(), FRAMES, 512);
        let gain = Decibels(-6.0).as_amplitude();
        for (q, r) in quiet.iter().zip(reference(FRAMES)) {
            assert!((q - r * gain).abs() < 1e-6, "{q} vs {}", r * gain);
        }
    }

    #[test]
    fn set_volume_tweens_to_silence() {
        let (mut sound, mut handle) = sequenced();
        render(sound.as_mut(), 2_048, 512);
        handle.set_volume(Decibels::SILENCE, tween(5));
        render(sound.as_mut(), 2_048, 512);
        assert!(render(sound.as_mut(), 2_048, 512).iter().all(|&s| s == 0.0));
    }

    #[test]
    fn pause_holds_events_and_resume_plays_on() {
        let (mut sound, mut handle) = SynthSoundData::new().into_sound().unwrap();
        handle.pause(tween(1));
        render(sound.as_mut(), 1_024, 512);
        assert_eq!(handle.state(), PlaybackState::Paused);

        handle.drum(DrumHit::Kick).unwrap();
        assert!(render(sound.as_mut(), 1_024, 512).iter().all(|&s| s == 0.0));
        assert!(handle.is_idle(), "events must wait while paused");

        handle.resume(tween(1));
        let out = render(sound.as_mut(), 1_024, 512);
        assert!(out.iter().any(|&s| s != 0.0), "kick after resume");
        render(sound.as_mut(), 512, 512);
        assert_eq!(handle.state(), PlaybackState::Playing);
        assert!(!sound.finished());
    }

    #[test]
    fn stop_finishes_the_sound_for_good() {
        let (mut sound, mut handle) = SynthSoundData::new().into_sound().unwrap();
        handle.stop(tween(50));
        render(sound.as_mut(), 512, 512);
        assert_eq!(handle.state(), PlaybackState::Stopping);
        assert!(!sound.finished(), "still fading out");
        render(sound.as_mut(), 4_096, 512);
        assert_eq!(handle.state(), PlaybackState::Stopped);
        assert!(sound.finished());
        handle.resume(tween(1));
        render(sound.as_mut(), 512, 512);
        assert_eq!(handle.state(), PlaybackState::Stopped);
    }

    #[test]
    fn dropping_the_handle_finishes_the_sound() {
        let (sound, handle) = SynthSoundData::new().into_sound().unwrap();
        assert!(!sound.finished());
        drop(handle);
        assert!(sound.finished());
    }

    #[test]
    fn a_full_queue_returns_the_event() {
        let (_sound, mut handle) = SynthSoundData::new()
            .with_event_capacity(2)
            .into_sound()
            .unwrap();
        handle.drum(DrumHit::Kick).unwrap();
        handle.drum(DrumHit::Kick).unwrap();
        assert_eq!(handle.free_slots(), 0);
        let QueueFull(event) = handle.drum(DrumHit::Clap).unwrap_err();
        assert!(matches!(event, AudioEvent::Drum(DrumHit::Clap)));
    }

    #[test]
    fn survives_a_sample_rate_change() {
        let (mut sound, mut handle) = sequenced();
        let info = MockInfoBuilder::new().build();
        let mut out = [Frame::ZERO; 512];
        sound.process(&mut out, 1.0 / 44_100.0, &info);
        handle.note_on(ChannelNo(0), MidiNote(60)).unwrap();
        for _ in 0..20 {
            sound.process(&mut out, 1.0 / 48_000.0, &info);
            assert!(out.iter().all(|f| f.left.is_finite()));
        }
        assert!(out.iter().any(|f| f.left != 0.0));
    }

    #[test]
    fn custom_channel_counts_work() {
        let (mut sound, mut handle) = SynthSoundData::<(), 8>::with_channels()
            .into_sound()
            .unwrap();
        handle.note_on(ChannelNo(7), MidiNote(60)).unwrap();
        assert!(render(sound.as_mut(), 2_048, 512).iter().any(|&s| s != 0.0));
    }
}
