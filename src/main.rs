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
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{
    io::{self, Stdout},
    panic,
    time::Duration,
};
use viz::scope::ScopeBuf;

// Scope display: 512 decimated samples (~46ms of audio at 44100 Hz / 4x decimation)
const SCOPE_CAPACITY: usize = 512;

fn main() -> Result<()> {
    // ── Panic hook: restore terminal even on unexpected panics ──────────────
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal_raw();
        original_hook(info);
    }));

    // ── Audio ────────────────────────────────────────────────────────────────
    let (stream, event_tx, scope_rx) = setup_audio()?;
    // Keep stream alive for the duration of the program
    let _stream = stream;

    // ── Terminal setup ───────────────────────────────────────────────────────
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    // Try to enable keyboard release events (for proper note-off).
    // This is supported in kitty, WezTerm, newer xterm-kitty terminals.
    // If it fails we silently continue (notes will sustain until a new note).
    let keyboard_enhance = execute!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
    )
    .is_ok();

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // ── App state ────────────────────────────────────────────────────────────
    let mut state = AppState::new(event_tx);
    let mut scope = ScopeBuf::new(scope_rx, SCOPE_CAPACITY);

    // ── Event loop ───────────────────────────────────────────────────────────
    let result = run_loop(&mut terminal, &mut state, &mut scope);

    // ── Cleanup ──────────────────────────────────────────────────────────────
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

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    state: &mut AppState,
    scope: &mut ScopeBuf,
) -> Result<()> {
    let tick = Duration::from_millis(16); // ~60 fps

    loop {
        // Drain new waveform samples
        scope.update();
        let chart_data = scope.as_chart_data();

        // Render
        terminal.draw(|frame| ui::draw(frame, state, &chart_data))?;

        // Poll for input with a short timeout so the scope stays animated
        if event::poll(tick)? {
            match event::read()? {
                event::Event::Key(key) => {
                    if input::handle_key(key, state) || state.should_quit {
                        return Ok(());
                    }
                }
                event::Event::Resize(_, _) => {
                    // Terminal resized – ratatui handles on next draw
                }
                _ => {}
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
