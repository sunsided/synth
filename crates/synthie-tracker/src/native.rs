//! Terminal entry point: crossterm input, ratatui rendering, cpal output.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyboardEnhancementFlags,
        MouseButton, MouseEventKind, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use synthie::audio::engine::setup_audio_silenced_with_sequencer;
use synthie_ui::ScopeBuf;

use crate::SCOPE_CAPACITY;
use crate::app::input::{handle_click, handle_key};
use crate::app::{state::TrackerState, ui::draw};
use crate::reference::Reference;
use crate::tracker::Song;
use crate::tracker::modfile;
use crate::tracker::player::Player;

pub fn main() -> Result<()> {
    // ── Set up panic hook so the terminal is always restored ─────────────────
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal().ok();
        default_panic(info);
    }));

    // ── Start the audio engine ───────────────────────────────────────────────
    let (player, sequencer) = Player::new();
    let audio_ok = Arc::new(AtomicBool::new(true));
    let audio_ok_cb = Arc::clone(&audio_ok);
    let (_stream, event_tx, scope_rx) =
        setup_audio_silenced_with_sequencer(sequencer, move |err| {
            // Only flag fatal device loss; xruns (ErrorKind::Xrun) are non-fatal and
            // self-recovering — latching false on every xrun would permanently show
            // AUDIO ERR after the first underrun even when audio continues.
            if err.kind() == cpal::ErrorKind::DeviceNotAvailable {
                audio_ok_cb.store(false, Ordering::Relaxed);
            }
        })?;

    // ── Load a MOD given on the command line, before taking the terminal ─────
    let module = load_mod_args()?;

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
    let mut state = TrackerState::new(event_tx, player, audio_ok);
    if let Some((title, song, reference)) = module {
        state.status_msg = format!(
            "{title}: {} rows imported · Space plays synths, F2 the original",
            song.pattern.len
        );
        state.song = song;
        state.reference = Some(reference);
    }
    let mut scope = ScopeBuf::new(scope_rx, SCOPE_CAPACITY);

    // ── Main event loop ───────────────────────────────────────────────────────
    let result = run(&mut terminal, &mut state, &mut scope);

    // ── Tear down terminal ────────────────────────────────────────────────────
    restore_terminal()?;

    // Propagate any error from the event loop.
    result
}

/// `synthie-tracker [song.mod [order-position]]`: import the MOD into the
/// grid and open its original sound for reference playback.
fn load_mod_args() -> Result<Option<(String, Song, Reference)>> {
    let mut args = std::env::args_os().skip(1);
    let Some(path) = args.next() else {
        return Ok(None);
    };
    let start = args
        .next()
        .map(|pos| pos.to_string_lossy().parse::<usize>())
        .transpose()
        .context("order position must be a number")?
        .unwrap_or(0);
    let bytes = std::fs::read(&path).with_context(|| format!("cannot read {}", path.display()))?;
    let (title, song) = modfile::import(&bytes, start)
        .with_context(|| format!("cannot import {}", path.display()))?;
    let reference = Reference::new(&bytes)?;
    Ok(Some((title, song, reference)))
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

        // Poll for input (16 ms ≈ 60 fps).  Between polls the playing-step
        // indicator updates via atomic reads, so the grid refreshes smoothly.
        if event::poll(Duration::from_millis(16))? {
            let quit = match event::read()? {
                Event::Key(key) => handle_key(key.into(), state),
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                    handle_click(mouse.column, mouse.row, state)
                }
                _ => false,
            };
            if quit {
                break;
            }
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
