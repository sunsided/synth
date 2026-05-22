//! Terminal UI rendering for the synthie tracker.
//!
//! Layout:
//! ```text
//! ┌─────────────────── SYNTHIE TRACKER ──────────────────────┐
//! │  ♩ 120 BPM  │  16 steps  │  ● PLAYING  │  Oct: 4         │
//! ├────┬──────────┬──────────┬──────────┬──────────┬──────────┤
//! │ROW │  CH 1    │  CH 2    │  CH 3    │  CH 4    │  DRUMS   │
//! │    │ C64 Bass │ Arp Lead │ Plucky P │ PWM Lead │          │
//! ├────┼──────────┼──────────┼──────────┼──────────┼──────────┤
//! │ 00 │ C-4      │ ---      │ ---      │ ---      │ K..      │
//! │[01]│[G-4     ]│[---     ]│[---     ]│[---     ]│[...     ]│
//! │ 02 │ E-4      │ ---      │ ---      │ ---      │ K h      │
//! └────┴──────────┴──────────┴──────────┴──────────┴──────────┘
//!  Space:Play/Stop  Esc:Panic  Del:Clear  ↑↓:Row  ←→:Track
//!  Z-M/Q-U:Note  k/h/o:Drum  Tab:Patch  +/-:BPM  [/]:Oct  F1:Help
//! ```

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
};

use crate::app::state::TrackerState;
use crate::tracker::{DRUM_TRACK, SYNTH_TRACKS, note_name};

// ─── Palette ─────────────────────────────────────────────────────────────────

/// Primary foreground colour.
const FG: Color = Color::Cyan;
/// Dimmed foreground (inactive labels, borders).
const FG_DIM: Color = Color::DarkGray;
/// Highlight colour (header values, cursor row marker).
const FG_HIGHLIGHT: Color = Color::Yellow;
/// Note text colour.
const FG_NOTE: Color = Color::Green;
/// Drum hit colour.
const FG_DRUM: Color = Color::Magenta;
/// Error indicator colour.
const FG_ERROR: Color = Color::Red;
/// Background.
const BG: Color = Color::Black;
/// Active border colour.
const BORDER_ACTIVE: Color = Color::Cyan;
/// Inactive border colour.
const BORDER_INACTIVE: Color = Color::DarkGray;

// ─── Column widths ───────────────────────────────────────────────────────────

/// Width of the row-number column (characters).
const COL_ROW_W: u16 = 4;
/// Width of each synth track column.
const COL_SYNTH_W: u16 = 10;
/// Width of the drum track column.
const COL_DRUM_W: u16 = 8;

// ─── Entry point ─────────────────────────────────────────────────────────────

/// Top-level draw call: dispatches to the help overlay or the main editor.
pub fn draw(frame: &mut Frame, state: &TrackerState) {
    if state.show_help {
        draw_help(frame);
        return;
    }
    draw_main(frame, state);
}

// ─── Main layout ─────────────────────────────────────────────────────────────

fn draw_main(frame: &mut Frame, state: &TrackerState) {
    let area = frame.area();

    // Split into: header bar | grid | help bar
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(0),    // pattern grid
            Constraint::Length(3), // help bar
        ])
        .split(area);

    draw_header(frame, rows[0], state);
    draw_grid(frame, rows[1], state);
    draw_status(frame, rows[2]);
}

// ─── Header ──────────────────────────────────────────────────────────────────

fn draw_header(frame: &mut Frame, area: Rect, state: &TrackerState) {
    let (play_icon, play_color) = if !state.audio_ok() {
        ("✗ AUDIO ERR", FG_ERROR)
    } else if state.is_playing() {
        ("● PLAYING", FG_NOTE)
    } else {
        ("■ STOPPED", FG_DIM)
    };

    let text = Line::from(vec![
        Span::styled(" ♩ ", Style::default().fg(FG_DIM)),
        Span::styled(
            format!("{:.0} BPM", state.song.bpm),
            Style::default()
                .fg(FG_HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  │  ", Style::default().fg(FG_DIM)),
        Span::styled(
            format!("{} steps", state.song.pattern.len),
            Style::default().fg(FG),
        ),
        Span::styled("  │  ", Style::default().fg(FG_DIM)),
        Span::styled(
            play_icon,
            Style::default().fg(play_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  │  Oct: ", Style::default().fg(FG_DIM)),
        Span::styled(
            format!("{}", state.octave),
            Style::default().fg(FG_HIGHLIGHT),
        ),
    ]);

    let block = Block::default()
        .title(Span::styled(
            " SYNTHIE TRACKER ",
            Style::default()
                .fg(FG_HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_ACTIVE))
        .style(Style::default().bg(BG));

    let paragraph = Paragraph::new(text).block(block).alignment(Alignment::Left);

    frame.render_widget(paragraph, area);
}

// ─── Pattern grid ────────────────────────────────────────────────────────────

fn draw_grid(frame: &mut Frame, area: Rect, state: &TrackerState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_INACTIVE))
        .style(Style::default().bg(BG));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 4 {
        return;
    }

    // Row layout: row-number column + synth columns + drum column
    let mut constraints = vec![Constraint::Length(COL_ROW_W)];
    for _ in 0..SYNTH_TRACKS {
        constraints.push(Constraint::Length(COL_SYNTH_W));
    }
    constraints.push(Constraint::Length(COL_DRUM_W));

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(inner);

    // Vertical layout inside each column: header line + separator + content rows
    let visible_rows = inner.height.saturating_sub(2) as usize; // subtract header + sep

    // ── Column headers ────────────────────────────────────────────────────────
    draw_column_headers(frame, &cols, state);

    // ── Step rows ─────────────────────────────────────────────────────────────
    draw_step_rows(frame, &cols, state, visible_rows);
}

fn draw_column_headers(frame: &mut Frame, cols: &[Rect], state: &TrackerState) {
    // Row number header cell
    if let Some(&cell) = cols.first() {
        let header = Paragraph::new(Line::from(vec![Span::styled(
            " ROW",
            Style::default().fg(FG_DIM),
        )]));
        let col_area = Rect {
            y: cell.y,
            height: 2,
            ..cell
        };
        frame.render_widget(header, col_area);
    }

    // Synth track headers
    for track in 0..SYNTH_TRACKS {
        if let Some(&cell) = cols.get(track + 1) {
            let is_active = state.cursor_track == track;
            let patch_idx = state.song.track_patches[track];
            let patch_name = state.presets.get(patch_idx).map_or("", |p| p.name.as_str());

            let header_style = if is_active {
                Style::default()
                    .fg(FG_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(FG_DIM)
            };

            let lines = vec![
                Line::from(Span::styled(format!(" CH {}", track + 1), header_style)),
                Line::from(Span::styled(
                    format!(" {}", truncate(patch_name, (COL_SYNTH_W - 1) as usize)),
                    Style::default().fg(if is_active { FG } else { FG_DIM }),
                )),
            ];
            let col_area = Rect {
                y: cell.y,
                height: 2,
                ..cell
            };
            frame.render_widget(Paragraph::new(lines), col_area);
        }
    }

    // Drum track header
    if let Some(&cell) = cols.get(DRUM_TRACK + 1) {
        let is_active = state.cursor_track == DRUM_TRACK;
        let header_style = if is_active {
            Style::default()
                .fg(FG_HIGHLIGHT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(FG_DIM)
        };
        let lines = vec![
            Line::from(Span::styled(" DRUMS", header_style)),
            Line::from(Span::styled(
                " K H O",
                Style::default().fg(if is_active { FG_DRUM } else { FG_DIM }),
            )),
        ];
        let col_area = Rect {
            y: cell.y,
            height: 2,
            ..cell
        };
        frame.render_widget(Paragraph::new(lines), col_area);
    }
}

#[allow(clippy::too_many_lines)]
fn draw_step_rows(frame: &mut Frame, cols: &[Rect], state: &TrackerState, visible_rows: usize) {
    let pattern_len = state.song.pattern.len;
    let cursor_row = state.cursor_row;
    let playing_step = if state.is_playing() {
        Some(state.playing_step())
    } else {
        None
    };

    // Scroll window: keep cursor centred.
    let scroll_start = scroll_offset(cursor_row, pattern_len, visible_rows);

    for display_row in 0..visible_rows {
        let step = scroll_start + display_row;
        if step >= pattern_len {
            break;
        }

        let is_cursor = step == cursor_row;
        let is_playing = playing_step == Some(step);

        let row_y_offset = 2 + u16::try_from(display_row).unwrap_or(u16::MAX); // 2 = header height

        // ── Row number ────────────────────────────────────────────────────────
        if let Some(&cell) = cols.first() {
            let marker = if is_playing { "►" } else { " " };
            let row_style = if is_playing {
                Style::default()
                    .fg(FG_HIGHLIGHT)
                    .add_modifier(Modifier::BOLD)
            } else if is_cursor {
                Style::default().fg(FG_HIGHLIGHT)
            } else {
                Style::default().fg(FG_DIM)
            };
            let text = format!("{marker}{step:02}");
            let row_cell = Rect {
                y: cell.y + row_y_offset,
                height: 1,
                ..cell
            };
            frame.render_widget(Paragraph::new(Span::styled(text, row_style)), row_cell);
        }

        // ── Synth track cells ─────────────────────────────────────────────────
        for track in 0..SYNTH_TRACKS {
            if let Some(&cell) = cols.get(track + 1) {
                let step_data = &state.song.pattern.synth[track][step];
                let is_cursor_cell = is_cursor && state.cursor_track == track;

                let (text, text_style) = match step_data.note {
                    Some(note) => (
                        format!(" {}", note_name(note)),
                        Style::default().fg(if is_cursor_cell {
                            FG_HIGHLIGHT
                        } else {
                            FG_NOTE
                        }),
                    ),
                    None => (" ---".to_string(), Style::default().fg(FG_DIM)),
                };

                let cell_style = if is_cursor_cell {
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD)
                } else if is_playing {
                    Style::default().bg(Color::Reset)
                } else {
                    Style::default()
                };

                let row_cell = Rect {
                    y: cell.y + row_y_offset,
                    height: 1,
                    ..cell
                };
                frame.render_widget(
                    Paragraph::new(Span::styled(text, text_style)).style(cell_style),
                    row_cell,
                );
            }
        }

        // ── Drum cell ─────────────────────────────────────────────────────────
        if let Some(&cell) = cols.get(DRUM_TRACK + 1) {
            let drum = &state.song.pattern.drums[step];
            let is_cursor_cell = is_cursor && state.cursor_track == DRUM_TRACK;

            let k = if drum.kick { 'K' } else { '.' };
            let h = if drum.hihat_closed { 'h' } else { '.' };
            let o = if drum.hihat_open { 'O' } else { '.' };
            let text = format!(" {k} {h} {o}");

            let text_style = if drum.kick || drum.hihat_closed || drum.hihat_open {
                Style::default().fg(if is_cursor_cell {
                    FG_HIGHLIGHT
                } else {
                    FG_DRUM
                })
            } else {
                Style::default().fg(FG_DIM)
            };

            let cell_style = if is_cursor_cell {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let row_cell = Rect {
                y: cell.y + row_y_offset,
                height: 1,
                ..cell
            };
            frame.render_widget(
                Paragraph::new(Span::styled(text, text_style)).style(cell_style),
                row_cell,
            );
        }
    }
}

// ─── Status bar ──────────────────────────────────────────────────────────────

fn draw_status(frame: &mut Frame, area: Rect) {
    let lines = vec![Line::from(vec![
        Span::styled(" Space", Style::default().fg(FG_HIGHLIGHT)),
        Span::styled(":Play/Stop  ", Style::default().fg(FG_DIM)),
        Span::styled("Esc", Style::default().fg(FG_HIGHLIGHT)),
        Span::styled(":Panic  ", Style::default().fg(FG_DIM)),
        Span::styled("Del", Style::default().fg(FG_HIGHLIGHT)),
        Span::styled(":Clear  ", Style::default().fg(FG_DIM)),
        Span::styled("↑↓", Style::default().fg(FG_HIGHLIGHT)),
        Span::styled(":Row  ", Style::default().fg(FG_DIM)),
        Span::styled("←→", Style::default().fg(FG_HIGHLIGHT)),
        Span::styled(":Track  ", Style::default().fg(FG_DIM)),
        Span::styled("Tab", Style::default().fg(FG_HIGHLIGHT)),
        Span::styled(":Patch  ", Style::default().fg(FG_DIM)),
        Span::styled("F1", Style::default().fg(FG_HIGHLIGHT)),
        Span::styled(":Help", Style::default().fg(FG_DIM)),
    ])];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_INACTIVE))
        .style(Style::default().bg(BG));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

// ─── Help overlay ────────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn draw_help(frame: &mut Frame) {
    let area = frame.area();

    // Centre a popup that is 60 wide × 22 tall.
    let w = 64_u16.min(area.width);
    let h = 24_u16.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let popup = Rect {
        x,
        y,
        width: w,
        height: h,
    };

    // Blank the popup area.
    frame.render_widget(Clear, popup);

    let text = vec![
        Line::from(Span::styled(
            "  SYNTHIE TRACKER – KEY REFERENCE",
            Style::default()
                .fg(FG_HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  TRANSPORT",
            Style::default().fg(FG).add_modifier(Modifier::UNDERLINED),
        )),
        Line::from(vec![
            Span::styled("  Space       ", Style::default().fg(FG_HIGHLIGHT)),
            Span::styled("Play / Stop", Style::default().fg(FG)),
        ]),
        Line::from(vec![
            Span::styled("  Esc         ", Style::default().fg(FG_HIGHLIGHT)),
            Span::styled("Panic (silence all voices)", Style::default().fg(FG)),
        ]),
        Line::from(vec![
            Span::styled("  + / -       ", Style::default().fg(FG_HIGHLIGHT)),
            Span::styled("BPM up / down", Style::default().fg(FG)),
        ]),
        Line::from(vec![
            Span::styled("  PgUp / PgDn ", Style::default().fg(FG_HIGHLIGHT)),
            Span::styled("Extend / shorten pattern", Style::default().fg(FG)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  NAVIGATION",
            Style::default().fg(FG).add_modifier(Modifier::UNDERLINED),
        )),
        Line::from(vec![
            Span::styled("  ↑ ↓ ← →     ", Style::default().fg(FG_HIGHLIGHT)),
            Span::styled("Move cursor", Style::default().fg(FG)),
        ]),
        Line::from(vec![
            Span::styled("  Tab         ", Style::default().fg(FG_HIGHLIGHT)),
            Span::styled("Next preset on current track", Style::default().fg(FG)),
        ]),
        Line::from(vec![
            Span::styled("  Shift+Tab   ", Style::default().fg(FG_HIGHLIGHT)),
            Span::styled("Previous preset", Style::default().fg(FG)),
        ]),
        Line::from(vec![
            Span::styled("  [ / ]       ", Style::default().fg(FG_HIGHLIGHT)),
            Span::styled("Octave down / up", Style::default().fg(FG)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  NOTE ENTRY  (synth tracks)",
            Style::default().fg(FG).add_modifier(Modifier::UNDERLINED),
        )),
        Line::from(Span::styled(
            "  Z S X D C V G B H N J M   lower octave  (C…B)",
            Style::default().fg(FG_NOTE),
        )),
        Line::from(Span::styled(
            "  Q 2 W 3 E R 5 T 6 Y 7 U   upper octave  (C…B)",
            Style::default().fg(FG_NOTE),
        )),
        Line::from(vec![
            Span::styled("  Del / Bksp  ", Style::default().fg(FG_HIGHLIGHT)),
            Span::styled("Clear step", Style::default().fg(FG)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  DRUM ENTRY  (drum track)",
            Style::default().fg(FG).add_modifier(Modifier::UNDERLINED),
        )),
        Line::from(vec![
            Span::styled("  k / h / o   ", Style::default().fg(FG_HIGHLIGHT)),
            Span::styled(
                "Toggle Kick / closed Hi-hat / Open hi-hat",
                Style::default().fg(FG_DRUM),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(FG_DIM),
        )),
    ];

    let block = Block::default()
        .title(Span::styled(
            " Help ",
            Style::default()
                .fg(FG_HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        ))
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER_ACTIVE))
        .padding(Padding::horizontal(1))
        .style(Style::default().bg(BG));

    frame.render_widget(
        Paragraph::new(text).block(block).wrap(Wrap { trim: false }),
        popup,
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

/// Truncate a string to `max_chars` characters (no ellipsis).
fn truncate(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}
