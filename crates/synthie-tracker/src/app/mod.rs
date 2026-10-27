//! Application layer: state, input handling, and UI rendering.

pub mod input;
pub mod state;
pub mod ui;

/// Whether the app can exit.  In a browser tab there is nothing to quit to,
/// so `Esc` only panics and the quit keys, `:` command line and hints go away.
pub const CAN_QUIT: bool = !cfg!(target_arch = "wasm32");
