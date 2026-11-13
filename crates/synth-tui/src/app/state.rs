//! Application state: UI section navigation, parameter adjustment, and
//! the bridge between the UI thread and the audio engine.

use crossbeam_channel::Sender;
use synthie::param::{Param, ParamKind};
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
    /// Why saving is off: the saved presets could not be read, and saving
    /// would overwrite them.
    save_blocked: Option<String>,
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
            let _ = audio_tx.try_send(AudioEvent::LoadPatch(Box::new(p.params.clone())));
        }

        Self {
            params: patches
                .first()
                .map_or_else(SynthParams::default, |p| p.params.clone()),
            selected_section: Section::Osc,
            selected_param: 0,
            held_notes: Vec::new(),
            key_notes: [None; PIANO_KEY_COUNT],
            octave: 4,
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
            save_blocked: None,
            audio_tx,
        }
    }

    /// Send an `AudioEvent` to the audio thread (best-effort; drops if channel
    /// full, which only happens once the audio stream has stopped).
    fn send(&self, event: AudioEvent) {
        let _ = self.audio_tx.try_send(event);
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
        self.current_patch_name = name.to_string();
        self.selected_preset = self.upsert_patch(Patch::new(name, self.params.clone()));
        self.status_msg = if let Some(why) = &self.save_blocked {
            format!("Kept {name} for this session only: {why}")
        } else if let Err(e) = crate::preset_store::save(&self.patches) {
            format!("Save failed: {e}")
        } else {
            format!("Saved: {name}")
        };
    }

    /// Add the presets saved in earlier sessions to the bank, replacing
    /// built-ins of the same name. If they could not be read, saving is
    /// turned off so the next save does not overwrite them.
    pub fn load_user_presets(&mut self, saved: anyhow::Result<Vec<Patch>>) {
        match saved {
            Ok(saved) => {
                for patch in saved {
                    self.upsert_patch(patch);
                }
            }
            Err(e) => {
                let why = format!("saved presets unreadable ({e:#})");
                self.status_msg = format!("{why}; saving is off");
                self.save_blocked = Some(why);
            }
        }
    }

    /// Replace the patch named like `patch`, or append it; returns its index.
    fn upsert_patch(&mut self, patch: Patch) -> usize {
        if let Some(i) = self.patches.iter().position(|p| p.name == patch.name) {
            self.patches[i] = patch;
            i
        } else {
            self.patches.push(patch);
            self.patches.len() - 1
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
        match (self.selected_section, self.selected_param) {
            (Section::Presets, _) => return self.adjust_preset(delta),
            (Section::Sfx, 0) => self.cycle_sfx_action(delta),
            (section, row) => {
                if let Some((param, step)) = row_param(section, row) {
                    let value = step.apply(param, param.get(&self.params), delta);
                    // `Param::set` clamps to the engine's own range.
                    param.set(&mut self.params, value);
                }
            }
        }
        self.push_params();
    }

    /// Cycle the SFX section's action selector.
    fn cycle_sfx_action(&mut self, d: f32) {
        let n = SfxAction::ALL.len();
        if d > 0.0 {
            self.sfx_action = (self.sfx_action + 1) % n;
        } else if d < 0.0 {
            self.sfx_action = (self.sfx_action + n - 1) % n;
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

/// How one key press moves a parameter.
#[derive(Clone, Copy)]
enum Step {
    /// Add this much per press.
    Add(f32),
    /// Multiply by the first factor going up, the second going down.
    Scale(f32, f32),
    /// Round, then one whole unit per press (semitones).
    Whole,
    /// Next or previous choice, wrapping; flips a toggle.
    Cycle,
}

impl Step {
    /// `value` moved one step in the direction of `d`, before clamping.
    fn apply(self, param: Param, value: f32, d: f32) -> f32 {
        match self {
            Step::Add(step) => value + d * step,
            // Rates may be 0 in a loaded patch; a floor keeps them movable.
            Step::Scale(up, down) => value.max(0.01) * if d > 0.0 { up } else { down },
            Step::Whole => value.round() + d.signum(),
            Step::Cycle => match param.kind() {
                ParamKind::Choice(names) if d != 0.0 => {
                    #[allow(clippy::cast_precision_loss)] // a handful of variants
                    let n = names.len() as f32;
                    (value + d.signum()).rem_euclid(n)
                }
                ParamKind::Toggle if d != 0.0 => 1.0 - value,
                _ => value,
            },
        }
    }
}

/// The parameter on `row` of `section`, and how a key press moves it.
/// Separator rows, the SFX action selector and the preset list have none.
fn row_param(section: Section, row: usize) -> Option<(Param, Step)> {
    use Param as P;
    use Step::{Add, Cycle, Scale, Whole};
    const RATE: Step = Scale(1.15, 0.87);
    Some(match (section, row) {
        (Section::Osc, 0) => (P::OscWaveform, Cycle),
        (Section::Osc, 1) => (P::OscPulseWidth, Add(0.05)),
        (Section::Osc, 2) => (P::OscDetune, Add(5.0)),
        (Section::Osc, 3) => (P::OscNoiseMix, Add(0.05)),
        (Section::Osc, 5) => (P::Osc2Waveform, Cycle),
        (Section::Osc, 6) => (P::Osc2Detune, Add(5.0)),
        (Section::Osc, 7) => (P::Osc2Semitones, Whole),
        (Section::Osc, 8) => (P::Osc2Mix, Add(0.05)),
        (Section::Osc, 9) => (P::Osc2HardSync, Cycle),

        (Section::Env, 0) => (P::EnvAttack, Add(0.01)),
        (Section::Env, 1) => (P::EnvDecay, Add(0.01)),
        (Section::Env, 2) => (P::EnvSustain, Add(0.05)),
        (Section::Env, 3) => (P::EnvRelease, Add(0.05)),
        (Section::Env, 4) => (P::EnvReverse, Cycle),
        (Section::Env, 5) => (P::GlideTime, Add(0.01)),

        (Section::Filter, 0) => (P::FilterMode, Cycle),
        (Section::Filter, 1) => (P::FilterCutoff, Scale(1.12, 0.89)),
        (Section::Filter, 2) => (P::FilterResonance, Add(0.05)),
        (Section::Filter, 3) => (P::FilterDrive, Add(0.05)),

        (Section::Lfo | Section::Mod, 0) => (P::LfoRate, RATE),
        (Section::Lfo | Section::Mod, 1) => (P::LfoDepth, Add(0.05)),
        (Section::Lfo, 2) | (Section::Mod, 3) => (P::LfoTarget, Cycle),
        (Section::Mod, 2) => (P::LfoShape, Cycle),
        (Section::Mod, 5) => (P::Lfo2Rate, RATE),
        (Section::Mod, 6) => (P::Lfo2Depth, Add(0.05)),
        (Section::Mod, 7) => (P::Lfo2Shape, Cycle),
        (Section::Mod, 8) => (P::Lfo2Target, Cycle),
        (Section::Mod, 10) => (P::FilterEnvAttack, Add(0.01)),
        (Section::Mod, 11) => (P::FilterEnvDecay, Add(0.01)),
        (Section::Mod, 12) => (P::FilterEnvSustain, Add(0.05)),
        (Section::Mod, 13) => (P::FilterEnvRelease, Add(0.05)),
        (Section::Mod, 14) => (P::FilterEnvDepth, Add(0.05)),
        (Section::Mod, 16) => (P::PitchEnvAttack, Add(0.01)),
        (Section::Mod, 17) => (P::PitchEnvDecay, Add(0.01)),
        (Section::Mod, 18) => (P::PitchEnvSustain, Add(0.05)),
        (Section::Mod, 19) => (P::PitchEnvRelease, Add(0.05)),
        (Section::Mod, 20) => (P::PitchEnvDepth, Add(0.05)),

        (Section::Sfx, 1) => (P::SfxSlide, Add(0.25)),
        (Section::Sfx, 2) => (P::SfxSlideAccel, Add(1.0)),
        (Section::Sfx, 3) => (P::SfxMinFreq, Add(10.0)),
        (Section::Sfx, 4) => (P::SfxJumpSemitones, Whole),
        (Section::Sfx, 5) => (P::SfxJumpTime, Add(0.01)),
        (Section::Sfx, 6) => (P::SfxPwSweep, Add(0.1)),
        (Section::Sfx, 7) => (P::SfxRepeatRate, Add(0.5)),

        (Section::Fx, 0) => (P::ReverbMix, Add(0.05)),
        (Section::Fx, 1) => (P::ReverbSize, Add(0.05)),
        (Section::Fx, 2) => (P::ReverbDamping, Add(0.05)),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        let (tx, _rx) = crossbeam_channel::unbounded();
        AppState::new(tx)
    }

    #[test]
    fn every_adjustable_row_moves_a_parameter() {
        for &section in Section::ALL {
            for row in 0..section.param_count() {
                let selector = section == Section::Sfx && row == 0;
                assert_eq!(
                    row_param(section, row).is_some(),
                    !section.is_display_row(row) && !selector,
                    "{section:?} row {row}"
                );
            }
        }
    }

    #[test]
    fn steps_keep_the_engine_range() {
        let mut s = state();
        s.selected_section = Section::Env;
        s.selected_param = 5; // glide: the engine allows up to 10 s
        s.params.global.glide_time = 5.0;
        s.adjust_param(-1.0);
        assert!((s.params.global.glide_time - 4.99).abs() < 1e-5);

        s.selected_section = Section::Lfo;
        s.selected_param = 0; // a rate of 0 must still move up
        s.params.lfo.lfo_rate = 0.0;
        s.adjust_param(1.0);
        assert!(s.params.lfo.lfo_rate > 0.0);

        s.selected_param = 2; // choices wrap both ways
        let target = s.params.lfo.lfo_target;
        s.adjust_param(1.0);
        s.adjust_param(-1.0);
        assert_eq!(s.params.lfo.lfo_target, target);
    }

    #[test]
    fn saved_presets_join_the_bank_and_replace_same_named_ones() {
        let mut s = state();
        let builtins = s.patches.len();
        let first = s.patches[0].name.clone();
        let tweaked = Patch::new(first.clone(), s.patches[3].params.clone());
        let mine = Patch::new("Mine", s.patches[2].params.clone());
        s.load_user_presets(Ok(vec![tweaked.clone(), mine.clone()]));
        assert_eq!(s.patches.len(), builtins + 1);
        assert_eq!(s.patches[0], tweaked);
        assert_eq!(s.patches[builtins], mine);
    }

    #[test]
    fn unreadable_presets_turn_saving_off_instead_of_overwriting_them() {
        let mut s = state();
        s.load_user_presets(Err(anyhow::anyhow!("bad json")));
        assert!(s.status_msg.contains("bad json"), "{}", s.status_msg);
        // Must not reach the file: this would write next to the test binary.
        s.save_current_as_preset("Mine");
        assert!(
            s.status_msg.contains("this session only"),
            "{}",
            s.status_msg
        );
        assert_eq!(s.patches.last().unwrap().name, "Mine");
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
