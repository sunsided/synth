//! Background playback thread: step-sequencer engine.
//!
//! [`Player::spawn`] starts a dedicated thread that listens on a control
//! channel.  The UI thread drives it via [`Player::play`], [`Player::stop`],
//! and [`Player::update_song`].
//!
//! The current step index and playing state are exposed through
//! [`Arc<AtomicUsize>`] / [`Arc<AtomicBool>`] so the UI can read them
//! lock-free each frame without blocking the audio callback.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use synthie::params::{AudioEvent, ChannelNo, DrumHit, MidiNote, Patch};

use super::{Song, SYNTH_TRACKS};

// ─── Control messages ────────────────────────────────────────────────────────

/// Messages sent from the UI thread to the player thread.
enum PlayerCtrl {
    /// Start playback with the given song snapshot and preset bank.
    Play(Box<Song>, Vec<Patch>),
    /// Stop playback and silence all voices.
    Stop,
    /// Update the song snapshot without interrupting playback.
    Update(Box<Song>),
}

// ─── Player handle ───────────────────────────────────────────────────────────

/// Handle to the background playback thread.
pub struct Player {
    ctrl_tx: Sender<PlayerCtrl>,
    /// Atomically updated with the index of the step that just fired.
    pub current_step: Arc<AtomicUsize>,
    /// `true` while the sequencer is running.
    pub is_playing: Arc<AtomicBool>,
}

impl Player {
    /// Spawn the player thread and return a handle.
    ///
    /// The thread keeps a reference to `audio_tx` and drives the synth engine
    /// via [`AudioEvent`] messages.
    pub fn spawn(audio_tx: Sender<AudioEvent>) -> Self {
        let (ctrl_tx, ctrl_rx) = bounded::<PlayerCtrl>(64);
        let current_step = Arc::new(AtomicUsize::new(0));
        let is_playing = Arc::new(AtomicBool::new(false));

        let step_arc = Arc::clone(&current_step);
        let playing_arc = Arc::clone(&is_playing);

        std::thread::Builder::new()
            .name("synthie-tracker-player".into())
            .spawn(move || run(audio_tx, ctrl_rx, step_arc, playing_arc))
            .expect("failed to spawn player thread");

        Self {
            ctrl_tx,
            current_step,
            is_playing,
        }
    }

    /// Start playback from the beginning of the pattern.
    pub fn play(&self, song: Box<Song>, patches: Vec<Patch>) {
        let _ = self.ctrl_tx.send(PlayerCtrl::Play(song, patches));
    }

    /// Stop playback and send an audio panic.
    pub fn stop(&self) {
        let _ = self.ctrl_tx.send(PlayerCtrl::Stop);
    }

    /// Push an updated song snapshot to the player without interrupting playback.
    pub fn update_song(&self, song: Box<Song>) {
        let _ = self.ctrl_tx.send(PlayerCtrl::Update(song));
    }
}

// ─── Step duration ───────────────────────────────────────────────────────────

/// Duration of one 16th-note step at the given BPM.
fn step_duration(bpm: f32) -> Duration {
    Duration::from_secs_f32(60.0 / (bpm * 4.0))
}

// ─── Player loop ─────────────────────────────────────────────────────────────

#[allow(clippy::needless_pass_by_value)] // owned values required by thread lifetime
fn run(
    audio_tx: Sender<AudioEvent>,
    ctrl_rx: Receiver<PlayerCtrl>,
    current_step: Arc<AtomicUsize>,
    is_playing: Arc<AtomicBool>,
) {
    let mut playing = false;
    let mut song = Box::new(Song::default());
    let mut step_idx: usize = 0;
    let mut step_deadline = Instant::now();
    // Track which note is currently sounding on each synth channel.
    let mut active_notes = [None::<MidiNote>; SYNTH_TRACKS];

    loop {
        let now = Instant::now();
        let timeout = if playing {
            step_deadline.saturating_duration_since(now)
        } else {
            Duration::from_millis(50)
        };

        match ctrl_rx.recv_timeout(timeout) {
            Ok(PlayerCtrl::Play(new_song, patches)) => {
                note_off_all(&audio_tx, &mut active_notes);
                // (Re)load presets for each channel from the patch bank.
                load_patches(&audio_tx, &new_song, &patches);
                song = new_song;
                playing = true;
                step_idx = 0;
                step_deadline = Instant::now();
                is_playing.store(true, Ordering::Relaxed);
                fire_step(&audio_tx, &song, step_idx, &mut active_notes);
                current_step.store(step_idx, Ordering::Relaxed);
                step_deadline += step_duration(song.bpm);
                step_idx = (step_idx + 1) % song.pattern.len;
            }

            Ok(PlayerCtrl::Stop) => {
                playing = false;
                is_playing.store(false, Ordering::Relaxed);
                note_off_all(&audio_tx, &mut active_notes);
                let _ = audio_tx.send(AudioEvent::Panic);
            }

            Ok(PlayerCtrl::Update(new_song)) => {
                // BPM change takes effect from the next step deadline onwards.
                song = new_song;
            }

            Err(RecvTimeoutError::Timeout) if playing => {
                // Advance the sequencer, catching up if we fell behind.
                let dur = step_duration(song.bpm);
                while Instant::now() >= step_deadline {
                    fire_step(&audio_tx, &song, step_idx, &mut active_notes);
                    current_step.store(step_idx, Ordering::Relaxed);
                    step_deadline += dur;
                    step_idx = (step_idx + 1) % song.pattern.len;
                }
            }

            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn load_patches(audio_tx: &Sender<AudioEvent>, song: &Song, patches: &[Patch]) {
    for (ch_idx, &patch_idx) in song.track_patches.iter().enumerate() {
        if let Some(patch) = patches.get(patch_idx) {
            #[allow(clippy::cast_possible_truncation)] // ch_idx < SYNTH_TRACKS ≤ 255
            let ch = ChannelNo(ch_idx as u8);
            let _ = audio_tx.send(AudioEvent::LoadPatchChannel(
                ch,
                Box::new(patch.params.clone()),
            ));
        }
    }
}

fn note_off_all(audio_tx: &Sender<AudioEvent>, active_notes: &mut [Option<MidiNote>; SYNTH_TRACKS]) {
    for (ch_idx, note) in active_notes.iter_mut().enumerate() {
        if let Some(n) = note.take() {
            #[allow(clippy::cast_possible_truncation)] // ch_idx < SYNTH_TRACKS ≤ 255
            let ch = ChannelNo(ch_idx as u8);
            let _ = audio_tx.send(AudioEvent::NoteOffChannel(ch, n));
        }
    }
}

fn fire_step(
    audio_tx: &Sender<AudioEvent>,
    song: &Song,
    step: usize,
    active_notes: &mut [Option<MidiNote>; SYNTH_TRACKS],
) {
    // Release all still-sounding notes before triggering new ones.
    note_off_all(audio_tx, active_notes);

    // Trigger synth-track notes.
    for (track_idx, track_steps) in song.pattern.synth.iter().enumerate() {
        if let Some(note) = track_steps[step].note {
            #[allow(clippy::cast_possible_truncation)] // track_idx < SYNTH_TRACKS ≤ 255
            let ch = ChannelNo(track_idx as u8);
            let _ = audio_tx.send(AudioEvent::NoteOnChannel(ch, note));
            active_notes[track_idx] = Some(note);
        }
    }

    // Trigger drum hits.
    let drum = &song.pattern.drums[step];
    if drum.kick {
        let _ = audio_tx.send(AudioEvent::Drum(DrumHit::Kick));
    }
    if drum.hihat_closed {
        let _ = audio_tx.send(AudioEvent::Drum(DrumHit::HiHatClosed));
    }
    if drum.hihat_open {
        let _ = audio_tx.send(AudioEvent::Drum(DrumHit::HiHatOpen));
    }
}
