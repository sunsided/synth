//! Application entry point.
//!
//! Bootstraps the CPAL audio engine, configures the crossterm terminal, runs
//! the event loop, and restores terminal state on exit (including on panics).

mod app;
mod audio;
mod params;
mod presets;
mod viz;

use anyhow::Result;
use app::{input, state::AppState, ui};
use audio::engine::setup_audio;
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
    time::Duration,
};
use viz::scope::ScopeBuf;

/// Number of decimated samples held in the scope display buffer
/// (~46 ms of audio at 44100 Hz / 4× decimation).
const SCOPE_CAPACITY: usize = 512;

/// Application entry point.
///
/// Sets up the panic hook, audio engine, terminal, and event loop, then
/// restores terminal state before returning.
fn main() -> Result<()> {
    // Install panic hook so the terminal is restored even on unexpected panics.
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal_raw();
        original_hook(info);
    }));

    let (stream, event_tx, scope_rx) = setup_audio()?;
    // Keep stream alive for the duration of the program.
    let _stream = stream;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    // Try to enable keyboard release events (for proper note-off).
    // Supported in kitty, WezTerm, and newer xterm-compatible terminals.
    // Failure is silent: notes will sustain until a new note is pressed.
    let keyboard_enhance = execute!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
    )
    .is_ok();

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = AppState::new(event_tx);
    let mut scope = ScopeBuf::new(scope_rx, SCOPE_CAPACITY);

    let result = run_loop(&mut terminal, &mut state, &mut scope);

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
    scope: &mut ScopeBuf,
) -> Result<()> {
    let tick = Duration::from_millis(16); // ~60 fps

    loop {
        // Drain new waveform samples from the audio thread.
        scope.update();
        let chart_data = scope.as_chart_data();

        terminal.draw(|frame| ui::draw(frame, state, &chart_data))?;

        // Poll with a short timeout so the scope stays animated.
        if event::poll(tick)? {
            // Terminal resize and other events are no-ops; ratatui redraws on next frame.
            if let event::Event::Key(key) = event::read()?
                && (input::handle_key(key, state) || state.should_quit)
            {
                return Ok(());
            }
        }

        if state.should_quit {
            return Ok(());
        }
    }
}

/// Restore terminal state without a full `Terminal` object (panic path).
fn restore_terminal_raw() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    Ok(())
}
