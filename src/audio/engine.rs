//! CPAL audio stream setup and the real-time audio callback.
//!
//! `setup_audio` allocates all DSP state, opens the default output device, and
//! returns the live `cpal::Stream` together with the channels used to exchange
//! events and scope data with the UI thread.

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{bounded, Receiver, Sender};

use crate::audio::voice::{NoteStack, Voice};
use crate::params::{AudioEvent, SynthParams};

/// Capacity of the scope channel (number of `Vec<f32>` batches that can be
/// queued before the audio thread starts dropping them).
const SCOPE_CHANNEL_CAPACITY: usize = 32;

/// Send every Nth sample to the scope to reduce channel traffic.
const SCOPE_DECIMATION: usize = 4;

/// Number of decimated samples accumulated before flushing to the scope channel.
const SCOPE_BATCH: usize = 128;

/// All mutable state owned exclusively by the audio callback.
///
/// No fields are shared with the UI thread; synchronisation happens only
/// through the bounded `event_rx` / `scope_tx` channels.
struct AudioState {
    /// Current synthesiser parameter snapshot.
    params: SynthParams,
    /// The single monophonic voice.
    voice: Voice,
    /// Tracks which MIDI notes are currently held (last-note priority).
    note_stack: NoteStack,
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
        let params = SynthParams::default();
        let voice = Voice::new();
        Self {
            params,
            voice,
            note_stack: NoteStack::new(),
            event_rx,
            scope_tx,
            // Pre-allocate to avoid heap allocation inside the callback.
            scope_accum: Vec::with_capacity(SCOPE_BATCH * 2),
            scope_dec_counter: 0,
            sample_rate,
        }
    }

    /// Drain all pending events from the UI thread.  Called once per buffer.
    fn drain_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                AudioEvent::NoteOn(midi) => {
                    self.note_stack.press(midi);
                    self.voice.note_on(midi, &self.params, self.sample_rate);
                }
                AudioEvent::NoteOff(midi) => {
                    if let Some(next) = self.note_stack.release(midi) {
                        // There is still a held note → legato retrigger.
                        self.voice.note_on(next, &self.params, self.sample_rate);
                    } else {
                        self.voice.note_off();
                    }
                }
                AudioEvent::Panic => {
                    self.note_stack.clear();
                    self.voice.panic();
                }
                AudioEvent::LoadPatch(p) => {
                    self.params = *p;
                    // Apply reverb params immediately without waiting for a note event.
                    self.voice
                        .reverb
                        .set_params(self.params.reverb_size, self.params.reverb_damping);
                }
            }
        }
    }

    /// Render `channels` interleaved output frames into `data`.
    fn process(&mut self, data: &mut [f32], channels: usize) {
        self.drain_events();

        for frame in data.chunks_mut(channels) {
            let sample = self.voice.process(&self.params, self.sample_rate);

            // Guard against denormals / clipping before writing to hardware.
            let sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
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

    let sample_rate = config.sample_rate().0 as f32;
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
