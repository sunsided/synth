//! Core library for the `synth` crate.
//!
//! Exposes DSP modules, core parameter types, and the built-in preset bank.
//! Terminal UI, runtime bootstrap, and filesystem persistence are owned by the
//! binary crate and are not part of this public API.
//!
//! Fuzz harnesses and other non-TUI consumers can import [`audio`] and
//! [`params`] directly without pulling in any terminal dependencies.

// Safety dance.
#![forbid(unsafe_code)]
// Enable doc_cfg feature on docs.rs for feature-gated API documentation.
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod audio;
pub mod params;
pub mod presets;
