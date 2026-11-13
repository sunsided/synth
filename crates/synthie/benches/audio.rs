//! Cost of the audio path, as a game embedding the engine sees it.
//!
//! Throughput is in frames, so criterion's `elem/s` divided by the sample
//! rate (48 000) is the real-time factor: how many times faster than real
//! time one core renders that scenario. A game typically renders 256–1024
//! frames per callback; a 512-frame block at 48 kHz is a 10.7 ms budget.
//!
//! ```text
//! cargo bench -p synthie                           # everything
//! cargo bench -p synthie -- block/                 # one group
//! cargo bench -p synthie -- --save-baseline main   # before a change
//! cargo bench -p synthie -- --baseline main        # after: compare
//! ```

use std::hint::black_box;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use synthie::audio::processor::SynthProcessor;
use synthie::audio::sequencer::{SequencedProcessor, Sequencer};
use synthie::param::Param;
use synthie::params::{AudioEvent, ChannelNo, DrumHit, MidiNote};
use synthie::presets::sid::default_patches;
use synthie::render::render_note;
use synthie::sfx::{self, SfxKind};

const SAMPLE_RATE: f32 = 48_000.0;
/// Frames per block in the fixed-size groups.
const BLOCK: usize = 512;
/// Interleaved output channels (stereo).
const OUT: usize = 2;
/// A held chord per channel: four voices, the polyphony.
const CHORD: [u8; 4] = [48, 55, 60, 64];

type Synth = SynthProcessor<4>;
/// Puts a fresh [`synth`] into a scenario's state.
type Setup = fn(&mut Synth);

/// A processor with a preset on each channel, kept sustaining so held notes
/// cost their full price for the whole run.
fn synth() -> Synth {
    let mut synth = Synth::new(SAMPLE_RATE);
    let presets = default_patches();
    let mut events = Vec::new();
    for ch in 0..4_u8 {
        let ch_no = ChannelNo(ch);
        let patch = presets[usize::from(ch) % presets.len()].params.clone();
        events.push(AudioEvent::LoadPatchChannel(ch_no, Box::new(patch)));
        events.push(AudioEvent::SetParam(ch_no, Param::EnvSustain, 0.8));
        events.push(AudioEvent::ArpEnabled(ch_no, false));
    }
    synth.process_block(&events, &mut [0.0; OUT], OUT);
    synth
}

/// Hold `CHORD` on the first `channels` channels.
fn hold(synth: &mut Synth, channels: u8) {
    let events: Vec<_> = (0..channels)
        .flat_map(|ch| {
            CHORD.map(|n| AudioEvent::NoteOnChannel(ChannelNo(ch), MidiNote(n + ch * 12)))
        })
        .collect();
    synth.process_block(&events, &mut [0.0; OUT * 64], OUT);
}

/// Turn up the shared reverb, chorus and delay.
fn wet(synth: &mut Synth) {
    let fx = [
        (Param::ReverbMix, 0.5),
        (Param::ReverbSize, 0.8),
        (Param::ChorusMix, 0.5),
        (Param::DelayMix, 0.4),
        (Param::DelayFeedback, 0.6),
    ];
    let events: Vec<_> = fx
        .map(|(p, v)| AudioEvent::SetParam(ChannelNo::DEFAULT, p, v))
        .into();
    synth.process_block(&events, &mut [0.0; OUT], OUT);
}

fn frames(n: usize) -> Throughput {
    Throughput::Elements(n as u64)
}

/// One block in typical game situations, from silence to everything on.
fn block(c: &mut Criterion) {
    let mut group = c.benchmark_group("block");
    group.throughput(frames(BLOCK));
    let mut buf = vec![0.0_f32; BLOCK * OUT];

    let scenarios: [(&str, Setup); 6] = [
        ("idle", |_| {}),
        ("one_note", |s| {
            s.process_block(&[AudioEvent::NoteOn(MidiNote(60))], &mut [0.0; OUT], OUT);
        }),
        ("one_channel_chord", |s| hold(s, 1)),
        ("full_polyphony", |s| hold(s, 4)),
        ("full_polyphony_wet", |s| {
            hold(s, 4);
            wet(s);
        }),
        ("idle_wet_tail", wet),
    ];
    for (name, setup) in scenarios {
        let mut synth = synth();
        setup(&mut synth);
        group.bench_function(name, |b| {
            b.iter(|| synth.process_block(&[], black_box(&mut buf), OUT));
        });
    }

    // Every drum retriggered each block: a busy percussion track.
    let mut synth = synth();
    let hits: Vec<_> = DrumHit::ALL.into_iter().map(AudioEvent::Drum).collect();
    group.bench_function("all_drums", |b| {
        b.iter(|| synth.process_block(black_box(&hits), &mut buf, OUT));
    });
    group.finish();
}

/// The same work in different callback sizes: shows the per-call overhead.
fn block_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_size");
    for size in [64, 128, 256, 512, 1024, 2048] {
        let mut synth = synth();
        hold(&mut synth, 4);
        wet(&mut synth);
        let mut buf = vec![0.0_f32; size * OUT];
        group.throughput(frames(size));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| synth.process_block(&[], black_box(&mut buf), OUT));
        });
    }
    group.finish();
}

/// Each built-in preset holding a chord on one channel, dry.
fn preset(c: &mut Criterion) {
    let mut group = c.benchmark_group("preset");
    group.throughput(frames(BLOCK));
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(2));
    let mut buf = vec![0.0_f32; BLOCK * OUT];
    for patch in default_patches() {
        let mut synth = Synth::new(SAMPLE_RATE);
        let mut events = vec![
            AudioEvent::LoadPatch(Box::new(patch.params.clone())),
            AudioEvent::SetParam(ChannelNo::DEFAULT, Param::EnvSustain, 0.8),
        ];
        events.extend(CHORD.map(|n| AudioEvent::NoteOn(MidiNote(n))));
        synth.process_block(&events, &mut [0.0; OUT], OUT);
        group.bench_function(patch.name.as_str(), |b| {
            b.iter(|| synth.process_block(&[], black_box(&mut buf), OUT));
        });
    }
    group.finish();
}

/// A song on 16ths at 140 BPM: notes on every channel, drums and a filter
/// sweep each step, so blocks get split at the sequencer's ticks.
struct Song {
    step: u8,
}

impl Sequencer for Song {
    fn tick(&mut self, sample_rate: f32, out: &mut Vec<AudioEvent>) -> usize {
        self.step = self.step.wrapping_add(1);
        for ch in 0..4 {
            let ch_no = ChannelNo(ch);
            let note = MidiNote(36 + self.step.wrapping_mul(5).wrapping_add(ch * 7) % 48);
            out.push(AudioEvent::NoteOffChannel(ch_no, MidiNote(note.0 ^ 1)));
            out.push(AudioEvent::NoteOnChannel(ch_no, note));
        }
        out.push(AudioEvent::Drum(
            DrumHit::ALL[usize::from(self.step) % DrumHit::ALL.len()],
        ));
        out.push(AudioEvent::SetParam(
            ChannelNo(1),
            Param::FilterCutoff,
            200.0 + f32::from(self.step) * 40.0,
        ));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let sixteenth = (sample_rate * 60.0 / (140.0 * 4.0)) as usize;
        sixteenth
    }
}

fn sequenced(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequenced");
    group.throughput(frames(BLOCK));
    let mut synth = synth();
    wet(&mut synth);
    let mut synth = SequencedProcessor::new(synth, Song { step: 0 });
    let mut buf = vec![0.0_f32; BLOCK * OUT];
    group.bench_function("busy_song", |b| {
        b.iter(|| synth.process_block(&[], black_box(&mut buf), OUT));
    });
    group.finish();
}

/// Rendering a sound effect ahead of time, e.g. while a level loads.
fn offline(c: &mut Criterion) {
    let mut group = c.benchmark_group("offline");
    group.sample_size(20);
    for kind in [SfxKind::Laser, SfxKind::Explosion, SfxKind::PowerUp] {
        let params = sfx::generate(kind, 7);
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let rate = SAMPLE_RATE as u32;
        let len = render_note(&params, MidiNote(60), 0.3, rate, 5.0).len();
        group.throughput(frames(len));
        group.bench_function(kind.name(), |b| {
            b.iter(|| render_note(black_box(&params), MidiNote(60), 0.3, rate, 5.0));
        });
    }
    group.finish();
}

criterion_group!(benches, block, block_size, preset, sequenced, offline);
criterion_main!(benches);
