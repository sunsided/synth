//! Application layer: input handling, UI state, and terminal rendering.

pub mod input;
pub mod state;
pub mod ui;

/// Whether the app can exit.  In a browser tab there is nothing to quit to,
/// so `Esc` only silences notes and the quit keys, `:` command line and hints
/// go away.
pub const CAN_QUIT: bool = !cfg!(target_arch = "wasm32");
