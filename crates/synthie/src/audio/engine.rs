//! CPAL audio stream setup and the real-time audio callback.
//!
//! `setup_audio` allocates all DSP state, opens the default output device, and
//! returns the live `cpal::Stream` together with the channels used to exchange
//! events and scope data with the UI thread.

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, bounded};

use crate::audio::chorus::Chorus;
use crate::audio::delay::Delay;
use crate::audio::drums::DrumMachine;
use crate::audio::fx::Reverb;
use crate::audio::voice::Voice;
use crate::params::{AudioEvent, ChannelNo, MidiNote, SynthParams};

/// Capacity of the scope channel (number of `Vec<f32>` batches that can be
/// queued before the audio thread starts dropping them).
const SCOPE_CHANNEL_CAPACITY: usize = 32;

/// Send every Nth sample to the scope to reduce channel traffic.
const SCOPE_DECIMATION: usize = 4;

/// Number of decimated samples accumulated before flushing to the scope channel.
const SCOPE_BATCH: usize = 128;

/// Number of simultaneous voices per synthesis channel.
const POLYPHONY: usize = 4;

/// Polyphony as `f32` for scaling the summed voice mix.
const POLYPHONY_F32: f32 = 4.0;

/// Number of independent synthesis channels (each has its own voice pool and params).
pub const NUM_CHANNELS: usize = 2;

/// Master output gain applied after summing all synthesis channels.
///
/// At −6 dB this gives two full-volume channels headroom to sum cleanly
/// before the final hard clamp.  Per-channel loudness is controlled via
/// `SynthParams::global::volume`; this constant is a fixed headroom budget,
/// not a per-channel normaliser.
const MASTER_GAIN: f32 = 0.5;

/// Voice slot metadata for note routing and age-based stealing.
#[derive(Clone, Copy, Default)]
struct VoiceSlot {
    note: Option<MidiNote>,
    age: u64,
}

/// One independent synthesis channel: a voice pool with its own parameter snapshot.
///
/// Channel 0 is the default target of the channel-less `NoteOn` / `LoadPatch`
/// events and also drives the shared reverb tail.
struct AudioChannel {
    /// Current synthesiser parameter snapshot for this channel.
    params: SynthParams,
    /// Polyphonic voice pool.
    voices: [Voice; POLYPHONY],
    /// Per-voice metadata for note routing and stealing.
    slots: [VoiceSlot; POLYPHONY],
    /// Monotonic allocation counter for oldest-voice stealing.
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

    /// Render one sample: sum all voices and normalise by polyphony count.
    fn process(&mut self, sample_rate: f32) -> f32 {
        self.voices
            .iter_mut()
            .map(|v| v.process(&self.params, sample_rate))
            .sum::<f32>()
            / POLYPHONY_F32
    }
}

/// All mutable state owned exclusively by the audio callback.
///
/// No fields are shared with the UI thread; synchronisation happens only
/// through the bounded `event_rx` / `scope_tx` channels.
struct AudioState {
    /// Independent synthesis channels, each with its own voice pool and params.
    channels: [AudioChannel; NUM_CHANNELS],
    /// Shared post-mix reverb send (driven by channel 0's FX params).
    reverb: Reverb,
    /// Shared post-mix chorus (driven by channel 0's chorus params).
    chorus: Chorus,
    /// Shared post-mix delay (driven by channel 0's delay params).
    delay: Delay,
    /// Parallel drum machine (kick + hi-hats).
    drums: DrumMachine,
    /// Receives `AudioEvent` messages from the UI thread.
    event_rx: Receiver<AudioEvent>,
    /// Sends decimated waveform batches to the scope display.
    scope_tx: Sender<Vec<f32>>,
    /// Accumulates decimated samples before a batch flush.
    scope_accum: Vec<f32>,
    /// Counts samples between scope decimation steps.
    scope_dec_counter: usize,
    /// Audio sample rate in Hz.
    sample_rate: f32,
}

impl AudioState {
    /// Construct initial audio state for the given sample rate.
    fn new(sample_rate: f32, event_rx: Receiver<AudioEvent>, scope_tx: Sender<Vec<f32>>) -> Self {
        let channels: [AudioChannel; NUM_CHANNELS] = std::array::from_fn(|_| AudioChannel::new());
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
            event_rx,
            scope_tx,
            // Pre-allocate to avoid heap allocation inside the callback.
            scope_accum: Vec::with_capacity(SCOPE_BATCH * 2),
            scope_dec_counter: 0,
            sample_rate,
        }
    }

    /// Sync reverb state from channel 0's FX params.
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

    /// Drain all pending events from the UI thread. Called once per buffer.
    fn drain_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                AudioEvent::NoteOn(midi) => self.note_on(ChannelNo::DEFAULT, midi),
                AudioEvent::NoteOff(midi) => self.note_off(ChannelNo::DEFAULT, midi),
                AudioEvent::Panic => self.panic(),
                AudioEvent::LoadPatch(p) => {
                    self.channels[0].params = *p;
                    self.apply_reverb_params();
                }
                AudioEvent::Drum(hit) => self.drums.trigger(hit),
                AudioEvent::NoteOnChannel(ch, midi) => self.note_on(ch, midi),
                AudioEvent::NoteOffChannel(ch, midi) => self.note_off(ch, midi),
                AudioEvent::LoadPatchChannel(ch, p) => {
                    if let Some(channel) = self.channels.get_mut(ch.as_usize()) {
                        channel.params = *p;
                    }
                    if ch == ChannelNo::DEFAULT {
                        self.apply_reverb_params();
                    }
                }
            }
        }
    }

    /// Render `hw_channels` interleaved output frames into `data`.
    fn process(&mut self, data: &mut [f32], hw_channels: usize) {
        self.drain_events();

        let sample_rate = self.sample_rate;
        let reverb_mix = self.channels[0].params.fx.reverb_mix;
        let chorus_params = self.channels[0].params.chorus.clone();
        let delay_params = self.channels[0].params.delay.clone();

        for frame in data.chunks_mut(hw_channels) {
            // Drums are part of the pre-FX mix: they pass through chorus, delay, and reverb.
            let mix: f32 = self
                .channels
                .iter_mut()
                .map(|ch| ch.process(sample_rate))
                .sum::<f32>()
                + self.drums.process(sample_rate);
            let mix = self.chorus.process(mix, &chorus_params);
            let mix = self.delay.process(mix, &delay_params);
            let sample = self.reverb.process(mix, reverb_mix) * MASTER_GAIN;

            // Guard against denormals / clipping before writing to hardware.
            let sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                std::hint::cold_path();
                0.0
            };

            for ch in frame.iter_mut() {
                *ch = sample;
            }

            // Push decimated sample to scope; drop batch if channel is full
            // (never block the audio thread).
            self.scope_dec_counter += 1;
            if self.scope_dec_counter >= SCOPE_DECIMATION {
                self.scope_dec_counter = 0;
                self.scope_accum.push(sample);
                if self.scope_accum.len() >= SCOPE_BATCH {
                    let batch = std::mem::replace(
                        &mut self.scope_accum,
                        Vec::with_capacity(SCOPE_BATCH * 2),
                    );
                    let _ = self.scope_tx.try_send(batch);
                }
            }
        }
    }
}

/// Initialise CPAL audio output.
///
/// Returns:
/// * `cpal::Stream` – must be kept alive for the duration of the program.
/// * `Sender<AudioEvent>` – send note on/off and param changes from the UI thread.
/// * `Receiver<Vec<f32>>` – scope samples for waveform display.
///
/// # Errors
///
/// Returns an error if no default audio output device is available or if the
/// device's stream configuration cannot be determined or opened.
pub fn setup_audio() -> Result<(cpal::Stream, Sender<AudioEvent>, Receiver<Vec<f32>>)> {
    let (event_tx, event_rx) = bounded::<AudioEvent>(1024);
    let (scope_tx, scope_rx) = bounded::<Vec<f32>>(SCOPE_CHANNEL_CAPACITY);

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("no default audio output device")?;

    let config = device
        .default_output_config()
        .context("failed to query default output config")?;

    #[allow(clippy::cast_precision_loss)]
    // sample rate fits within f32 for all practical audio rates
    let sample_rate = config.sample_rate() as f32;
    let hw_channels = config.channels() as usize;

    // Convert from the device's native format config to a plain StreamConfig.
    let stream_config: cpal::StreamConfig = config.into();

    let mut audio_state = AudioState::new(sample_rate, event_rx, scope_tx);

    let stream = device
        .build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                audio_state.process(data, hw_channels);
            },
            |err| eprintln!("audio stream error: {err}"),
            None,
        )
        .context("failed to build output stream")?;

    stream.play().context("failed to start audio stream")?;

    Ok((stream, event_tx, scope_rx))
}
