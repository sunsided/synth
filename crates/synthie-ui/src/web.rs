//! Browser plumbing shared by the web builds of the front-ends.
//!
//! Ratzilla renders the unchanged ratatui UI into a WebGL2 canvas, and keys
//! come from listeners on the document.  Web Audio runs its callbacks on the
//! main thread, so rendering has to stay cheap: WebGL2 draws the grid on the
//! GPU instead of updating thousands of DOM nodes per frame.
//!
//! The page is expected to size the canvas to the viewport via CSS
//! (`canvas { width: 100vw; height: 100vh }`); the backend follows the
//! canvas' display size every frame, and [`centered`] keeps the UI in the
//! middle as the window is resized.

use ratatui::layout::{Alignment, Constraint, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::{Frame, Terminal};
use ratzilla::backend::webgl2::WebGl2BackendOptions;
use ratzilla::{FontAtlasConfig, WebGl2Backend};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use crate::key::{Key, KeyCode};
use crate::theme::{CYAN, DIM, MAGENTA};
use crate::widgets::{panel, popup};

/// Same stack as the pages' CSS; the browser falls back per glyph.
const FONTS: [&str; 6] = [
    "JetBrains Mono",
    "Fira Code",
    "DejaVu Sans Mono",
    "Menlo",
    "Consolas",
    "monospace",
];
const FONT_SIZE: f32 = 15.0;

/// Install the panic hook and create the WebGL2-backed terminal.
///
/// # Panics
///
/// Panics if the page has no WebGL2 support.
#[must_use]
pub fn terminal() -> Terminal<WebGl2Backend> {
    console_error_panic_hook::set_once();
    // A dynamic atlas rasterises glyphs with the page's fonts; the built-in
    // static atlas lacks glyphs such as the arrows in the key hints.
    let options = WebGl2BackendOptions::new()
        .disable_auto_css_resize()
        .font_atlas_config(FontAtlasConfig::dynamic(&FONTS, FONT_SIZE));
    let backend =
        WebGl2Backend::new_with_options(options).expect("failed to create WebGL2 backend");
    Terminal::new(backend).expect("failed to create terminal")
}

/// The part of `area` the UI is drawn into: at most `max_cols` × `max_rows`
/// cells, centred, so large windows get an even margin.
#[must_use]
pub fn centered(area: Rect, max_cols: u16, max_rows: u16) -> Rect {
    area.centered(
        Constraint::Length(max_cols.min(area.width)),
        Constraint::Length(max_rows.min(area.height)),
    )
}

/// The "press any key to start audio" box shown until the first key press.
/// Browsers only start audio from a user gesture.
pub fn draw_start_prompt(frame: &mut Frame, area: Rect, title: &str, hint: &str) {
    let text = vec![
        Line::raw(""),
        Line::styled("press any key to start audio", CYAN),
        Line::raw(""),
        Line::styled(hint.to_string(), DIM),
    ];
    let rect = popup(frame, area, 40, 6);
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .block(panel(title, true).border_style(MAGENTA)),
        rect,
    );
}

/// Call `on_key` for every `keydown` and `keyup` on the document, so the
/// canvas never needs focus.
///
/// The browser's own handling (Tab focus, Space scrolling, quick find, …) is
/// suppressed for every key the app may use.  Ctrl / Cmd shortcuts, reload
/// (F5), fullscreen (F11) and devtools (F12) keep working, except the Ctrl
/// combinations listed in `app_ctrl_keys`, which go to the app instead.
///
/// # Panics
///
/// Panics if the page has no document.
pub fn listen_for_keys(app_ctrl_keys: &'static [char], mut on_key: impl FnMut(Key) + 'static) {
    let listener =
        Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |event: web_sys::KeyboardEvent| {
            let key = Key::from(&event);
            let app_ctrl = matches!(key.code, KeyCode::Char(c) if app_ctrl_keys.contains(&c));
            let browser_key = (key.ctrl && !app_ctrl)
                || event.meta_key()
                || matches!(key.code, KeyCode::F(5 | 11 | 12));
            if browser_key {
                return;
            }
            event.prevent_default();
            on_key(key);
        });
    let document = web_sys::window()
        .and_then(|w| w.document())
        .expect("no document");
    for kind in ["keydown", "keyup"] {
        document
            .add_event_listener_with_callback(kind, listener.as_ref().unchecked_ref())
            .expect("failed to listen for keys");
    }
    // The listener lives as long as the page.
    listener.forget();
}

/// Call `on_blur` when the page loses focus.  Key-up events stop arriving
/// then, so apps playing held notes should release them here.
///
/// # Panics
///
/// Panics if the page has no window.
pub fn on_blur(on_blur: impl FnMut() + 'static) {
    let listener = Closure::<dyn FnMut()>::new(on_blur);
    web_sys::window()
        .expect("no window")
        .add_event_listener_with_callback("blur", listener.as_ref().unchecked_ref())
        .expect("failed to listen for blur");
    listener.forget();
}
