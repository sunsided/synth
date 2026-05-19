//! Built-in arpeggiator: fixed-size note list, four playback modes, per-sample
//! phase-accumulator timing, configurable gate.

use crate::params::{ArpMode, ArpParams, MidiNote};

/// Events produced by one call to [`Arpeggiator::tick`].
///
/// At most one `off` and one `on` event fire per sample. When both are `Some`,
/// apply `off` before `on` to avoid re-triggering the same voice slot.
#[cfg(feature = "arp")]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ArpEvents {
    /// Note to release this sample, if any.
    pub off: Option<MidiNote>,
    /// Note to trigger this sample, if any.
    pub on: Option<MidiNote>,
}

/// Per-channel arpeggiator with phase-accumulator timing.
///
/// Lives inside `AudioChannel`. All timing state is here; note-list state
/// lives in [`ArpParams`] so it survives param snapshots sent via `LoadPatch`.
#[cfg(feature = "arp")]
#[derive(Debug, Clone)]
pub struct Arpeggiator {
    /// Fractional position within the current step, `0.0..1.0`.
    phase: f32,
    /// Index into the active note list.
    pub(crate) step: u8,
    /// Note currently held by the arp; `None` when between steps.
    pub(crate) sounding: Option<MidiNote>,
    /// `true` once `NoteOff` has been emitted for the current step's note.
    gate_fired: bool,
    /// Direction flag for `UpDown` mode: `0` = ascending, `1` = descending.
    direction: u8,
    /// Galois LFSR state for `Random` mode.
    lfsr: u32,
}

#[cfg(feature = "arp")]
impl Default for Arpeggiator {
    fn default() -> Self {
        Self {
            phase: 0.0,
            step: 0,
            sounding: None,
            gate_fired: false,
            direction: 0,
            lfsr: 0xACE1_FEED,
        }
    }
}

#[cfg(feature = "arp")]
impl Arpeggiator {
    /// Create a new, idle arpeggiator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance one sample and return any note events that fire this sample.
    ///
    /// Call once per sample from inside the audio render loop.
    /// When both `off` and `on` are `Some`, apply `off` first.
    pub fn tick(&mut self, sample_rate: f32, params: &ArpParams) -> ArpEvents {
        if params.count == 0 {
            return ArpEvents::default();
        }

        let mut result = ArpEvents::default();

        // Gate expiry: NoteOff fires when phase crosses the gate threshold mid-step.
        if self.sounding.is_some() && !self.gate_fired && self.phase >= params.gate {
            result.off = self.sounding;
            self.gate_fired = true;
        }

        self.phase += params.rate / sample_rate;

        if self.phase >= 1.0 {
            self.phase -= 1.0;
            let pre_gate_fired = self.gate_fired;
            self.gate_fired = false;

            if let Some(note) = self.sounding.take() {
                // gate = 1.0 case: NoteOff deferred to step boundary.
                if !pre_gate_fired {
                    result.off = Some(note);
                }
                self.advance_step(params);
            }
            // If sounding was None this is the very first step boundary; don't advance.
        }

        // Start a new note whenever the arp is not currently sounding one.
        if self.sounding.is_none() && params.count > 0 {
            let note = params.notes[self.step as usize];
            self.sounding = Some(note);
            result.on = Some(note);
        }

        result
    }

    fn advance_step(&mut self, params: &ArpParams) {
        let count = params.count;
        if count == 0 {
            return;
        }
        match params.mode {
            ArpMode::Up => {
                self.step = (self.step + 1) % count;
            }
            ArpMode::Down => {
                self.step = (self.step + count - 1) % count;
            }
            ArpMode::UpDown => {
                self.advance_step_updown(count);
            }
            ArpMode::Random => {
                self.tick_lfsr();
                // Safe: result is always < count, which is <= 4, fitting in u8.
                #[allow(clippy::cast_possible_truncation)]
                {
                    self.step = (self.lfsr % u32::from(count)) as u8;
                }
            }
        }
    }

    fn advance_step_updown(&mut self, count: u8) {
        if count <= 1 {
            self.step = 0;
            return;
        }
        if self.direction == 0 {
            self.step += 1;
            if self.step >= count {
                // Reached top — bounce back to count-2, reverse direction.
                self.step = count.saturating_sub(2);
                self.direction = 1;
            }
        } else if self.step == 0 {
            // Reached bottom — bounce to 1, reverse direction.
            self.step = 1.min(count - 1);
            self.direction = 0;
        } else {
            self.step -= 1;
        }
    }

    fn tick_lfsr(&mut self) {
        let bit = self.lfsr & 1;
        self.lfsr >>= 1;
        if bit != 0 {
            self.lfsr ^= 0xB4BC_D35C;
        }
    }

    /// Add `note` to the arp list if it is not already present and space remains.
    pub fn add_note(&mut self, params: &mut ArpParams, note: MidiNote) {
        if params.count >= 4 {
            return;
        }
        if params.notes[..params.count as usize].contains(&note) {
            return;
        }
        params.notes[params.count as usize] = note;
        params.count += 1;
    }

    /// Remove `note` from the arp list, shifting the remaining entries down.
    pub fn remove_note(&mut self, params: &mut ArpParams, note: MidiNote) {
        let count = params.count as usize;
        if let Some(pos) = params.notes[..count].iter().position(|&n| n == note) {
            for i in pos..count - 1 {
                params.notes[i] = params.notes[i + 1];
            }
            params.count -= 1;
            if params.count > 0 && self.step >= params.count {
                self.step = params.count - 1;
            }
        }
    }

    /// Replace the arp list with `notes` (up to 4 entries) and reset playback state.
    pub fn set_notes(&mut self, params: &mut ArpParams, notes: &[MidiNote]) {
        let n = notes.len().min(4);
        params.notes[..n].copy_from_slice(&notes[..n]);
        // Safe: n is bounded by 4, which fits in u8.
        #[allow(clippy::cast_possible_truncation)]
        {
            params.count = n as u8;
        }
        self.step = 0;
        self.direction = 0;
        self.phase = 0.0;
        self.sounding = None;
        self.gate_fired = false;
    }

    /// Clear the arp list and all playback state.
    ///
    /// `AudioChannel::panic()` silences voices directly afterwards, so no
    /// `NoteOff` event is needed here.
    pub fn panic(&mut self, params: &mut ArpParams) {
        params.count = 0;
        self.step = 0;
        self.direction = 0;
        self.phase = 0.0;
        self.sounding = None;
        self.gate_fired = false;
    }
}

#[cfg(all(test, feature = "arp"))]
mod tests {
    use super::*;
    use crate::params::{ArpMode, ArpParams, MidiNote};

    const SR: f32 = 44100.0;

    /// Helper: Up mode params with rate = SR (1 step per sample, simplifies assertions).
    fn up_params(notes: &[u8]) -> ArpParams {
        // Safe: notes.len().min(4) is always <= 4, which fits in u8.
        #[allow(clippy::cast_possible_truncation)]
        let count = notes.len().min(4) as u8;
        let mut note_arr = ArpParams::default().notes;
        for (i, &n) in notes.iter().enumerate().take(4) {
            note_arr[i] = MidiNote(n);
        }
        ArpParams {
            enabled: true,
            rate: SR,
            mode: ArpMode::Up,
            notes: note_arr,
            count,
            ..ArpParams::default()
        }
    }

    #[test]
    fn count_zero_returns_no_events() {
        let mut arp = Arpeggiator::new();
        let params = ArpParams {
            count: 0,
            ..ArpParams::default()
        };
        let e = arp.tick(SR, &params);
        assert_eq!(e.on, None);
        assert_eq!(e.off, None);
    }

    #[test]
    fn first_tick_fires_note_on_immediately() {
        let mut arp = Arpeggiator::new();
        let params = up_params(&[60, 64, 67]);
        let e = arp.tick(SR, &params);
        assert_eq!(e.on, Some(MidiNote(60)));
        assert_eq!(e.off, None);
    }

    #[test]
    fn up_mode_cycles_three_notes_in_order() {
        let mut arp = Arpeggiator::new();
        let params = up_params(&[60, 64, 67]);

        // Tick 1: step 0, first note on
        let e = arp.tick(SR, &params);
        assert_eq!(e.on, Some(MidiNote(60)), "tick 1 NoteOn");
        assert_eq!(e.off, None, "tick 1 no NoteOff");

        // Tick 2: step advances to 1 (rate = SR -> phase hits 1.0 each tick)
        let e = arp.tick(SR, &params);
        assert_eq!(e.off, Some(MidiNote(60)), "tick 2 NoteOff 60");
        assert_eq!(e.on, Some(MidiNote(64)), "tick 2 NoteOn 64");

        // Tick 3: step advances to 2
        let e = arp.tick(SR, &params);
        assert_eq!(e.off, Some(MidiNote(64)), "tick 3 NoteOff 64");
        assert_eq!(e.on, Some(MidiNote(67)), "tick 3 NoteOn 67");

        // Tick 4: wraps back to step 0
        let e = arp.tick(SR, &params);
        assert_eq!(e.off, Some(MidiNote(67)), "tick 4 NoteOff 67");
        assert_eq!(e.on, Some(MidiNote(60)), "tick 4 NoteOn 60 (wrap)");
    }

    #[test]
    fn gate_fires_note_off_mid_step() {
        // rate = SR/2 -> 2 samples per step; gate = 0.5 -> NoteOff at sample boundary
        let mut arp = Arpeggiator::new();
        let mut params = up_params(&[60, 64]);
        params.rate = SR / 2.0;
        params.gate = 0.5;

        // Sample 1: phase 0->0.5, NoteOn(60), no gate fire (phase was 0 < 0.5 at gate check)
        let e = arp.tick(SR, &params);
        assert_eq!(e.on, Some(MidiNote(60)), "s1 NoteOn");
        assert_eq!(e.off, None, "s1 no NoteOff");

        // Sample 2: gate check phase=0.5 >= 0.5 -> NoteOff(60);
        //           phase -> 1.0 -> step advance -> NoteOn(64)
        let e = arp.tick(SR, &params);
        assert_eq!(e.off, Some(MidiNote(60)), "s2 NoteOff");
        assert_eq!(e.on, Some(MidiNote(64)), "s2 NoteOn");
    }

    #[test]
    fn gate_1_fires_note_off_at_step_boundary() {
        let mut arp = Arpeggiator::new();
        let mut params = up_params(&[60, 64]);
        params.rate = SR / 2.0;
        params.gate = 1.0;

        // Sample 1: NoteOn(60), no NoteOff
        let e = arp.tick(SR, &params);
        assert_eq!(e.on, Some(MidiNote(60)));
        assert_eq!(e.off, None);

        // Sample 2: phase 0.5 < 1.0 -> no gate fire; phase -> 1.0 -> boundary:
        //           NoteOff(60) at boundary, then NoteOn(64)
        let e = arp.tick(SR, &params);
        assert_eq!(e.off, Some(MidiNote(60)), "gate=1.0 NoteOff at boundary");
        assert_eq!(e.on, Some(MidiNote(64)));
    }

    #[test]
    fn down_mode_cycles_descending() {
        let mut arp = Arpeggiator::new();
        let mut params = up_params(&[60, 64, 67]);
        params.mode = ArpMode::Down;

        // Down starts at step 0 (lowest index), advances down.
        // Tick 1: NoteOn notes[0]=60
        let e = arp.tick(SR, &params);
        assert_eq!(e.on, Some(MidiNote(60)));

        // Tick 2: step wraps down -> (0 + 3 - 1) % 3 = 2 -> notes[2]=67
        let e = arp.tick(SR, &params);
        assert_eq!(e.off, Some(MidiNote(60)));
        assert_eq!(e.on, Some(MidiNote(67)));

        // Tick 3: step -> (2 + 3 - 1) % 3 = 1 -> notes[1]=64
        let e = arp.tick(SR, &params);
        assert_eq!(e.off, Some(MidiNote(67)));
        assert_eq!(e.on, Some(MidiNote(64)));

        // Tick 4: step -> (1 + 3 - 1) % 3 = 0 -> notes[0]=60
        let e = arp.tick(SR, &params);
        assert_eq!(e.off, Some(MidiNote(64)));
        assert_eq!(e.on, Some(MidiNote(60)));
    }

    #[test]
    fn updown_mode_bounces_at_ends() {
        let mut arp = Arpeggiator::new();
        let mut params = up_params(&[60, 64, 67]);
        params.mode = ArpMode::UpDown;

        // Expected sequence: 0(60), 1(64), 2(67), 1(64), 0(60), 1(64), 2(67), ...
        let expected_notes = [60u8, 64, 67, 64, 60, 64, 67, 64, 60];
        let mut prev_on: Option<MidiNote> = None;
        for (i, &n) in expected_notes.iter().enumerate() {
            let e = arp.tick(SR, &params);
            assert_eq!(
                e.on,
                Some(MidiNote(n)),
                "tick {} expected NoteOn({})",
                i + 1,
                n
            );
            if i > 0 {
                assert_eq!(
                    e.off,
                    prev_on,
                    "tick {} expected NoteOff({:?})",
                    i + 1,
                    prev_on
                );
            }
            prev_on = e.on;
        }
    }

    #[test]
    fn random_mode_stays_in_bounds() {
        let mut arp = Arpeggiator::new();
        let mut params = up_params(&[60, 64, 67, 69]); // 4 notes
        params.mode = ArpMode::Random;

        // Run 1000 ticks; every triggered note must be one of the four.
        for _ in 0..1000 {
            let e = arp.tick(SR, &params);
            if let Some(note) = e.on {
                assert!(
                    params.notes[..params.count as usize].contains(&note),
                    "random arp emitted note {note:?} not in list"
                );
            }
        }
    }

    #[test]
    fn random_mode_single_note_always_returns_same_note() {
        let mut arp = Arpeggiator::new();
        let mut params = up_params(&[60]);
        params.mode = ArpMode::Random;

        for _ in 0..20 {
            let e = arp.tick(SR, &params);
            if let Some(note) = e.on {
                assert_eq!(note, MidiNote(60));
            }
        }
    }
}
