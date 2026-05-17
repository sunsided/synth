//! CPAL audio stream setup — thin I/O wrapper around [`SynthProcessor`].
//!
//! `setup_audio` / `setup_audio_n` open the default output device and wire a
//! crossbeam event channel into the real-time callback, which drives
//! [`SynthProcessor::process_block`]. Scope samples are decimated and forwarded
//! to the UI thread via a second channel.

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, bounded};

use crate::audio::processor::SynthProcessor;
use crate::params::AudioEvent;

pub use crate::audio::processor::NUM_CHANNELS;

/// Capacity of the scope channel (batches of `Vec<f32>` that can be queued
/// before the audio thread starts dropping them).
const SCOPE_CHANNEL_CAPACITY: usize = 32;

/// Send every Nth sample to the scope to reduce channel traffic.
const SCOPE_DECIMATION: usize = 4;

/// Number of decimated samples accumulated before flushing to the scope channel.
const SCOPE_BATCH: usize = 128;

/// Initialise CPAL audio output with a configurable number of synthesis channels.
///
/// `N` controls how many independent voice pools are allocated.  Use
/// [`ChannelNo`] values `0..N` with the channel-specific [`AudioEvent`] variants
/// to address individual channels.  `N = 0` is rejected at compile time.
///
/// Returns:
/// * `cpal::Stream` – keep alive for the duration of the program.
/// * `Sender<AudioEvent>` – send note on/off and param changes from the UI thread.
/// * `Receiver<Vec<f32>>` – decimated waveform batches for scope display.
///
/// [`ChannelNo`]: crate::params::ChannelNo
///
/// # Errors
///
/// Returns an error if no default audio output device is available or if the
/// stream configuration cannot be determined or opened.
pub fn setup_audio_n<const N: usize>()
-> Result<(cpal::Stream, Sender<AudioEvent>, Receiver<Vec<f32>>)> {
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

    let stream_config: cpal::StreamConfig = config.into();

    let mut processor = SynthProcessor::<N>::new(sample_rate);
    let mut events_buf: Vec<AudioEvent> = Vec::with_capacity(64);
    let mut scope_accum: Vec<f32> = Vec::with_capacity(SCOPE_BATCH * 2);
    let mut scope_dec_counter: usize = 0;

    let stream = device
        .build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                events_buf.clear();
                while let Ok(event) = event_rx.try_recv() {
                    events_buf.push(event);
                }

                processor.process_block(&events_buf, data, hw_channels);

                // Forward decimated samples to the scope display.
                for frame in data.chunks(hw_channels) {
                    scope_dec_counter += 1;
                    if scope_dec_counter >= SCOPE_DECIMATION {
                        scope_dec_counter = 0;
                        if let Some(&sample) = frame.first() {
                            scope_accum.push(sample);
                        }
                        if scope_accum.len() >= SCOPE_BATCH {
                            let batch = std::mem::replace(
                                &mut scope_accum,
                                Vec::with_capacity(SCOPE_BATCH * 2),
                            );
                            let _ = scope_tx.try_send(batch);
                        }
                    }
                }
            },
            |err| eprintln!("audio stream error: {err}"),
            None,
        )
        .context("failed to build output stream")?;

    stream.play().context("failed to start audio stream")?;

    Ok((stream, event_tx, scope_rx))
}

/// Initialise CPAL audio output with [`NUM_CHANNELS`] synthesis channels.
///
/// Convenience wrapper around [`setup_audio_n`]. For a custom channel count
/// call `setup_audio_n::<N>()` directly.
///
/// # Errors
///
/// Returns an error if no default audio output device is available or if the
/// stream configuration cannot be determined or opened.
pub fn setup_audio() -> Result<(cpal::Stream, Sender<AudioEvent>, Receiver<Vec<f32>>)> {
    setup_audio_n::<NUM_CHANNELS>()
}
