//! Sound-effect generators and offline rendering under arbitrary inputs:
//! generated patches stay in range, renders stay bounded and finite, and
//! every WAV file is well formed.

use proptest::prelude::*;
use synthie::params::{MidiNote, SynthParams};
use synthie::render::{MAX_RENDER_SECONDS, render_note, write_wav};
use synthie::sfx::{SfxKind, generate, mutate, randomize};

fn in_range(p: &SynthParams) -> bool {
    let mut sane = p.clone();
    sane.sanitize();
    sane == *p
}

fn kind() -> impl Strategy<Value = SfxKind> {
    prop::sample::select(SfxKind::ALL)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any seed gives an in-range patch, and mutating any patch keeps it so.
    #[test]
    fn generated_patches_stay_in_range(kind in kind(), a in any::<u64>(), b in any::<u64>()) {
        let p = generate(kind, a);
        prop_assert!(in_range(&p));
        prop_assert!(in_range(&mutate(&p, b)));
        let r = randomize(a);
        prop_assert!(in_range(&r));
        prop_assert!(in_range(&mutate(&r, b)));
    }

    /// Whatever the patch, note and lengths, a render is finite, inside
    /// [-1, 1], no longer than asked and never longer than the hard limit.
    #[test]
    fn renders_are_bounded_and_finite(
        seed in any::<u64>(),
        note in 0_u8..128,
        hold in prop_oneof![-1.0_f32..2.0, Just(f32::INFINITY), Just(f32::NAN)],
        // An infinite `max` renders the full 10-minute limit; a unit test covers it.
        max in prop_oneof![-1.0_f32..2.0, Just(f32::NAN)],
    ) {
        let rate = 4_000;
        let s = render_note(&randomize(seed), MidiNote(note), hold, rate, max);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
        let limit = (max.clamp(0.0, MAX_RENDER_SECONDS) * rate as f32) as usize;
        prop_assert!(s.len() <= limit, "{} samples, limit {}", s.len(), limit);
        prop_assert!(s.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
    }

    /// Any samples (NaN included) at any valid rate make a well-formed WAV.
    #[test]
    fn wav_files_are_well_formed(
        samples in prop::collection::vec(any::<f32>(), 0..256),
        rate in 1_u32..=u32::MAX / 2,
    ) {
        let mut wav = Vec::new();
        write_wav(&mut wav, &samples, rate).unwrap();
        let u32_at = |i: usize| u32::from_le_bytes(wav[i..i + 4].try_into().unwrap());
        prop_assert_eq!(wav.len(), 44 + 2 * samples.len());
        prop_assert_eq!(u32_at(4) as usize, wav.len() - 8);
        prop_assert_eq!(u32_at(24), rate);
        prop_assert_eq!(u32_at(28), rate * 2);
        prop_assert_eq!(u32_at(40) as usize, 2 * samples.len());
    }
}
