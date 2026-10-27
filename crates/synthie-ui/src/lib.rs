//! Shared look and feel for the synthie terminal front-ends.
//!
//! Both `synth-tui` and `synthie-tracker` draw with the same neon/retro
//! chiptune palette ([`theme`]), the same building blocks ([`widgets`]), the
//! same rolling oscilloscope buffer ([`scope`]), and the same quit flow
//! ([`modal`]): `Esc` asks before leaving, `:q` leaves right away.

pub mod key;
pub mod modal;
pub mod scope;
pub mod theme;
pub mod widgets;

pub use key::{Key, KeyCode, KeyKind};
pub use modal::{Modal, ModalAction};
pub use scope::ScopeBuf;
