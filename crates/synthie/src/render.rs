//! Offline rendering: play a patch into a buffer and save it as a WAV file,
//! for sound effects used outside synthie (game engines, DAWs).
//!
//! ```
//! use synthie::params::{MidiNote, SynthParams};
//! use synthie::render::{render_note, write_wav};
//!
//! let samples = render_note(&SynthParams::default(), MidiNote::A4, 0.2, 44_100, 5.0);
//! let mut wav = Vec::new();
//! write_wav(&mut wav, &samples, 44_100).unwrap();
//! assert_eq!(&wav[..4], b"RIFF");
//! ```

use crate::audio::processor::{SynthProcessor, VoiceNormalization};
use crate::params::{AudioEvent, MidiNote, SynthParams};

/// Frames rendered per block; also the timing resolution of the note-off.
const BLOCK: usize = 64;

/// Below this level a sample counts as silence.
const SILENCE: f32 = 1e-4;

/// Render `note` played with `params` for `hold_seconds`, then released,
/// as mono samples at `sample_rate`.
///
/// Rendering stops once the voice and the FX tails have died away (50 ms of
/// silence after the release), or after `max_seconds`, whichever comes
/// first; trailing silence is trimmed. A lone note plays at a voice's full
/// level (no polyphony normalization), at the default master gain.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub fn render_note(
    params: &SynthParams,
    note: MidiNote,
    hold_seconds: f32,
    sample_rate: u32,
    max_seconds: f32,
) -> Vec<f32> {
    let rate = sample_rate as f32;
    let mut synth = SynthProcessor::<1>::new(rate);
    synth.set_voice_normalization(VoiceNormalization::Off);

    let max = (max_seconds.max(0.0) * rate) as usize;
    let hold = (hold_seconds.max(0.0) * rate) as usize;
    let tail = (0.05 * rate) as usize;
    let mut out = Vec::new();
    let mut quiet = 0;
    let mut events = vec![
        AudioEvent::LoadPatch(Box::new(params.clone())),
        AudioEvent::NoteOn(note),
    ];
    let mut released = false;
    let mut block = [0.0; BLOCK];

    while out.len() < max {
        if !released && out.len() >= hold {
            events.push(AudioEvent::NoteOff(note));
            released = true;
        }
        synth.process_block(&events, &mut block, 1);
        events.clear();
        out.extend_from_slice(&block);

        if block.iter().all(|s| s.abs() < SILENCE) {
            quiet += BLOCK;
        } else {
            quiet = 0;
        }
        if released && synth.is_idle() && quiet >= tail {
            break;
        }
    }
    out.truncate(max);
    let end = out
        .iter()
        .rposition(|s| s.abs() >= SILENCE)
        .map_or(0, |i| i + 1);
    out.truncate(end);
    out
}

/// Write `samples` (mono, `-1.0..=1.0`) as a 16-bit PCM WAV file.
///
/// # Errors
///
/// Returns any error from `out`, or `InvalidInput` if the data is too long
/// for a WAV file (over 4 GiB).
pub fn write_wav(
    mut out: impl std::io::Write,
    samples: &[f32],
    sample_rate: u32,
) -> std::io::Result<()> {
    let too_long = || std::io::Error::new(std::io::ErrorKind::InvalidInput, "too long for WAV");
    let data_len = u32::try_from(samples.len() * 2).map_err(|_| too_long())?;
    let riff_len = data_len.checked_add(36).ok_or_else(too_long)?;

    out.write_all(b"RIFF")?;
    out.write_all(&riff_len.to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16_u32.to_le_bytes())?; // fmt chunk size
    out.write_all(&1_u16.to_le_bytes())?; // PCM
    out.write_all(&1_u16.to_le_bytes())?; // mono
    out.write_all(&sample_rate.to_le_bytes())?;
    out.write_all(&(sample_rate * 2).to_le_bytes())?; // byte rate
    out.write_all(&2_u16.to_le_bytes())?; // block align
    out.write_all(&16_u16.to_le_bytes())?; // bits per sample
    out.write_all(b"data")?;
    out.write_all(&data_len.to_le_bytes())?;
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        #[allow(clippy::cast_possible_truncation)] // clamped to the i16 range
        let v = (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    out.write_all(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sfx::{SfxKind, generate};

    #[test]
    fn a_one_shot_stops_at_its_end_and_trims_the_tail() {
        let mut p = generate(SfxKind::Blip, 1);
        p.fx.reverb_mix = 0.0;
        let s = render_note(&p, MidiNote(72), 1.0, 44_100, 10.0);
        // The blip decays in ≤ 0.1 s but the key is held for 1 s; the
        // release ends shortly after it. Well under the 10 s limit.
        assert!(s.len() > 1_000 && s.len() < 60_000, "{} samples", s.len());
        assert!(s.last().unwrap().abs() >= SILENCE);
        assert!(s.iter().any(|x| x.abs() > 0.1));
    }

    #[test]
    fn a_held_drone_stops_at_the_limit() {
        let mut p = SynthParams::default();
        p.env.release = 20.0;
        let s = render_note(&p, MidiNote::A4, 30.0, 8_000, 2.0);
        assert_eq!(s.len(), 16_000);
    }

    #[test]
    fn matches_live_playback() {
        // Same processor, same events: the render is what a live voice plays.
        let p = generate(SfxKind::Laser, 3);
        let s = render_note(&p, MidiNote(72), 0.2, 44_100, 2.0);
        let mut synth = SynthProcessor::<1>::new(44_100.0);
        synth.set_voice_normalization(VoiceNormalization::Off);
        let mut live = vec![0.0; 2_048];
        synth.process_block(
            &[
                AudioEvent::LoadPatch(Box::new(p)),
                AudioEvent::NoteOn(MidiNote(72)),
            ],
            &mut live,
            1,
        );
        assert_eq!(&s[..2_048], &live[..]);
    }

    #[test]
    fn wav_header_and_data_are_well_formed() {
        let mut wav = Vec::new();
        write_wav(&mut wav, &[0.0, 1.0, -1.0, 2.0], 22_050).unwrap();
        assert_eq!(wav.len(), 44 + 8);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 44);
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 22_050);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 8);
        let pcm: Vec<i16> = wav[44..]
            .chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(pcm, [0, i16::MAX, -i16::MAX, i16::MAX]);
    }
}
