//! Output-level controls of [`SynthProcessor`]: master gain and voice
//! normalization, exercised through the public API the way an embedder would.
//!
//! Most assertions here are **bit-exact**. Multiplying by a power of two only
//! changes a float's exponent, so it commutes with every rounding step of the
//! (linear) mix and FX chain. Rendering at gain `2^-k` and scaling back up by
//! `2^k` therefore reproduces the internal pre-gain signal exactly, which lets
//! us predict the output for any other gain without a tolerance.

use proptest::prelude::*;
use synthie::prelude::*;
use synthie::presets::sid::default_patches;

const SR: f32 = 44_100.0;

/// Gain used to recover the internal pre-gain signal without clipping it.
const PROBE_GAIN: f32 = 1.0 / 1024.0;

type Processor = SynthProcessor<NUM_CHANNELS>;

fn processor(gain: f32, normalization: VoiceNormalization) -> Processor {
    let mut proc = Processor::new(SR);
    proc.set_master_gain(gain);
    proc.set_voice_normalization(normalization);
    proc
}

fn render(proc: &mut Processor, events: &[AudioEvent], frames: usize) -> Vec<f32> {
    let mut buf = vec![0.0_f32; frames];
    proc.process_block(events, &mut buf, 1);
    buf
}

/// A scene: one preset per channel, then a chord spread across the channels.
#[derive(Clone, Debug)]
struct Scene {
    presets: [usize; NUM_CHANNELS],
    notes: Vec<(u8, u8)>,
    drum: Option<DrumHit>,
}

impl Scene {
    fn events(&self) -> Vec<AudioEvent> {
        let patches = default_patches();
        let mut events: Vec<AudioEvent> = self
            .presets
            .iter()
            .enumerate()
            .map(|(ch, &idx)| {
                let params = patches[idx % patches.len()].params.clone();
                #[allow(clippy::cast_possible_truncation)] // NUM_CHANNELS is tiny
                AudioEvent::LoadPatchChannel(ChannelNo(ch as u8), Box::new(params))
            })
            .collect();
        events.extend(
            self.notes
                .iter()
                .map(|&(ch, note)| AudioEvent::NoteOnChannel(ChannelNo(ch), MidiNote(note))),
        );
        events.extend(self.drum.map(AudioEvent::Drum));
        events
    }

    /// Render the scene from a fresh processor.
    fn render(&self, gain: f32, normalization: VoiceNormalization, frames: usize) -> Vec<f32> {
        render(&mut processor(gain, normalization), &self.events(), frames)
    }
}

fn scene() -> impl Strategy<Value = Scene> {
    #[allow(clippy::cast_possible_truncation)]
    let channels = 0..NUM_CHANNELS as u8;
    (
        prop::array::uniform4(0usize..64),
        prop::collection::vec((channels, 24u8..=96), 1..=8),
        prop::option::of(prop::sample::select(vec![
            DrumHit::Kick,
            DrumHit::HiHatClosed,
            DrumHit::HiHatOpen,
        ])),
    )
        .prop_map(|(presets, notes, drum)| Scene {
            presets,
            notes,
            drum,
        })
}

/// Any finite gain, from silent to absurdly loud, positive or negative.
fn gain() -> impl Strategy<Value = f32> {
    prop_oneof![
        Just(0.0_f32),
        Just(DEFAULT_MASTER_GAIN),
        -4.0_f32..4.0,
        (-30i32..30, any::<bool>()).prop_map(|(exp, neg)| {
            let g = 2.0_f32.powi(exp);
            if neg { -g } else { g }
        }),
    ]
}

fn bits(buf: &[f32]) -> Vec<u32> {
    buf.iter().map(|s| s.to_bits()).collect()
}

fn peak(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// The master gain is a pure post-FX multiply followed by the clamp:
    /// `out(g) == clamp(pre_gain * g)`, bit for bit, for any scene and gain.
    #[test]
    fn master_gain_is_an_exact_post_fx_multiply(scene in scene(), g in gain()) {
        let probe = scene.render(PROBE_GAIN, VoiceNormalization::Polyphony, 2048);
        let out = scene.render(g, VoiceNormalization::Polyphony, 2048);
        let expected: Vec<f32> = probe
            .iter()
            .map(|s| ((s / PROBE_GAIN) * g).clamp(-1.0, 1.0))
            .collect();
        prop_assert_eq!(bits(&out), bits(&expected));
    }

    /// Turning normalization off is exactly a 4× boost of the synth voices,
    /// i.e. indistinguishable from keeping it on at four times the gain —
    /// as long as no drum (which is never normalized) is in the mix.
    #[test]
    fn normalization_off_equals_four_times_the_gain(
        mut scene in scene(),
        g in gain(),
    ) {
        scene.drum = None;
        let off = scene.render(g, VoiceNormalization::Off, 2048);
        let on = scene.render(4.0 * g, VoiceNormalization::Polyphony, 2048);
        prop_assert_eq!(bits(&off), bits(&on));
    }

    /// Negating the gain mirrors the waveform exactly (polarity inversion).
    #[test]
    fn negative_gain_inverts_polarity(scene in scene(), g in gain()) {
        let pos = scene.render(g, VoiceNormalization::Polyphony, 1024);
        let neg = scene.render(-g, VoiceNormalization::Polyphony, 1024);
        let mirrored: Vec<f32> = pos.iter().map(|s| -s).collect();
        prop_assert_eq!(bits(&neg), bits(&mirrored));
    }

    /// No gain, however large, and no normalization mode can push the
    /// output outside `-1..=1` or make it non-finite.
    #[test]
    fn output_stays_finite_and_in_range(
        scene in scene(),
        g in prop_oneof![gain(), Just(f32::MAX), Just(-f32::MAX)],
        off in any::<bool>(),
    ) {
        let normalization = if off { VoiceNormalization::Off } else { VoiceNormalization::Polyphony };
        let out = scene.render(g, normalization, 2048);
        prop_assert!(out.iter().all(|s| s.is_finite() && (-1.0..=1.0).contains(s)));
    }

    /// NaN and infinities never reach the audio path; the last good gain stays.
    #[test]
    fn non_finite_gains_are_ignored(
        g in gain(),
        bad in prop::sample::select(vec![f32::NAN, f32::INFINITY, f32::NEG_INFINITY]),
    ) {
        let mut proc = processor(g, VoiceNormalization::Polyphony);
        proc.set_master_gain(bad);
        prop_assert_eq!(proc.master_gain().to_bits(), g.to_bits());
    }
}

/// The scenario from issue #41: a single note with the defaults peaks at
/// exactly `0.5 / 4 = 1/8` of what the unattenuated engine produces.
#[test]
fn defaults_attenuate_a_lone_note_by_exactly_one_eighth() {
    let scene = Scene {
        presets: [0; NUM_CHANNELS],
        notes: vec![(0, 69)],
        drum: None,
    };
    let default = scene.render(DEFAULT_MASTER_GAIN, VoiceNormalization::Polyphony, 8192);
    let raw = scene.render(PROBE_GAIN, VoiceNormalization::Off, 8192);
    let expected: Vec<f32> = raw.iter().map(|s| s / PROBE_GAIN / 8.0).collect();
    assert_eq!(bits(&default), bits(&expected));
    assert!(peak(&default) > 0.01, "the note should be audible");
}

/// Fresh processors start at the documented defaults.
#[test]
fn new_processors_use_the_documented_defaults() {
    let proc = Processor::new(SR);
    assert_eq!(proc.master_gain().to_bits(), DEFAULT_MASTER_GAIN.to_bits());
    assert_eq!(proc.voice_normalization(), VoiceNormalization::Polyphony);
}

/// Drums sit outside the voice pools, so normalization must not touch them.
#[test]
fn drums_ignore_voice_normalization() {
    for hit in [DrumHit::Kick, DrumHit::HiHatClosed, DrumHit::HiHatOpen] {
        let events = || [AudioEvent::Drum(hit)];
        let on = render(
            &mut processor(1.0, VoiceNormalization::Polyphony),
            &events(),
            4096,
        );
        let off = render(
            &mut processor(1.0, VoiceNormalization::Off),
            &events(),
            4096,
        );
        assert!(peak(&on) > 0.01, "{hit:?} should be audible");
        assert_eq!(bits(&on), bits(&off), "{hit:?} changed with normalization");
    }
}

/// Streaming: the settings can change between `process_block` calls (e.g. a
/// volume slider) without disturbing voices, envelopes or FX tails. Ride the
/// controls on one processor and compare every block against an untouched
/// twin rendering the same performance.
#[test]
fn settings_can_be_ridden_between_blocks_while_streaming() {
    use VoiceNormalization::{Off, Polyphony};

    const BLOCK: usize = 128;
    // (gain, normalization) per block: up, down, normalization flips, silence,
    // polarity flip, and back home to the defaults.
    let automation = [
        (0.5, Polyphony),
        (1.0, Polyphony),
        (0.25, Polyphony),
        (0.5, Off),
        (0.125, Off),
        (0.0, Polyphony),
        (-0.5, Polyphony),
        (2.0, Off),
        (0.5, Polyphony),
    ];

    let scene = Scene {
        presets: [0, 3, 7, 11],
        notes: vec![(0, 57), (1, 60), (2, 64), (3, 69)],
        drum: Some(DrumHit::Kick),
    };
    let mut ridden = processor(DEFAULT_MASTER_GAIN, Polyphony);
    let mut twin = processor(PROBE_GAIN, Polyphony);

    let mut events = scene.events();
    for (block, &(gain, normalization)) in automation.iter().cycle().take(64).enumerate() {
        ridden.set_master_gain(gain);
        ridden.set_voice_normalization(normalization);
        // The twin always plays the same normalization at the probe gain, so
        // its output reveals the exact pre-gain signal for this block.
        twin.set_voice_normalization(normalization);
        let out = render(&mut ridden, &events, BLOCK);
        let probe = render(&mut twin, &events, BLOCK);
        events.clear();

        let expected: Vec<f32> = probe
            .iter()
            .map(|s| ((s / PROBE_GAIN) * gain).clamp(-1.0, 1.0))
            .collect();
        assert_eq!(bits(&out), bits(&expected), "block {block} diverged");
    }
}

/// The documented trade-off: at unity gain a full four-note chord fits with
/// normalization on, but slams into the clamp with it off.
#[test]
fn normalization_buys_headroom_for_full_chords() {
    let chord = Scene {
        presets: [0; NUM_CHANNELS],
        notes: vec![(0, 48), (0, 55), (0, 60), (0, 64)],
        drum: None,
    };
    let on = chord.render(1.0, VoiceNormalization::Polyphony, 8192);
    let off = chord.render(1.0, VoiceNormalization::Off, 8192);
    assert!(peak(&on) < 1.0, "a normalized chord should not clip");
    let clipped = off.iter().filter(|s| s.abs() >= 1.0).count();
    assert!(
        clipped > 0,
        "an unnormalized chord at unity gain should clip"
    );
}
