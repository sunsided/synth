//! Neon/retro chiptune palette and animation clock.
//!
//! All colours are 24-bit; every modern terminal emulator (kitty, `WezTerm`,
//! Alacritty, GNOME Terminal, Windows Terminal, iTerm2) renders them as-is.

use ratatui::style::{Color, Modifier, Style};
use std::sync::OnceLock;
use std::time::Instant;

/// Screen background: deep indigo-black.
pub const BG: Color = Color::Rgb(12, 9, 24);
/// Alternate background used for beat stripes and zebra rows.
pub const BG_STRIPE: Color = Color::Rgb(24, 18, 46);
/// Background of the focused row / cursor line.
pub const BG_CURSOR: Color = Color::Rgb(52, 30, 92);
/// Background of the tracker playhead row.
pub const BG_PLAYHEAD: Color = Color::Rgb(0, 62, 74);

/// Primary text.
pub const FG: Color = Color::Rgb(222, 216, 255);
/// Secondary text: labels, hints.
pub const DIM: Color = Color::Rgb(112, 100, 160);
/// Tertiary: inactive borders, empty cells, separators.
pub const FAINT: Color = Color::Rgb(58, 48, 92);

/// Electric cyan.
pub const CYAN: Color = Color::Rgb(0, 240, 255);
/// Hot magenta.
pub const MAGENTA: Color = Color::Rgb(255, 46, 170);
/// Ultraviolet.
pub const PURPLE: Color = Color::Rgb(168, 92, 255);
/// Laser yellow.
pub const YELLOW: Color = Color::Rgb(255, 228, 77);
/// Phosphor green.
pub const GREEN: Color = Color::Rgb(57, 255, 140);
/// Sunset orange.
pub const ORANGE: Color = Color::Rgb(255, 140, 40);
/// Warning red.
pub const RED: Color = Color::Rgb(255, 56, 96);

/// Gradient stops for [`gradient`]: cyan → purple → magenta → orange.
const STOPS: [(u8, u8, u8); 4] = [
    (0, 240, 255),
    (168, 92, 255),
    (255, 46, 170),
    (255, 140, 40),
];

/// Gradient stops for [`heat`]: green → yellow → red.
const HEAT: [(u8, u8, u8); 3] = [(57, 255, 140), (255, 228, 77), (255, 56, 96)];

/// Sample a multi-stop gradient at `t` ∈ [0, 1].
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)] // channel values are clamped to 0..=255; stop counts are tiny
fn sample(stops: &[(u8, u8, u8)], t: f32) -> Color {
    let pos = t.clamp(0.0, 1.0) * (stops.len() - 1) as f32;
    let idx = (pos.floor() as usize).min(stops.len() - 2);
    let frac = pos - idx as f32;
    let (from, to) = (stops[idx], stops[idx + 1]);
    let lerp = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * frac).round() as u8;
    Color::Rgb(lerp(from.0, to.0), lerp(from.1, to.1), lerp(from.2, to.2))
}

/// Signature neon gradient (cyan → purple → magenta → orange) at `t` ∈ [0, 1].
#[must_use]
pub fn gradient(t: f32) -> Color {
    sample(&STOPS, t)
}

/// Like [`gradient`] but periodic: `t` wraps and the colour ping-pongs, so it
/// can be scrolled over time without a visible seam.
#[must_use]
pub fn gradient_cyclic(t: f32) -> Color {
    let f = t.rem_euclid(1.0);
    gradient(1.0 - (2.0 * f - 1.0).abs())
}

/// Level-meter colour ramp (green → yellow → red) at `t` ∈ [0, 1].
#[must_use]
pub fn heat(t: f32) -> Color {
    sample(&HEAT, t)
}

/// Seconds since the UI first asked for the time; drives all animation.
#[must_use]
pub fn clock() -> f32 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

/// Smooth 0‥1 oscillation at `hz`, used for breathing/pulsing indicators.
#[must_use]
pub fn pulse(hz: f32) -> f32 {
    0.5 + 0.5 * (clock() * std::f32::consts::TAU * hz).sin()
}

/// Plain style with foreground `fg`.
#[must_use]
pub fn fg(color: Color) -> Style {
    Style::default().fg(color)
}

/// Bold style with foreground `fg`.
#[must_use]
pub fn bold(color: Color) -> Style {
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}
