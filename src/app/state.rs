use crate::params::{AudioEvent, Patch, SynthParams};
use crate::presets::sid;
use crossbeam_channel::Sender;

// ---------------------------------------------------------------------------
// UI section navigation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Osc,
    Env,
    Filter,
    Lfo,
    Fx,
    Presets,
}

impl Section {
    pub const ALL: &'static [Section] = &[
        Section::Osc,
        Section::Env,
        Section::Filter,
        Section::Lfo,
        Section::Fx,
        Section::Presets,
    ];

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

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|&s| s == self).unwrap_or(0);
        let len = Self::ALL.len();
        Self::ALL[(idx + len - 1) % len]
    }

    /// Number of adjustable params in this section (0 = list navigation).
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

// ---------------------------------------------------------------------------
// AppState
// ---------------------------------------------------------------------------

pub struct AppState {
    pub params: SynthParams,
    pub selected_section: Section,
    pub selected_param: usize,
    pub active_note: Option<u8>,
    pub octave: i8,
    #[allow(dead_code)]
    pub envelope_stage_name: String, // updated by audio feedback (future use)
    pub current_patch_name: String,
    pub patches: Vec<Patch>,
    pub selected_preset: usize,
    pub should_quit: bool,
    pub show_help: bool,
    pub status_msg: String,
    audio_tx: Sender<AudioEvent>,
}

impl AppState {
    pub fn new(audio_tx: Sender<AudioEvent>) -> Self {
        let patches = sid::default_patches();
        let current_patch_name = patches
            .first()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Default".to_string());

        // Seed the audio thread with the first preset
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

    // -----------------------------------------------------------------------
    // Audio event helpers
    // -----------------------------------------------------------------------

    fn send(&self, event: AudioEvent) {
        let _ = self.audio_tx.send(event);
    }

    pub fn note_on(&mut self, midi: u8) {
        self.active_note = Some(midi);
        self.send(AudioEvent::NoteOn(midi));
    }

    pub fn note_off(&mut self, midi: u8) {
        if self.active_note == Some(midi) {
            self.active_note = None;
        }
        self.send(AudioEvent::NoteOff(midi));
    }

    pub fn panic_all_notes(&mut self) {
        self.active_note = None;
        self.send(AudioEvent::Panic);
    }

    fn push_params(&self) {
        self.send(AudioEvent::LoadPatch(Box::new(self.params.clone())));
    }

    // -----------------------------------------------------------------------
    // Preset management
    // -----------------------------------------------------------------------

    pub fn load_preset(&mut self, idx: usize) {
        if let Some(patch) = self.patches.get(idx) {
            self.current_patch_name = patch.name.clone();
            self.params = patch.params.clone();
            self.selected_preset = idx;
            self.push_params();
            self.status_msg = format!("Loaded: {}", self.current_patch_name);
        }
    }

    pub fn save_current_as_preset(&mut self, name: &str) {
        let patch = Patch::new(name, self.params.clone());
        self.current_patch_name = name.to_string();
        // Overwrite if name exists; else append
        if let Some(existing) = self.patches.iter_mut().find(|p| p.name == name) {
            existing.params = self.params.clone();
        } else {
            self.patches.push(patch);
            self.selected_preset = self.patches.len() - 1;
        }
        // Persist to disk (best-effort)
        let path = crate::presets::store::user_presets_path();
        if let Err(e) = crate::presets::store::save_patches(&self.patches, &path) {
            self.status_msg = format!("Save failed: {e}");
        } else {
            self.status_msg = format!("Saved: {name}");
        }
    }

    // -----------------------------------------------------------------------
    // Section / param navigation
    // -----------------------------------------------------------------------

    pub fn next_section(&mut self) {
        self.selected_section = self.selected_section.next();
        self.selected_param = 0;
    }

    pub fn prev_section(&mut self) {
        self.selected_section = self.selected_section.prev();
        self.selected_param = 0;
    }

    pub fn next_param(&mut self) {
        let count = self.selected_section.param_count();
        if count > 0 {
            self.selected_param = (self.selected_param + 1) % count;
        }
    }

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

    // -----------------------------------------------------------------------
    // Parameter adjustment
    // -----------------------------------------------------------------------

    /// `delta` is +1.0 for "up one step" or -1.0 for "down one step".
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

    fn adjust_osc(&mut self, d: f32) {
        match self.selected_param {
            0 => {
                self.params.waveform = if d > 0.0 {
                    self.params.waveform.next()
                } else {
                    self.params.waveform.prev()
                };
            }
            1 => {
                self.params.pulse_width = (self.params.pulse_width + d * 0.05).clamp(0.05, 0.95);
            }
            2 => {
                self.params.detune = (self.params.detune + d * 5.0).clamp(-100.0, 100.0);
            }
            3 => {
                self.params.noise_mix = (self.params.noise_mix + d * 0.05).clamp(0.0, 1.0);
            }
            _ => {}
        }
    }

    fn adjust_env(&mut self, d: f32) {
        match self.selected_param {
            0 => self.params.attack = (self.params.attack + d * 0.01).clamp(0.001, 4.0),
            1 => self.params.decay = (self.params.decay + d * 0.01).clamp(0.001, 4.0),
            2 => self.params.sustain = (self.params.sustain + d * 0.05).clamp(0.0, 1.0),
            3 => self.params.release = (self.params.release + d * 0.05).clamp(0.001, 8.0),
            4 => {
                if d != 0.0 {
                    self.params.env_reverse = !self.params.env_reverse;
                }
            }
            5 => self.params.glide_time = (self.params.glide_time + d * 0.01).clamp(0.0, 2.0),
            _ => {}
        }
    }

    fn adjust_filter(&mut self, d: f32) {
        match self.selected_param {
            0 => {
                if d != 0.0 {
                    self.params.filter_mode = self.params.filter_mode.next();
                }
            }
            1 => {
                self.params.cutoff =
                    (self.params.cutoff * if d > 0.0 { 1.12 } else { 0.89 }).clamp(20.0, 18000.0);
            }
            2 => {
                self.params.resonance = (self.params.resonance + d * 0.05).clamp(0.0, 0.99);
            }
            3 => {
                self.params.drive = (self.params.drive + d * 0.05).clamp(0.0, 1.0);
            }
            _ => {}
        }
    }

    fn adjust_lfo(&mut self, d: f32) {
        match self.selected_param {
            0 => {
                self.params.lfo_rate =
                    (self.params.lfo_rate * if d > 0.0 { 1.15 } else { 0.87 }).clamp(0.01, 20.0);
            }
            1 => {
                self.params.lfo_depth = (self.params.lfo_depth + d * 0.05).clamp(0.0, 1.0);
            }
            2 => {
                if d != 0.0 {
                    self.params.lfo_target = self.params.lfo_target.next();
                }
            }
            _ => {}
        }
    }

    fn adjust_fx(&mut self, d: f32) {
        match self.selected_param {
            0 => self.params.reverb_mix = (self.params.reverb_mix + d * 0.05).clamp(0.0, 1.0),
            1 => self.params.reverb_size = (self.params.reverb_size + d * 0.05).clamp(0.0, 1.0),
            2 => {
                self.params.reverb_damping = (self.params.reverb_damping + d * 0.05).clamp(0.0, 1.0)
            }
            _ => {}
        }
    }

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

    // -----------------------------------------------------------------------
    // Convenience controls not tied to section/param
    // -----------------------------------------------------------------------

    pub fn volume_up(&mut self) {
        self.params.volume = (self.params.volume + 0.05).min(1.0);
        self.push_params();
    }

    pub fn volume_down(&mut self) {
        self.params.volume = (self.params.volume - 0.05).max(0.0);
        self.push_params();
    }

    pub fn octave_up(&mut self) {
        self.octave = (self.octave + 1).min(8);
    }

    pub fn octave_down(&mut self) {
        self.octave = (self.octave - 1).max(0);
    }

    // -----------------------------------------------------------------------
    // Param label helpers (for UI display)
    // -----------------------------------------------------------------------

    /// Returns (label, value_string) pairs for the currently selected section.
    pub fn section_params(&self) -> Vec<(&'static str, String)> {
        let p = &self.params;
        match self.selected_section {
            Section::Osc => vec![
                ("Wave", p.waveform.name().to_string()),
                ("PW", format!("{:.2}", p.pulse_width)),
                ("Det", format!("{:+.0}ct", p.detune)),
                ("Nse", format!("{:.2}", p.noise_mix)),
            ],
            Section::Env => vec![
                ("Atk", format!("{:.3}s", p.attack)),
                ("Dec", format!("{:.3}s", p.decay)),
                ("Sus", format!("{:.2}", p.sustain)),
                ("Rel", format!("{:.2}s", p.release)),
                ("Rev", if p.env_reverse { "ON" } else { "off" }.to_string()),
                ("Gld", format!("{:.2}s", p.glide_time)),
            ],
            Section::Filter => vec![
                ("Mode", p.filter_mode.name().to_string()),
                ("Cut", format!("{:.0}Hz", p.cutoff)),
                ("Res", format!("{:.2}", p.resonance)),
                ("Drv", format!("{:.2}", p.drive)),
            ],
            Section::Lfo => vec![
                ("Rate", format!("{:.2}Hz", p.lfo_rate)),
                ("Dep", format!("{:.2}", p.lfo_depth)),
                ("Tgt", p.lfo_target.name().to_string()),
            ],
            Section::Fx => vec![
                ("RvbMix", format!("{:.2}", p.reverb_mix)),
                ("RvbSz", format!("{:.2}", p.reverb_size)),
                ("RvbDmp", format!("{:.2}", p.reverb_damping)),
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
