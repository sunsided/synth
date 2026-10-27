//! Browser entry point: the shared [`synthie_ui::web`] plumbing renders the
//! ratatui UI into a WebGL2 canvas and feeds keys in; cpal plays through
//! Web Audio.
//!
//! Browsers only start audio from a user gesture, so the first key press
//! resumes the audio context instead of playing a note.  Key-up events give
//! note-offs; when the page loses focus they stop arriving, so all notes are
//! released then.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cpal::traits::StreamTrait;
use ratzilla::WebRenderer;
use synthie::audio::engine::setup_audio_silenced;
use synthie_ui::{KeyKind, ScopeBuf, web};

use crate::SCOPE_CAPACITY;
use crate::app::{input::handle_key, state::AppState, ui::draw_in};

/// Largest UI size in cells; bigger windows get the UI centred with a margin.
const MAX_COLS: u16 = 120;
const MAX_ROWS: u16 = 40;

pub fn main() {
    let terminal = web::terminal();

    let (stream, event_tx, scope_rx) =
        setup_audio_silenced().expect("failed to open Web Audio output");

    let state = Rc::new(RefCell::new(AppState::new(event_tx)));
    let started = Rc::new(Cell::new(false));

    // Ctrl+S saves the patch here instead of opening the browser's save dialog.
    web::listen_for_keys(&['s'], {
        let state = Rc::clone(&state);
        let started = Rc::clone(&started);
        move |key| {
            if !started.get() {
                if key.kind == KeyKind::Press {
                    // Inside a user gesture, so the audio context may resume now.
                    let _ = stream.play();
                    started.set(true);
                }
                return;
            }
            // Never asks to quit here: `CAN_QUIT` is false in the browser.
            let _ = handle_key(key, &mut state.borrow_mut());
        }
    });
    web::on_blur({
        let state = Rc::clone(&state);
        move || state.borrow_mut().panic_all_notes()
    });

    let mut scope = ScopeBuf::new(scope_rx, SCOPE_CAPACITY);
    terminal.draw_web(move |frame| {
        scope.update();
        let state = state.borrow();
        let data = scope.as_chart_data();
        let area = web::centered(frame.area(), MAX_COLS, MAX_ROWS);
        draw_in(frame, area, &state, &data, scope.level());
        if !started.get() {
            web::draw_start_prompt(frame, area, "SYNTHIE", "then play Z…M / Q…U · F1 for keys");
        }
    });
}
