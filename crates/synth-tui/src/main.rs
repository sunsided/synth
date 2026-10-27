//! Binary entry point for the `synth` TUI application.
//!
//! Dispatches to the terminal (`native`) or browser (`web`) runtime; both share
//! the application layer.  Core DSP and parameter types come from `synthie`.
//!
//! The application layer (`app`) and preset persistence (`preset_store`) are
//! binary-local modules declared here; the shared look and the waveform scope
//! buffer come from `synthie-ui`.

mod app;
#[cfg(not(target_arch = "wasm32"))]
mod native;
mod preset_store;
#[cfg(target_arch = "wasm32")]
mod web;

/// Number of decimated samples held in the scope display buffer
/// (~46 ms of audio at 44100 Hz / 4× decimation).
const SCOPE_CAPACITY: usize = 512;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    native::run_tui()
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::main();
}
