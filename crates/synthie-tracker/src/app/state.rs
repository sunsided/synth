//! Central application state for the tracker.
//!
//! [`TrackerState`] owns the [`Song`] data, cursor position, and the
//! [`Player`] handle.  All user actions go through mutation methods here;
//! the UI and input modules read from and call into this struct.

use std::sync::atomic::Ordering;

use crossbeam_channel::Sender;
use synthie::params::{AudioEvent, ChannelNo, MidiNote, Patch};
use synthie::presets::sid::default_patches;

use crate::tracker::player::Player;
use crate::tracker::{DRUM_TRACK, DrumStep, STEPS, SYNTH_TRACKS, Song, TOTAL_TRACKS};

// ─── DrumKind ────────────────────────────────────────────────────────────────

/// Which drum instrument to toggle on the drum track.
#[derive(Copy, Clone)]
pub enum DrumKind {
    Kick,
    HiHatClosed,
    HiHatOpen,
}

// ─── TrackerState ────────────────────────────────────────────────────────────

/// All mutable state owned by the UI thread.
pub struct TrackerState {
    /// Song being edited.
    pub song: Song,
    /// Loaded preset bank.
    pub presets: Vec<Patch>,

    /// Cursor row (step index, 0..`song.pattern.len`).
    pub cursor_row: usize,
    /// Cursor track column (0..`TOTAL_TRACKS`).
    pub cursor_track: usize,

    /// Current keyboard octave for note entry.
    pub octave: i8,
    /// `true` → show the help overlay.
    pub show_help: bool,

    /// Background playback thread.
    player: Player,
    /// Channel for direct audio events (patch loads, panics).
    event_tx: Sender<AudioEvent>,
}

impl TrackerState {
    /// Construct initial state and pre-load presets onto the audio engine.
    pub fn new(event_tx: Sender<AudioEvent>) -> Self {
        let presets = default_patches();
        let player = Player::spawn(event_tx.clone());

        // Pre-load default presets onto each channel.
        for (ch_idx, patch) in presets.iter().take(SYNTH_TRACKS).enumerate() {
            #[allow(clippy::cast_possible_truncation)] // ch_idx < SYNTH_TRACKS ≤ 255
            let ch = ChannelNo(ch_idx as u8);
            let _ = event_tx.send(AudioEvent::LoadPatchChannel(
                ch,
                Box::new(patch.params.clone()),
            ));
        }

        Self {
            song: Song::default(),
            presets,
            cursor_row: 0,
            cursor_track: 0,
            octave: 4,
            show_help: false,
            player,
            event_tx,
        }
    }

    // ─── Playback ────────────────────────────────────────────────────────────

    /// `true` while the sequencer is running.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.player.is_playing.load(Ordering::Relaxed)
    }

    /// Index of the step that most recently fired during playback.
    #[must_use]
    pub fn playing_step(&self) -> usize {
        self.player.current_step.load(Ordering::Relaxed)
    }

    /// Start playback from the first step.
    pub fn play(&self) {
        self.player
            .play(Box::new(self.song.clone()), self.presets.clone());
    }

    /// Stop playback.
    pub fn stop(&self) {
        self.player.stop();
    }

    /// Toggle playback state.
    pub fn toggle_play(&self) {
        if self.is_playing() {
            self.stop();
        } else {
            self.play();
        }
    }

    /// Push the current song snapshot to the player thread without restarting.
    fn notify_player(&self) {
        self.player.update_song(Box::new(self.song.clone()));
    }

    // ─── Cursor movement ─────────────────────────────────────────────────────

    /// Move the cursor up one row, wrapping to the last step.
    pub fn cursor_up(&mut self) {
        if self.cursor_row == 0 {
            self.cursor_row = self.song.pattern.len - 1;
        } else {
            self.cursor_row -= 1;
        }
    }

    /// Move the cursor down one row, wrapping to the first step.
    pub fn cursor_down(&mut self) {
        self.cursor_row = (self.cursor_row + 1) % self.song.pattern.len;
    }

    /// Move the cursor left one track, wrapping.
    pub fn cursor_left(&mut self) {
        if self.cursor_track == 0 {
            self.cursor_track = TOTAL_TRACKS - 1;
        } else {
            self.cursor_track -= 1;
        }
    }

    /// Move the cursor right one track, wrapping.
    pub fn cursor_right(&mut self) {
        self.cursor_track = (self.cursor_track + 1) % TOTAL_TRACKS;
    }

    // ─── Note entry ──────────────────────────────────────────────────────────

    /// Enter a note at the cursor position on a synth track, then advance.
    ///
    /// No-op if the cursor is on the drum track.
    pub fn enter_note(&mut self, midi: MidiNote) {
        if self.cursor_track < SYNTH_TRACKS {
            self.song.pattern.synth[self.cursor_track][self.cursor_row].note = Some(midi);
            self.notify_player();
            self.cursor_down();
        }
    }

    /// Clear the step at the cursor position, then advance.
    ///
    /// On a synth track this removes the note; on the drum track it clears all hits.
    pub fn clear_step(&mut self) {
        if self.cursor_track < SYNTH_TRACKS {
            self.song.pattern.synth[self.cursor_track][self.cursor_row].note = None;
        } else {
            self.song.pattern.drums[self.cursor_row] = DrumStep::default();
        }
        self.notify_player();
        self.cursor_down();
    }

    // ─── Drum entry ──────────────────────────────────────────────────────────

    /// Toggle a drum hit at the cursor row and advance.
    ///
    /// No-op if the cursor is on a synth track.
    pub fn toggle_drum(&mut self, kind: DrumKind) {
        if self.cursor_track == DRUM_TRACK {
            let step = &mut self.song.pattern.drums[self.cursor_row];
            match kind {
                DrumKind::Kick => step.kick = !step.kick,
                DrumKind::HiHatClosed => step.hihat_closed = !step.hihat_closed,
                DrumKind::HiHatOpen => step.hihat_open = !step.hihat_open,
            }
            self.notify_player();
            self.cursor_down();
        }
    }

    // ─── BPM / octave ────────────────────────────────────────────────────────

    /// Increase BPM by 1 (max 300).
    pub fn bpm_up(&mut self) {
        self.song.bpm = (self.song.bpm + 1.0).min(300.0);
        self.notify_player();
    }

    /// Decrease BPM by 1 (min 30).
    pub fn bpm_down(&mut self) {
        self.song.bpm = (self.song.bpm - 1.0).max(30.0);
        self.notify_player();
    }

    /// Raise the keyboard octave by one (max 8).
    pub fn octave_up(&mut self) {
        self.octave = (self.octave + 1).min(8);
    }

    /// Lower the keyboard octave by one (min 0).
    pub fn octave_down(&mut self) {
        self.octave = (self.octave - 1).max(0);
    }

    // ─── Patch selection ─────────────────────────────────────────────────────

    /// Cycle to the next preset on the current synth track.
    pub fn next_patch(&mut self) {
        if self.cursor_track < SYNTH_TRACKS {
            let current = self.song.track_patches[self.cursor_track];
            let next = (current + 1) % self.presets.len();
            self.song.track_patches[self.cursor_track] = next;
            self.reload_track_patch(self.cursor_track);
        }
    }

    /// Cycle to the previous preset on the current synth track.
    pub fn prev_patch(&mut self) {
        if self.cursor_track < SYNTH_TRACKS {
            let current = self.song.track_patches[self.cursor_track];
            let prev = if current == 0 {
                self.presets.len() - 1
            } else {
                current - 1
            };
            self.song.track_patches[self.cursor_track] = prev;
            self.reload_track_patch(self.cursor_track);
        }
    }

    // ─── Panic ───────────────────────────────────────────────────────────────

    /// Silence all voices immediately.
    pub fn panic_all(&self) {
        let _ = self.event_tx.send(AudioEvent::Panic);
    }

    // ─── Pattern length ──────────────────────────────────────────────────────

    /// Increase the active pattern length by 1 step (max `STEPS`).
    pub fn pattern_len_up(&mut self) {
        if self.song.pattern.len < STEPS {
            self.song.pattern.len += 1;
            self.notify_player();
        }
    }

    /// Decrease the active pattern length by 1 step (min 1).
    pub fn pattern_len_down(&mut self) {
        if self.song.pattern.len > 1 {
            self.song.pattern.len -= 1;
            // Keep cursor in bounds.
            if self.cursor_row >= self.song.pattern.len {
                self.cursor_row = self.song.pattern.len - 1;
            }
            self.notify_player();
        }
    }

    // ─── Internal helpers ────────────────────────────────────────────────────

    fn reload_track_patch(&self, track: usize) {
        let patch_idx = self.song.track_patches[track];
        if let Some(patch) = self.presets.get(patch_idx) {
            #[allow(clippy::cast_possible_truncation)] // track < SYNTH_TRACKS ≤ 255
            let ch = ChannelNo(track as u8);
            let _ = self.event_tx.send(AudioEvent::LoadPatchChannel(
                ch,
                Box::new(patch.params.clone()),
            ));
        }
    }
}
