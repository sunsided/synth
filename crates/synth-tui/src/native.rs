//! Terminal entry point: crossterm input, ratatui rendering, cpal output.

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    io::{self, Stdout},
    panic,
    time::{Duration, Instant},
};
use synthie::audio::engine::{ScopeBatch, setup_audio_silenced};
use synthie_ui::{Key, KeyKind, ScopeBuf};

use crate::SCOPE_CAPACITY;
use crate::app::{
    input,
    state::{AppState, PIANO_KEY_COUNT},
    ui,
};

/// Without key-release events, a piano key counts as released this long
/// after its last press or auto-repeat. Longer than common auto-repeat
/// delays (500–660 ms), so holding a key keeps one note going.
const RELEASE_FALLBACK: Duration = Duration::from_millis(700);

/// Launch the terminal synthesiser application.
///
/// Sets up the panic hook, audio engine, terminal, and event loop, then
/// restores terminal state before returning.
pub fn run_tui() -> Result<()> {
    // Install panic hook so the terminal is restored even on unexpected panics.
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal_raw();
        original_hook(info);
    }));

    let (stream, event_tx, scope_rx) = setup_audio_silenced()?;
    // Keep stream alive for the duration of the program.
    let _stream = stream;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    // Key-release events give proper note-offs. Kitty, foot, Ghostty and
    // WezTerm (with `enable_kitty_keyboard = true`) report them; elsewhere
    // the release is timed out instead, see `ReleaseFallback`.
    let keyboard_enhance = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false)
        && execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
        )
        .is_ok();

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = AppState::new(event_tx);
    state.load_user_presets(crate::preset_store::load());
    let mut scope = ScopeBuf::new(scope_rx, SCOPE_CAPACITY);

    let fallback = (!keyboard_enhance).then(ReleaseFallback::default);
    let result = run_loop(&mut terminal, &mut state, &mut scope, fallback);

    if keyboard_enhance {
        let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    }
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    disable_raw_mode()?;
    terminal.show_cursor()?;

    result
}

/// Main event loop.  Draws the UI each frame, polls for input, and returns
/// when the user requests a quit.
fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut AppState,
    scope: &mut ScopeBuf<ScopeBatch>,
    mut fallback: Option<ReleaseFallback>,
) -> Result<()> {
    let tick = Duration::from_millis(16); // ~60 fps

    loop {
        // Drain new waveform samples from the audio thread.
        scope.update();
        let chart_data = scope.as_chart_data();
        let level = scope.level();

        terminal.draw(|frame| ui::draw(frame, state, &chart_data, level))?;

        // Poll with a short timeout so the scope stays animated.
        if event::poll(tick)? {
            // Terminal resize and other events are no-ops; ratatui redraws on next frame.
            if let event::Event::Key(key) = event::read()? {
                let key = Key::from(key);
                // Ctrl+key is a shortcut (save, export), never a piano key.
                let piano = fallback
                    .as_ref()
                    .and(input::key_to_semitone(key.code))
                    .filter(|_| key.kind == KeyKind::Press && !key.ctrl);
                if let (Some(f), Some(semi)) = (&mut fallback, piano)
                    && state.piano_key_held(semi)
                {
                    // Without release events, auto-repeat arrives as presses.
                    f.touch(semi, Instant::now());
                    continue;
                }
                if input::handle_key(key, state) {
                    return Ok(());
                }
                if let (Some(f), Some(semi)) = (&mut fallback, piano)
                    && state.piano_key_held(semi)
                {
                    f.touch(semi, Instant::now());
                }
            }
        }
        if let Some(f) = &mut fallback {
            for semi in f.expired(Instant::now()) {
                state.piano_key_up(semi);
            }
        }
    }
}

/// Stands in for key-release events on terminals that don't report them:
/// remembers when each piano key was last pressed (or auto-repeated) and
/// reports it released once it has been quiet for [`RELEASE_FALLBACK`].
#[derive(Default)]
struct ReleaseFallback {
    last_press: [Option<Instant>; PIANO_KEY_COUNT],
}

impl ReleaseFallback {
    /// Piano key `semi` was pressed or auto-repeated at `now`.
    fn touch(&mut self, semi: u8, now: Instant) {
        if let Some(slot) = self.last_press.get_mut(usize::from(semi)) {
            *slot = Some(now);
        }
    }

    /// Piano keys quiet for long enough at `now`; each is reported once.
    fn expired(&mut self, now: Instant) -> Vec<u8> {
        let mut keys = Vec::new();
        for (semi, slot) in (0_u8..).zip(&mut self.last_press) {
            if slot.is_some_and(|t| now.duration_since(t) >= RELEASE_FALLBACK) {
                *slot = None;
                keys.push(semi);
            }
        }
        keys
    }
}

/// Restore terminal state without a full `Terminal` object (panic path).
fn restore_terminal_raw() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_fallback_times_out_quiet_keys_once() {
        let t0 = Instant::now();
        let mut f = ReleaseFallback::default();
        f.touch(3, t0);
        assert_eq!(f.expired(t0 + Duration::from_millis(600)), Vec::<u8>::new());
        // An auto-repeat keeps the key held.
        f.touch(3, t0 + Duration::from_millis(600));
        assert_eq!(
            f.expired(t0 + Duration::from_millis(1_200)),
            Vec::<u8>::new()
        );
        assert_eq!(f.expired(t0 + Duration::from_millis(1_300)), [3]);
        assert_eq!(f.expired(t0 + Duration::from_secs(5)), Vec::<u8>::new());
    }
}
