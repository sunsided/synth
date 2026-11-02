//! Kernkraft 400 (Zombie Nation, 1999), the whole song, after the hook of
//! David Whittaker's Lazy Jones (C64, 1984).
//!
//! Transcribed from Arne Mulder's General MIDI sequence (Toplist Sequencing
//! Team, 2000): 70 bpm in half time, Bb minor, 76 bars of sixteenths. Every
//! MIDI track maps to one lane; doubled tracks and sound effects are left
//! out, and 32nd-note rolls fall onto the sixteenth grid.
//!
//! | Bars | Section |
//! |---|---|
//! | 1-12 | Intro: sub-bass, kick and claps, octave ostinati |
//! | 13-32 | Hook, with a snare build into sixteenth hats |
//! | 33-40 | Breakdown riff over a G/Bb sub-bass |
//! | 41-51 | Bb pedal build |
//! | 52-75 | Hook again, to the final crash |
//!
//! The song runs on a [`SongPlayer`], so every note lands sample-accurately.
//! Each lane has a channel of its own: lanes sharing a channel would release
//! each other's notes whenever they play the same pitch.
//!
//! ```sh
//! cargo run -p synthie --example kernkraft --release
//! ```

use std::time::Duration;

use anyhow::Result;
use synthie::audio::engine::setup_audio_n_with_sequencer;
use synthie::params::ChannelNo;
use synthie::pattern::{Song, SongPlayer, Tempo};
use synthie::presets::sid::Preset;

/// Synthesis channels: one per melodic lane, plus spares.
const CHANNELS: usize = 8;

/// Bars in the song.
const BARS: usize = 76;

// ─── Melody and bass ─────────────────────────────────────────────────────────

const REST: &str = ". . . . . . . . . . . . . . . .";
/// Releases a note held over from the previous bar.
const OFF: &str = "- . . . . . . . . . . . . . . .";

/// The Lazy Jones hook: a Bbm arpeggio twice, then Eb and F.
const HOOK_LEAD_1: &str = ". Bb3 C#4 Eb4 F4 - . . . Bb3 C#4 Eb4 F4 F#4 F4 C#4";
const HOOK_LEAD_2: &str = "Eb4 - . . C#4 - F4 Bb3 - . . . . . . .";

/// Octave-jumping bass under the hook, following the chords.
const HOOK_BASS_1: &str = "Bb1 Bb2 Bb1 Bb2 Bb1 Bb2 Bb1 Bb2 Bb1 Bb2 Bb1 Bb2 Bb1 Bb2 Bb1 Bb2";
const HOOK_BASS_2: &str = "Eb2 Eb3 Eb2 Eb3 F2 F3 F2 F3 Bb1 Bb2 Bb1 Bb2 Bb1 Bb2 Bb1 Bb2";

/// The intro ostinato, upper octave first, against [`COUNTER`].
const OSTINATO: &str = "Bb2 Bb1 Bb2 Bb1 Bb2 Bb1 Bb2 Bb1 Bb2 Bb1 Bb2 Bb1 Bb2 Bb1 Bb2 Bb1";
const COUNTER: &str = HOOK_BASS_1;

const SUB: &str = "Bb0 Bb1 Bb0 Bb1 Bb0 Bb1 Bb0 Bb1 Bb0 Bb1 Bb0 Bb1 Bb0 Bb1 Bb0 Bb1";
/// Breakdown sub-bass: G pushing into Bb.
const SUB_G: &str = "G1 G1 Bb1 Bb1 Bb1 Bb1 Bb1 Bb1 G1 G1 Bb1 Bb1 Bb1 Bb1 Bb1 Bb1";
const SUB_G_END: &str = "G1 G1 Bb1 Bb1 Bb1 Bb1 Bb1 Bb1 - . . . . . . .";
const PEDAL: &str = "Bb0 Bb0 Bb0 Bb0 Bb0 Bb0 Bb0 Bb0 Bb0 Bb0 Bb0 Bb0 Bb0 Bb0 Bb0 Bb0";

/// The breakdown riff.
const STAB_1: &str = "G2 F#2 - . . . . . G2 F#2 - . . . . .";
const STAB_2: &str = "G2 F#2 - . . . . . G2 F#2 D2 C2 - . . .";

// ─── Drums ───────────────────────────────────────────────────────────────────

const KICK_8THS: &str = "k . k . k . k . k . k . k . k .";
const KICK_HALF: &str = "k . k . k . k . . . . . . . . .";
const GROOVE: &str = "k . k h k+x . k . k . k h k+x . k .";
const BREAK: &str = ". . . h x . . . . . . h x . . .";
const KICK_ROLL: &str = "k k k k+h k+x k k k k k k k+h k+x k k k";
const CRASH: &str = "c . . . . . . . . . . . . . . .";
const CRASH_KICKS: &str = "k+c . k . k . k . k . k . k . k .";

const SNARE_1: &str = ". . s s . . s . . . s s . . s .";
const SNARE_2: &str = ". . s s . . s . . s s s . s s s";
const KICK_SNARE_1: &str = "k . k+s s k . k+s . k . k+s s k . k+s .";
const KICK_SNARE_2: &str = "k . k+s s k . k+s . k s k+s s k s k+s s";
const HATS_1: &str = "k h k+s s+h k h k+s h k h k+s s+h k h k+s h";
const HATS_2: &str = "k h k+s s+h k h k+s h k s+h k+s s+h k s+h k+s s+h";
const HATS_FILL: &str = "k h k+s s+h k h k+s h k s+h k+s k+s+h k k+s+h k+s k+s+h";
const SIXTEENTHS_1: &str = "k+h h k+s+h s+h k+h h k+s+h h k+h h k+s+h s+h k+h h k+s+h h";
const SIXTEENTHS_2: &str = "k+h h k+s+h s+h k+h h k+s+h h k+h s+h k+s+h s+h k+h s+h k+s+h s+h";
const RIFF_BEAT: &str = "k+h h k+h h k+h+x h k+h h k+h h k+h h k+h+x h k+h h";
const PEDAL_BEAT: &str = "k . . k+h k . . . k . . k+h k . . .";
const PEDAL_CRASH: &str = "k+c . . k+h k . . . k . . k+h k . . .";

/// The hook's drums: silence, a crash, then a 16-bar snare build.
const HOOK_DRUMS: [(usize, &str); 19] = [
    (2, REST),
    (1, CRASH),
    (1, REST),
    (1, SNARE_1),
    (1, SNARE_2),
    (1, SNARE_1),
    (1, SNARE_2),
    (1, KICK_SNARE_1),
    (1, KICK_SNARE_2),
    (1, KICK_SNARE_1),
    (1, KICK_SNARE_2),
    (1, HATS_1),
    (1, HATS_2),
    (1, HATS_1),
    (1, HATS_FILL),
    (1, SIXTEENTHS_1),
    (1, SIXTEENTHS_2),
    (1, SIXTEENTHS_1),
    (1, SIXTEENTHS_2),
];

// ─── Arrangement ─────────────────────────────────────────────────────────────

/// Join `(bars, pattern)` sections into one lane; checks the length.
fn lane(sections: &[(usize, &str)]) -> String {
    let bars: usize = sections.iter().map(|&(n, _)| n).sum();
    assert_eq!(bars, BARS, "lane is {bars} bars long");
    sections
        .iter()
        .flat_map(|&(n, bar)| std::iter::repeat_n(bar, n))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// `pairs` repetitions of a two-bar phrase.
fn phrase(pairs: usize, first: &'static str, second: &'static str) -> Vec<(usize, &'static str)> {
    std::iter::repeat_n([(1, first), (1, second)], pairs)
        .flatten()
        .collect()
}

fn lead() -> String {
    let mut s = vec![(12, REST)];
    s.extend(phrase(10, HOOK_LEAD_1, HOOK_LEAD_2));
    s.push((19, REST));
    s.extend(phrase(12, HOOK_LEAD_1, HOOK_LEAD_2));
    s.push((1, REST));
    lane(&s)
}

fn bass() -> String {
    let mut s = vec![(5, REST), (7, OSTINATO)];
    s.extend(phrase(10, HOOK_BASS_1, HOOK_BASS_2));
    s.extend([(1, OFF), (11, REST), (7, OSTINATO)]);
    s.extend(phrase(12, HOOK_BASS_1, HOOK_BASS_2));
    s.push((1, OFF));
    lane(&s)
}

fn counter() -> String {
    lane(&[
        (7, REST),
        (5, COUNTER),
        (1, OFF),
        (33, REST),
        (5, COUNTER),
        (1, OFF),
        (24, REST),
    ])
}

fn sub() -> String {
    lane(&[
        (1, REST),
        (11, SUB),
        (1, OFF),
        (19, REST),
        (7, SUB_G),
        (1, SUB_G_END),
        (11, SUB),
        (1, OFF),
        (24, REST),
    ])
}

fn pedal() -> String {
    lane(&[(40, REST), (11, PEDAL), (1, OFF), (24, REST)])
}

fn stab() -> String {
    let mut s = vec![(32, REST)];
    s.extend(phrase(4, STAB_1, STAB_2));
    s.push((36, REST));
    lane(&s)
}

fn drums() -> String {
    let mut s = vec![
        (1, REST),
        (2, KICK_8THS),
        (5, GROOVE),
        (1, BREAK),
        (2, GROOVE),
        (1, KICK_ROLL),
    ];
    s.extend(HOOK_DRUMS);
    s.extend([
        (4, RIFF_BEAT),
        (1, CRASH_KICKS),
        (2, KICK_8THS),
        (1, KICK_HALF),
        (1, PEDAL_CRASH),
        (10, PEDAL_BEAT),
    ]);
    s.extend(HOOK_DRUMS);
    s.extend([(1, CRASH), (3, REST), (1, CRASH)]);
    lane(&s)
}

/// Switch the lane to patch `index` on its first row: `.` becomes `@n`,
/// a note `X` becomes `X@n`.
fn with_patch(pattern: &str, index: usize) -> String {
    let (first, rest) = pattern.split_once(' ').unwrap_or((pattern, ""));
    let first = if first == "." {
        String::new()
    } else {
        first.to_string()
    };
    format!("{first}@{index} {rest}")
}

fn song() -> Result<Song> {
    let lanes = [
        (Preset::ArpLead, lead()),
        (Preset::C64Bass, bass()),
        (Preset::C64Bass, counter()),
        (Preset::SlideBass, sub()),
        (Preset::SlideBass, pedal()),
        (Preset::MetalPulse, stab()),
    ];
    let mut song = Song::new(Tempo::bpm(70.0, 4)).one_shot();
    for (i, (patch, pattern)) in lanes.into_iter().enumerate() {
        let ch = ChannelNo(u8::try_from(i)?);
        song = song
            .patch(patch.params())
            .lane(ch, &with_patch(&pattern, i))?;
    }
    // Drum hits ignore the lane's channel.
    Ok(song.lane(ChannelNo(7), &drums())?)
}

fn main() -> Result<()> {
    let song = song()?;
    let seconds = song.duration_secs() + 3.0;
    let (_stream, _events, _scope) = setup_audio_n_with_sequencer::<CHANNELS, _, _>(
        cpal::BufferSize::Default,
        SongPlayer::new(song),
        |err| eprintln!("audio stream error: {err}"),
    )?;
    std::thread::sleep(Duration::from_secs_f32(seconds));
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_song_parses_and_every_lane_is_76_bars() {
        let song = super::song().unwrap();
        assert_eq!(song.lanes.len(), 7);
        for lane in &song.lanes {
            assert_eq!(lane.rows.len(), super::BARS * 16);
        }
        // The patch change rides on the first row instead of adding one.
        assert_eq!(song.lanes[0].rows[0].patch, Some(0));
        assert_eq!(song.lanes[5].rows[0].patch, Some(5));
    }
}
