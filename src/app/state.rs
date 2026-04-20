//! Application state: UI section navigation, parameter adjustment, and
//! the bridge between the UI thread and the audio engine.

use crate::params::{AudioEvent, Patch, SynthParams};
use crate::presets::sid;
use crossbeam_channel::Sender;

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
            Section::Osc => 4,     // waveform, pulse_width, detune, noise_mix
            Section::Env => 6,     // attack, decay, sustain, release, env_reverse, glide
            Section::Filter => 4,  // mode, cutoff, resonance, drive
            Section::Lfo => 3,     // rate, depth, target
            Section::Fx => 3,      // reverb_mix, reverb_size, reverb_damping
            Section::Presets => 0, // list mode
        }
    }
}

/// Central application state owned by the UI thread.
pub struct AppState {
    /// Live parameter snapshot; updated by `adjust_*` methods and preset loads.
    pub params: SynthParams,
    /// Currently focused UI panel.
    pub selected_section: Section,
    /// Index of the focused parameter within `selected_section`.
    pub selected_param: usize,
    /// MIDI note currently held (if any), for display in the status bar.
    pub active_note: Option<u8>,
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
    /// When `true`, the event loop exits after the current frame.
    pub should_quit: bool,
    /// When `true`, the help overlay is drawn instead of the main UI.
    pub show_help: bool,
    /// Transient status message shown in the status bar (e.g. "Loaded: …").
    pub status_msg: String,
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
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Default".to_string());

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
            active_note: None,
            octave: 4,
            envelope_stage_name: "Idle".to_string(),
            current_patch_name,
            patches,
            selected_preset: 0,
            should_quit: false,
            show_help: false,
            status_msg: String::new(),
            audio_tx,
        }
    }

    /// Send an `AudioEvent` to the audio thread (best-effort; drops if channel full).
    fn send(&self, event: AudioEvent) {
        let _ = self.audio_tx.send(event);
    }

    /// Record a note-on and forward it to the audio thread.
    pub fn note_on(&mut self, midi: u8) {
        self.active_note = Some(midi);
        self.send(AudioEvent::NoteOn(midi));
    }

    /// Record a note-off and forward it to the audio thread.
    pub fn note_off(&mut self, midi: u8) {
        if self.active_note == Some(midi) {
            self.active_note = None;
        }
        self.send(AudioEvent::NoteOff(midi));
    }

    /// Send a panic event (all notes off, voice reset) to the audio thread.
    pub fn panic_all_notes(&mut self) {
        self.active_note = None;
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
    /// a new entry is appended.  Persists to disk as a best-effort operation.
    pub fn save_current_as_preset(&mut self, name: &str) {
        let patch = Patch::new(name, self.params.clone());
        self.current_patch_name = name.to_string();
        if let Some(existing) = self.patches.iter_mut().find(|p| p.name == name) {
            existing.params = self.params.clone();
        } else {
            self.patches.push(patch);
            self.selected_preset = self.patches.len() - 1;
        }
        let path = crate::presets::store::user_presets_path();
        if let Err(e) = crate::presets::store::save_patches(&self.patches, &path) {
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

    /// Move the param cursor forward within the current section (wraps).
    pub fn next_param(&mut self) {
        let count = self.selected_section.param_count();
        if count > 0 {
            self.selected_param = (self.selected_param + 1) % count;
        }
    }

    /// Move the param cursor backward within the current section (wraps).
    pub fn prev_param(&mut self) {
        let count = self.selected_section.param_count();
        if count > 0 {
            self.selected_param = if self.selected_param == 0 {
                count - 1
            } else {
                self.selected_param - 1
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
            4 => {
                if d != 0.0 {
                    self.params.env.env_reverse = !self.params.env.env_reverse;
                }
            }
            5 => {
                self.params.global.glide_time =
                    (self.params.global.glide_time + d * 0.01).clamp(0.0, 2.0)
            }
            _ => {}
        }
    }

    /// Apply `delta` to the focused filter parameter.
    fn adjust_filter(&mut self, d: f32) {
        match self.selected_param {
            0 => {
                if d != 0.0 {
                    self.params.filter.filter_mode = self.params.filter.filter_mode.next();
                }
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
            2 => {
                if d != 0.0 {
                    self.params.lfo.lfo_target = self.params.lfo.lfo_target.next();
                }
            }
            _ => {}
        }
    }

    /// Apply `delta` to the focused FX parameter.
    fn adjust_fx(&mut self, d: f32) {
        match self.selected_param {
            0 => self.params.fx.reverb_mix = (self.params.fx.reverb_mix + d * 0.05).clamp(0.0, 1.0),
            1 => {
                self.params.fx.reverb_size = (self.params.fx.reverb_size + d * 0.05).clamp(0.0, 1.0)
            }
            2 => {
                self.params.fx.reverb_damping =
                    (self.params.fx.reverb_damping + d * 0.05).clamp(0.0, 1.0)
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
        if d > 0.0 && self.selected_preset + 1 < n {
            self.selected_preset += 1;
        } else if d < 0.0 && self.selected_preset > 0 {
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

    /// Returns `(label, value_string)` pairs for the currently selected section.
    ///
    /// Used by the UI to build the parameter list display.
    pub fn section_params(&self) -> Vec<(&'static str, String)> {
        let p = &self.params;
        match self.selected_section {
            Section::Osc => vec![
                ("Wave", p.osc.waveform.name().to_string()),
                ("PW", format!("{:.2}", p.osc.pulse_width)),
                ("Det", format!("{:+.0}ct", p.osc.detune)),
                ("Nse", format!("{:.2}", p.osc.noise_mix)),
            ],
            Section::Env => vec![
                ("Atk", format!("{:.3}s", p.env.attack)),
                ("Dec", format!("{:.3}s", p.env.decay)),
                ("Sus", format!("{:.2}", p.env.sustain)),
                ("Rel", format!("{:.2}s", p.env.release)),
                (
                    "Rev",
                    if p.env.env_reverse { "ON" } else { "off" }.to_string(),
                ),
                ("Gld", format!("{:.2}s", p.global.glide_time)),
            ],
            Section::Filter => vec![
                ("Mode", p.filter.filter_mode.name().to_string()),
                ("Cut", format!("{:.0}Hz", p.filter.cutoff)),
                ("Res", format!("{:.2}", p.filter.resonance)),
                ("Drv", format!("{:.2}", p.filter.drive)),
            ],
            Section::Lfo => vec![
                ("Rate", format!("{:.2}Hz", p.lfo.lfo_rate)),
                ("Dep", format!("{:.2}", p.lfo.lfo_depth)),
                ("Tgt", p.lfo.lfo_target.name().to_string()),
            ],
            Section::Fx => vec![
                ("RvbMix", format!("{:.2}", p.fx.reverb_mix)),
                ("RvbSz", format!("{:.2}", p.fx.reverb_size)),
                ("RvbDmp", format!("{:.2}", p.fx.reverb_damping)),
            ],
            Section::Presets => self
                .patches
                .iter()
                .map(|p| ("", p.name.clone()))
                .collect::<Vec<_>>()
                .into_iter()
                .map(|(_, name)| ("", name))
                .collect(),
        }
    }
}
