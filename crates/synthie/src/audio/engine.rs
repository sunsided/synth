//! CPAL audio stream setup — thin I/O wrapper around [`SynthProcessor`].
//!
//! [`setup_audio`] / [`setup_audio_n`] open the default output device and wire a
//! crossbeam event channel into the real-time callback, which drives
//! [`SynthProcessor::process_block`]. Scope samples are decimated and forwarded
//! to the UI thread via a second channel.
//!
//! TUI applications should prefer [`setup_audio_silenced`], which uses a larger
//! buffer to reduce underruns and suppresses stream-error output that would
//! otherwise corrupt the alternate screen.

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender, bounded};

use crate::audio::processor::SynthProcessor;
use crate::params::AudioEvent;

pub use crate::audio::processor::NUM_CHANNELS;

/// Capacity of the scope channel (batches of `Vec<f32>` that can be queued
/// before the audio thread starts dropping them).
const SCOPE_CHANNEL_CAPACITY: usize = 32;

/// Capacity of the event channel and the per-callback drain buffer.
/// Pre-sizing the drain buffer to this value avoids heap allocation on the
/// audio thread even when a full burst of events arrives in one callback.
const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// Send every Nth sample to the scope to reduce channel traffic.
const SCOPE_DECIMATION: usize = 4;

/// Number of decimated samples accumulated before flushing to the scope channel.
const SCOPE_BATCH: usize = 128;

/// Initialise CPAL audio output with a configurable channel count, buffer size,
/// and stream-error handler.
///
/// `N` controls how many independent voice pools are allocated.  Use
/// [`ChannelNo`] values `0..N` with the channel-specific [`AudioEvent`] variants
/// to address individual channels.  `N = 0` is rejected at compile time.
///
/// `buffer_size` is passed directly to CPAL; use [`cpal::BufferSize::Default`]
/// for driver-chosen sizing or [`cpal::BufferSize::Fixed`] to request a specific
/// frame count.  Larger buffers reduce underruns at the cost of latency.
///
/// `on_error` is called from the audio thread when CPAL reports a stream error.
/// In TUI applications pass `|_| {}` to prevent messages from corrupting the
/// alternate screen; see also [`setup_audio_silenced`].
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
pub fn setup_audio_n_with_error_handler<const N: usize, F>(
    buffer_size: cpal::BufferSize,
    on_error: F,
) -> Result<(cpal::Stream, Sender<AudioEvent>, Receiver<Vec<f32>>)>
where
    F: Fn(cpal::StreamError) + Send + 'static,
{
    let (event_tx, event_rx) = bounded::<AudioEvent>(EVENT_CHANNEL_CAPACITY);
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

    let stream_config = cpal::StreamConfig {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        buffer_size,
    };

    let mut processor = SynthProcessor::<N>::new(sample_rate);
    let mut events_buf: Vec<AudioEvent> = Vec::with_capacity(EVENT_CHANNEL_CAPACITY);
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
            on_error,
            None,
        )
        .context("failed to build output stream")?;

    stream.play().context("failed to start audio stream")?;

    Ok((stream, event_tx, scope_rx))
}

/// Initialise CPAL audio output with a configurable number of synthesis channels.
///
/// Convenience wrapper around [`setup_audio_n_with_error_handler`] that uses the
/// driver-default buffer size and prints stream errors to stderr.  For a custom
/// buffer size or error handler, call [`setup_audio_n_with_error_handler`] directly.
///
/// # Errors
///
/// Returns an error if no default audio output device is available or if the
/// stream configuration cannot be determined or opened.
pub fn setup_audio_n<const N: usize>()
-> Result<(cpal::Stream, Sender<AudioEvent>, Receiver<Vec<f32>>)> {
    setup_audio_n_with_error_handler::<N, _>(cpal::BufferSize::Default, |err| {
        eprintln!("audio stream error: {err}");
    })
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

/// Initialise CPAL audio output for TUI applications.
///
/// Like [`setup_audio`] but uses `Fixed(1024)` (~23 ms at 44.1 kHz) so that
/// note events queued from a sequencer thread are picked up within one callback
/// period, keeping step timing tight.  Stream-error messages are silenced so
/// they do not corrupt the alternate screen.
///
/// For error visibility (e.g. to detect when the stream stops), use
/// [`setup_audio_silenced_with_error_handler`] instead.
///
/// # Errors
///
/// Returns an error if no default audio output device is available or if the
/// stream configuration cannot be determined or opened.
pub fn setup_audio_silenced() -> Result<(cpal::Stream, Sender<AudioEvent>, Receiver<Vec<f32>>)> {
    setup_audio_silenced_with_error_handler(|_| {})
}

/// Like [`setup_audio_silenced`] but calls `on_error` when CPAL reports a
/// fatal stream error.
///
/// Use this in applications that need to detect and surface audio stream
/// failure — for example, by setting an `AtomicBool` that the UI thread
/// polls to show an error indicator.
///
/// # Errors
///
/// Returns an error if no default audio output device is available or if the
/// stream configuration cannot be determined or opened.
pub fn setup_audio_silenced_with_error_handler<F>(
    on_error: F,
) -> Result<(cpal::Stream, Sender<AudioEvent>, Receiver<Vec<f32>>)>
where
    F: Fn(cpal::StreamError) + Send + 'static,
{
    // Fixed(4096) (~93 ms at 44.1 kHz) gives the DSP enough headroom to avoid
    // xruns.  Smaller values (512/1024) cause buffer underruns on typical desktop
    // audio stacks; Default lets the driver pick its own quantum which can be
    // even smaller.  At 140 BPM the step period (107 ms) exceeds the buffer
    // period so note events still land in separate callbacks without bunching.
    setup_audio_n_with_error_handler::<NUM_CHANNELS, _>(cpal::BufferSize::Fixed(4096), on_error)
}
