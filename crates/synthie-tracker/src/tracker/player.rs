//! Step-sequencer engine, run inside the audio callback.
//!
//! [`Player::new`] returns a UI-side [`Player`] handle and a
//! [`TrackerSequencer`] that is handed to the audio engine via
//! [`setup_audio_silenced_with_sequencer`].  The engine calls
//! [`TrackerSequencer::tick`] at exact frame offsets, so every track's notes
//! and drum hits land sample-accurately and stay locked together, independent
//! of the hardware buffer size.
//!
//! The current step index and playing state are exposed through
//! [`Arc<AtomicUsize>`] / [`Arc<AtomicBool>`] so the UI can read them
//! lock-free each frame.
//!
//! [`setup_audio_silenced_with_sequencer`]: synthie::audio::engine::setup_audio_silenced_with_sequencer

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crossbeam_channel::{Receiver, Sender, bounded};
use synthie::audio::engine::Sequencer;
use synthie::params::{AudioEvent, ChannelNo, DrumHit, MidiNote};

use super::{SYNTH_TRACKS, Song, Step};

/// Longest interval between ticks while idle or between steps, in frames.
/// Bounds how long a play / stop / edit message waits (~6 ms at 44.1 kHz).
const MAX_TICK_FRAMES: usize = 256;

// ─── Control messages ────────────────────────────────────────────────────────

/// Messages sent from the UI thread to the sequencer.
///
/// Songs travel by value: they are heap-free, so the audio thread never frees
/// memory when it replaces one.
#[allow(clippy::large_enum_variant)] // by-value on purpose, see above
enum PlayerCtrl {
    /// Start playback from the first step with the given song snapshot.
    Play(Song),
    /// Stop playback and silence all voices.
    Stop,
    /// Update the song snapshot without interrupting playback.
    Update(Song),
}

// ─── Player handle ───────────────────────────────────────────────────────────

/// UI-side handle to the [`TrackerSequencer`] running on the audio thread.
pub struct Player {
    ctrl_tx: Sender<PlayerCtrl>,
    /// Atomically updated with the index of the step that just fired.
    pub current_step: Arc<AtomicUsize>,
    /// `true` while the sequencer is running.
    pub is_playing: Arc<AtomicBool>,
}

impl Player {
    /// Create a handle and the sequencer it controls.  Pass the sequencer to
    /// the audio engine.
    #[must_use]
    pub fn new() -> (Self, TrackerSequencer) {
        let (ctrl_tx, ctrl_rx) = bounded::<PlayerCtrl>(64);
        let current_step = Arc::new(AtomicUsize::new(0));
        let is_playing = Arc::new(AtomicBool::new(false));

        let sequencer = TrackerSequencer {
            ctrl_rx,
            current_step: Arc::clone(&current_step),
            is_playing: Arc::clone(&is_playing),
            song: Song::default(),
            playing: false,
            step_idx: 0,
            until_step: 0.0,
            last_wait: 0,
            active_notes: [None; SYNTH_TRACKS],
        };
        let player = Self {
            ctrl_tx,
            current_step,
            is_playing,
        };
        (player, sequencer)
    }

    /// Start playback from the beginning of the pattern.
    pub fn play(&self, song: Song) {
        let _ = self.ctrl_tx.try_send(PlayerCtrl::Play(song));
    }

    /// Stop playback and send an audio panic.
    pub fn stop(&self) {
        let _ = self.ctrl_tx.try_send(PlayerCtrl::Stop);
    }

    /// Push an updated song snapshot without interrupting playback.
    pub fn update_song(&self, song: Song) {
        let _ = self.ctrl_tx.try_send(PlayerCtrl::Update(song));
    }
}

// ─── Sequencer ───────────────────────────────────────────────────────────────

/// Audio-thread half of the player; see the module docs.
pub struct TrackerSequencer {
    ctrl_rx: Receiver<PlayerCtrl>,
    current_step: Arc<AtomicUsize>,
    is_playing: Arc<AtomicBool>,
    song: Song,
    playing: bool,
    /// Step that fires next.
    step_idx: usize,
    /// Frames until the next step; fractional so tempo never drifts.
    until_step: f64,
    /// Frames returned by the previous tick, i.e. elapsed since then.
    last_wait: usize,
    /// Note currently sounding on each synth channel; notes sustain across
    /// empty steps until the next note or note-off.
    active_notes: [Option<MidiNote>; SYNTH_TRACKS],
}

impl Sequencer for TrackerSequencer {
    fn tick(&mut self, sample_rate: f32, out: &mut Vec<AudioEvent>) -> usize {
        #[allow(clippy::cast_precision_loss)] // ≤ MAX_TICK_FRAMES
        {
            self.until_step -= self.last_wait as f64;
        }

        while let Ok(msg) = self.ctrl_rx.try_recv() {
            match msg {
                PlayerCtrl::Play(song) => {
                    note_off_all(out, &mut self.active_notes);
                    self.song = song;
                    self.playing = true;
                    self.step_idx = 0;
                    self.until_step = 0.0;
                    self.is_playing.store(true, Ordering::Relaxed);
                }
                PlayerCtrl::Stop => {
                    self.playing = false;
                    self.is_playing.store(false, Ordering::Relaxed);
                    note_off_all(out, &mut self.active_notes);
                    out.push(AudioEvent::Panic);
                }
                PlayerCtrl::Update(song) => {
                    // Clamp so a pattern-length reduction can't fire an out-of-range step.
                    if self.step_idx >= song.pattern.len {
                        self.step_idx = 0;
                    }
                    self.song = song;
                }
            }
        }

        if !self.playing {
            self.last_wait = MAX_TICK_FRAMES;
            return self.last_wait;
        }

        // Fire the step once it is due (within half a frame).
        if self.until_step < 0.5 {
            fire_step(out, &self.song, self.step_idx, &mut self.active_notes);
            self.current_step.store(self.step_idx, Ordering::Relaxed);
            self.step_idx = (self.step_idx + 1) % self.song.pattern.len;
            self.until_step += step_frames(self.song.bpm, sample_rate);
        }

        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )] // clamped to 1..=MAX_TICK_FRAMES
        let wait = self.until_step.round().clamp(1.0, MAX_TICK_FRAMES as f64) as usize;
        self.last_wait = wait;
        wait
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Length of one 16th-note step in frames.
fn step_frames(bpm: f32, sample_rate: f32) -> f64 {
    f64::from(sample_rate) * 60.0 / (f64::from(bpm) * 4.0)
}

fn note_off_all(out: &mut Vec<AudioEvent>, active_notes: &mut [Option<MidiNote>; SYNTH_TRACKS]) {
    for (ch_idx, note) in active_notes.iter_mut().enumerate() {
        if let Some(n) = note.take() {
            #[allow(clippy::cast_possible_truncation)] // ch_idx < SYNTH_TRACKS ≤ 255
            let ch = ChannelNo(ch_idx as u8);
            out.push(AudioEvent::NoteOffChannel(ch, n));
        }
    }
}

fn fire_step(
    out: &mut Vec<AudioEvent>,
    song: &Song,
    step: usize,
    active_notes: &mut [Option<MidiNote>; SYNTH_TRACKS],
) {
    // Synth tracks: a note or note-off releases the sounding note; empty
    // steps let it ring on.
    for (track_idx, track_steps) in song.pattern.synth.iter().enumerate() {
        let step = track_steps[step];
        if step == Step::Empty {
            continue;
        }
        #[allow(clippy::cast_possible_truncation)] // track_idx < SYNTH_TRACKS ≤ 255
        let ch = ChannelNo(track_idx as u8);
        if let Some(prev) = active_notes[track_idx].take() {
            out.push(AudioEvent::NoteOffChannel(ch, prev));
        }
        if let Step::Note(note) = step {
            out.push(AudioEvent::NoteOnChannel(ch, note));
            active_notes[track_idx] = Some(note);
        }
    }

    // Trigger drum hits.
    let drum = &song.pattern.drums[step];
    if drum.kick {
        out.push(AudioEvent::Drum(DrumHit::Kick));
    }
    if drum.hihat_closed {
        out.push(AudioEvent::Drum(DrumHit::HiHatClosed));
    }
    if drum.hihat_open {
        out.push(AudioEvent::Drum(DrumHit::HiHatOpen));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_events(out: &mut Vec<AudioEvent>) -> Vec<(bool, u8)> {
        out.drain(..)
            .filter_map(|e| match e {
                AudioEvent::NoteOnChannel(_, n) => Some((true, n.as_u8())),
                AudioEvent::NoteOffChannel(_, n) => Some((false, n.as_u8())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn notes_sustain_until_next_note_or_off() {
        let mut out = Vec::new();
        let mut song = Song::default();
        song.pattern.synth[0][0] = Step::Note(MidiNote(60));
        song.pattern.synth[0][2] = Step::Note(MidiNote(62));
        song.pattern.synth[0][3] = Step::Off;
        let mut active = [None; SYNTH_TRACKS];

        fire_step(&mut out, &song, 0, &mut active);
        assert_eq!(synth_events(&mut out), [(true, 60)]);

        // Empty step: the note keeps ringing.
        fire_step(&mut out, &song, 1, &mut active);
        assert_eq!(synth_events(&mut out), []);

        fire_step(&mut out, &song, 2, &mut active);
        assert_eq!(synth_events(&mut out), [(false, 60), (true, 62)]);

        fire_step(&mut out, &song, 3, &mut active);
        assert_eq!(synth_events(&mut out), [(false, 62)]);
        assert_eq!(active[0], None);
    }

    /// Drive the sequencer like the engine does and record the frame at which
    /// each step's kick fires.
    #[test]
    fn steps_fire_on_exact_frames_without_drift() {
        const SR: f32 = 44_100.0;
        let (player, mut seq) = Player::new();
        let mut song = Song {
            bpm: 140.0,
            ..Song::default()
        };
        for drum in &mut song.pattern.drums {
            drum.kick = true;
        }
        player.play(song);

        let mut out = Vec::new();
        let mut frame = 0usize;
        let mut kicks = Vec::new();
        while kicks.len() < 200 {
            let wait = seq.tick(SR, &mut out);
            if out
                .drain(..)
                .any(|e| matches!(e, AudioEvent::Drum(DrumHit::Kick)))
            {
                kicks.push(frame);
            }
            frame += wait;
        }

        let step = step_frames(140.0, SR); // 4725 frames
        for (i, &at) in kicks.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let ideal = i as f64 * step;
            #[allow(clippy::cast_precision_loss)]
            let err = (at as f64 - ideal).abs();
            assert!(err <= 1.0, "step {i} at frame {at}, expected {ideal}");
        }
    }
}
