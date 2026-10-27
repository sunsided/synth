//! Rolling waveform scope buffer.
//!
//! The audio thread sends batches of decimated `f32` samples over a bounded
//! channel.  The UI calls [`ScopeBuf::update`] each frame to drain any pending
//! batches, then reads [`ScopeBuf::as_chart_data`] to feed the ratatui chart
//! and [`ScopeBuf::level`] to drive a peak meter.

use crossbeam_channel::Receiver;
use std::collections::VecDeque;

/// Per-frame multiplier applied to the peak level when no louder sample
/// arrives; ≈ 20 dB/s fall-off at 60 fps, like a hardware PPM.
const PEAK_DECAY: f32 = 0.93;

/// Rolling waveform scope buffer.
///
/// The audio thread sends batches of decimated samples via a bounded channel.
/// The UI calls `update()` each frame to drain new batches, then reads
/// `samples()` or `as_chart_data()` for display.
pub struct ScopeBuf {
    /// Channel receiver for incoming sample batches from the audio thread.
    rx: Receiver<Vec<f32>>,
    /// Ring buffer holding the most recent `capacity` samples.
    buf: VecDeque<f32>,
    /// Maximum number of samples retained in the ring buffer.
    capacity: usize,
    /// Smoothed absolute peak in 0‥1 (instant attack, exponential release).
    level: f32,
}

impl ScopeBuf {
    /// Create a new scope buffer with the given channel receiver and sample capacity.
    #[must_use]
    pub fn new(rx: Receiver<Vec<f32>>, capacity: usize) -> Self {
        Self {
            rx,
            buf: VecDeque::with_capacity(capacity),
            capacity,
            level: 0.0,
        }
    }

    /// Drain all pending batches from the channel into the rolling buffer and
    /// advance the peak meter by one frame.
    ///
    /// Should be called once per UI frame before reading scope data.
    pub fn update(&mut self) {
        let mut peak = 0.0_f32;
        while let Ok(batch) = self.rx.try_recv() {
            for s in batch {
                peak = peak.max(s.abs());
                if self.buf.len() >= self.capacity {
                    self.buf.pop_front();
                }
                self.buf.push_back(s);
            }
        }
        self.level = peak.max(self.level * PEAK_DECAY).min(1.0);
    }

    /// Iterate over the current waveform data (oldest → newest).
    pub fn samples(&self) -> impl Iterator<Item = f32> + '_ {
        self.buf.iter().copied()
    }

    /// Return waveform data as `(x, y)` pairs suitable for a ratatui `Dataset`.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // scope index fits within f64 mantissa for display sizes
    pub fn as_chart_data(&self) -> Vec<(f64, f64)> {
        self.buf
            .iter()
            .enumerate()
            .map(|(i, &s)| (i as f64, f64::from(s)))
            .collect()
    }

    /// Smoothed absolute peak level in 0‥1.
    #[must_use]
    pub fn level(&self) -> f32 {
        self.level
    }

    /// Number of samples currently in the buffer.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Returns `true` if the buffer contains no samples.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}
