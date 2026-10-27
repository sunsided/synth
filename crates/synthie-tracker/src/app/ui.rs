//! Terminal UI rendering for the synthie tracker.
//!
//! Layout:
//! ```text
//! ╭──────────────────────────────────────────────────────────────────────────────╮
//! │▓▒░ S Y N T H I E ░▒▓  TRACKER  ▶ PLAY        ♩ 120 BPM  LEN 16  OCT 4  05/16 ▉▉▉···│
//! ╰──────────────────────────────────────────────────────────────────────────────╯
//! ╭ ◆ PATTERN ─────────────────────────────────────────────╮╭ ◇ SCOPE ──────────╮
//! │ROW │ CH1 ●    │ CH2 ○    │ CH3 ○    │ CH4 ○    │ DRUMS ●  ││    ⡠⠊⠉⠢⡀          │
//! │    │ C64 Bass │ Arp Lead │ Plucky P │ PWM Lead │ K  H  O  ││ ⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒  │
//! │────┼──────────┼──────────┼──────────┼──────────┼─────────││                   │
//! │ 00 │ C-4      │ ···      │ ···      │ ···      │ ●  ·  ·  │╰───────────────────╯
//! │▶01 │ G-4      │ ···      │ ···      │ ···      │ ·  ●  ·  │╭ ◇ CHANNELS ───────╮
//! │ 02 │ E-4      │ ···      │ ···      │ ···      │ ●  ●  ·  ││ CH1 ▉▉▉▉▉▋······  │
//! ╰────────────────────────────────────────────────────────╯╰───────────────────╯
//!  Space play  Esc panic  Del clear  ↑↓ row  ←→ track  Tab patch  F1 help
//! ```

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use synthie_ui::theme::{
    self, BG, BG_CURSOR, BG_PLAYHEAD, BG_STRIPE, CYAN, DIM, FAINT, FG, GREEN, MAGENTA, ORANGE,
    PURPLE, RED, YELLOW, bold, fg,
};
use synthie_ui::widgets::{
    self, bar, db_readout, help_entry, help_heading, key_hints, logo, meter, panel, vu,
};

use crate::app::state::TrackerState;
use crate::tracker::{DRUM_TRACK, SYNTH_TRACKS, TOTAL_TRACKS, note_name};

// ─── Palette ─────────────────────────────────────────────────────────────────

/// Signature colour per synth channel; the drum track uses [`DRUM_COLOR`].
const CHANNEL_COLORS: [Color; SYNTH_TRACKS] = [CYAN, GREEN, YELLOW, ORANGE];
/// Drum track colour.
const DRUM_COLOR: Color = MAGENTA;
/// Background of the cursor row (between stripe and cursor-cell intensity).
const BG_CURSOR_ROW: Color = Color::Rgb(36, 26, 66);

// ─── Column widths ───────────────────────────────────────────────────────────

/// Width of the row-number column (characters).
const COL_ROW_W: usize = 4;
/// Width of each synth track column.
const COL_SYNTH_W: usize = 10;
/// Width of the drum track column.
const COL_DRUM_W: usize = 10;
/// Total width of one grid line, including the `│` separators.
const GRID_W: usize = COL_ROW_W + (1 + COL_SYNTH_W) * SYNTH_TRACKS + 1 + COL_DRUM_W;
/// Minimum width of the scope/mixer side column; it takes all spare width.
const SIDE_W: u16 = 34;

/// Colour identifying `track`.
fn track_color(track: usize) -> Color {
    CHANNEL_COLORS.get(track).copied().unwrap_or(DRUM_COLOR)
}

// ─── Entry point ─────────────────────────────────────────────────────────────

/// Top-level draw call: the editor, plus the help overlay when requested.
///
/// `scope_data` and `level` come from the shared output scope buffer.
pub fn draw(frame: &mut Frame, state: &TrackerState, scope_data: &[(f64, f64)], level: f32) {
    widgets::paint_background(frame);
    let area = frame.area();

    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_header(frame, header, state);

    // The side column only appears once the grid has room to breathe.
    let grid_min = u16::try_from(GRID_W).unwrap_or(u16::MAX) + 2;
    if body.width >= grid_min + SIDE_W {
        let [grid, side] =
            Layout::horizontal([Constraint::Length(grid_min), Constraint::Min(SIDE_W)]).areas(body);
        draw_grid(frame, grid, state);
        draw_side(frame, side, state, scope_data, level);
    } else {
        draw_grid(frame, body, state);
    }

    draw_footer(frame, footer, state);

    if state.show_help {
        draw_help(frame, area);
    }
    widgets::modal(frame, area, footer, &state.modal);
}

// ─── Header ──────────────────────────────────────────────────────────────────

/// Logo, transport badge, and song stats with a playhead progress bar.
fn draw_header(frame: &mut Frame, area: Rect, state: &TrackerState) {
    let block = panel("", false).title("");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let badge = if !state.audio_ok() {
        Span::styled(" ✗ AUDIO ERR ", badge_style(RED))
    } else if state.is_playing() {
        // Breathe between full and dimmed green while running.
        let glow = theme::pulse(1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // glow ∈ [0, 1]
        let shade = Color::Rgb(0, 170 + (glow * 85.0) as u8, 90 + (glow * 50.0) as u8);
        Span::styled(" ▶ PLAY ", badge_style(shade))
    } else {
        Span::styled(" ■ STOP ", Style::default().fg(DIM).bg(BG_STRIPE))
    };

    let mut left = logo();
    left.spans
        .extend([Span::styled("  TRACKER  ", bold(FG)), badge]);
    frame.render_widget(Paragraph::new(left), inner);

    let len = state.song.pattern.len;
    let (pos, frac) = if state.is_playing() {
        let step = state.playing_step();
        #[allow(clippy::cast_precision_loss)] // pattern length ≤ STEPS
        let frac = (step + 1) as f32 / len.max(1) as f32;
        (format!("{step:02}/{len:02}"), frac)
    } else {
        (format!("--/{len:02}"), 0.0)
    };
    let mut right = vec![
        Span::styled("♩ ", fg(DIM)),
        Span::styled(format!("{:.0}", state.song.bpm), bold(YELLOW)),
        Span::styled(" BPM  LEN ", fg(DIM)),
        Span::styled(len.to_string(), bold(CYAN)),
        Span::styled("  OCT ", fg(DIM)),
        Span::styled(state.octave.to_string(), bold(CYAN)),
        Span::styled("  STEP ", fg(DIM)),
        Span::styled(pos, fg(FG)),
        Span::raw(" "),
    ];
    right.extend(meter(frac, 16));
    right.push(Span::raw(" "));
    frame.render_widget(
        Paragraph::new(Line::from(right)).alignment(Alignment::Right),
        inner,
    );
}

/// Solid status badge in `color`.
fn badge_style(color: Color) -> Style {
    Style::default()
        .fg(BG)
        .bg(color)
        .add_modifier(Modifier::BOLD)
}

// ─── Pattern grid ────────────────────────────────────────────────────────────

/// Pattern editor: two header lines, a rule, then one line per step.
fn draw_grid(frame: &mut Frame, area: Rect, state: &TrackerState) {
    let block = panel("PATTERN", true);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 4 {
        return;
    }

    let mut lines = column_headers(state);
    lines.push(rule());

    let visible_rows = usize::from(inner.height).saturating_sub(lines.len());
    let len = state.song.pattern.len;
    let start = scroll_offset(state.cursor_row, len, visible_rows);
    let playing = state.is_playing().then(|| state.playing_step());
    lines.extend(
        (start..len)
            .take(visible_rows)
            .map(|step| step_line(state, step, playing == Some(step))),
    );

    frame.render_widget(Paragraph::new(lines), inner);
}

/// Pad or truncate `s` to exactly `width` characters.
fn fit(s: &str, width: usize) -> String {
    let mut out: String = s.chars().take(width).collect();
    let n = out.chars().count();
    out.extend(std::iter::repeat_n(' ', width - n));
    out
}

/// Faint `│` column separator.
fn sep() -> Span<'static> {
    Span::styled("│", fg(FAINT))
}

/// Channel name + activity LED, and the assigned patch, for every track.
fn column_headers(state: &TrackerState) -> Vec<Line<'static>> {
    let mut names = vec![Span::styled(fit("ROW", COL_ROW_W), fg(DIM))];
    let mut patches = vec![Span::raw(fit("", COL_ROW_W))];

    for track in 0..TOTAL_TRACKS {
        let color = track_color(track);
        let active = state.cursor_track == track;
        let bg = if active { BG_CURSOR } else { BG };
        let (title, subtitle, width) = if track == DRUM_TRACK {
            ("DRUMS".to_string(), " K  H  O".to_string(), COL_DRUM_W)
        } else {
            let patch_idx = state.song.track_patches[track];
            let patch = state.presets.get(patch_idx).map_or("", |p| p.name.as_str());
            (format!("CH{}", track + 1), format!(" {patch}"), COL_SYNTH_W)
        };
        let led = if state.activity[track] > 0.25 {
            Span::styled("●", fg(color).bg(bg))
        } else {
            Span::styled("○", fg(FAINT).bg(bg))
        };
        let title_style = if active {
            bold(color).bg(bg)
        } else {
            fg(color)
        };

        names.push(sep());
        names.push(Span::styled(format!(" {title} "), title_style));
        names.push(led);
        names.push(Span::styled(
            fit("", width.saturating_sub(title.len() + 3)),
            Style::default().bg(bg),
        ));

        patches.push(sep());
        patches.push(Span::styled(
            fit(&subtitle, width),
            fg(if active { FG } else { DIM }).bg(bg),
        ));
    }
    vec![Line::from(names), Line::from(patches)]
}

/// Horizontal rule under the headers with `┼` at column boundaries.
fn rule() -> Line<'static> {
    let mut s = "─".repeat(COL_ROW_W);
    for width in std::iter::repeat_n(COL_SYNTH_W, SYNTH_TRACKS).chain([COL_DRUM_W]) {
        s.push('┼');
        s.push_str(&"─".repeat(width));
    }
    Line::styled(s, fg(FAINT))
}

/// One step of the pattern across all tracks.
fn step_line(state: &TrackerState, step: usize, is_playing: bool) -> Line<'static> {
    let is_cursor_row = step == state.cursor_row;
    let is_beat = step.is_multiple_of(4);
    let row_bg = if is_playing {
        BG_PLAYHEAD
    } else if is_cursor_row {
        BG_CURSOR_ROW
    } else if is_beat {
        BG_STRIPE
    } else {
        BG
    };

    let (marker, num_style) = if is_playing {
        ("▶", bold(CYAN))
    } else if is_cursor_row {
        (" ", bold(YELLOW))
    } else if is_beat {
        (" ", fg(PURPLE))
    } else {
        (" ", fg(FAINT))
    };
    let mut spans = vec![Span::styled(
        fit(&format!("{marker}{step:02}"), COL_ROW_W),
        num_style,
    )];

    for track in 0..TOTAL_TRACKS {
        spans.push(sep());
        let is_cursor_cell = is_cursor_row && state.cursor_track == track;
        spans.extend(cell_spans(state, track, step, is_cursor_cell, is_playing));
    }

    Line::from(spans).style(Style::default().bg(row_bg))
}

/// Spans for a single grid cell.  The cursor cell is drawn inverted in hot
/// magenta; notes light up white as the playhead passes over them.
fn cell_spans(
    state: &TrackerState,
    track: usize,
    step: usize,
    is_cursor: bool,
    is_playing: bool,
) -> Vec<Span<'static>> {
    let color = track_color(track);
    let cursor = Style::default()
        .fg(BG)
        .bg(MAGENTA)
        .add_modifier(Modifier::BOLD);

    if track == DRUM_TRACK {
        let drum = &state.song.pattern.drums[step];
        let mut spans = vec![Span::raw(" ")];
        for (on, glyph) in [
            (drum.kick, ORANGE),
            (drum.hihat_closed, CYAN),
            (drum.hihat_open, YELLOW),
        ] {
            let style = match (is_cursor, on) {
                (true, _) => cursor,
                (false, true) => bold(glyph),
                (false, false) => fg(FAINT),
            };
            spans.push(Span::styled(if on { "●" } else { "·" }, style));
            spans.push(Span::styled(
                "  ",
                if is_cursor { cursor } else { fg(FAINT) },
            ));
        }
        let used = 1 + 3 * 3;
        spans.push(Span::styled(
            fit("", COL_DRUM_W.saturating_sub(used)),
            if is_cursor { cursor } else { fg(FAINT) },
        ));
        return spans;
    }

    let (text, style) = match state.song.pattern.synth[track][step].note {
        Some(note) => {
            let style = if is_cursor {
                cursor
            } else if is_playing {
                bold(FG)
            } else {
                fg(color)
            };
            (format!(" {}", note_name(note)), style)
        }
        None => (
            " ···".to_string(),
            if is_cursor { cursor } else { fg(FAINT) },
        ),
    };
    vec![Span::styled(fit(&text, COL_SYNTH_W), style)]
}

// ─── Side column ─────────────────────────────────────────────────────────────

/// Output scope above a per-channel activity mixer.
fn draw_side(frame: &mut Frame, area: Rect, state: &TrackerState, data: &[(f64, f64)], level: f32) {
    let mixer_h = u16::try_from(TOTAL_TRACKS).unwrap_or(u16::MAX) + 6;
    let [scope_area, mixer_area] =
        Layout::vertical([Constraint::Min(5), Constraint::Length(mixer_h)]).areas(area);

    widgets::scope(frame, scope_area, data, panel("SCOPE", false));

    let block = panel("CHANNELS", false);
    let inner = block.inner(mixer_area);
    frame.render_widget(block, mixer_area);

    let width = usize::from(inner.width);
    let bar_w = width.saturating_sub(8);
    let mut lines = vec![Line::raw("")];
    for track in 0..TOTAL_TRACKS {
        let color = track_color(track);
        let label = if track == DRUM_TRACK {
            "DRM".to_string()
        } else {
            format!("CH{}", track + 1)
        };
        let mut spans = vec![Span::styled(format!(" {label}  "), bold(color))];
        spans.extend(bar(state.activity[track], bar_w, |_| color));
        lines.push(Line::from(spans));
    }
    lines.push(Line::raw(""));

    let mut out = vec![Span::styled(" OUT  ", bold(FG))];
    out.extend(vu(level, bar_w));
    lines.push(Line::from(out));
    let mut readout = vec![Span::raw("      ")];
    readout.extend(db_readout(level));
    lines.push(Line::from(readout));

    frame.render_widget(Paragraph::new(lines), inner);
}

// ─── Status bar ──────────────────────────────────────────────────────────────

/// Key-cap hints plus the transient status message.
fn draw_footer(frame: &mut Frame, area: Rect, state: &TrackerState) {
    frame.render_widget(
        Paragraph::new(key_hints(&[
            ("Space", "play"),
            ("Esc", "panic·quit"),
            ("Del", "clear"),
            ("↑↓", "row"),
            ("←→", "track"),
            ("Tab", "patch"),
            ("+ -", "BPM"),
            ("F1", "help"),
            (":q", "quit"),
        ])),
        area,
    );
    if !state.status_msg.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("✦ ", fg(MAGENTA)),
                Span::styled(state.status_msg.clone(), bold(YELLOW)),
                Span::raw(" "),
            ]))
            .alignment(Alignment::Right),
            area,
        );
    }
}

// ─── Help overlay ────────────────────────────────────────────────────────────

/// Key reference popup drawn over the editor.
fn draw_help(frame: &mut Frame, area: Rect) {
    const KW: usize = 11;
    let lines = vec![
        Line::raw(""),
        help_heading("TRANSPORT"),
        help_entry("Space", KW, "play / stop"),
        help_entry("Esc", KW, "panic, then ask to quit"),
        help_entry(":q ⏎", KW, "quit right away  (also :wq, :x)"),
        help_entry("+ -", KW, "BPM up / down"),
        help_entry("PgUp PgDn", KW, "extend / shorten pattern"),
        Line::raw(""),
        help_heading("NAVIGATION"),
        help_entry("↑ ↓ ← →", KW, "move cursor"),
        help_entry("Tab", KW, "next preset on current track"),
        help_entry("Shift+Tab", KW, "previous preset"),
        help_entry("[ ]", KW, "octave down / up"),
        Line::raw(""),
        help_heading("NOTE ENTRY  (synth tracks)"),
        Line::styled(
            "    Z S X D C V G B H N J M   lower octave (C…B)",
            fg(GREEN),
        ),
        Line::styled(
            "    Q 2 W 3 E R 5 T 6 Y 7 U   upper octave (C…B)",
            fg(GREEN),
        ),
        help_entry("Del Bksp", KW, "clear step"),
        Line::raw(""),
        help_heading("DRUM ENTRY  (drum track)"),
        Line::from(vec![
            Span::raw("  "),
            widgets::keycap("k"),
            Span::styled(" kick  ", fg(ORANGE)),
            widgets::keycap("h"),
            Span::styled(" closed hat  ", fg(CYAN)),
            widgets::keycap("o"),
            Span::styled(" open hat", fg(YELLOW)),
        ]),
        Line::raw(""),
        Line::styled("press any key to close", fg(DIM)).centered(),
    ];
    let rect = widgets::popup(frame, area, 62, 27);
    frame.render_widget(
        Paragraph::new(lines).block(panel("KEY REFERENCE", true)),
        rect,
    );
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Compute the first visible step so that the cursor row stays centred.
fn scroll_offset(cursor: usize, len: usize, visible: usize) -> usize {
    if visible >= len {
        return 0;
    }
    let half = visible / 2;
    if cursor < half {
        0
    } else if cursor + visible - half > len {
        len.saturating_sub(visible)
    } else {
        cursor - half
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn draws_at_common_and_tiny_sizes() {
        let (tx, _rx) = crossbeam_channel::unbounded();
        let mut state = TrackerState::new(tx, Arc::new(AtomicBool::new(true)));
        state.activity = [1.0, 0.5, 0.0, 0.2, 0.9];
        let data: Vec<(f64, f64)> = (0..128)
            .map(|i| (f64::from(i), (f64::from(i) / 10.0).sin()))
            .collect();
        for (w, h) in [(160, 50), (120, 40), (80, 24), (40, 12), (10, 5), (1, 1)] {
            for track in 0..TOTAL_TRACKS {
                state.cursor_track = track;
                state.show_help = track == 0;
                let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                term.draw(|f| draw(f, &state, &data, 0.4)).unwrap();
            }
        }
        for modal in [
            synthie_ui::Modal::ConfirmQuit,
            synthie_ui::Modal::Command("wq".into()),
        ] {
            state.modal = modal;
            for (w, h) in [(120, 40), (20, 6), (1, 1)] {
                let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                term.draw(|f| draw(f, &state, &data, 0.5)).unwrap();
            }
        }
    }

    #[test]
    fn grid_lines_have_fixed_width() {
        let (tx, _rx) = crossbeam_channel::unbounded();
        let state = TrackerState::new(tx, Arc::new(AtomicBool::new(true)));
        assert_eq!(rule().width(), GRID_W);
        for line in column_headers(&state) {
            assert_eq!(line.width(), GRID_W);
        }
        for step in 0..state.song.pattern.len {
            assert_eq!(step_line(&state, step, false).width(), GRID_W);
        }
    }
}
