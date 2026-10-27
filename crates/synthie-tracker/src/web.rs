//! Browser entry point: Ratzilla renders the ratatui UI into a WebGL2 canvas, cpal
//! plays through Web Audio, and keys come from a `keydown` listener on the
//! document.
//!
//! Browsers only start audio from a user gesture, so the first key press
//! resumes the audio context instead of reaching the tracker.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cpal::traits::StreamTrait;
use ratzilla::backend::webgl2::WebGl2BackendOptions;
use ratzilla::ratatui::Terminal;
use ratzilla::ratatui::layout::{Alignment, Constraint, Rect};
use ratzilla::ratatui::text::Line;
use ratzilla::ratatui::widgets::Paragraph;
use ratzilla::{FontAtlasConfig, WebGl2Backend, WebRenderer};
use synthie::audio::engine::setup_audio_silenced_with_sequencer;
use synthie_ui::theme::{CYAN, DIM, MAGENTA};
use synthie_ui::widgets::{panel, popup};
use synthie_ui::{Key, KeyCode, ScopeBuf};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use crate::SCOPE_CAPACITY;
use crate::app::{input::handle_key, state::TrackerState, ui::draw_in};
use crate::tracker::player::Player;

pub fn main() {
    console_error_panic_hook::set_once();

    let (player, sequencer) = Player::new();
    let audio_ok = Arc::new(AtomicBool::new(true));
    let audio_ok_cb = Arc::clone(&audio_ok);
    let (stream, event_tx, scope_rx) = setup_audio_silenced_with_sequencer(sequencer, move |err| {
        if err.kind() == cpal::ErrorKind::DeviceNotAvailable {
            audio_ok_cb.store(false, Ordering::Relaxed);
        }
    })
    .expect("failed to open WebAudio output");

    let state = Rc::new(RefCell::new(TrackerState::new(event_tx, player, audio_ok)));
    let started = Rc::new(Cell::new(false));

    listen_for_keys(Rc::clone(&state), Rc::clone(&started), stream);

    // Web Audio runs its callbacks on the main thread, so rendering must stay
    // cheap: WebGL2 draws the grid on the GPU instead of updating thousands of
    // DOM nodes per frame.  It has no key events, but we listen ourselves.
    // CSS sizes the canvas to the viewport (see index.html); the backend
    // follows its display size every frame, so the centred UI re-centres on
    // window resize.
    // A dynamic atlas rasterises glyphs with the page's fonts; the built-in
    // static atlas lacks glyphs such as the arrows in the key hints.
    let options = WebGl2BackendOptions::new()
        .disable_auto_css_resize()
        .font_atlas_config(FontAtlasConfig::dynamic(&FONTS, FONT_SIZE));
    let backend =
        WebGl2Backend::new_with_options(options).expect("failed to create WebGL2 backend");
    let terminal = Terminal::new(backend).expect("failed to create terminal");
    let mut scope = ScopeBuf::new(scope_rx, SCOPE_CAPACITY);
    terminal.draw_web(move |frame| {
        scope.update();
        let mut state = state.borrow_mut();
        state.tick_meters();
        let data = scope.as_chart_data();
        let area = centered(frame.area());
        draw_in(frame, area, &state, &data, scope.level());
        if !started.get() {
            draw_start_prompt(frame, area);
        }
    });
}

/// Route `keydown` events into the tracker.  The listener sits on the
/// document, so the grid never needs focus, and it suppresses the browser's
/// own handling (Tab focus, Space scrolling, …) for every key the tracker may
/// use, leaving Ctrl / Cmd shortcuts, reload, fullscreen and devtools alone.
fn listen_for_keys(
    state: Rc<RefCell<TrackerState>>,
    started: Rc<Cell<bool>>,
    stream: cpal::Stream,
) {
    let on_key =
        Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |event: web_sys::KeyboardEvent| {
            let key = Key::from(&event);
            let browser_key =
                key.ctrl || event.meta_key() || matches!(key.code, KeyCode::F(5 | 11 | 12));
            if browser_key {
                return;
            }
            event.prevent_default();

            if !started.get() {
                // Inside a user gesture, so the audio context may resume now.
                let _ = stream.play();
                started.set(true);
                return;
            }

            // Never asks to quit here: `CAN_QUIT` is false in the browser.
            let _ = handle_key(key, &mut state.borrow_mut());
        });

    web_sys::window()
        .and_then(|w| w.document())
        .expect("no document")
        .add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref())
        .expect("failed to listen for keys");
    // The listener lives as long as the page.
    on_key.forget();
}

/// Same stack as `index.html`; the browser falls back per glyph.
const FONTS: [&str; 6] = [
    "JetBrains Mono",
    "Fira Code",
    "DejaVu Sans Mono",
    "Menlo",
    "Consolas",
    "monospace",
];
const FONT_SIZE: f32 = 15.0;

/// Largest UI size in cells; bigger windows get the UI centred with a margin.
const MAX_COLS: u16 = 120;
const MAX_ROWS: u16 = 40;

fn centered(area: Rect) -> Rect {
    area.centered(
        Constraint::Length(MAX_COLS.min(area.width)),
        Constraint::Length(MAX_ROWS.min(area.height)),
    )
}

fn draw_start_prompt(frame: &mut ratzilla::ratatui::Frame, area: Rect) {
    let text = vec![
        Line::raw(""),
        Line::styled("press any key to start audio", CYAN),
        Line::raw(""),
        Line::styled("then Space to play · F1 for keys", DIM),
    ];
    let rect = popup(frame, area, 40, 6);
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .block(panel("SYNTHIE TRACKER", true).border_style(MAGENTA)),
        rect,
    );
}
