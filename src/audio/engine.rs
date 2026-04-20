//! CPAL audio stream setup and the real-time audio callback.
//!
//! `setup_audio` allocates all DSP state, opens the default output device, and
//! returns the live `cpal::Stream` together with the channels used to exchange
//! events and scope data with the UI thread.

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, bounded};
use std::f32::consts::FRAC_PI_4;

use crate::audio::drums::DrumMachine;
use crate::audio::fx::Reverb;
use crate::audio::voice::Voice;
use crate::params::{AudioEvent, SynthParams};

/// Capacity of the scope channel (number of `Vec<f32>` batches that can be
/// queued before the audio thread starts dropping them).
const SCOPE_CHANNEL_CAPACITY: usize = 32;

/// Send every Nth sample to the scope to reduce channel traffic.
const SCOPE_DECIMATION: usize = 4;

/// Number of decimated samples accumulated before flushing to the scope channel.
const SCOPE_BATCH: usize = 128;

/// Number of simultaneous voices.
const POLYPHONY: usize = 4;

/// Polyphony as `f32` for scaling the summed voice mix.
const POLYPHONY_F32: f32 = 4.0;

/// Voice slot metadata for note routing and age-based stealing.
#[derive(Clone, Copy, Default)]
struct VoiceSlot {
    note: Option<u8>,
    age: u64,
    pan: f32,
}

/// All mutable state owned exclusively by the audio callback.
///
/// No fields are shared with the UI thread; synchronisation happens only
/// through the bounded `event_rx` / `scope_tx` channels.
struct AudioState {
    /// Current synthesiser parameter snapshot.
    params: SynthParams,
    /// Polyphonic voice pool.
    voices: [Voice; POLYPHONY],
    /// Per-voice metadata for note routing and stealing.
    slots: [VoiceSlot; POLYPHONY],
    /// Monotonic allocation counter for oldest-voice stealing.
    age_counter: u64,
    /// Shared post-mix reverb send.
    reverb: Reverb,
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

fn pan_gains(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    debug_assert!((-1.0..=1.0).contains(&pan));
    let angle = (pan + 1.0) * FRAC_PI_4;
    (angle.cos(), angle.sin())
}

impl AudioState {
    /// Construct initial audio state for the given sample rate.
    fn new(sample_rate: f32, event_rx: Receiver<AudioEvent>, scope_tx: Sender<Vec<f32>>) -> Self {
        let params = SynthParams::default();
        let mut reverb = Reverb::new();
        reverb.set_params(params.fx.reverb_size, params.fx.reverb_damping);
        Self {
            params,
            voices: std::array::from_fn(|_| Voice::new()),
            slots: std::array::from_fn(|_| VoiceSlot::default()),
            age_counter: 0,
            reverb,
            drums: DrumMachine::new(sample_rate),
            event_rx,
            scope_tx,
            // Pre-allocate to avoid heap allocation inside the callback.
            scope_accum: Vec::with_capacity(SCOPE_BATCH * 2),
            scope_dec_counter: 0,
            sample_rate,
        }
    }

    fn apply_reverb_params(&mut self) {
        self.reverb
            .set_params(self.params.fx.reverb_size, self.params.fx.reverb_damping);
    }

    fn is_voice_idle(&self, idx: usize) -> bool {
        let voice = &self.voices[idx];
        !voice.active && !voice.env.is_active() && self.slots[idx].note.is_none()
    }

    fn allocate_voice_index(&self, midi: u8) -> usize {
        if let Some(idx) = self.slots.iter().position(|slot| slot.note == Some(midi)) {
            return idx;
        }

        if let Some(idx) = (0..POLYPHONY).find(|&idx| self.is_voice_idle(idx)) {
            return idx;
        }

        self.slots
            .iter()
            .enumerate()
            .min_by_key(|(_, slot)| slot.age)
            .map_or(0, |(idx, _)| idx)
    }

    fn note_on(&mut self, midi: u8, pan: f32) {
        self.apply_reverb_params();
        let idx = self.allocate_voice_index(midi);
        self.age_counter = self.age_counter.saturating_add(1);
        self.slots[idx].note = Some(midi);
        self.slots[idx].age = self.age_counter;
        self.slots[idx].pan = pan.clamp(-1.0, 1.0);
        self.voices[idx].note_on(midi, &self.params, self.sample_rate);
    }

    fn note_off(&mut self, midi: u8) {
        if let Some(idx) = self.slots.iter().position(|slot| slot.note == Some(midi)) {
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
        self.drums.panic();
        self.age_counter = 0;
    }

    /// Drain all pending events from the UI thread. Called once per buffer.
    fn drain_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                AudioEvent::NoteOn { midi, pan } => self.note_on(midi, pan),
                AudioEvent::NoteOff(midi) => self.note_off(midi),
                AudioEvent::Panic => self.panic(),
                AudioEvent::LoadPatch(p) => {
                    self.params = *p;
                    self.apply_reverb_params();
                }
                AudioEvent::Drum { hit, pan } => self.drums.trigger(hit, pan),
            }
        }
    }

    /// Render `channels` interleaved output frames into `data`.
    fn process(&mut self, data: &mut [f32], channels: usize) {
        self.drain_events();

        for frame in data.chunks_mut(channels) {
            let mut left = 0.0;
            let mut right = 0.0;
            let mut voice_mono = 0.0;

            for (voice, slot) in self.voices.iter_mut().zip(self.slots.iter()) {
                let sample = voice.process(&self.params, self.sample_rate) / POLYPHONY_F32;
                voice_mono += sample;
                let (l_gain, r_gain) = pan_gains(slot.pan);
                left += sample * l_gain;
                right += sample * r_gain;
            }

            let reverb_wet = self.reverb.process(voice_mono, self.params.fx.reverb_mix);
            left += reverb_wet;
            right += reverb_wet;

            let (kick, hats) = self.drums.process_components(self.sample_rate);
            let (kick_left_gain, kick_right_gain) = pan_gains(kick.1);
            left += kick.0 * kick_left_gain;
            right += kick.0 * kick_right_gain;

            let (hat_left_gain, hat_right_gain) = pan_gains(hats.1);
            left += hats.0 * hat_left_gain;
            right += hats.0 * hat_right_gain;

            let left = if left.is_finite() {
                left.clamp(-1.0, 1.0)
            } else {
                std::hint::cold_path();
                0.0
            };
            let right = if right.is_finite() {
                right.clamp(-1.0, 1.0)
            } else {
                std::hint::cold_path();
                0.0
            };
            let mono = (left + right) * 0.5;

            match channels {
                0 => {}
                1 => {
                    frame[0] = mono;
                }
                _ => {
                    frame[0] = left;
                    frame[1] = right;
                    for ch in &mut frame[2..] {
                        *ch = mono;
                    }
                }
            }

            // Push decimated sample to scope; drop batch if channel is full
            // (never block the audio thread).
            self.scope_dec_counter += 1;
            if self.scope_dec_counter >= SCOPE_DECIMATION {
                self.scope_dec_counter = 0;
                self.scope_accum.push(mono);
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
    let channels = config.channels() as usize;

    // Convert from the device's native format config to a plain StreamConfig.
    let stream_config: cpal::StreamConfig = config.into();

    let mut audio_state = AudioState::new(sample_rate, event_rx, scope_tx);

    let stream = device
        .build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                audio_state.process(data, channels);
            },
            |err| eprintln!("audio stream error: {err}"),
            None,
        )
        .context("failed to build output stream")?;

    stream.play().context("failed to start audio stream")?;

    Ok((stream, event_tx, scope_rx))
}
