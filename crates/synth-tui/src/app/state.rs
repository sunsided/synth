//! Application state: UI section navigation, parameter adjustment, and
//! the bridge between the UI thread and the audio engine.

use crossbeam_channel::Sender;
use synthie::params::{AudioEvent, MidiNote, Patch, SynthParams};
use synthie::presets::sid;
use synthie::sfx::{self, SfxKind};
use synthie_ui::Modal;

/// Top-level UI section, each corresponding to one panel of controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// Oscillator controls (waveform, pulse width, detune, noise mix).
    Osc,
    /// Amplitude envelope controls (ADSR, reverse, glide).
    Env,
    /// Filter controls (mode, cutoff, resonance, drive).
    Filter,
    /// LFO controls (rate, depth, target).
    Lfo,
    /// Full modulation editor: LFO1, LFO2, filter env, pitch env.
    Mod,
    /// Sound-effect generator, slide, pitch jump, pulse-width sweep, repeat.
    Sfx,
    /// Effects controls (reverb mix, size, damping).
    Fx,
    /// Preset list (browse and load patches).
    Presets,
}

impl Section {
    /// Ordered slice of all variants, used for Tab-cycling.
    pub const ALL: &'static [Section] = &[
        Section::Osc,
        Section::Env,
        Section::Filter,
        Section::Lfo,
        Section::Mod,
        Section::Sfx,
        Section::Fx,
        Section::Presets,
    ];

    /// Short uppercase display name shown in panel headers.
    pub fn name(self) -> &'static str {
        match self {
            Section::Osc => "OSC",
            Section::Env => "ENV",
            Section::Filter => "FILTER",
            Section::Lfo => "LFO",
            Section::Mod => "MOD",
            Section::Sfx => "SFX",
            Section::Fx => "FX",
            Section::Presets => "PRESETS",
        }
    }

    /// Return the next section, wrapping around.
    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// Return the previous section, wrapping around.
    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        let len = Self::ALL.len();
        Self::ALL[(idx + len - 1) % len]
    }

    /// Number of adjustable parameters in this section (0 = list navigation only).
    pub fn param_count(self) -> usize {
        match self {
            Section::Osc => 10, // wave, pw, det, noise | separator | wave2, det2, semi2, mix2, sync
            Section::Filter => 4,
            Section::Env => 6, // attack, decay, sustain, release, env_reverse, glide
            Section::Lfo | Section::Fx => 3, // rate/reverb_mix, depth/reverb_size, target/reverb_damping
            Section::Mod => 21, // LFO1(4) + sep + LFO2(4) + sep + FiltEnv(5) + sep + PitchEnv(5)
            Section::Sfx => 8, // generator, slide, accel, min freq, jump, jump time, pw sweep, repeat
            Section::Presets => 0, // list mode
        }
    }

    /// Returns `true` if `idx` is a display-only row that cannot be adjusted.
    /// Used by navigation to skip non-interactive rows (e.g. the OSC2 separator).
    fn is_display_row(self, idx: usize) -> bool {
        match self {
            Section::Osc => idx == 4,
            Section::Mod => matches!(idx, 4 | 9 | 15),
            _ => false,
        }
    }
}

/// What Enter does in the SFX section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SfxAction {
    /// A new random patch of this category.
    Generate(SfxKind),
    /// Every parameter random.
    Randomize,
    /// Nudge the current patch a little.
    Mutate,
}

impl SfxAction {
    /// Every action, in the order the selector cycles through them.
    const ALL: [SfxAction; 9] = [
        SfxAction::Generate(SfxKind::Pickup),
        SfxAction::Generate(SfxKind::Laser),
        SfxAction::Generate(SfxKind::Explosion),
        SfxAction::Generate(SfxKind::PowerUp),
        SfxAction::Generate(SfxKind::Hit),
        SfxAction::Generate(SfxKind::Jump),
        SfxAction::Generate(SfxKind::Blip),
        SfxAction::Randomize,
        SfxAction::Mutate,
    ];

    /// Display name in the selector.
    fn name(self) -> &'static str {
        match self {
            SfxAction::Generate(kind) => kind.name(),
            SfxAction::Randomize => "Random",
            SfxAction::Mutate => "Mutate",
        }
    }
}

/// Number of computer-keyboard piano keys: two rows of one octave each.
pub const PIANO_KEY_COUNT: usize = 24;

/// Central application state owned by the UI thread.
pub struct AppState {
    /// Live parameter snapshot; updated by `adjust_*` methods and preset loads.
    pub params: SynthParams,
    /// Currently focused UI panel.
    pub selected_section: Section,
    /// Index of the focused parameter within `selected_section`.
    pub selected_param: usize,
    /// MIDI notes currently held, oldest first; lit on the piano strip.
    pub held_notes: Vec<MidiNote>,
    /// Note started by each computer-keyboard piano key (two octaves), so a
    /// release turns off that note even if the octave changed meanwhile.
    key_notes: [Option<MidiNote>; PIANO_KEY_COUNT],
    /// Current keyboard octave (piano keys map to this octave and octave+1).
    pub octave: i8,
    /// Envelope stage name for potential future status feedback (unused in display).
    #[allow(dead_code)]
    pub envelope_stage_name: String,
    /// Name of the currently loaded patch, shown in the title.
    pub current_patch_name: String,
    /// All available patches (built-in + any saved user patches).
    pub patches: Vec<Patch>,
    /// Index of the highlighted entry in the preset list.
    pub selected_preset: usize,
    /// Section and param focused before jumping to the preset browser with
    /// [`AppState::toggle_presets`], restored when jumping back.
    return_focus: (Section, usize),
    /// When `true`, the help overlay is drawn instead of the main UI.
    pub show_help: bool,
    /// Quit confirmation / `:` command line, when open.
    pub modal: Modal,
    /// Transient status message shown in the status bar (e.g. "Loaded: …").
    pub status_msg: String,
    /// Index into [`SfxAction::ALL`] of the SFX section's selector.
    sfx_action: usize,
    /// Seed of the next generated sound effect.
    sfx_seed: u64,
    /// Channel to send `AudioEvent` messages to the audio thread.
    audio_tx: Sender<AudioEvent>,
}

impl AppState {
    /// Construct initial application state and seed the audio thread with the
    /// first preset from the built-in SID bank.
    pub fn new(audio_tx: Sender<AudioEvent>) -> Self {
        let patches = sid::default_patches();
        let current_patch_name = patches
            .first()
            .map_or_else(|| "Default".to_string(), |p| p.name.clone());

        // Seed the audio thread with the first preset.
        if let Some(p) = patches.first() {
            let _ = audio_tx.send(AudioEvent::LoadPatch(Box::new(p.params.clone())));
        }

        Self {
            params: patches
                .first()
                .map(|p| p.params.clone())
                .unwrap_or_default(),
            selected_section: Section::Osc,
            selected_param: 0,
            held_notes: Vec::new(),
            key_notes: [None; PIANO_KEY_COUNT],
            octave: 4,
            envelope_stage_name: "Idle".to_string(),
            current_patch_name,
            patches,
            selected_preset: 0,
            return_focus: (Section::Osc, 0),
            show_help: false,
            modal: Modal::None,
            status_msg: String::new(),
            sfx_action: 0,
            // Different sounds each session; no clock needed (wasm-safe).
            sfx_seed: std::hash::BuildHasher::hash_one(&std::hash::RandomState::new(), 0_u8),
            audio_tx,
        }
    }

    /// Send an `AudioEvent` to the audio thread (best-effort; drops if channel full).
    fn send(&self, event: AudioEvent) {
        let _ = self.audio_tx.send(event);
    }

    /// The most recently pressed note still held, for the header and the
    /// output panel.
    #[must_use]
    pub fn active_note(&self) -> Option<MidiNote> {
        self.held_notes.last().copied()
    }

    /// Record a note-on and forward it to the audio thread.
    pub fn note_on(&mut self, midi: impl Into<MidiNote>) {
        let midi = midi.into();
        self.held_notes.retain(|&n| n != midi);
        self.held_notes.push(midi);
        self.send(AudioEvent::NoteOn(midi));
    }

    /// Record a note-off and forward it to the audio thread.
    pub fn note_off(&mut self, midi: impl Into<MidiNote>) {
        let midi = midi.into();
        self.held_notes.retain(|&n| n != midi);
        self.send(AudioEvent::NoteOff(midi));
    }

    /// Piano key `key` (0 = C of the lower row) went down, playing `midi`.
    pub fn piano_key_down(&mut self, key: u8, midi: MidiNote) {
        let previous = self
            .key_notes
            .get_mut(usize::from(key))
            .and_then(|slot| slot.replace(midi));
        if let Some(previous) = previous.filter(|&n| n != midi) {
            self.note_off(previous);
        }
        self.note_on(midi);
    }

    /// Whether piano key `key` is down (has started a note not yet released).
    #[cfg(not(target_arch = "wasm32"))] // only the terminal needs the release fallback
    pub fn piano_key_held(&self, key: u8) -> bool {
        self.key_notes
            .get(usize::from(key))
            .is_some_and(Option::is_some)
    }

    /// Piano key `key` came up: release the note it started.
    pub fn piano_key_up(&mut self, key: u8) {
        let started = self
            .key_notes
            .get_mut(usize::from(key))
            .and_then(Option::take);
        if let Some(midi) = started {
            self.note_off(midi);
        }
    }

    /// Send a panic event (all notes off, voice reset) to the audio thread.
    pub fn panic_all_notes(&mut self) {
        self.held_notes.clear();
        self.key_notes = [None; PIANO_KEY_COUNT];
        self.send(AudioEvent::Panic);
    }

    /// Push the current `params` snapshot to the audio thread as a `LoadPatch` event.
    fn push_params(&self) {
        self.send(AudioEvent::LoadPatch(Box::new(self.params.clone())));
    }

    /// Load the patch at `idx` from the patch list, updating params and pushing to audio.
    pub fn load_preset(&mut self, idx: usize) {
        if let Some(patch) = self.patches.get(idx) {
            self.current_patch_name = patch.name.clone();
            self.params = patch.params.clone();
            self.selected_preset = idx;
            self.push_params();
            self.status_msg = format!("Loaded: {}", self.current_patch_name);
        }
    }

    /// Save current params as a named preset.
    ///
    /// If a patch with the same name already exists it is overwritten; otherwise
    /// a new entry is appended.  Persists the bank as a best-effort operation.
    pub fn save_current_as_preset(&mut self, name: &str) {
        let patch = Patch::new(name, self.params.clone());
        self.current_patch_name = name.to_string();
        if let Some(existing) = self.patches.iter_mut().find(|p| p.name == name) {
            existing.params = self.params.clone();
        } else {
            self.patches.push(patch);
            self.selected_preset = self.patches.len() - 1;
        }
        if let Err(e) = crate::preset_store::save(&self.patches) {
            self.status_msg = format!("Save failed: {e}");
        } else {
            self.status_msg = format!("Saved: {name}");
        }
    }

    /// Advance to the next section and reset the param cursor.
    pub fn next_section(&mut self) {
        self.selected_section = self.selected_section.next();
        self.selected_param = 0;
    }

    /// Retreat to the previous section and reset the param cursor.
    pub fn prev_section(&mut self) {
        self.selected_section = self.selected_section.prev();
        self.selected_param = 0;
    }

    /// Jump straight to the preset browser, or back to the parameter that was
    /// focused before the jump.
    pub fn toggle_presets(&mut self) {
        if self.selected_section == Section::Presets {
            (self.selected_section, self.selected_param) = self.return_focus;
        } else {
            self.return_focus = (self.selected_section, self.selected_param);
            self.selected_section = Section::Presets;
            self.selected_param = 0;
        }
    }

    /// Move the param cursor forward within the current section (wraps).
    /// Automatically skips display-only rows (e.g. the OSC2 separator).
    pub fn next_param(&mut self) {
        let count = self.selected_section.param_count();
        if count > 0 {
            let next = (self.selected_param + 1) % count;
            self.selected_param = if self.selected_section.is_display_row(next) {
                (next + 1) % count
            } else {
                next
            };
        }
    }

    /// Move the param cursor backward within the current section (wraps).
    /// Automatically skips display-only rows (e.g. the OSC2 separator).
    pub fn prev_param(&mut self) {
        let count = self.selected_section.param_count();
        if count > 0 {
            let prev = if self.selected_param == 0 {
                count - 1
            } else {
                self.selected_param - 1
            };
            self.selected_param = if self.selected_section.is_display_row(prev) {
                if prev == 0 { count - 1 } else { prev - 1 }
            } else {
                prev
            };
        }
    }

    /// Adjust the currently focused parameter by `delta` (+1.0 / -1.0 per step).
    pub fn adjust_param(&mut self, delta: f32) {
        match self.selected_section {
            Section::Osc => self.adjust_osc(delta),
            Section::Env => self.adjust_env(delta),
            Section::Filter => self.adjust_filter(delta),
            Section::Lfo => self.adjust_lfo(delta),
            Section::Mod => self.adjust_mod(delta),
            Section::Sfx => self.adjust_sfx(delta),
            Section::Fx => self.adjust_fx(delta),
            Section::Presets => self.adjust_preset(delta),
        }
        if self.selected_section != Section::Presets {
            self.push_params();
        }
    }

    /// Apply `delta` to the focused oscillator parameter.
    fn adjust_osc(&mut self, d: f32) {
        match self.selected_param {
            0 => {
                self.params.osc.waveform = if d > 0.0 {
                    self.params.osc.waveform.next()
                } else {
                    self.params.osc.waveform.prev()
                };
            }
            1 => {
                self.params.osc.pulse_width =
                    (self.params.osc.pulse_width + d * 0.05).clamp(0.05, 0.95);
            }
            2 => {
                self.params.osc.detune = (self.params.osc.detune + d * 5.0).clamp(-100.0, 100.0);
            }
            3 => {
                self.params.osc.noise_mix = (self.params.osc.noise_mix + d * 0.05).clamp(0.0, 1.0);
            }
            // 4 = separator row — no-op, falls through to wildcard
            5 => {
                self.params.osc2.waveform = if d > 0.0 {
                    self.params.osc2.waveform.next()
                } else {
                    self.params.osc2.waveform.prev()
                };
            }
            6 => {
                self.params.osc2.detune = (self.params.osc2.detune + d * 5.0).clamp(-100.0, 100.0);
            }
            7 if d != 0.0 => {
                self.params.osc2.semitones =
                    (self.params.osc2.semitones.round() + d.signum()).clamp(-24.0, 24.0);
            }
            8 => {
                self.params.osc2.osc2_mix = (self.params.osc2.osc2_mix + d * 0.05).clamp(0.0, 1.0);
            }
            9 if d != 0.0 => {
                self.params.osc2.hard_sync = !self.params.osc2.hard_sync;
            }
            _ => {}
        }
    }

    /// Apply `delta` to the focused envelope parameter.
    fn adjust_env(&mut self, d: f32) {
        match self.selected_param {
            0 => self.params.env.attack = (self.params.env.attack + d * 0.01).clamp(0.001, 4.0),
            1 => self.params.env.decay = (self.params.env.decay + d * 0.01).clamp(0.001, 4.0),
            2 => self.params.env.sustain = (self.params.env.sustain + d * 0.05).clamp(0.0, 1.0),
            3 => self.params.env.release = (self.params.env.release + d * 0.05).clamp(0.001, 8.0),
            4 if d != 0.0 => {
                self.params.env.env_reverse = !self.params.env.env_reverse;
            }
            5 => {
                self.params.global.glide_time =
                    (self.params.global.glide_time + d * 0.01).clamp(0.0, 2.0);
            }
            _ => {}
        }
    }

    /// Apply `delta` to the focused filter parameter.
    fn adjust_filter(&mut self, d: f32) {
        match self.selected_param {
            0 if d != 0.0 => {
                self.params.filter.filter_mode = self.params.filter.filter_mode.next();
            }
            1 => {
                self.params.filter.cutoff = (self.params.filter.cutoff
                    * if d > 0.0 { 1.12 } else { 0.89 })
                .clamp(20.0, 18000.0);
            }
            2 => {
                self.params.filter.resonance =
                    (self.params.filter.resonance + d * 0.05).clamp(0.0, 0.99);
            }
            3 => {
                self.params.filter.drive = (self.params.filter.drive + d * 0.05).clamp(0.0, 1.0);
            }
            _ => {}
        }
    }

    /// Apply `delta` to the focused LFO parameter.
    fn adjust_lfo(&mut self, d: f32) {
        match self.selected_param {
            0 => {
                self.params.lfo.lfo_rate = (self.params.lfo.lfo_rate
                    * if d > 0.0 { 1.15 } else { 0.87 })
                .clamp(0.01, 20.0);
            }
            1 => {
                self.params.lfo.lfo_depth = (self.params.lfo.lfo_depth + d * 0.05).clamp(0.0, 1.0);
            }
            2 if d != 0.0 => {
                self.params.lfo.lfo_target = self.params.lfo.lfo_target.next();
            }
            _ => {}
        }
    }

    /// Apply `delta` to the focused modulation parameter (Mod section).
    fn adjust_mod(&mut self, d: f32) {
        match self.selected_param {
            // LFO1
            0 => {
                self.params.lfo.lfo_rate = (self.params.lfo.lfo_rate
                    * if d > 0.0 { 1.15 } else { 0.87 })
                .clamp(0.01, 20.0);
            }
            1 => {
                self.params.lfo.lfo_depth = (self.params.lfo.lfo_depth + d * 0.05).clamp(0.0, 1.0);
            }
            2 if d != 0.0 => {
                self.params.lfo.lfo_shape = if d > 0.0 {
                    self.params.lfo.lfo_shape.next()
                } else {
                    self.params.lfo.lfo_shape.prev()
                };
            }
            3 if d != 0.0 => {
                self.params.lfo.lfo_target = if d > 0.0 {
                    self.params.lfo.lfo_target.next()
                } else {
                    self.params.lfo.lfo_target.prev()
                };
            }
            // 4 = separator — no-op
            // LFO2
            5 => {
                self.params.lfo2.lfo_rate = (self.params.lfo2.lfo_rate
                    * if d > 0.0 { 1.15 } else { 0.87 })
                .clamp(0.01, 20.0);
            }
            6 => {
                self.params.lfo2.lfo_depth =
                    (self.params.lfo2.lfo_depth + d * 0.05).clamp(0.0, 1.0);
            }
            7 if d != 0.0 => {
                self.params.lfo2.lfo_shape = if d > 0.0 {
                    self.params.lfo2.lfo_shape.next()
                } else {
                    self.params.lfo2.lfo_shape.prev()
                };
            }
            8 if d != 0.0 => {
                self.params.lfo2.lfo_target = if d > 0.0 {
                    self.params.lfo2.lfo_target.next()
                } else {
                    self.params.lfo2.lfo_target.prev()
                };
            }
            // 9 = separator — no-op
            // Filter envelope
            10 => {
                self.params.filter_env.attack =
                    (self.params.filter_env.attack + d * 0.01).clamp(0.001, 4.0);
            }
            11 => {
                self.params.filter_env.decay =
                    (self.params.filter_env.decay + d * 0.01).clamp(0.001, 4.0);
            }
            12 => {
                self.params.filter_env.sustain =
                    (self.params.filter_env.sustain + d * 0.05).clamp(0.0, 1.0);
            }
            13 => {
                self.params.filter_env.release =
                    (self.params.filter_env.release + d * 0.05).clamp(0.001, 8.0);
            }
            14 => {
                self.params.filter_env.depth =
                    (self.params.filter_env.depth + d * 0.05).clamp(-1.0, 1.0);
            }
            // 15 = separator — no-op
            // Pitch envelope
            16 => {
                self.params.pitch_env.attack =
                    (self.params.pitch_env.attack + d * 0.01).clamp(0.001, 4.0);
            }
            17 => {
                self.params.pitch_env.decay =
                    (self.params.pitch_env.decay + d * 0.01).clamp(0.001, 4.0);
            }
            18 => {
                self.params.pitch_env.sustain =
                    (self.params.pitch_env.sustain + d * 0.05).clamp(0.0, 1.0);
            }
            19 => {
                self.params.pitch_env.release =
                    (self.params.pitch_env.release + d * 0.05).clamp(0.001, 8.0);
            }
            20 => {
                self.params.pitch_env.depth =
                    (self.params.pitch_env.depth + d * 0.05).clamp(-1.0, 1.0);
            }
            _ => {}
        }
    }

    /// Apply `delta` to the focused sound-effect parameter.
    fn adjust_sfx(&mut self, d: f32) {
        let sfx = &mut self.params.sfx;
        match self.selected_param {
            0 if d != 0.0 => {
                let n = SfxAction::ALL.len();
                self.sfx_action = if d > 0.0 {
                    (self.sfx_action + 1) % n
                } else {
                    (self.sfx_action + n - 1) % n
                };
            }
            1 => sfx.slide = (sfx.slide + d * 0.25).clamp(-20.0, 20.0),
            2 => sfx.slide_accel = (sfx.slide_accel + d).clamp(-100.0, 100.0),
            3 => sfx.min_freq = (sfx.min_freq + d * 10.0).clamp(0.0, 2000.0),
            4 if d != 0.0 => {
                sfx.jump_semitones = (sfx.jump_semitones.round() + d.signum()).clamp(-24.0, 24.0);
            }
            5 => sfx.jump_time = (sfx.jump_time + d * 0.01).clamp(0.0, 2.0),
            6 => sfx.pw_sweep = (sfx.pw_sweep + d * 0.1).clamp(-4.0, 4.0),
            7 => sfx.repeat_rate = (sfx.repeat_rate + d * 0.5).clamp(0.0, 50.0),
            _ => {}
        }
    }

    /// Run the SFX section's selected action on the current patch: generate
    /// a sound of the selected category, randomize, or mutate. Keeps the
    /// master volume.
    pub fn run_sfx_action(&mut self) {
        self.sfx_seed = self.sfx_seed.wrapping_add(1);
        let seed = self.sfx_seed;
        let volume = self.params.global.volume;
        let action = SfxAction::ALL[self.sfx_action];
        self.params = match action {
            SfxAction::Generate(kind) => sfx::generate(kind, seed),
            SfxAction::Randomize => sfx::randomize(seed),
            SfxAction::Mutate => sfx::mutate(&self.params, seed),
        };
        self.params.global.volume = volume;
        if action != SfxAction::Mutate {
            self.current_patch_name = format!("{} {:04x}", action.name(), seed & 0xffff);
        }
        self.push_params();
        self.status_msg = format!("{}: {}", action.name(), self.current_patch_name);
    }

    /// Render the current patch as one note (the held note, or C of the
    /// keyboard octave) and save it as `<patch name>.wav` in the working
    /// directory.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn export_wav(&mut self) {
        const RATE: u32 = 44_100;
        let note = self.active_note().unwrap_or_else(|| {
            MidiNote(u8::try_from(12 * (i32::from(self.octave) + 1)).unwrap_or(60))
        });
        let samples = synthie::render::render_note(&self.params, note, 1.0, RATE, 10.0);
        let stem: String = self
            .current_patch_name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = format!("{stem}.wav");
        let result = std::fs::File::create(&path).and_then(|file| {
            synthie::render::write_wav(std::io::BufWriter::new(file), &samples, RATE)
        });
        self.status_msg = match result {
            Ok(()) => format!("Exported: {path}"),
            Err(e) => format!("Export failed: {e}"),
        };
    }

    /// Apply `delta` to the focused FX parameter.
    fn adjust_fx(&mut self, d: f32) {
        match self.selected_param {
            0 => self.params.fx.reverb_mix = (self.params.fx.reverb_mix + d * 0.05).clamp(0.0, 1.0),
            1 => {
                self.params.fx.reverb_size =
                    (self.params.fx.reverb_size + d * 0.05).clamp(0.0, 1.0);
            }
            2 => {
                self.params.fx.reverb_damping =
                    (self.params.fx.reverb_damping + d * 0.05).clamp(0.0, 1.0);
            }
            _ => {}
        }
    }

    /// Move the preset selection cursor up or down.
    fn adjust_preset(&mut self, d: f32) {
        let n = self.patches.len();
        if n == 0 {
            return;
        }
        // The list grows downward: Up (+1) moves towards the top.
        if d < 0.0 && self.selected_preset + 1 < n {
            self.selected_preset += 1;
        } else if d > 0.0 && self.selected_preset > 0 {
            self.selected_preset -= 1;
        }
    }

    /// Increase master volume by one step.
    pub fn volume_up(&mut self) {
        self.params.global.volume = (self.params.global.volume + 0.05).min(1.0);
        self.push_params();
    }

    /// Decrease master volume by one step.
    pub fn volume_down(&mut self) {
        self.params.global.volume = (self.params.global.volume - 0.05).max(0.0);
        self.push_params();
    }

    /// Shift the keyboard octave up by one (max 8).
    pub fn octave_up(&mut self) {
        self.octave = (self.octave + 1).min(8);
    }

    /// Shift the keyboard octave down by one (min 0).
    pub fn octave_down(&mut self) {
        self.octave = (self.octave - 1).max(0);
    }

    /// Display rows (label, formatted value, gauge level) for `section`.
    ///
    /// Rows with an empty label are visual separators; their value holds the
    /// sub-heading text.  `level` is the value mapped onto 0‥1 for continuous
    /// parameters and `None` for enums, toggles, and separators.
    #[allow(clippy::too_many_lines)] // one flat table of rows per section
    pub fn params_for(&self, section: Section) -> Vec<ParamRow> {
        let p = &self.params;
        match section {
            Section::Osc => vec![
                ParamRow::text("Wave", p.osc.waveform.name()),
                ParamRow::new(
                    "PW",
                    format!("{:.2}", p.osc.pulse_width),
                    lin(p.osc.pulse_width, 0.05, 0.95),
                ),
                ParamRow::new(
                    "Det",
                    format!("{:+.0}ct", p.osc.detune),
                    lin(p.osc.detune, -100.0, 100.0),
                ),
                ParamRow::new("Noise", format!("{:.2}", p.osc.noise_mix), p.osc.noise_mix),
                ParamRow::separator("OSC2"),
                ParamRow::text("Wave2", p.osc2.waveform.name()),
                ParamRow::new(
                    "Det2",
                    format!("{:+.0}ct", p.osc2.detune),
                    lin(p.osc2.detune, -100.0, 100.0),
                ),
                ParamRow::new(
                    "Semi2",
                    format!("{:+.0}st", p.osc2.semitones),
                    lin(p.osc2.semitones, -24.0, 24.0),
                ),
                ParamRow::new("Mix2", format!("{:.2}", p.osc2.osc2_mix), p.osc2.osc2_mix),
                ParamRow::toggle("Sync", p.osc2.hard_sync),
            ],
            Section::Env => vec![
                ParamRow::new(
                    "Atk",
                    format!("{:.3}s", p.env.attack),
                    log(p.env.attack, 0.001, 4.0),
                ),
                ParamRow::new(
                    "Dec",
                    format!("{:.3}s", p.env.decay),
                    log(p.env.decay, 0.001, 4.0),
                ),
                ParamRow::new("Sus", format!("{:.2}", p.env.sustain), p.env.sustain),
                ParamRow::new(
                    "Rel",
                    format!("{:.2}s", p.env.release),
                    log(p.env.release, 0.001, 8.0),
                ),
                ParamRow::toggle("Rev", p.env.env_reverse),
                ParamRow::new(
                    "Glide",
                    format!("{:.2}s", p.global.glide_time),
                    lin(p.global.glide_time, 0.0, 2.0),
                ),
            ],
            Section::Filter => vec![
                ParamRow::text("Mode", p.filter.filter_mode.name()),
                ParamRow::new(
                    "Cut",
                    format!("{:.0}Hz", p.filter.cutoff),
                    log(p.filter.cutoff, 20.0, 18000.0),
                ),
                ParamRow::new(
                    "Res",
                    format!("{:.2}", p.filter.resonance),
                    lin(p.filter.resonance, 0.0, 0.99),
                ),
                ParamRow::new("Drive", format!("{:.2}", p.filter.drive), p.filter.drive),
            ],
            Section::Lfo => vec![
                ParamRow::new(
                    "Rate",
                    format!("{:.2}Hz", p.lfo.lfo_rate),
                    log(p.lfo.lfo_rate, 0.01, 20.0),
                ),
                ParamRow::new("Depth", format!("{:.2}", p.lfo.lfo_depth), p.lfo.lfo_depth),
                ParamRow::text("Tgt", p.lfo.lfo_target.name()),
            ],
            Section::Mod => vec![
                ParamRow::new(
                    "Rate",
                    format!("{:.2}Hz", p.lfo.lfo_rate),
                    log(p.lfo.lfo_rate, 0.01, 20.0),
                ),
                ParamRow::new("Depth", format!("{:.2}", p.lfo.lfo_depth), p.lfo.lfo_depth),
                ParamRow::text("Shape", p.lfo.lfo_shape.name()),
                ParamRow::text("Tgt", p.lfo.lfo_target.name()),
                ParamRow::separator("LFO2"),
                ParamRow::new(
                    "Rate2",
                    format!("{:.2}Hz", p.lfo2.lfo_rate),
                    log(p.lfo2.lfo_rate, 0.01, 20.0),
                ),
                ParamRow::new("Dep2", format!("{:.2}", p.lfo2.lfo_depth), p.lfo2.lfo_depth),
                ParamRow::text("Shp2", p.lfo2.lfo_shape.name()),
                ParamRow::text("Tgt2", p.lfo2.lfo_target.name()),
                ParamRow::separator("FILTER ENV"),
                ParamRow::new(
                    "FAtk",
                    format!("{:.3}s", p.filter_env.attack),
                    log(p.filter_env.attack, 0.001, 4.0),
                ),
                ParamRow::new(
                    "FDec",
                    format!("{:.3}s", p.filter_env.decay),
                    log(p.filter_env.decay, 0.001, 4.0),
                ),
                ParamRow::new(
                    "FSus",
                    format!("{:.2}", p.filter_env.sustain),
                    p.filter_env.sustain,
                ),
                ParamRow::new(
                    "FRel",
                    format!("{:.2}s", p.filter_env.release),
                    log(p.filter_env.release, 0.001, 8.0),
                ),
                ParamRow::new(
                    "FDep",
                    format!("{:+.2}", p.filter_env.depth),
                    lin(p.filter_env.depth, -1.0, 1.0),
                ),
                ParamRow::separator("PITCH ENV"),
                ParamRow::new(
                    "PAtk",
                    format!("{:.3}s", p.pitch_env.attack),
                    log(p.pitch_env.attack, 0.001, 4.0),
                ),
                ParamRow::new(
                    "PDec",
                    format!("{:.3}s", p.pitch_env.decay),
                    log(p.pitch_env.decay, 0.001, 4.0),
                ),
                ParamRow::new(
                    "PSus",
                    format!("{:.2}", p.pitch_env.sustain),
                    p.pitch_env.sustain,
                ),
                ParamRow::new(
                    "PRel",
                    format!("{:.2}s", p.pitch_env.release),
                    log(p.pitch_env.release, 0.001, 8.0),
                ),
                ParamRow::new(
                    "PDep",
                    format!("{:+.2}", p.pitch_env.depth),
                    lin(p.pitch_env.depth, -1.0, 1.0),
                ),
            ],
            Section::Sfx => vec![
                ParamRow::text("Gen", SfxAction::ALL[self.sfx_action].name()),
                ParamRow::new(
                    "Slide",
                    format!("{:+.2}o/s", p.sfx.slide),
                    lin(p.sfx.slide, -20.0, 20.0),
                ),
                ParamRow::new(
                    "Accel",
                    format!("{:+.0}", p.sfx.slide_accel),
                    lin(p.sfx.slide_accel, -100.0, 100.0),
                ),
                ParamRow::new(
                    "MinF",
                    format!("{:.0}Hz", p.sfx.min_freq),
                    lin(p.sfx.min_freq, 0.0, 2000.0),
                ),
                ParamRow::new(
                    "Jump",
                    format!("{:+.0}st", p.sfx.jump_semitones),
                    lin(p.sfx.jump_semitones, -24.0, 24.0),
                ),
                ParamRow::new(
                    "JTime",
                    format!("{:.2}s", p.sfx.jump_time),
                    lin(p.sfx.jump_time, 0.0, 2.0),
                ),
                ParamRow::new(
                    "PWSwp",
                    format!("{:+.1}/s", p.sfx.pw_sweep),
                    lin(p.sfx.pw_sweep, -4.0, 4.0),
                ),
                ParamRow::new(
                    "Rpt",
                    format!("{:.1}Hz", p.sfx.repeat_rate),
                    lin(p.sfx.repeat_rate, 0.0, 50.0),
                ),
            ],
            Section::Fx => vec![
                ParamRow::new("RvbMix", format!("{:.2}", p.fx.reverb_mix), p.fx.reverb_mix),
                ParamRow::new(
                    "RvbSz",
                    format!("{:.2}", p.fx.reverb_size),
                    p.fx.reverb_size,
                ),
                ParamRow::new(
                    "RvbDmp",
                    format!("{:.2}", p.fx.reverb_damping),
                    p.fx.reverb_damping,
                ),
            ],
            // The preset browser renders `self.patches` directly.
            Section::Presets => Vec::new(),
        }
    }
}

/// One display row of a parameter panel.
#[derive(Debug, Clone)]
pub struct ParamRow {
    /// Short label; empty for separator rows.
    pub label: &'static str,
    /// Formatted value (or sub-heading text for separators).
    pub value: String,
    /// Value mapped onto 0‥1 for gauge display; `None` for discrete values.
    pub level: Option<f32>,
}

impl ParamRow {
    /// Continuous parameter with a gauge.
    fn new(label: &'static str, value: String, level: f32) -> Self {
        Self {
            label,
            value,
            level: Some(level.clamp(0.0, 1.0)),
        }
    }

    /// Discrete parameter (enum name) without a gauge.
    fn text(label: &'static str, value: &str) -> Self {
        Self {
            label,
            value: value.to_string(),
            level: None,
        }
    }

    /// On/off parameter.
    fn toggle(label: &'static str, on: bool) -> Self {
        Self::text(label, if on { "ON" } else { "off" })
    }

    /// Non-interactive sub-heading row.
    fn separator(title: &str) -> Self {
        Self::text("", title)
    }

    /// `true` for sub-heading rows.
    pub fn is_separator(&self) -> bool {
        self.label.is_empty()
    }
}

/// Linear position of `v` within `lo..=hi`.
fn lin(v: f32, lo: f32, hi: f32) -> f32 {
    (v - lo) / (hi - lo)
}

/// Logarithmic position of `v` within `lo..=hi` (both > 0); used for times
/// and frequencies so the gauge moves evenly per perceived step.
fn log(v: f32, lo: f32, hi: f32) -> f32 {
    (v.max(lo) / lo).ln() / (hi / lo).ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        let (tx, _rx) = crossbeam_channel::unbounded();
        AppState::new(tx)
    }

    #[test]
    fn preset_up_down_follow_the_list() {
        let mut s = state();
        s.selected_section = Section::Presets;
        s.adjust_param(-1.0); // Down
        assert_eq!(s.selected_preset, 1);
        s.adjust_param(1.0); // Up
        assert_eq!(s.selected_preset, 0);
        s.adjust_param(1.0); // Up at the top stays put
        assert_eq!(s.selected_preset, 0);
    }

    #[test]
    fn toggle_presets_jumps_there_and_back() {
        let mut s = state();
        s.selected_section = Section::Filter;
        s.selected_param = 2;
        s.toggle_presets();
        assert_eq!(s.selected_section, Section::Presets);
        s.toggle_presets();
        assert_eq!((s.selected_section, s.selected_param), (Section::Filter, 2));
    }

    #[test]
    fn chords_light_every_held_note() {
        let mut s = state();
        s.note_on(MidiNote(60));
        s.note_on(MidiNote(64));
        s.note_on(MidiNote(67));
        assert_eq!(s.held_notes, [MidiNote(60), MidiNote(64), MidiNote(67)]);
        assert_eq!(s.active_note(), Some(MidiNote(67)));

        // Releasing the newest falls back to the next most recent one.
        s.note_off(MidiNote(67));
        assert_eq!(s.active_note(), Some(MidiNote(64)));
        s.note_off(MidiNote(60));
        assert_eq!(s.held_notes, [MidiNote(64)]);

        s.panic_all_notes();
        assert_eq!(s.held_notes, []);
    }

    #[test]
    fn key_release_ends_its_own_note_after_an_octave_change() {
        let mut s = state();
        s.piano_key_down(0, MidiNote(60)); // C4
        s.octave_up();
        s.piano_key_up(0); // would be C5 in the new octave
        assert_eq!(s.held_notes, []);
    }

    #[test]
    fn sfx_selector_generates_and_mutates_keeping_volume() {
        let mut s = state();
        s.params.global.volume = 0.3;
        s.selected_section = Section::Sfx;
        s.adjust_param(1.0); // Pickup -> Laser
        s.run_sfx_action();
        assert!(s.current_patch_name.starts_with("Laser "));
        assert!(s.params.sfx.slide < 0.0);
        assert!((s.params.global.volume - 0.3).abs() < f32::EPSILON);

        let laser = s.params.clone();
        s.adjust_param(-1.0); // back to Pickup …
        s.adjust_param(-1.0); // … and around to Mutate
        s.run_sfx_action();
        assert_ne!(s.params, laser);
        assert_eq!(s.params.osc.waveform, laser.osc.waveform);
        assert!(s.current_patch_name.starts_with("Laser "));
    }
}
