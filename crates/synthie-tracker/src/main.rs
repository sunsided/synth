use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
use synthie::audio::engine::setup_audio_silenced_with_error_handler;
use synthie_ui::ScopeBuf;

mod app;
mod tracker;

use app::{input::handle_key, state::TrackerState, ui::draw};

/// Number of decimated samples held in the scope display buffer.
const SCOPE_CAPACITY: usize = 512;

fn main() -> Result<()> {
    // ── Set up panic hook so the terminal is always restored ─────────────────
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal().ok();
        default_panic(info);
    }));

    // ── Start the audio engine ───────────────────────────────────────────────
    let audio_ok = Arc::new(AtomicBool::new(true));
    let audio_ok_cb = Arc::clone(&audio_ok);
    let (_stream, event_tx, scope_rx) = setup_audio_silenced_with_error_handler(move |err| {
        // Only flag fatal device loss; xruns (ErrorKind::Xrun) are non-fatal and
        // self-recovering — latching false on every xrun would permanently show
        // AUDIO ERR after the first underrun even when audio continues.
        if err.kind() == cpal::ErrorKind::DeviceNotAvailable {
            audio_ok_cb.store(false, Ordering::Relaxed);
        }
    })?;

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
    let mut state = TrackerState::new(event_tx, audio_ok);
    let mut scope = ScopeBuf::new(scope_rx, SCOPE_CAPACITY);

    // ── Main event loop ───────────────────────────────────────────────────────
    let result = run(&mut terminal, &mut state, &mut scope);

    // ── Tear down terminal ────────────────────────────────────────────────────
    restore_terminal()?;

    // Propagate any error from the event loop.
    result
}

/// Run the main event loop; returns when the user quits.
fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: &mut TrackerState,
    scope: &mut ScopeBuf,
) -> Result<()> {
    loop {
        scope.update();
        state.tick_meters();
        let chart_data = scope.as_chart_data();
        let level = scope.level();
        terminal.draw(|f| draw(f, state, &chart_data, level))?;

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
    let mut stdout = std::io::stdout();
    // Pop keyboard enhancement separately and ignore errors: the terminal may not
    // support the protocol, and a failure here must not abort the rest of cleanup.
    execute!(stdout, PopKeyboardEnhancementFlags).ok();
    execute!(stdout, DisableMouseCapture, LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}
