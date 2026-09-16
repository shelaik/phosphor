//! The bottom bar: which commands it offers, where they land, and how it draws.
//!
//! Its own file because it is a small layout engine, not a widget. The bar used
//! to stop at the right edge and silently drop every chip that did not fit — at
//! 120 columns that was the whole last group, including the help and the
//! mouse-mode switch. A defect nobody notices, because the evidence for it is
//! an *absence*. It now wraps, so [`place_chips`] has to decide where each chip
//! goes before the layout can know how tall the bar is, and that decision is
//! worth being able to read on its own.

use super::*;

pub(super) fn shortcut_chips(app: &App) -> Vec<Vec<(&'static str, &'static str, u16)>> {
    if app.confirm.is_some() {
        return vec![vec![("s", "conferma", 0), ("Esc", "annulla", 0)]];
    }
    if app.picker.is_some() {
        return vec![vec![("↑↓", "scegli", 0), ("⏎", "apri/seleziona", 0), ("←", ".. su", 0), ("Esc", "annulla", 0)]];
    }
    if app.searching {
        return vec![vec![("scrivi", "", 0), ("⌫", "canc", 0), ("⏎", "ok", 0), ("Esc", "annulla", 0)]];
    }
    if app.reader.is_some() {
        return vec![vec![("↑↓", "scorri", 0), ("m", "→ markdown", A_MARKDOWN), ("Esc", "chiudi", 0)]];
    }
    if app.help {
        return vec![vec![("↑↓", "scorri", 0), ("PgUp/PgDn", "salta", 0), ("Esc", "chiudi", 0)]];
    }
    if app.gsearch.is_some() {
        return vec![vec![("scrivi", "", 0), ("⏎", "cerca/apri", 0), ("↑↓", "scegli", 0), ("Esc", "chiudi", 0)]];
    }
    if app.detail || app.show_agents {
        return vec![vec![("Esc", "chiudi", 0), ("v", "leggi", A_READ), ("r", "ripr", A_RESUME), ("a", "agenti", A_AGENTS)]];
    }
    if app.tab == 0 {
        vec![
            vec![("↑", "", A_UP), ("↓", "", A_DOWN), ("+/-", "riprese", 0)],
            vec![("/", "cerca", A_SEARCH), ("f", "filtro", A_FILTER), ("o", "ord", A_SORTCOL), ("s", "dir", A_SORTDIR)],
            vec![("⏎", "dett", A_DETAIL), ("v", "leggi", A_READ), ("g", "cerca tutto", A_GSEARCH), ("a", "agenti", A_AGENTS), ("r", "ripr", A_RESUME), ("e", "export", A_EXPORT)],
            vec![("A", "espandi/comprimi", A_EXPAND_ALL), ("x", "porta", A_EXPBUNDLE), ("i", "importa", A_IMPORT), ("t", "tema", A_THEME_NEXT), ("p", "px", A_PIXEL), ("?", "aiuto", A_HELP), ("🖰", if app.mouse_only { "solo mouse" } else { "mouse+tasti" }, A_MOUSE_MODE), ("q", "esci", A_QUIT)],
        ]
    } else {
        vec![
            vec![("m", "metrica", A_METRIC)],
            vec![("Tab", "vista", A_TAB), ("t", "tema", A_THEME_NEXT), ("p", "px", A_PIXEL)],
            vec![("?", "aiuto", A_HELP), ("🖰", if app.mouse_only { "solo mouse" } else { "mouse+tasti" }, A_MOUSE_MODE), ("q", "esci", A_QUIT)],
        ]
    }
}

/// The bottom bar never grows past this, however many commands there are: it
/// sits above the table, and a bar that keeps growing eats the thing you came
/// to look at. What does not fit is still in the help, which is clickable.
pub(super) const BAR_MAX_LINES: u16 = 3;

/// Width of one chip as it will be drawn, and whether it is drawn in the
/// mouse-only form (the word alone).
///
/// In mouse-only the WORD leads and the key is dropped: a letter you are not
/// going to press is noise, and the label is what you aim at. A chip with no
/// label keeps its key — it is all it has.
fn chip_width(key: &str, label: &str, mouse_only: bool) -> (u16, bool) {
    let mouse = mouse_only && !label.is_empty();
    let text = if mouse {
        format!("{label} ")
    } else if label.is_empty() {
        format!("{key} ")
    } else {
        format!("{key} {label} ")
    };
    (text.chars().count() as u16, mouse)
}

/// One chip, placed: which row of the bar, where on it, and which chip of
/// which group it is.
#[derive(Clone, Copy)]
pub(super) struct Placed {
    pub(super) row: u16,
    pub(super) x: u16,
    pub(super) w: u16,
    gi: usize,
    ci: usize,
}

/// Where every chip goes.
///
/// Wrapping at all is the point. The bar used to stop at the right edge and
/// silently drop everything after it — at 120 columns that was the whole last
/// group, including the help and the mouse-mode switch, which is exactly the
/// kind of thing nobody notices because the evidence is *absence*.
pub(super) fn place_chips(app: &App, width: u16) -> (u16, Vec<Placed>) {
    let groups = shortcut_chips(app);
    let mut out = Vec::new();
    let (mut row, mut x) = (0u16, 1u16);
    for (gi, group) in groups.iter().enumerate() {
        let gw: u16 = group.iter().map(|(k, l, _)| chip_width(k, l, app.mouse_only).0).sum();
        // A group moves to the next row whole when it can: the groups are the
        // only grouping the bar has, and splitting one is worse than a gap.
        if x > 1 && x + 2 + gw >= width && row + 1 < BAR_MAX_LINES {
            row += 1;
            x = 1;
        } else if x > 1 {
            x += 2; // the ║ separator
        }
        for (ci, (key, label, _)) in group.iter().enumerate() {
            let (w, _) = chip_width(key, label, app.mouse_only);
            if x + w >= width {
                if row + 1 >= BAR_MAX_LINES {
                    break;
                }
                row += 1;
                x = 1;
            }
            out.push(Placed { row, x, w, gi, ci });
            x += w;
        }
    }
    (row + 1, out)
}

/// How many rows the bar needs at this width. Called before the layout splits,
/// so the bar asks for the height it will actually use.
pub(super) fn shortcut_bar_height(app: &App, width: u16) -> u16 {
    place_chips(app, width).0.clamp(1, BAR_MAX_LINES)
}

pub(super) fn render_shortcut_bar(app: &mut App, th: &Theme, area: Rect) -> Paragraph<'static> {
    let groups = shortcut_chips(app);
    let (rows_used, placed) = place_chips(app, area.width);
    let mut lines: Vec<Line> = Vec::new();
    let mut hot = Hotspots::default();
    for r in 0..rows_used.min(area.height.max(1)) {
        let mut spans: Vec<Span> = vec![Span::styled(" ", Style::default())];
        let mut prev_gi: Option<usize> = None;
        for p in placed.iter().copied().filter(|p| p.row == r) {
            let (key, label, action) = groups[p.gi][p.ci];
            if prev_gi.is_some_and(|g| g != p.gi) {
                spans.push(Span::styled("║ ", Style::default().fg(th.dim)));
            }
            prev_gi = Some(p.gi);
            let (_, mouse) = chip_width(key, label, app.mouse_only);
            if mouse {
                spans.push(Span::styled(
                    label.to_string(),
                    Style::default().fg(th.accent).add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::styled(key.to_string(), Style::default().fg(th.accent).add_modifier(Modifier::BOLD)));
                if !label.is_empty() {
                    spans.push(Span::styled(format!(" {label}"), Style::default().fg(th.dim)));
                }
            }
            spans.push(Span::styled(" ", Style::default()));
            hot.push(Rect { x: area.x + p.x, y: area.y + p.row, width: p.w, height: 1 }, action);
        }
        lines.push(Line::from(spans));
    }
    app.shortcut_groups = hot;
    Paragraph::new(lines).style(Style::default().bg(th.bg))
}
