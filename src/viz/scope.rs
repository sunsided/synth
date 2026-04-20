//! Rolling waveform scope buffer.
//!
//! The audio thread sends batches of decimated `f32` samples over a bounded
//! channel.  The UI calls [`ScopeBuf::update`] each frame to drain any pending
//! batches, then reads [`ScopeBuf::as_chart_data`] to feed the ratatui chart.

use crossbeam_channel::Receiver;
use std::collections::VecDeque;

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
}

impl ScopeBuf {
    /// Create a new scope buffer with the given channel receiver and sample capacity.
    pub fn new(rx: Receiver<Vec<f32>>, capacity: usize) -> Self {
        Self {
            rx,
            buf: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Drain all pending batches from the channel into the rolling buffer.
    ///
    /// Should be called once per UI frame before reading scope data.
    pub fn update(&mut self) {
        while let Ok(batch) = self.rx.try_recv() {
            for s in batch {
                if self.buf.len() >= self.capacity {
                    self.buf.pop_front();
                }
                self.buf.push_back(s);
            }
        }
    }

    /// Iterate over the current waveform data (oldest → newest).
    #[allow(dead_code)]
    pub fn samples(&self) -> impl Iterator<Item = f32> + '_ {
        self.buf.iter().copied()
    }

    /// Return waveform data as `(x, y)` pairs suitable for a ratatui `Dataset`.
    pub fn as_chart_data(&self) -> Vec<(f64, f64)> {
        self.buf
            .iter()
            .enumerate()
            .map(|(i, &s)| (i as f64, s as f64))
            .collect()
    }

    /// Number of samples currently in the buffer.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Returns `true` if the buffer contains no samples.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}
