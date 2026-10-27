//! Untrusted patches: whatever a patch contains, loading it must neither
//! crash the engine nor leave it unable to play the next one.

use proptest::prelude::*;
use synthie::prelude::*;
use synthie::presets::sid::default_patches;

const SR: f32 = 44_100.0;

/// Every float field of a patch, by name, for range checks.
fn floats(p: &SynthParams) -> Vec<(&'static str, f32, f32, f32)> {
    let mut v = vec![
        ("osc.pulse_width", p.osc.pulse_width, 0.05, 0.95),
        ("osc.detune", p.osc.detune, -100.0, 100.0),
        ("osc.noise_mix", p.osc.noise_mix, 0.0, 1.0),
        ("osc2.detune", p.osc2.detune, -100.0, 100.0),
        ("osc2.osc2_mix", p.osc2.osc2_mix, 0.0, 1.0),
        ("env.attack", p.env.attack, 0.001, 10.0),
        ("env.decay", p.env.decay, 0.001, 10.0),
        ("env.sustain", p.env.sustain, 0.0, 1.0),
        ("env.release", p.env.release, 0.001, 20.0),
        ("filter.cutoff", p.filter.cutoff, 20.0, 18_000.0),
        ("filter.resonance", p.filter.resonance, 0.0, 0.99),
        ("filter.drive", p.filter.drive, 0.0, 1.0),
        ("fx.reverb_mix", p.fx.reverb_mix, 0.0, 1.0),
        ("fx.reverb_size", p.fx.reverb_size, 0.0, 1.0),
        ("fx.reverb_damping", p.fx.reverb_damping, 0.0, 1.0),
        ("crusher.bits", p.crusher.bits, 1.0, 16.0),
        ("crusher.rate", p.crusher.rate, 1.0, 16.0),
        ("delay.time_ms", p.delay.time_ms, 1.0, 2_000.0),
        ("delay.feedback", p.delay.feedback, 0.0, 0.95),
        ("delay.mix", p.delay.mix, 0.0, 1.0),
        ("chorus.rate", p.chorus.rate, 0.1, 5.0),
        ("chorus.depth_ms", p.chorus.depth_ms, 0.0, 10.0),
        ("chorus.mix", p.chorus.mix, 0.0, 1.0),
        ("global.volume", p.global.volume, 0.0, 1.0),
        ("global.glide_time", p.global.glide_time, 0.0, 10.0),
    ];
    for (name, lfo) in [("lfo", &p.lfo), ("lfo2", &p.lfo2)] {
        v.push((name, lfo.lfo_rate, 0.0, 50.0));
        v.push((name, lfo.lfo_depth, 0.0, 1.0));
    }
    for (name, env) in [("filter_env", &p.filter_env), ("pitch_env", &p.pitch_env)] {
        v.push((name, env.attack, 0.001, 10.0));
        v.push((name, env.decay, 0.001, 10.0));
        v.push((name, env.sustain, 0.0, 1.0));
        v.push((name, env.release, 0.001, 20.0));
        v.push((name, env.depth, -1.0, 1.0));
    }
    #[cfg(feature = "arp")]
    {
        v.push(("arp.rate", p.arp.rate, 0.0, 50.0));
        v.push(("arp.gate", p.arp.gate, 0.0, 1.0));
    }
    v
}

/// Overwrite every float field with values drawn from `values` (cycled).
fn scramble(p: &mut SynthParams, values: &[f32]) {
    let mut it = values.iter().copied().cycle();
    let mut next = || it.next().unwrap();
    for f in [
        &mut p.osc.pulse_width,
        &mut p.osc.detune,
        &mut p.osc.noise_mix,
        &mut p.osc2.detune,
        &mut p.osc2.osc2_mix,
        &mut p.env.attack,
        &mut p.env.decay,
        &mut p.env.sustain,
        &mut p.env.release,
        &mut p.filter.cutoff,
        &mut p.filter.resonance,
        &mut p.filter.drive,
        &mut p.lfo.lfo_rate,
        &mut p.lfo.lfo_depth,
        &mut p.lfo2.lfo_rate,
        &mut p.lfo2.lfo_depth,
        &mut p.filter_env.attack,
        &mut p.filter_env.decay,
        &mut p.filter_env.sustain,
        &mut p.filter_env.release,
        &mut p.filter_env.depth,
        &mut p.pitch_env.attack,
        &mut p.pitch_env.decay,
        &mut p.pitch_env.sustain,
        &mut p.pitch_env.release,
        &mut p.pitch_env.depth,
        &mut p.fx.reverb_mix,
        &mut p.fx.reverb_size,
        &mut p.fx.reverb_damping,
        &mut p.crusher.bits,
        &mut p.crusher.rate,
        &mut p.delay.time_ms,
        &mut p.delay.feedback,
        &mut p.delay.mix,
        &mut p.chorus.rate,
        &mut p.chorus.depth_ms,
        &mut p.chorus.mix,
        &mut p.global.volume,
        &mut p.global.glide_time,
    ] {
        *f = next();
    }
    #[cfg(feature = "arp")]
    {
        p.arp.rate = next();
        p.arp.gate = next();
    }
}

fn peak(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
}

/// Load `patch` on every channel, play a chord and a drum, release.
fn abuse(proc: &mut SynthProcessor<NUM_CHANNELS>, patch: &SynthParams) -> Vec<f32> {
    let mut events = vec![];
    for ch in 0..4 {
        events.push(AudioEvent::LoadPatchChannel(
            ChannelNo(ch),
            Box::new(patch.clone()),
        ));
        events.push(AudioEvent::NoteOnChannel(
            ChannelNo(ch),
            MidiNote(48 + 7 * ch),
        ));
    }
    events.push(AudioEvent::Drum(DrumHit::Kick));
    let mut out = vec![0.0_f32; 8_192];
    proc.process_block(&events, &mut out, 1);
    let mut tail = vec![0.0_f32; 8_192];
    let off: Vec<_> = (0..4)
        .map(|ch| AudioEvent::NoteOffChannel(ChannelNo(ch), MidiNote(48 + 7 * ch)))
        .collect();
    proc.process_block(&off, &mut tail, 1);
    out.extend(tail);
    out
}

/// After whatever came before, the default patch must play normally again.
fn plays_normally(proc: &mut SynthProcessor<NUM_CHANNELS>) -> bool {
    let mut buf = vec![0.0_f32; 4_096];
    proc.process_block(
        &[
            AudioEvent::Panic,
            AudioEvent::LoadPatch(Box::default()),
            AudioEvent::NoteOn(MidiNote(69)),
        ],
        &mut buf,
        1,
    );
    let mut fresh = SynthProcessor::<NUM_CHANNELS>::new(SR);
    let mut reference = vec![0.0_f32; 4_096];
    fresh.process_block(
        &[
            AudioEvent::LoadPatch(Box::default()),
            AudioEvent::NoteOn(MidiNote(69)),
        ],
        &mut reference,
        1,
    );
    // Panic resets every voice and FX tail, so the engine is as good as new.
    buf.iter()
        .map(|s| s.to_bits())
        .eq(reference.iter().map(|s| s.to_bits()))
}

#[test]
fn sanitizing_leaves_every_built_in_preset_untouched() {
    for patch in default_patches() {
        let mut p = patch.params.clone();
        p.sanitize();
        assert_eq!(p, patch.params, "{} was changed", patch.name);
    }
    let mut p = SynthParams::default();
    p.sanitize();
    assert_eq!(p, SynthParams::default());
}

/// The patch the fuzzer found: a chorus rate this high overflowed the LFO
/// step to infinity, the NaN reached the delay and reverb feedback, and the
/// engine stayed silent for good, even after a panic and a sane patch.
#[test]
fn the_fuzzer_found_chorus_patch_no_longer_silences_the_engine() {
    let mut patch = SynthParams::default();
    // The exact values from the crash input.
    patch.chorus.depth_ms = f32::from_bits(0x0000_FFFF); // 9.2e-41, subnormal
    patch.chorus.mix = f32::from_bits(0x5D00_0000); // 5.8e17
    patch.chorus.rate = f32::from_bits(0x7EFF_0000); // 1.7e38
    let mut proc = SynthProcessor::<NUM_CHANNELS>::new(SR);
    let out = abuse(&mut proc, &patch);
    assert!(peak(&out) > 0.01, "the clamped patch still plays");
    assert!(plays_normally(&mut proc));
}

#[test]
fn panic_cuts_the_fx_tails() {
    let mut patch = default_patches()[1].params.clone();
    patch.fx.reverb_mix = 1.0;
    patch.delay.mix = 1.0;
    patch.delay.feedback = 0.95;
    patch.chorus.mix = 1.0;
    let mut proc = SynthProcessor::<NUM_CHANNELS>::new(SR);
    abuse(&mut proc, &patch);
    let mut after = vec![1.0_f32; 4_096];
    proc.process_block(&[AudioEvent::Panic], &mut after, 1);
    assert_eq!(
        peak(&after),
        0.0,
        "panic should silence the reverb and echoes too"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Any floats at all, including NaN and infinities, come out finite and
    /// in their documented range, and sanitizing twice changes nothing.
    #[test]
    fn sanitize_brings_every_field_into_range(values in prop::collection::vec(any::<f32>(), 1..64)) {
        let mut p = SynthParams::default();
        scramble(&mut p, &values);
        p.sanitize();
        for (name, v, lo, hi) in floats(&p) {
            prop_assert!(v.is_finite() && (lo..=hi).contains(&v), "{name} = {v} not in {lo}..={hi}");
        }
        let again = { let mut q = p.clone(); q.sanitize(); q };
        prop_assert_eq!(again, p);
    }

    /// Whatever patch a channel gets, the output stays finite and in range,
    /// and the engine plays the next sane patch exactly like a new one.
    #[test]
    fn no_patch_can_break_the_engine(
        values in prop::collection::vec(prop_oneof![
            any::<f32>(),
            Just(f32::MAX), Just(-f32::MAX), Just(0.0), Just(f32::MIN_POSITIVE),
            -1e6_f32..1e6,
        ], 1..64),
    ) {
        let mut patch = SynthParams::default();
        scramble(&mut patch, &values);
        let mut proc = SynthProcessor::<NUM_CHANNELS>::new(SR);
        let out = abuse(&mut proc, &patch);
        prop_assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        prop_assert!(plays_normally(&mut proc));
    }
}
