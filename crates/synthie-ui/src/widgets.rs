//! Reusable building blocks: panels, logo, key hints, meters, piano strip,
//! oscilloscope, and popup placement.

use crate::theme::{
    self, BG, CYAN, DIM, FAINT, FG, MAGENTA, PURPLE, RED, bold, fg, gradient, gradient_cyclic,
};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, BorderType, Chart, Clear, Dataset, GraphType, Paragraph},
};

/// Left-aligned eighth blocks, index = filled eighths (0‥8).
const EIGHTHS: [char; 9] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// White key fill on the piano strip.
const KEY_WHITE: Color = Color::Rgb(206, 200, 232);
/// Black key fill on the piano strip.
const KEY_BLACK: Color = Color::Rgb(28, 22, 48);
/// Hairline between adjacent white keys.
const KEY_EDGE: Color = Color::Rgb(120, 108, 168);

/// Fill the whole frame with the theme background.
pub fn paint_background(frame: &mut Frame) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG).fg(FG)), area);
}

/// Rounded neon panel.  Focused panels glow magenta with a filled diamond;
/// unfocused ones recede into the background.  An empty `title` draws a
/// bare frame.
#[must_use]
pub fn panel(title: &str, focused: bool) -> Block<'_> {
    let (border, glyph, title_style) = if focused {
        (MAGENTA, " ◆ ", bold(theme::YELLOW))
    } else {
        (FAINT, " ◇ ", fg(DIM))
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(fg(border))
        .style(Style::default().bg(BG));
    if title.is_empty() {
        return block;
    }
    block.title(Line::from(vec![
        Span::styled(glyph, fg(border)),
        Span::styled(title, title_style),
        Span::raw(" "),
    ]))
}

/// Animated `▓▒░ S Y N T H I E ░▒▓` wordmark with a slowly scrolling gradient.
#[must_use]
#[allow(clippy::cast_precision_loss)] // logo is a couple dozen chars
pub fn logo() -> Line<'static> {
    const TEXT: &str = "▓▒░ S Y N T H I E ░▒▓";
    let n = TEXT.chars().count() as f32;
    let shift = theme::clock() * 0.15;
    Line::from(
        TEXT.chars()
            .enumerate()
            .map(|(i, c)| {
                Span::styled(
                    c.to_string(),
                    bold(gradient_cyclic(i as f32 / n * 0.5 + shift)),
                )
            })
            .collect::<Vec<_>>(),
    )
}

/// Footer line of `key → action` hints rendered as glowing key caps.
#[must_use]
pub fn key_hints<'a>(hints: &[(&'a str, &'a str)]) -> Line<'a> {
    let mut spans = Vec::with_capacity(hints.len() * 2 + 1);
    spans.push(Span::raw(" "));
    for &(key, action) in hints {
        spans.push(keycap(key));
        spans.push(Span::styled(format!(" {action}  "), fg(DIM)));
    }
    Line::from(spans)
}

/// A single key cap, e.g. ` Tab `.
#[must_use]
pub fn keycap(key: &str) -> Span<'_> {
    Span::styled(
        format!(" {key} "),
        Style::default()
            .fg(BG)
            .bg(PURPLE)
            .add_modifier(Modifier::BOLD),
    )
}

/// Horizontal bar `width` cells wide filled to `frac` ∈ [0, 1] with 1/8-cell
/// resolution.  Each cell is coloured by `color(position)`; unfilled cells
/// show a faint dotted track.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)] // widths are terminal columns; eighths are clamped to 0..=8
pub fn bar(frac: f32, width: usize, color: impl Fn(f32) -> Color) -> Vec<Span<'static>> {
    let filled = (frac.clamp(0.0, 1.0) * width as f32 * 8.0).round() as usize;
    let denom = width.saturating_sub(1).max(1) as f32;
    (0..width)
        .map(|i| {
            let eighths = filled.saturating_sub(i * 8).min(8);
            if eighths == 0 {
                Span::styled("·", fg(FAINT))
            } else {
                Span::styled(EIGHTHS[eighths].to_string(), fg(color(i as f32 / denom)))
            }
        })
        .collect()
}

/// Parameter gauge using the signature neon gradient.
#[must_use]
pub fn meter(frac: f32, width: usize) -> Vec<Span<'static>> {
    bar(frac, width, gradient)
}

/// Output level meter (green → yellow → red), `level` linear 0‥1.
///
/// The bar is scaled in dB over a 48 dB window so quiet signals still move it.
#[must_use]
pub fn vu(level: f32, width: usize) -> Vec<Span<'static>> {
    bar(level_to_frac(level), width, theme::heat)
}

/// Map a linear amplitude onto a 0‥1 meter position over a −48‥0 dBFS window.
#[must_use]
pub fn level_to_frac(level: f32) -> f32 {
    if level <= 0.0 {
        return 0.0;
    }
    ((20.0 * level.log10() + 48.0) / 48.0).clamp(0.0, 1.0)
}

/// Two-row piano keyboard strip starting at MIDI note `low` (should be a C)
/// spanning `keys` semitones.  Keys for which `lit` returns `true` glow;
/// `label` may stamp a character (e.g. the computer key that plays it) onto
/// each key.
///
/// Each semitone takes two columns, so the strip is `2 × keys` wide.
#[must_use]
pub fn piano(
    low: u8,
    keys: u8,
    lit: impl Fn(u8) -> bool,
    label: impl Fn(u8) -> Option<char>,
) -> [Line<'static>; 2] {
    let is_black = |n: u8| matches!(n % 12, 1 | 3 | 6 | 8 | 10);
    let white_fill = |n: u8| if lit(n) { MAGENTA } else { KEY_WHITE };
    let mut top = Vec::new();
    let mut bottom = Vec::new();
    for n in low..low.saturating_add(keys) {
        let tag = label(n).unwrap_or(' ');
        if is_black(n) {
            let (bg, ink) = if lit(n) { (CYAN, BG) } else { (KEY_BLACK, DIM) };
            top.push(Span::styled(
                format!("{tag} "),
                Style::default().fg(ink).bg(bg),
            ));
            // Lower half belongs to the white key on the left.
            bottom.push(Span::styled(
                "  ",
                Style::default().bg(white_fill(n.saturating_sub(1))),
            ));
        } else {
            let fill = white_fill(n);
            let edge = if n == low { ' ' } else { '▏' };
            let top_edge = if n != low && !is_black(n.saturating_sub(1)) {
                '▏'
            } else {
                ' '
            };
            top.push(Span::styled(
                format!("{top_edge} "),
                Style::default().fg(KEY_EDGE).bg(fill),
            ));
            bottom.push(Span::styled(
                edge.to_string(),
                Style::default().fg(KEY_EDGE).bg(fill),
            ));
            bottom.push(Span::styled(
                tag.to_string(),
                Style::default().fg(KEY_BLACK).bg(fill),
            ));
        }
    }
    [Line::from(top), Line::from(bottom)]
}

/// Vertical half-range of the scope for a signal peaking at `peak`: the
/// smallest of ±1, ±½, ±¼ … ±1/16 that still fits it, so quiet patches fill
/// the display instead of drawing a flat line.
fn scope_range(peak: f64) -> f64 {
    let mut range = 1.0;
    while range > 1.0 / 16.0 && peak <= range / 2.0 {
        range /= 2.0;
    }
    range
}

/// Auto-ranging Braille oscilloscope with a faint zero line, drawn inside
/// `block`.  The zoom factor is shown in the bottom-right corner when > 1.
#[allow(clippy::cast_precision_loss)] // sample count fits well within f64 mantissa
pub fn scope(frame: &mut Frame, area: Rect, data: &[(f64, f64)], block: Block) {
    if data.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::raw(""),
                Line::styled("· · ·  no signal  · · ·", fg(FAINT)),
            ])
            .alignment(Alignment::Center)
            .block(block),
            area,
        );
        return;
    }

    let x_max = data.len() as f64;
    let range = scope_range(data.iter().fold(0.0, |m, &(_, y)| y.abs().max(m)));
    let block = if range < 1.0 {
        block.title_bottom(
            Line::styled(format!(" ×{} ", (1.0 / range).round()), fg(DIM)).right_aligned(),
        )
    } else {
        block
    };
    let zero = [(0.0, 0.0), (x_max, 0.0)];
    let datasets = vec![
        Dataset::default()
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(fg(FAINT))
            .data(&zero),
        Dataset::default()
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(fg(CYAN))
            .data(data),
    ];
    let chart = Chart::new(datasets)
        .block(block)
        .style(Style::default().bg(BG))
        .x_axis(Axis::default().bounds([0.0, x_max]))
        .y_axis(Axis::default().bounds([-range, range]));
    frame.render_widget(chart, area);
}

/// Footer row: key-cap hints on the left and, if non-empty, a status
/// message on the right.  Hints are clipped rather than overlapped.
pub fn footer(frame: &mut Frame, area: Rect, hints: &[(&str, &str)], status: &str) {
    let [left, right] = footer_areas(area, status);
    frame.render_widget(Paragraph::new(key_hints(hints)), left);
    if !status.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ✦ ", fg(MAGENTA)),
                Span::styled(status, bold(theme::YELLOW)),
                Span::raw(" "),
            ])),
            right,
        );
    }
}

/// The hint and status parts of a [`footer`] drawn into `area`.
fn footer_areas(area: Rect, status: &str) -> [Rect; 2] {
    let status_w = if status.is_empty() {
        0
    } else {
        u16::try_from(status.chars().count() + 4).unwrap_or(u16::MAX)
    };
    ratatui::layout::Layout::horizontal([Constraint::Min(0), Constraint::Length(status_w)])
        .areas(area)
}

/// Where a click landed on one of a [`footer`]'s key hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HintHit {
    /// Index of the hint in the `hints` slice.
    pub index: usize,
    /// For a click on the key cap: the glyph under the cursor and the number
    /// of glyphs on the cap, e.g. `(1, 2)` for the `↓` of `↑↓`. `None` for a
    /// click on the hint's label.
    pub cap_glyph: Option<(u16, u16)>,
}

/// The key hint at cell `(col, row)` of a [`footer`] drawn into `area` with
/// the same `hints` and `status`, if any.
///
/// Measures the same spans [`key_hints`] renders, so hit areas match what is
/// on screen, including hints clipped by the status message. The gaps
/// between hints do not count.
#[must_use]
pub fn footer_hint_at(
    area: Rect,
    hints: &[(&str, &str)],
    status: &str,
    col: u16,
    row: u16,
) -> Option<HintHit> {
    let [left, _] = footer_areas(area, status);
    if row != left.y || col < left.x || col >= left.right() {
        return None;
    }
    let col = usize::from(col - left.x);
    // The line starts with one space of padding.
    let mut x = 1;
    for (index, &(key, action)) in hints.iter().enumerate() {
        let cap = keycap(key).width();
        let label = Span::raw(format!(" {action}")).width(); // without the gap
        if (x..x + cap).contains(&col) {
            let glyphs = u16::try_from(Span::raw(key).width())
                .unwrap_or(u16::MAX)
                .max(1);
            // Clicks on the cap's padding count as its first or last glyph.
            let glyph = u16::try_from(col.saturating_sub(x + 1)).unwrap_or(u16::MAX);
            return Some(HintHit {
                index,
                cap_glyph: Some((glyph.min(glyphs - 1), glyphs)),
            });
        }
        if (x + cap..x + cap + label).contains(&col) {
            return Some(HintHit {
                index,
                cap_glyph: None,
            });
        }
        x += cap + Span::raw(format!(" {action}  ")).width();
    }
    None
}

/// Peak readout spans: `-6.0 dB` plus a red `CLIP` LED near full scale.
#[must_use]
pub fn db_readout(level: f32) -> Vec<Span<'static>> {
    let db = if level > 0.0 {
        format!("{:>6.1} dB", 20.0 * level.log10())
    } else {
        "  -inf dB".to_string()
    };
    let clip = if level >= 0.99 {
        Span::styled("  ● CLIP", bold(RED))
    } else {
        Span::styled("  ○ CLIP", fg(FAINT))
    };
    vec![Span::styled(db, fg(FG)), clip]
}

/// Centre a `width` × `height` popup inside `area`, clear it, and return it.
pub fn popup(frame: &mut Frame, area: Rect, width: u16, height: u16) -> Rect {
    let rect = area.centered(
        Constraint::Length(width.min(area.width)),
        Constraint::Length(height.min(area.height)),
    );
    frame.render_widget(Clear, rect);
    rect
}

/// Help-screen heading line, e.g. `▌ TRANSPORT`.
#[must_use]
pub fn help_heading(title: &str) -> Line<'_> {
    Line::from(vec![
        Span::styled("▌ ", fg(MAGENTA)),
        Span::styled(title, bold(CYAN)),
    ])
}

/// Help-screen entry: key cap padded to `key_width`, then the description.
#[must_use]
pub fn help_entry<'a>(key: &'a str, key_width: usize, action: &'a str) -> Line<'a> {
    let pad = key_width.saturating_sub(key.chars().count());
    Line::from(vec![
        Span::raw("  "),
        keycap(key),
        Span::raw(" ".repeat(pad + 1)),
        Span::styled(action, fg(FG)),
    ])
}

/// Centred "quit?" confirmation dialog.
pub fn quit_dialog(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::raw(""),
        Line::styled("Leave SYNTHIE?", bold(FG)).centered(),
        Line::raw(""),
        quit_buttons(),
    ];
    let rect = popup(frame, area, QUIT_W, QUIT_H);
    frame.render_widget(Paragraph::new(lines).block(panel("QUIT?", true)), rect);
}

/// Size of the [`quit_dialog`] box.
const QUIT_W: u16 = 34;
const QUIT_H: u16 = 7;
/// Row of the buttons inside the [`quit_dialog`] box, border included.
const QUIT_BUTTONS_ROW: u16 = 4;
/// Gap between the two buttons of the [`quit_dialog`].
const QUIT_GAP: &str = "    ";

/// The `Y quit    N stay` line of the [`quit_dialog`].
fn quit_buttons() -> Line<'static> {
    Line::from(vec![
        keycap("Y"),
        Span::styled(" quit", fg(DIM)),
        Span::raw(QUIT_GAP),
        keycap("N"),
        Span::styled(" stay", fg(DIM)),
    ])
    .centered()
}

/// Which button of a [`quit_dialog`] drawn into `area` is at cell
/// `(col, row)`: `Some(true)` for "quit" (its `Y` cap or label),
/// `Some(false)` for "stay", `None` anywhere else.
#[must_use]
pub fn quit_dialog_button_at(area: Rect, col: u16, row: u16) -> Option<bool> {
    let rect = area.centered(
        Constraint::Length(QUIT_W.min(area.width)),
        Constraint::Length(QUIT_H.min(area.height)),
    );
    // Inside the border; ratatui centres a line with the spare cells
    // split evenly, the odd one going to the right.
    let inner_w = rect.width.saturating_sub(2);
    let line = quit_buttons();
    let line_w = u16::try_from(line.width()).unwrap_or(u16::MAX);
    if row != rect.y + QUIT_BUTTONS_ROW || line_w > inner_w {
        return None;
    }
    let start = rect.x + 1 + (inner_w - line_w) / 2;
    let widths: Vec<u16> = line
        .spans
        .iter()
        .map(|s| u16::try_from(s.width()).unwrap_or(u16::MAX))
        .collect();
    let quit_end = start + widths[0] + widths[1];
    let stay_start = quit_end + widths[2];
    let stay_end = stay_start + widths[3] + widths[4];
    if (start..quit_end).contains(&col) {
        Some(true)
    } else if (stay_start..stay_end).contains(&col) {
        Some(false)
    } else {
        None
    }
}

/// Vim-style command line, e.g. `:q█`, for the footer row.
#[must_use]
pub fn command_line(text: &str) -> Line<'_> {
    Line::from(vec![
        Span::styled(":", bold(MAGENTA)),
        Span::styled(text, fg(FG)),
        Span::styled("█", fg(theme::gradient_cyclic(theme::clock() * 0.5))),
    ])
}

/// Render the open modal, if any: the dialog over `area`, or the command
/// line replacing the `footer` row.  Returns `true` when the footer was used.
pub fn modal(frame: &mut Frame, area: Rect, footer: Rect, modal: &crate::Modal) -> bool {
    match modal {
        crate::Modal::None => false,
        crate::Modal::ConfirmQuit => {
            quit_dialog(frame, area);
            false
        }
        crate::Modal::Command(text) => {
            frame.render_widget(Clear, footer);
            frame.render_widget(
                Paragraph::new(command_line(text)).style(Style::default().bg(BG)),
                footer,
            );
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HintHit, Rect, footer, footer_hint_at, quit_dialog, quit_dialog_button_at, scope_range,
    };

    #[test]
    fn scope_range_zooms_in_on_quiet_signals() {
        assert!((scope_range(0.9) - 1.0).abs() < f64::EPSILON);
        assert!((scope_range(0.3) - 0.5).abs() < f64::EPSILON);
        assert!((scope_range(0.1) - 0.125).abs() < f64::EPSILON);
        assert!((scope_range(0.06) - 1.0 / 16.0).abs() < f64::EPSILON);
        assert!((scope_range(0.0) - 1.0 / 16.0).abs() < f64::EPSILON);
    }

    /// Render a footer and return its single row as a string of cells.
    fn rendered(width: u16, hints: &[(&str, &str)], status: &str) -> Vec<String> {
        use ratatui::{Terminal, backend::TestBackend};
        let mut term = Terminal::new(TestBackend::new(width, 1)).unwrap();
        term.draw(|f| footer(f, f.area(), hints, status)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..width)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect()
    }

    fn col_of(cells: &[String], glyph: &str) -> u16 {
        u16::try_from(cells.iter().position(|c| c == glyph).unwrap()).unwrap()
    }

    #[test]
    fn footer_hits_match_what_is_drawn() {
        let hints = [("Space", "play"), ("↑↓", "row"), ("+ -", "BPM")];
        let area = Rect::new(0, 0, 60, 1);
        let cells = rendered(60, &hints, "");
        let hit = |glyph| footer_hint_at(area, &hints, "", col_of(&cells, glyph), 0);

        assert_eq!(hit("S").unwrap().index, 0);
        assert_eq!(hit("↑").unwrap().cap_glyph, Some((0, 2)));
        assert_eq!(hit("↓").unwrap().cap_glyph, Some((1, 2)));
        assert_eq!(hit("+").unwrap().cap_glyph, Some((0, 3)));
        assert_eq!(hit("-").unwrap().cap_glyph, Some((2, 3)));
        let row = hit("w").unwrap(); // the "w" of "row"
        assert_eq!((row.index, row.cap_glyph), (1, None));

        // The two-space gap after "play" and the leading pad hit nothing.
        let y = col_of(&cells, "y");
        assert_eq!(footer_hint_at(area, &hints, "", y + 1, 0), None);
        assert_eq!(footer_hint_at(area, &hints, "", y + 2, 0), None);
        assert_eq!(footer_hint_at(area, &hints, "", 0, 0), None);
        // Other rows and columns past the hints miss.
        assert_eq!(
            footer_hint_at(area, &hints, "", col_of(&cells, "S"), 1),
            None
        );
        assert_eq!(footer_hint_at(area, &hints, "", 59, 0), None);
    }

    #[test]
    fn footer_hits_follow_the_area_and_respect_the_status_message() {
        let hints = [("Space", "play"), ("Esc", "stop")];
        // Offset area, as in the centred browser layout.
        let area = Rect::new(10, 5, 40, 1);
        let at = |col| footer_hint_at(area, &hints, "", col, 5);
        assert_eq!(
            at(11),
            Some(HintHit {
                index: 0,
                cap_glyph: Some((0, 5))
            })
        );
        assert_eq!(at(1), None);
        // A long status message clips the second hint away.
        let status = "x".repeat(24);
        let esc_col = (10..50)
            .find(|&c| at(c).is_some_and(|h| h.index == 1))
            .unwrap();
        assert_eq!(footer_hint_at(area, &hints, &status, esc_col, 5), None);
    }

    #[test]
    fn quit_dialog_buttons_hit_where_they_are_drawn() {
        use ratatui::{Terminal, backend::TestBackend};
        // Even and odd sizes, and an offset area as in the browser layout.
        for (w, h, area) in [
            (120, 40, None),
            (81, 25, None),
            (34, 7, None),
            (120, 40, Some(Rect::new(7, 3, 101, 33))),
        ] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            let mut drawn = Rect::default();
            term.draw(|f| {
                drawn = area.unwrap_or_else(|| f.area());
                quit_dialog(f, drawn);
            })
            .unwrap();
            let buf = term.backend().buffer().clone();
            // Bottom-up: "Leave SYNTHIE?" above has a Y and an N too.
            let find = |glyph: &str| {
                (0..h)
                    .rev()
                    .flat_map(|y| (0..w).map(move |x| (x, y)))
                    .find(|&(x, y)| buf[(x, y)].symbol() == glyph)
                    .unwrap()
            };
            let (yx, yy) = find("Y");
            let (nx, ny) = find("N");
            let at = |x, y| quit_dialog_button_at(drawn, x, y);
            assert_eq!(at(yx, yy), Some(true), "{w}x{h}");
            assert_eq!(at(yx - 1, yy), Some(true), "Y cap padding, {w}x{h}");
            assert_eq!(at(yx + 5, yy), Some(true), "\"quit\" label, {w}x{h}");
            assert_eq!(at(yx - 2, yy), None, "left of the button, {w}x{h}");
            assert_eq!(at(yx + 7, yy), None, "gap, {w}x{h}");
            assert_eq!(at(nx, ny), Some(false), "{w}x{h}");
            assert_eq!(at(nx + 5, ny), Some(false), "\"stay\" label, {w}x{h}");
            assert_eq!(at(nx + 7, ny), None, "right of the button, {w}x{h}");
            assert_eq!(at(yx, yy - 1), None, "row above, {w}x{h}");
        }
    }
}
