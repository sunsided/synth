//! Library interface for the `synth` crate.
//!
//! Exposes the core DSP modules so external crates (e.g. fuzz harnesses)
//! can import synthesiser types without pulling in the TUI binary entry point.

pub mod audio;
pub mod params;
