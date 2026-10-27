//! Terminal UI rendering: layout, widgets, and the top-level draw entry point.
//!
//! Layout:
//! ```text
//! ╭──────────────────────────────────────────────────────────────────────╮
//! │ ▓▒░ S Y N T H I E ░▒▓  SID SYNTH ⟫ C64 Bass        ♪ C#4  OCT 4  VOL │
//! ╰──────────────────────────────────────────────────────────────────────╯
//! ╭ ◆ OSC ──────────────────────╮╭ ◇ WAVEFORM ──────────────── -6.0 dB ╮
//! │▶ Wave      PULSE            ││        ⡠⠊⠉⠢⡀      ⡠⠊⠉⠢⡀            │
//! │  PW         0.50 ▉▉▉▉▉▌···· ││ ⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒⠒   │
//! ╰─────────────────────────────╯╰─────────────────────────────────────╯
//! ╭ ◇ ENV ──────────────────────╮╭ ◇ ENVELOPES ────────╮╭ ◇ OUTPUT ───╮
//! │Atk 0.010s · Dec 0.200s · …  ││  ╱╲___              ││ ▉▉▉▉▉▋····· │
//! ╰─────────────────────────────╯╰─────────────────────╯╰─────────────╯
//!  …                             ╭ ◇ PRESETS ─────────────────────────╮
//!                                │▶  1 ● C64 Bass                     │
//!                                ╰────────────────────────────────────╯
//! ╭ ◇ KEYBOARD ─────────────────────────────────────────────── OCT 4–5 ╮
//! │              ▐█▌▐█▌ ▐█▌▐█▌▐█▌ ▐█▌▐█▌ ▐█▌▐█▌▐█▌                     │
//! ╰────────────────────────────────────────────────────────────────────╯
//!  Tab section  ←→ param  ↑↓ value  …
//! ```

use crate::app::CAN_QUIT;
use crate::app::state::{AppState, ParamRow, Section};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::Style,
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        HighlightSpacing, List, ListItem, ListState, Paragraph,
        canvas::{Canvas, Line as CanvasLine},
    },
};
use synthie::params::MidiNote;
use synthie_ui::theme::{
    self, BG, BG_CURSOR, CYAN, DIM, FAINT, FG, GREEN, MAGENTA, PURPLE, YELLOW, bold, fg,
};
use synthie_ui::widgets::{
    self, db_readout, help_entry, help_heading, logo, meter, panel, piano, vu,
};

/// Width of the parameter column.
const LEFT_W: u16 = 40;
/// Width of a parameter label.
const LABEL_W: usize = 6;
/// Width of a formatted parameter value.
const VALUE_W: usize = 9;

/// Parameter sections shown in the left column, top to bottom.
const PARAM_SECTIONS: [Section; 6] = [
    Section::Osc,
    Section::Env,
    Section::Filter,
    Section::Lfo,
    Section::Mod,
    Section::Fx,
];

/// Computer keys of the two piano rows, in semitone order from C.
const PIANO_KEYS: [char; 24] = [
    'z', 's', 'x', 'd', 'c', 'v', 'g', 'b', 'h', 'n', 'j', 'm', //
    'q', '2', 'w', '3', 'e', 'r', '5', 't', '6', 'y', '7', 'u',
];

/// Top-level draw entry point.  Called once per UI frame.
///
/// Renders the full synth UI and, when `state.show_help` is set, the key
/// reference on top of it.  `level` is the smoothed output peak (0‥1).
#[cfg(not(target_arch = "wasm32"))] // the browser build centres via `draw_in`
pub fn draw(frame: &mut Frame, state: &AppState, scope_data: &[(f64, f64)], level: f32) {
    draw_in(frame, frame.area(), state, scope_data, level);
}

/// Like [`draw`], but lays the UI out inside `area` (the rest of the frame
/// keeps the plain background).
pub fn draw_in(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    scope_data: &[(f64, f64)],
    level: f32,
) {
    widgets::paint_background(frame);

    let [header, body, keys, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(4),
        Constraint::Length(1),
    ])
    .areas(area);
    let [left, right] =
        Layout::horizontal([Constraint::Length(LEFT_W), Constraint::Min(0)]).areas(body);

    draw_header(frame, header, state);
    draw_sections(frame, left, state);
    draw_right(frame, right, state, scope_data, level);
    draw_keyboard(frame, keys, state);
    draw_footer(frame, footer, state);

    if state.show_help {
        draw_help(frame, area);
    }
    widgets::modal(frame, area, footer, &state.modal);
}

/// Name of a MIDI note in scientific pitch notation, e.g. `C#4`.
fn note_label(midi: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = i32::from(midi / 12) - 1;
    format!("{}{octave}", NAMES[usize::from(midi % 12)])
}

/// Title bar: animated logo, engine name and patch on the left; live note,
/// octave and volume on the right.
fn draw_header(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = panel("", false);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut left = logo();
    left.spans.extend([
        Span::styled("  SID SYNTH ", bold(FG)),
        Span::styled("⟫ ", fg(MAGENTA)),
        Span::styled(state.current_patch_name.clone(), bold(YELLOW)),
    ]);
    frame.render_widget(Paragraph::new(left), inner);

    let note = match state.active_note() {
        Some(midi) => Span::styled(format!("{:<4}", note_label(midi.as_u8())), bold(GREEN)),
        None => Span::styled("--- ", fg(FAINT)),
    };
    let mut right = vec![
        Span::styled("♪ ", fg(DIM)),
        note,
        Span::styled("  OCT ", fg(DIM)),
        Span::styled(state.octave.to_string(), bold(CYAN)),
        Span::styled("  VOL ", fg(DIM)),
    ];
    right.extend(meter(state.params.global.volume, 10));
    right.push(Span::styled(
        format!(" {:>3.0}% ", state.params.global.volume * 100.0),
        fg(FG),
    ));
    frame.render_widget(
        Paragraph::new(Line::from(right)).alignment(Alignment::Right),
        inner,
    );
}

/// Accordion of parameter sections: the focused one expands, the rest
/// collapse to a one-line summary.
fn draw_sections(frame: &mut Frame, area: Rect, state: &AppState) {
    let constraints = PARAM_SECTIONS.map(|s| {
        if s == state.selected_section {
            Constraint::Fill(1)
        } else {
            Constraint::Length(3)
        }
    });
    let areas = Layout::vertical(constraints).flex(Flex::Start).split(area);
    for (&section, &rect) in PARAM_SECTIONS.iter().zip(areas.iter()) {
        draw_section(frame, rect, state, section);
    }
}

/// Render a single parameter section panel.
fn draw_section(frame: &mut Frame, area: Rect, state: &AppState, section: Section) {
    let focused = state.selected_section == section;
    let block = panel(section.name(), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = state.params_for(section);
    if !focused {
        frame.render_widget(Paragraph::new(summary_line(&rows)), inner);
        return;
    }

    let meter_w = usize::from(inner.width).saturating_sub(2 + LABEL_W + 1 + VALUE_W + 2);
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| ListItem::new(param_line(row, i == state.selected_param, meter_w)))
        .collect();
    let mut list_state = ListState::default().with_selected(Some(state.selected_param));
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::default().bg(BG_CURSOR)),
        inner,
        &mut list_state,
    );
}

/// Compact `Label value · Label value …` overview of a collapsed section.
fn summary_line(rows: &[ParamRow]) -> Line<'static> {
    let mut spans = Vec::new();
    for row in rows.iter().filter(|r| !r.is_separator()) {
        if !spans.is_empty() {
            spans.push(Span::styled(" · ", fg(FAINT)));
        }
        spans.push(Span::styled(format!("{} ", row.label), fg(DIM)));
        spans.push(Span::styled(row.value.clone(), fg(CYAN)));
    }
    Line::from(spans)
}

/// One row of an expanded section: cursor, label, value, gauge.
fn param_line(row: &ParamRow, selected: bool, meter_w: usize) -> Line<'static> {
    if row.is_separator() {
        return Line::from(vec![
            Span::styled("  ── ", fg(FAINT)),
            Span::styled(row.value.clone(), bold(PURPLE)),
            Span::styled(" ──", fg(FAINT)),
        ]);
    }

    let cursor = if selected {
        Span::styled("▶ ", bold(MAGENTA))
    } else {
        Span::raw("  ")
    };
    let (label_style, value_style) = if selected {
        (bold(YELLOW), bold(FG))
    } else {
        (fg(DIM), fg(CYAN))
    };
    let mut spans = vec![
        cursor,
        Span::styled(format!("{:<LABEL_W$}", row.label), label_style),
        Span::styled(format!("{:>VALUE_W$}", row.value), value_style),
        Span::raw("  "),
    ];
    match row.level {
        Some(level) => spans.extend(meter(level, meter_w)),
        None if row.value == "ON" => spans.push(Span::styled("●", bold(GREEN))),
        None if row.value == "off" => spans.push(Span::styled("○", fg(FAINT))),
        None => {}
    }
    Line::from(spans)
}

/// Right column: scope on top, envelope + output meters, then presets.
fn draw_right(frame: &mut Frame, area: Rect, state: &AppState, data: &[(f64, f64)], level: f32) {
    let [scope_area, mid, presets] = Layout::vertical([
        Constraint::Percentage(50),
        Constraint::Length(9),
        Constraint::Min(5),
    ])
    .areas(area);
    let [env_area, out_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(30)]).areas(mid);

    let mut peak = vec![Span::styled(" PEAK ", fg(DIM))];
    peak.extend(db_readout(level));
    peak.push(Span::raw(" "));
    let block = panel("WAVEFORM", false).title_top(Line::from(peak).right_aligned());
    widgets::scope(frame, scope_area, data, block);

    draw_envelopes(frame, env_area, state);
    draw_output(frame, out_area, state, level);
    draw_presets(frame, presets, state);
}

/// Normalised ADSR polyline: attack, decay, a sustain plateau, release.
fn adsr(attack: f32, decay: f32, sustain: f32, release: f32) -> [(f64, f64); 5] {
    let (a, d, s, r) = (
        f64::from(attack),
        f64::from(decay),
        f64::from(sustain),
        f64::from(release),
    );
    let hold = ((a + d + r) * 0.3).max(0.05);
    let total = a + d + hold + r;
    [
        (0.0, 0.0),
        (a / total, 1.0),
        ((a + d) / total, s),
        ((a + d + hold) / total, s),
        (1.0, 0.0),
    ]
}

/// Amp envelope (cyan) with the filter envelope (magenta, scaled by its
/// depth) layered on top.
fn draw_envelopes(frame: &mut Frame, area: Rect, state: &AppState) {
    let p = &state.params;
    let legend = Line::from(vec![
        Span::styled("━ ", fg(CYAN)),
        Span::styled("AMP ", fg(DIM)),
        Span::styled("━ ", fg(MAGENTA)),
        Span::styled("FILTER ", fg(DIM)),
    ])
    .right_aligned();
    let block = panel("ENVELOPES", false).title_top(legend);

    let amp = adsr(p.env.attack, p.env.decay, p.env.sustain, p.env.release);
    let fe = &p.filter_env;
    let depth = f64::from(fe.depth.abs());
    let filter = adsr(fe.attack, fe.decay, fe.sustain, fe.release).map(|(x, y)| (x, y * depth));

    let canvas = Canvas::default()
        .block(block)
        .background_color(BG)
        .marker(Marker::Braille)
        .x_bounds([0.0, 1.0])
        .y_bounds([0.0, 1.05])
        .paint(|ctx| {
            for (shape, color) in [(&amp, CYAN), (&filter, MAGENTA)] {
                if color == MAGENTA && depth < f64::EPSILON {
                    continue;
                }
                for seg in shape.windows(2) {
                    ctx.draw(&CanvasLine::new(
                        seg[0].0, seg[0].1, seg[1].0, seg[1].1, color,
                    ));
                }
                ctx.layer();
            }
        });
    frame.render_widget(canvas, area);
}

/// Output meter, peak readout, and the current note in large type.
fn draw_output(frame: &mut Frame, area: Rect, state: &AppState, level: f32) {
    let block = panel("OUTPUT", false);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let width = usize::from(inner.width).saturating_sub(2);
    let mut meter_line = vec![Span::raw(" ")];
    meter_line.extend(vu(level, width));

    let mut readout = vec![Span::raw(" ")];
    readout.extend(db_readout(level));

    let note = match state.active_note() {
        Some(midi) => Span::styled(note_label(midi.as_u8()), bold(theme::gradient(0.6))),
        None => Span::styled("---", fg(FAINT)),
    };
    let glow = if state.active_note().is_some() {
        Span::styled(" ●", fg(GREEN))
    } else {
        Span::styled(" ○", fg(FAINT))
    };

    let lines = vec![
        Line::raw(""),
        Line::from(meter_line),
        Line::from(readout),
        Line::raw(""),
        Line::from(vec![Span::styled(" NOTE ", fg(DIM)), note, glow]),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Preset browser.
fn draw_presets(frame: &mut Frame, area: Rect, state: &AppState) {
    let focused = state.selected_section == Section::Presets;
    let hint = Line::from(Span::styled(
        format!(
            " {} patches · ↑↓ pick · ⏎ load · P back ",
            state.patches.len()
        ),
        fg(DIM),
    ))
    .right_aligned();
    let block = panel("PRESETS", focused).title_top(hint);

    let items: Vec<ListItem> = state
        .patches
        .iter()
        .enumerate()
        .map(|(i, patch)| {
            let loaded = patch.name == state.current_patch_name;
            let selected = i == state.selected_preset;
            let name_style = match (selected, focused) {
                (true, true) => bold(YELLOW),
                (true, false) | (false, _) if loaded => bold(GREEN),
                (true, false) => fg(FG),
                _ => fg(DIM),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:>2} ", i + 1), fg(FAINT)),
                if loaded {
                    Span::styled("● ", fg(GREEN))
                } else {
                    Span::raw("  ")
                },
                Span::styled(patch.name.clone(), name_style),
            ]))
        })
        .collect();

    let highlight = if focused {
        Style::default().bg(BG_CURSOR)
    } else {
        Style::default()
    };
    let list = List::new(items)
        .block(block)
        .highlight_style(highlight)
        .highlight_symbol(Span::styled("▶ ", bold(MAGENTA)))
        .highlight_spacing(HighlightSpacing::Always);
    let mut list_state = ListState::default().with_selected(Some(state.selected_preset));
    frame.render_stateful_widget(list, area, &mut list_state);
}

/// Two-octave piano strip matching the computer keyboard, lighting every
/// held note.
fn draw_keyboard(frame: &mut Frame, area: Rect, state: &AppState) {
    let low = u8::try_from((i32::from(state.octave) + 1) * 12).unwrap_or(0);
    let range = Line::from(Span::styled(
        format!(" OCT {}–{} ", state.octave, state.octave + 1),
        fg(DIM),
    ))
    .right_aligned();
    let block = panel("KEYBOARD", !state.held_notes.is_empty()).title_top(range);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = piano(
        low,
        25,
        |n| state.held_notes.contains(&MidiNote(n)),
        |n| PIANO_KEYS.get(usize::from(n - low)).copied(),
    );
    frame.render_widget(
        Paragraph::new(rows.to_vec()).alignment(Alignment::Center),
        inner,
    );
}

/// Key-cap hints plus the transient status message.
fn draw_footer(frame: &mut Frame, area: Rect, state: &AppState) {
    const HINTS: [(&str, &str); 9] = [
        ("Tab", "section"),
        ("P", "presets"),
        ("←→", "param"),
        ("↑↓", "value"),
        ("[ ]", "octave"),
        ("+ -", "volume"),
        ("^S", "save"),
        ("F1", "help"),
        (":q", "quit"), // keep last: dropped where the app cannot quit
    ];
    let hints = if CAN_QUIT {
        &HINTS[..]
    } else {
        &HINTS[..HINTS.len() - 1]
    };
    widgets::footer(frame, area, hints, &state.status_msg);
}

/// Key reference popup drawn over the main UI.
fn draw_help(frame: &mut Frame, area: Rect) {
    const KW: usize = 9;
    let mut lines = vec![
        Line::raw(""),
        help_heading("PIANO"),
        Line::styled(
            "    Z S X D C V G B H N J M   lower octave (C…B)",
            fg(GREEN),
        ),
        Line::styled(
            "    Q 2 W 3 E R 5 T 6 Y 7 U   upper octave (C…B)",
            fg(GREEN),
        ),
        Line::raw(""),
        help_heading("NAVIGATION"),
        help_entry("Tab", KW, "next section  (Shift+Tab: previous)"),
        help_entry("← →", KW, "previous / next parameter"),
        help_entry("↑ ↓", KW, "increase / decrease value"),
        help_entry("P", KW, "jump to presets / back"),
        help_entry("Enter", KW, "load highlighted preset"),
        help_entry("1–8", KW, "quick-load preset (in Presets)"),
        Line::raw(""),
        help_heading("PERFORMANCE"),
        help_entry("[ ]", KW, "octave down / up  (also , .)"),
        help_entry("+ -", KW, "volume up / down"),
        help_entry(
            "Esc",
            KW,
            if CAN_QUIT {
                "all notes off, then ask to quit"
            } else {
                "all notes off"
            },
        ),
        Line::raw(""),
        help_heading("SESSION"),
        help_entry("Ctrl+S", KW, "quick-save current patch"),
        help_entry("F1", KW, "toggle this screen"),
    ];
    if CAN_QUIT {
        lines.extend([
            help_entry(":q ⏎", KW, "quit right away  (also :wq, :x)"),
            help_entry("Ctrl+C", KW, "quit  (also Ctrl+Q, F12)"),
        ]);
    }
    lines.extend([
        Line::raw(""),
        Line::styled("  press F1 to close", fg(DIM)).centered(),
    ]);
    let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
    let rect = widgets::popup(frame, area, 60, height);
    frame.render_widget(
        Paragraph::new(lines).block(panel("KEYBOARD REFERENCE", true)),
        rect,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    /// Synthetic two-cycle sine for the scope.
    fn sine() -> Vec<(f64, f64)> {
        (0..256)
            .map(|i| {
                let x = f64::from(i);
                (x, (x / 256.0 * std::f64::consts::TAU * 2.0).sin() * 0.8)
            })
            .collect()
    }

    #[test]
    fn draws_at_common_and_tiny_sizes() {
        let (tx, _rx) = crossbeam_channel::unbounded();
        let mut state = AppState::new(tx);
        state.note_on(MidiNote(61));
        let data = sine();
        for (w, h) in [(160, 50), (120, 40), (80, 24), (40, 12), (10, 5), (1, 1)] {
            for help in [false, true] {
                state.show_help = help;
                for &section in Section::ALL {
                    state.selected_section = section;
                    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                    term.draw(|f| draw(f, &state, &data, 0.5)).unwrap();
                }
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
}
