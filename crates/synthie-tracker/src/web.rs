//! Browser entry point: the shared [`synthie_ui::web`] plumbing renders the
//! ratatui UI into a WebGL2 canvas and feeds keys in; cpal plays through
//! Web Audio.
//!
//! Browsers only start audio from a user gesture, so the first key press or
//! click resumes the audio context instead of reaching the tracker. Clicks
//! on the footer's key hints work like the keys.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpal::traits::StreamTrait;
use ratzilla::WebRenderer;
use ratzilla::event::{MouseButton, MouseEventKind};
use synthie::audio::engine::setup_audio_silenced_with_sequencer;
use synthie_ui::{KeyKind, ScopeBuf, web};

use crate::SCOPE_CAPACITY;
use crate::app::input::{handle_click, handle_key};
use crate::app::{state::TrackerState, ui::draw_in};
use crate::tracker::player::Player;

/// Largest UI size in cells; bigger windows get the UI centred with a margin.
const MAX_COLS: u16 = 120;
const MAX_ROWS: u16 = 40;

pub fn main() {
    let mut terminal = web::terminal();

    let (player, sequencer) = Player::new();
    let audio_ok = Arc::new(AtomicBool::new(true));
    let audio_ok_cb = Arc::clone(&audio_ok);
    let (stream, event_tx, scope_rx) = setup_audio_silenced_with_sequencer(sequencer, move |err| {
        if err.kind() == cpal::ErrorKind::DeviceNotAvailable {
            audio_ok_cb.store(false, Ordering::Relaxed);
        }
    })
    .expect("failed to open Web Audio output");

    let state = Rc::new(RefCell::new(TrackerState::new(event_tx, player, audio_ok)));
    let started = Rc::new(Cell::new(false));
    // Inside a user gesture, so the audio context may resume now.
    let start = Rc::new(move || {
        let _ = stream.play();
    });

    web::listen_for_keys(&[], {
        let state = Rc::clone(&state);
        let started = Rc::clone(&started);
        let start = Rc::clone(&start);
        move |key| {
            if !started.get() {
                if key.kind == KeyKind::Press {
                    start();
                    started.set(true);
                }
                return;
            }
            // Never asks to quit here: `CAN_QUIT` is false in the browser.
            let _ = handle_key(key, &mut state.borrow_mut());
        }
    });

    terminal
        .on_mouse_event({
            let state = Rc::clone(&state);
            let started = Rc::clone(&started);
            move |mouse| {
                if mouse.kind != MouseEventKind::ButtonDown(MouseButton::Left) {
                    return;
                }
                if !started.get() {
                    start();
                    started.set(true);
                    return;
                }
                // Never asks to quit here: `CAN_QUIT` is false in the browser.
                let _ = handle_click(mouse.col, mouse.row, &mut state.borrow_mut());
            }
        })
        .expect("failed to listen for mouse events");

    let mut scope = ScopeBuf::new(scope_rx, SCOPE_CAPACITY);
    terminal.draw_web(move |frame| {
        scope.update();
        let mut state = state.borrow_mut();
        state.tick_meters();
        let data = scope.as_chart_data();
        let area = web::centered(frame.area(), MAX_COLS, MAX_ROWS);
        draw_in(frame, area, &state, &data, scope.level());
        if !started.get() {
            web::draw_start_prompt(
                frame,
                area,
                "SYNTHIE TRACKER",
                "or click · Space plays · F1 keys",
            );
        }
    });
}
