use crossbeam_channel::Receiver;
use std::collections::VecDeque;

/// Rolling waveform scope buffer.
///
/// The audio thread sends batches of decimated samples via a bounded channel.
/// The UI calls `update()` each frame to drain new batches, then reads
/// `samples()` for display.
pub struct ScopeBuf {
    rx: Receiver<Vec<f32>>,
    buf: VecDeque<f32>,
    capacity: usize,
}

impl ScopeBuf {
    pub fn new(rx: Receiver<Vec<f32>>, capacity: usize) -> Self {
        Self {
            rx,
            buf: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Drain pending batches from the channel into the rolling buffer.
    /// Should be called once per UI frame.
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

    /// View as a `Vec<f64>` suitable for ratatui chart datasets.
    pub fn as_chart_data(&self) -> Vec<(f64, f64)> {
        self.buf
            .iter()
            .enumerate()
            .map(|(i, &s)| (i as f64, s as f64))
            .collect()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}
