use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use synthie::audio::engine::setup_audio_silenced;

mod app;
mod tracker;

use app::{input::handle_key, state::TrackerState, ui::draw};

fn main() -> Result<()> {
    // ── Set up panic hook so the terminal is always restored ─────────────────
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal().ok();
        default_panic(info);
    }));

    // ── Start the audio engine ───────────────────────────────────────────────
    let (_stream, event_tx, _scope_rx) = setup_audio_silenced()?;

    // ── Set up the ratatui terminal ──────────────────────────────────────────
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    // Request full keyboard protocol where supported (e.g. kitty terminals).
    let keyboard_flags = KeyboardEnhancementFlags::REPORT_EVENT_TYPES
        | KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;
    execute!(stdout, PushKeyboardEnhancementFlags(keyboard_flags)).ok();

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // ── Build application state ───────────────────────────────────────────────
    let mut state = TrackerState::new(event_tx);

    // ── Main event loop ───────────────────────────────────────────────────────
    let result = run(&mut terminal, &mut state);

    // ── Tear down terminal ────────────────────────────────────────────────────
    restore_terminal()?;

    // Propagate any error from the event loop.
    result
}

/// Run the main event loop; returns when the user quits.
fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: &mut TrackerState,
) -> Result<()> {
    loop {
        terminal.draw(|f| draw(f, state))?;

        // Poll for a key event (16 ms ≈ 60 fps).  Between polls the playing-step
        // indicator updates via atomic reads, so the grid refreshes smoothly.
        if event::poll(Duration::from_millis(16))?
            && let Event::Key(key) = event::read()?
            && handle_key(key, state)
        {
            break;
        }
    }

    Ok(())
}

/// Restore the terminal to its original state.
fn restore_terminal() -> Result<()> {
    execute!(
        std::io::stdout(),
        PopKeyboardEnhancementFlags,
        DisableMouseCapture,
        LeaveAlternateScreen,
    )?;
    disable_raw_mode()?;
    Ok(())
}
