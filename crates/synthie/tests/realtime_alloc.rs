//! The audio path must not touch the heap once running: an allocation or a
//! free on the audio thread can block on the allocator's lock and cause an
//! underrun. A counting allocator checks rendering, sequencing and every
//! kind of event a game sends while playing.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use synthie::audio::processor::SynthProcessor;
use synthie::audio::sequencer::{SequencedProcessor, Sequencer};
use synthie::param::Param;
use synthie::params::{AudioEvent, ChannelNo, DrumHit, MidiNote};
use synthie::presets::sid::default_patches;

struct Counting;

thread_local! {
    /// Allocations and frees on this thread while counting; `None` when off.
    /// Thread-local so tests running in parallel don't count each other.
    static COUNTS: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

fn bump(alloc: usize, free: usize) {
    let _ = COUNTS.try_with(|c| {
        if let Some((a, f)) = c.get() {
            c.set(Some((a + alloc, f + free)));
        }
    });
}

// SAFETY: forwards to the system allocator unchanged; only counts.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        bump(1, 0);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        bump(0, 1);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        bump(1, 1);
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Allocations and frees made by `f` on this thread.
fn count(f: impl FnOnce()) -> (usize, usize) {
    COUNTS.with(|c| c.set(Some((0, 0))));
    f();
    COUNTS.with(|c| c.replace(None)).unwrap()
}

/// A busy song: every 16th note changes notes on all channels, hits drums
/// and tweaks a parameter, like a game's music plus sound effects.
struct Busy {
    step: u8,
}

impl Sequencer for Busy {
    fn tick(&mut self, sample_rate: f32, out: &mut Vec<AudioEvent>) -> usize {
        self.step = self.step.wrapping_add(1);
        for ch in 0..4 {
            let note = MidiNote(36 + self.step.wrapping_add(ch * 7) % 48);
            let ch = ChannelNo(ch);
            out.push(AudioEvent::NoteOffChannel(
                ch,
                MidiNote(note.0.saturating_sub(5)),
            ));
            out.push(AudioEvent::NoteOnChannel(ch, note));
        }
        out.push(AudioEvent::Drum(
            DrumHit::ALL[usize::from(self.step) % DrumHit::ALL.len()],
        ));
        out.push(AudioEvent::SetParam(
            ChannelNo(1),
            Param::FilterCutoff,
            f32::from(self.step) * 50.0,
        ));
        #[cfg(feature = "arp")]
        {
            out.push(AudioEvent::ArpEnabled(ChannelNo(3), self.step % 32 < 16));
            out.push(AudioEvent::ArpSetNotes(
                ChannelNo(3),
                [MidiNote(60), MidiNote(64), MidiNote(67), MidiNote(72)],
                3,
            ));
        }
        if self.step.is_multiple_of(64) {
            out.push(AudioEvent::Panic);
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let sixteenth = (sample_rate * 60.0 / (140.0 * 4.0)) as usize;
        sixteenth
    }
}

#[test]
fn rendering_a_busy_song_never_touches_the_heap() {
    let mut synth = SequencedProcessor::new(SynthProcessor::<4>::new(48_000.0), Busy { step: 0 });
    let presets = default_patches();
    let patches: Vec<_> = (0..4)
        .map(|ch| {
            AudioEvent::LoadPatchChannel(
                ChannelNo(ch),
                Box::new(presets[usize::from(ch) + 3].params.clone()),
            )
        })
        .collect();
    let mut buf = vec![0.0_f32; 2 * 512];
    synth.process_block(&patches, &mut buf, 2);

    let live = [
        AudioEvent::NoteOn(MidiNote(60)),
        AudioEvent::Drum(DrumHit::Snare),
        AudioEvent::SetParam(ChannelNo(0), Param::ReverbMix, 0.4),
        AudioEvent::NoteOff(MidiNote(60)),
    ];
    let counts = count(|| {
        // About ten seconds in odd block sizes, so ticks split blocks.
        for i in 0..2_000 {
            let frames = [64, 128, 256, 37, 512][i % 5];
            let events = if i % 10 == 0 { &live[..] } else { &[] };
            synth.process_block(events, &mut buf[..2 * frames], 2);
        }
    });
    assert_eq!(counts, (0, 0), "(allocations, frees) on the audio path");
}

/// Loading a patch allocates nothing, but the boxed patch is freed where the
/// event is dropped. The cpal backend drops it on the audio thread; this
/// pins that one known free so a new one shows up.
#[test]
fn loading_a_patch_frees_only_its_box() {
    let mut synth = SynthProcessor::<4>::new(48_000.0);
    let mut buf = vec![0.0_f32; 256];
    let event =
        AudioEvent::LoadPatchChannel(ChannelNo(2), Box::new(default_patches()[5].params.clone()));
    let counts = count(|| {
        synth.process_block(std::slice::from_ref(&event), &mut buf, 1);
        drop(event);
    });
    assert_eq!(counts, (0, 1));
}
