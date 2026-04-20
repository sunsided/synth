use crate::app::state::{AppState, Section};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Chart, Dataset, GraphType, List, ListItem, ListState, Paragraph, Wrap,
    },
    Frame,
};

// ---------------------------------------------------------------------------
// Colour palette (SID-era terminal feel)
// ---------------------------------------------------------------------------
const FG: Color = Color::Cyan;
const FG_DIM: Color = Color::DarkGray;
const FG_HIGHLIGHT: Color = Color::Yellow;
const FG_VALUE: Color = Color::Green;
const BG: Color = Color::Black;
const BORDER_ACTIVE: Color = Color::Cyan;
const BORDER_INACTIVE: Color = Color::DarkGray;

// ---------------------------------------------------------------------------
// Top-level draw entry point
// ---------------------------------------------------------------------------

pub fn draw(frame: &mut Frame, state: &AppState, scope_data: &[(f64, f64)]) {
    let area = frame.area();

    if state.show_help {
        draw_help(frame, area);
        return;
    }

    // ── Outer layout: top body + bottom bar ────────────────────────────────
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let body = outer[0];
    let status_bar = outer[1];

    // ── Body: left column (controls) + right column (scope + extra params) ─
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(26), Constraint::Min(0)])
        .split(body);

    let left = columns[0];
    let right = columns[1];

    // ── Left column: title + param sections ────────────────────────────────
    let left_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // title
            Constraint::Length(6), // OSC
            Constraint::Length(8), // ENV
            Constraint::Length(6), // FILTER
            Constraint::Length(5), // LFO
            Constraint::Length(5), // FX
            Constraint::Min(0),    // spacer
        ])
        .split(left);

    draw_title(frame, left_sections[0], state);
    draw_section(frame, left_sections[1], state, Section::Osc);
    draw_section(frame, left_sections[2], state, Section::Env);
    draw_section(frame, left_sections[3], state, Section::Filter);
    draw_section(frame, left_sections[4], state, Section::Lfo);
    draw_section(frame, left_sections[5], state, Section::Fx);

    // ── Right column: scope (top) + presets (bottom) ───────────────────────
    let right_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Ratio(2, 3), Constraint::Ratio(1, 3)])
        .split(right);

    draw_scope(frame, right_sections[0], scope_data);
    draw_presets(frame, right_sections[1], state);

    // ── Status bar ─────────────────────────────────────────────────────────
    draw_status(frame, status_bar, state);
}

// ---------------------------------------------------------------------------
// Title widget
// ---------------------------------------------------------------------------

fn draw_title(frame: &mut Frame, area: Rect, state: &AppState) {
    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            " SID SYNTH ",
            Style::default()
                .fg(FG_HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", state.current_patch_name),
            Style::default().fg(FG_DIM),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(BORDER_INACTIVE)),
    );
    frame.render_widget(title, area);
}

// ---------------------------------------------------------------------------
// Generic parameter section
// ---------------------------------------------------------------------------

fn draw_section(frame: &mut Frame, area: Rect, state: &AppState, section: Section) {
    let is_active = state.selected_section == section;
    let border_style = Style::default().fg(if is_active {
        BORDER_ACTIVE
    } else {
        BORDER_INACTIVE
    });
    let title_style = if is_active {
        Style::default()
            .fg(FG_HIGHLIGHT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(FG_DIM)
    };

    let block = Block::default()
        .title(Span::styled(format!(" {} ", section.name()), title_style))
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let params = state.section_params();
    if section == Section::Presets || section != state.selected_section {
        // Non-active section: compact single-line overview
        if params.is_empty() {
            return;
        }
        // Show first few params on one line
        let max = params.len().min(4);
        let items: Vec<Span> = params[..max]
            .iter()
            .enumerate()
            .flat_map(|(i, (label, val))| {
                let sep = if i > 0 {
                    vec![Span::styled("  ", Style::default())]
                } else {
                    vec![]
                };
                let mut spans = sep;
                if !label.is_empty() {
                    spans.push(Span::styled(
                        format!("{label}:"),
                        Style::default().fg(FG_DIM),
                    ));
                }
                spans.push(Span::styled(val.clone(), Style::default().fg(FG_VALUE)));
                spans
            })
            .collect();
        let para = Paragraph::new(Line::from(items));
        frame.render_widget(para, inner);
        return;
    }

    // Active section: show all params, highlight selected
    let items: Vec<ListItem> = params
        .iter()
        .enumerate()
        .map(|(i, (label, val))| {
            let selected = i == state.selected_param;
            let label_span = Span::styled(
                format!("{:<7}", if label.is_empty() { " " } else { label }),
                Style::default().fg(if selected { FG_HIGHLIGHT } else { FG_DIM }),
            );
            let arrow = if selected {
                Span::styled("▶ ", Style::default().fg(FG_HIGHLIGHT))
            } else {
                Span::styled("  ", Style::default())
            };
            let val_span = Span::styled(
                format!("{val}"),
                Style::default()
                    .fg(if selected { FG_HIGHLIGHT } else { FG_VALUE })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            );
            ListItem::new(Line::from(vec![arrow, label_span, val_span]))
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, inner);
}

// ---------------------------------------------------------------------------
// Waveform scope
// ---------------------------------------------------------------------------

fn draw_scope(frame: &mut Frame, area: Rect, data: &[(f64, f64)]) {
    let block = Block::default()
        .title(Span::styled(" WAVEFORM ", Style::default().fg(FG_DIM)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_INACTIVE));

    if data.is_empty() {
        frame.render_widget(
            Paragraph::new("no signal")
                .style(Style::default().fg(FG_DIM))
                .alignment(Alignment::Center)
                .block(block),
            area,
        );
        return;
    }

    let x_max = data.len() as f64;

    let datasets = vec![Dataset::default()
        .name("")
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(FG))
        .data(data)];

    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(
            Axis::default()
                .style(Style::default().fg(FG_DIM))
                .bounds([0.0, x_max]),
        )
        .y_axis(
            Axis::default()
                .style(Style::default().fg(FG_DIM))
                .bounds([-1.0, 1.0])
                .labels([Span::raw("-1"), Span::raw(" 0"), Span::raw("+1")]),
        );

    frame.render_widget(chart, area);
}

// ---------------------------------------------------------------------------
// Presets panel
// ---------------------------------------------------------------------------

fn draw_presets(frame: &mut Frame, area: Rect, state: &AppState) {
    let is_active = state.selected_section == Section::Presets;
    let border_style = Style::default().fg(if is_active {
        BORDER_ACTIVE
    } else {
        BORDER_INACTIVE
    });
    let title_style = if is_active {
        Style::default()
            .fg(FG_HIGHLIGHT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(FG_DIM)
    };

    let block = Block::default()
        .title(Span::styled(" PRESETS ", title_style))
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items: Vec<ListItem> = state
        .patches
        .iter()
        .enumerate()
        .map(|(i, patch)| {
            let selected = i == state.selected_preset;
            let style = if selected && is_active {
                Style::default()
                    .fg(FG_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else if selected {
                Style::default().fg(FG_VALUE).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(FG_DIM)
            };
            ListItem::new(Line::from(Span::styled(
                format!(" {:>2}. {}", i + 1, patch.name),
                style,
            )))
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(state.selected_preset));

    frame.render_stateful_widget(List::new(items), inner, &mut list_state);
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

fn draw_status(frame: &mut Frame, area: Rect, state: &AppState) {
    let note_str = match state.active_note {
        Some(midi) => {
            let names = [
                "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
            ];
            let name = names[(midi % 12) as usize];
            let oct = (midi / 12) as i32 - 1;
            format!("{name}{oct}({midi})")
        }
        None => "---".to_string(),
    };

    let msg = if !state.status_msg.is_empty() {
        format!("  {}  ", state.status_msg)
    } else {
        String::new()
    };

    let bar = Paragraph::new(Line::from(vec![
        Span::styled(" Note:", Style::default().fg(FG_DIM)),
        Span::styled(format!("{note_str} "), Style::default().fg(FG_VALUE)),
        Span::styled("|", Style::default().fg(FG_DIM)),
        Span::styled(
            format!(" Oct:{} ", state.octave),
            Style::default().fg(FG_VALUE),
        ),
        Span::styled("|", Style::default().fg(FG_DIM)),
        Span::styled(
            format!(" Vol:{:.0}% ", state.params.volume * 100.0),
            Style::default().fg(FG_VALUE),
        ),
        Span::styled("|", Style::default().fg(FG_DIM)),
        Span::styled(msg, Style::default().fg(FG_HIGHLIGHT)),
        Span::styled(
            " Tab:section  ←→:param  ↑↓:value  []:octave  F1:help  Esc:panic  ^C:quit",
            Style::default().fg(FG_DIM),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(BORDER_INACTIVE)),
    );

    frame.render_widget(bar, area);
}

// ---------------------------------------------------------------------------
// Help overlay
// ---------------------------------------------------------------------------

fn draw_help(frame: &mut Frame, area: Rect) {
    let help_text = vec![
        Line::from(Span::styled(
            "  SID SYNTH – KEYBOARD REFERENCE",
            Style::default()
                .fg(FG_HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  PIANO KEYS",
            Style::default().fg(FG).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  Z S X D C V G B H N J M  →  C to B (bottom octave)",
            Style::default().fg(FG_VALUE),
        )),
        Line::from(Span::styled(
            "  Q 2 W 3 E R 5 T 6 Y 7 U  →  C to B (top octave)",
            Style::default().fg(FG_VALUE),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  NAVIGATION",
            Style::default().fg(FG).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  Tab / Shift+Tab           →  Next / prev section",
            Style::default().fg(FG_VALUE),
        )),
        Line::from(Span::styled(
            "  ← →  Left/Right Arrow     →  Prev / next parameter",
            Style::default().fg(FG_VALUE),
        )),
        Line::from(Span::styled(
            "  ↑ ↓  Up/Down Arrow        →  Increase / decrease value",
            Style::default().fg(FG_VALUE),
        )),
        Line::from(Span::styled(
            "  Enter  (in Presets)       →  Load highlighted preset",
            Style::default().fg(FG_VALUE),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  CONTROLS",
            Style::default().fg(FG).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  [ or ,                    →  Octave down",
            Style::default().fg(FG_VALUE),
        )),
        Line::from(Span::styled(
            "  ] or .                    →  Octave up",
            Style::default().fg(FG_VALUE),
        )),
        Line::from(Span::styled(
            "  + / -                     →  Volume up / down",
            Style::default().fg(FG_VALUE),
        )),
        Line::from(Span::styled(
            "  Esc                       →  All notes off (panic)",
            Style::default().fg(FG_VALUE),
        )),
        Line::from(Span::styled(
            "  Ctrl+S                    →  Quick-save current patch",
            Style::default().fg(FG_VALUE),
        )),
        Line::from(Span::styled(
            "  F1                        →  Toggle this help screen",
            Style::default().fg(FG_VALUE),
        )),
        Line::from(Span::styled(
            "  Ctrl+C / Ctrl+Q / F12     →  Quit",
            Style::default().fg(FG_VALUE),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  Press F1 to close",
            Style::default().fg(FG_DIM),
        )),
    ];

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .title(Span::styled(
                    " Help ",
                    Style::default()
                        .fg(FG_HIGHLIGHT)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(BORDER_ACTIVE)),
        )
        .wrap(Wrap { trim: false });

    // Centre a reasonably-sized box
    let popup_area = centred_rect(60, 80, area);
    // Clear background
    frame.render_widget(Block::default().style(Style::default().bg(BG)), popup_area);
    frame.render_widget(help, popup_area);
}

/// Return a centred `Rect` that is `percent_x`% wide and `percent_y`% tall.
fn centred_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(layout[1])[1]
}
