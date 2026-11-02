//! Sample-accurate sequencing for the I/O-free [`SynthProcessor`].
//!
//! A [`Sequencer`] is an event source that runs on the audio thread and asks
//! to be called again after a number of frames. [`SequencedProcessor`] pairs
//! one with a [`SynthProcessor`] and splits every rendered buffer at exactly
//! those frames, remembering the remainder across calls. Events therefore
//! land on the same frame whatever buffer size the host renders with, so any
//! backend that drives the processor itself (cpal, kira, an `AudioWorklet`, a
//! game engine's stream) gets identical timing.
//!
//! ```
//! use synthie::audio::processor::SynthProcessor;
//! use synthie::audio::sequencer::{SequencedProcessor, Sequencer};
//! use synthie::params::{AudioEvent, DrumHit};
//!
//! /// A kick every half second.
//! struct Pulse;
//!
//! impl Sequencer for Pulse {
//!     fn tick(&mut self, sample_rate: f32, out: &mut Vec<AudioEvent>) -> usize {
//!         out.push(AudioEvent::Drum(DrumHit::Kick));
//!         (sample_rate * 0.5) as usize
//!     }
//! }
//!
//! let mut synth = SequencedProcessor::new(SynthProcessor::<4>::new(48_000.0), Pulse);
//! let mut buf = [0.0; 2 * 256]; // stereo, interleaved
//! synth.process_block(&[], &mut buf, 2);
//! ```

use crate::audio::processor::SynthProcessor;
use crate::params::AudioEvent;

/// Initial capacity of the buffer a [`Sequencer`] pushes its events into.
///
/// Pre-sizing it keeps the audio thread from allocating as long as a single
/// tick emits no more events than this.
pub const SEQUENCER_EVENT_CAPACITY: usize = 256;

/// Event source driven from inside the audio callback.
///
/// The host calls [`tick`](Sequencer::tick) at the frame the previous call
/// asked for and applies the events it pushes at exactly that frame, so the
/// timing does not depend on the buffer size.
///
/// `tick` usually runs on a real-time thread: it must not block, and should
/// not allocate.
pub trait Sequencer {
    /// Called at the current output frame. Push events that fire now into
    /// `out` and return the number of frames until the next call. A return
    /// value of 0 is treated as 1.
    fn tick(&mut self, sample_rate: f32, out: &mut Vec<AudioEvent>) -> usize;
}

/// The no-op sequencer: never emits events.
impl Sequencer for () {
    fn tick(&mut self, _sample_rate: f32, _out: &mut Vec<AudioEvent>) -> usize {
        usize::MAX
    }
}

impl<S: Sequencer + ?Sized> Sequencer for &mut S {
    fn tick(&mut self, sample_rate: f32, out: &mut Vec<AudioEvent>) -> usize {
        (**self).tick(sample_rate, out)
    }
}

impl<S: Sequencer + ?Sized> Sequencer for Box<S> {
    fn tick(&mut self, sample_rate: f32, out: &mut Vec<AudioEvent>) -> usize {
        (**self).tick(sample_rate, out)
    }
}

/// A [`SynthProcessor`] driven by a [`Sequencer`] with sample-accurate
/// timing.
///
/// Each [`process_block`](Self::process_block) renders in slices split at
/// the frames the sequencer asks for, and carries the frames left until the
/// next tick over to the following call.
pub struct SequencedProcessor<const N: usize, S> {
    processor: SynthProcessor<N>,
    sequencer: S,
    /// Frames left until the sequencer's next tick; 0 → tick at the next
    /// rendered frame.
    until_tick: usize,
    /// Reused event buffer for the sequencer's ticks.
    pending: Vec<AudioEvent>,
}

impl<const N: usize, S: Sequencer> SequencedProcessor<N, S> {
    /// Pair `processor` with `sequencer`. The sequencer's first tick happens
    /// at the first rendered frame.
    #[must_use]
    pub fn new(processor: SynthProcessor<N>, sequencer: S) -> Self {
        Self {
            processor,
            sequencer,
            until_tick: 0,
            pending: Vec::with_capacity(SEQUENCER_EVENT_CAPACITY),
        }
    }

    /// Apply `events`, then render `hw_channels`-interleaved frames into
    /// `buf`, ticking the sequencer and applying its events at exactly the
    /// frames it asked for.
    ///
    /// `events` take effect at the first frame of `buf`, before any
    /// sequencer events due at that frame. Use them for live input (a key
    /// press, a game event); anything rhythmic belongs in the sequencer.
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
        self.processor.apply_events(events);

        let sample_rate = self.processor.sample_rate();
        let mut rest = buf;
        while !rest.is_empty() {
            if self.until_tick == 0 {
                self.pending.clear();
                self.until_tick = self.sequencer.tick(sample_rate, &mut self.pending).max(1);
                self.processor.apply_events(&self.pending);
            }
            let n = self.until_tick.min(rest.len() / hw_channels);
            let (slice, tail) = rest.split_at_mut(n * hw_channels);
            self.processor.process_block(&[], slice, hw_channels);
            self.until_tick -= n;
            rest = tail;
        }
    }

    /// The wrapped processor, e.g. to read [`SynthProcessor::is_idle`].
    #[must_use]
    pub fn processor(&self) -> &SynthProcessor<N> {
        &self.processor
    }

    /// The wrapped processor, e.g. to change the master gain.
    pub fn processor_mut(&mut self) -> &mut SynthProcessor<N> {
        &mut self.processor
    }

    /// The sequencer.
    #[must_use]
    pub fn sequencer(&self) -> &S {
        &self.sequencer
    }

    /// The sequencer, e.g. to change its tempo. Its next tick still happens
    /// at the frame its previous tick asked for.
    pub fn sequencer_mut(&mut self) -> &mut S {
        &mut self.sequencer
    }

    /// Swap in another sequencer, which ticks at the next rendered frame,
    /// and return the previous one.
    pub fn replace_sequencer(&mut self, sequencer: S) -> S {
        self.until_tick = 0;
        core::mem::replace(&mut self.sequencer, sequencer)
    }

    /// Frames until the sequencer's next tick; 0 means it ticks at the next
    /// rendered frame.
    #[must_use]
    pub fn frames_until_tick(&self) -> usize {
        self.until_tick
    }

    /// Take the processor and the sequencer apart again.
    #[must_use]
    pub fn into_parts(self) -> (SynthProcessor<N>, S) {
        (self.processor, self.sequencer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{ChannelNo, DrumHit, MidiNote};
    use crate::presets::sid::default_patches;

    const SR: f32 = 44_100.0;
    const N: usize = 4;

    /// A little song on awkward frame boundaries: notes on every channel,
    /// drums, and note-offs, stepping a prime number of frames.
    struct Song {
        step: usize,
    }

    impl Sequencer for Song {
        fn tick(&mut self, _sample_rate: f32, out: &mut Vec<AudioEvent>) -> usize {
            const NOTES: [u8; 8] = [48, 55, 60, 64, 67, 72, 64, 60];
            let step = self.step;
            self.step += 1;
            #[allow(clippy::cast_possible_truncation)]
            let ch = ChannelNo((step % N) as u8);
            let note = MidiNote(NOTES[step % NOTES.len()]);
            if step.is_multiple_of(2) {
                out.push(AudioEvent::NoteOnChannel(ch, note));
            } else {
                out.push(AudioEvent::NoteOffChannel(ch, note));
            }
            out.push(AudioEvent::Drum(DrumHit::ALL[step % DrumHit::ALL.len()]));
            1_171
        }
    }

    fn sequenced() -> SequencedProcessor<N, Song> {
        let mut processor = SynthProcessor::<N>::new(SR);
        let patches = default_patches();
        #[allow(clippy::cast_possible_truncation)]
        let loads: Vec<_> = (0..N)
            .map(|i| {
                AudioEvent::LoadPatchChannel(
                    ChannelNo(i as u8),
                    Box::new(patches[i * 3 % patches.len()].params.clone()),
                )
            })
            .collect();
        processor.process_block(&loads, &mut [], 1);
        SequencedProcessor::new(processor, Song { step: 0 })
    }

    /// Render `frames` frames, cycling through `block_sizes`.
    fn render(block_sizes: &[usize], frames: usize, hw_channels: usize) -> Vec<f32> {
        let mut synth = sequenced();
        let mut out = vec![0.0; frames * hw_channels];
        let mut offset = 0;
        for &size in block_sizes.iter().cycle() {
            if offset >= frames {
                break;
            }
            let n = size.min(frames - offset);
            synth.process_block(
                &[],
                &mut out[offset * hw_channels..(offset + n) * hw_channels],
                hw_channels,
            );
            offset += n;
        }
        out
    }

    fn bits(samples: &[f32]) -> Vec<u32> {
        samples.iter().map(|s| s.to_bits()).collect()
    }

    #[test]
    fn output_does_not_depend_on_the_buffer_size() {
        const FRAMES: usize = 3 * 44_100;
        let reference = render(&[64], FRAMES, 1);
        assert!(reference.iter().any(|s| s.abs() > 0.01), "song is silent");
        for sizes in [&[512][..], &[4096], &[1], &[37], &[1, 4096, 333, 64, 2]] {
            assert_eq!(
                bits(&render(sizes, FRAMES, 1)),
                bits(&reference),
                "block sizes {sizes:?}"
            );
        }
    }

    #[test]
    fn interleaved_output_matches_mono() {
        const FRAMES: usize = 20_000;
        let mono = render(&[512], FRAMES, 1);
        let stereo = render(&[300], FRAMES, 2);
        for (i, frame) in stereo.chunks(2).enumerate() {
            assert_eq!(frame[0].to_bits(), mono[i].to_bits(), "frame {i}");
            assert_eq!(frame[1].to_bits(), mono[i].to_bits(), "frame {i}");
        }
    }

    /// A sequencer that fires one kick at a fixed frame, then nothing.
    struct KickAt(Option<usize>);

    impl Sequencer for KickAt {
        fn tick(&mut self, _sample_rate: f32, out: &mut Vec<AudioEvent>) -> usize {
            if let Some(frame) = self.0.take() {
                frame
            } else {
                out.push(AudioEvent::Drum(DrumHit::Kick));
                usize::MAX
            }
        }
    }

    #[test]
    fn events_land_on_the_frame_the_sequencer_asked_for() {
        const AT: usize = 1_000;
        let mut synth = SequencedProcessor::new(SynthProcessor::<N>::new(SR), KickAt(Some(AT)));
        let mut seq = vec![0.0; 4_096];
        synth.process_block(&[], &mut seq, 1);

        // The same hit applied by hand at frame AT.
        let mut processor = SynthProcessor::<N>::new(SR);
        let mut manual = vec![0.0; 4_096];
        processor.process_block(&[], &mut manual[..AT], 1);
        processor.process_block(&[AudioEvent::Drum(DrumHit::Kick)], &mut manual[AT..], 1);

        assert_eq!(bits(&seq), bits(&manual));
        assert!(seq[..AT].iter().all(|&s| s == 0.0));
        assert!(seq[AT..].iter().any(|&s| s != 0.0));
    }

    #[test]
    fn external_events_apply_before_the_sequencers_events_at_the_same_frame() {
        // Panic first, then the sequencer's kick: the kick must survive.
        let mut synth = SequencedProcessor::new(SynthProcessor::<N>::new(SR), KickAt(Some(1)));
        let mut buf = [0.0; 256];
        synth.process_block(&[], &mut buf[..1], 1);
        synth.process_block(&[AudioEvent::Panic], &mut buf[1..], 1);
        assert!(
            !synth.processor().is_idle(),
            "kick was swallowed by the panic"
        );
    }

    /// Counts ticks and asks for `next` frames each time.
    struct Counter {
        ticks: usize,
        next: usize,
    }

    impl Sequencer for Counter {
        fn tick(&mut self, _sample_rate: f32, _out: &mut Vec<AudioEvent>) -> usize {
            self.ticks += 1;
            self.next
        }
    }

    #[test]
    fn a_zero_frame_request_ticks_every_frame() {
        let counter = Counter { ticks: 0, next: 0 };
        let mut synth = SequencedProcessor::new(SynthProcessor::<N>::new(SR), counter);
        synth.process_block(&[], &mut [0.0; 100], 1);
        assert_eq!(synth.sequencer().ticks, 100);
    }

    #[test]
    fn ticks_are_carried_across_blocks_and_empty_blocks_do_not_tick() {
        let counter = Counter { ticks: 0, next: 10 };
        let mut synth = SequencedProcessor::new(SynthProcessor::<N>::new(SR), counter);
        synth.process_block(&[], &mut [], 1);
        assert_eq!(synth.sequencer().ticks, 0);
        synth.process_block(&[], &mut [0.0; 15], 1); // ticks at 0 and 10
        assert_eq!(synth.sequencer().ticks, 2);
        assert_eq!(synth.frames_until_tick(), 5);
        synth.process_block(&[], &mut [0.0; 5], 1); // next tick due at 20
        assert_eq!(synth.sequencer().ticks, 2);
        synth.process_block(&[], &mut [0.0; 1], 1);
        assert_eq!(synth.sequencer().ticks, 3);
    }

    #[test]
    fn a_replaced_sequencer_ticks_at_once() {
        let mut synth = SequencedProcessor::new(
            SynthProcessor::<N>::new(SR),
            Counter {
                ticks: 0,
                next: 1_000,
            },
        );
        synth.process_block(&[], &mut [0.0; 10], 1);
        let old = synth.replace_sequencer(Counter {
            ticks: 0,
            next: 1_000,
        });
        assert_eq!(old.ticks, 1);
        synth.process_block(&[], &mut [0.0; 10], 1);
        assert_eq!(synth.sequencer().ticks, 1);
    }

    #[test]
    fn boxed_and_borrowed_sequencers_work() {
        let mut counter = Counter { ticks: 0, next: 4 };
        {
            let mut synth = SequencedProcessor::new(SynthProcessor::<N>::new(SR), &mut counter);
            synth.process_block(&[], &mut [0.0; 8], 1);
        }
        assert_eq!(counter.ticks, 2);

        let boxed: Box<dyn Sequencer> = Box::new(Counter { ticks: 0, next: 4 });
        let mut synth = SequencedProcessor::new(SynthProcessor::<N>::new(SR), boxed);
        synth.process_block(&[], &mut [0.0; 8], 1);
    }
}
