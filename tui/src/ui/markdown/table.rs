//! Table rendering: markdown tables and pre-parsed grids (CSV) as closed boxes.

use super::*;

impl<'a> Writer<'a> {
    /// Draw a table as a closed box.
    ///
    /// Columns are shrunk widest-first rather than scaled proportionally: a
    /// table of one prose column and three short ones should lose columns from
    /// the prose, not squeeze `n` down to four characters alongside it.
    pub(super) fn render_table(&mut self, rows: &[Row], aligns: &[Alignment]) {
        // Markdown tables live on a handful of source lines; mapping every
        // rendered line to the parse line keeps the reader's editor entry on
        // the table.
        let src = self.src_line;
        self.render_table_rows(rows, aligns, Fit::Wrap, |_| src);
    }

    /// Like `render_table`, but each rendered line maps back to the source line
    /// `src(row)` suggested for the grid row it draws — how a CSV table keeps
    /// the editor cursor on the raw file row it mirrors.
    /// Draws the box and returns the display column each table column ends at,
    /// measured from the left edge of the box.
    fn render_table_rows(
        &mut self,
        rows: &[Row],
        aligns: &[Alignment],
        fit: Fit,
        src: impl Fn(usize) -> usize,
    ) -> Vec<usize> {
        let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        if cols == 0 {
            return Vec::new();
        }
        let indent = self.indent();
        let mut widths = vec![0usize; cols];
        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(seg_width(cell));
            }
        }
        if fit == Fit::Grid {
            for w in widths.iter_mut() {
                *w = (*w).min(MAX_GRID_COL);
            }
        }
        // `│ ` before every cell plus a closing `│`.
        let overhead = 3 * cols + 1;
        let floor = match fit {
            Fit::Grid => MIN_GRID_COL,
            Fit::Wrap => self.width / 8,
        };
        shrink_to(&mut widths, self.width.saturating_sub(indent + overhead), floor);

        let frame = Style::default().fg(self.theme.faint);
        let pad = Seg { text: " ".repeat(indent), style: Style::default(), link: None };
        let rule = |left: &str, mid: &str, right: &str| {
            let bars: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
            format!("{left}{}{right}", bars.join(mid))
        };

        self.src_line = src(0);
        self.push_line(vec![
            pad.clone(),
            Seg { text: rule("┌", "┬", "┐"), style: frame, link: None },
        ]);
        let head = Style::default().fg(self.theme.accent).add_modifier(Modifier::BOLD);
        for (r, row) in rows.iter().enumerate() {
            self.src_line = src(r);
            // In a grid every data row is one line — `fit_segs` truncates what
            // does not reach — but the header still wraps: a column you cannot
            // name is a column you cannot read, and there is only one of it.
            let one_line = fit == Fit::Grid && r > 0;
            let cells: Vec<Vec<Vec<Seg>>> = widths
                .iter()
                .enumerate()
                .map(|(i, width)| {
                    let cell = row.get(i).cloned().unwrap_or_default();
                    if one_line {
                        vec![cell]
                    } else {
                        wrap(cell, *width)
                    }
                })
                .collect();
            let height = cells.iter().map(Vec::len).max().unwrap_or(1);
            for line in 0..height {
                let mut segs = vec![pad.clone()];
                for (i, width) in widths.iter().enumerate() {
                    segs.push(Seg { text: "│ ".to_string(), style: frame, link: None });
                    let empty = Vec::new();
                    let cell = cells[i].get(line).unwrap_or(&empty);
                    let align = aligns.get(i).copied().unwrap_or(Alignment::None);
                    segs.extend(fit_segs(cell, *width, align, (r == 0).then_some(head)));
                    segs.push(Seg { text: " ".to_string(), style: frame, link: None });
                }
                segs.push(Seg { text: "│".to_string(), style: frame, link: None });
                self.push_line(segs);
            }
            if r == 0 {
                self.push_line(vec![
                    pad.clone(),
                    Seg { text: rule("├", "┼", "┤"), style: frame, link: None },
                ]);
            }
        }
        self.src_line = src(rows.len().saturating_sub(1));
        self.push_line(vec![pad, Seg { text: rule("└", "┴", "┘"), style: frame, link: None }]);
        self.push_blank();
        // `│ cell ` per column, so a column ends 3 cells past its content.
        widths
            .iter()
            .scan(indent, |x, w| {
                *x += w + 3;
                Some(*x)
            })
            .collect()
    }
}

/// Render a pre-parsed grid (a CSV) as a closed box table — the same drawing
/// markdown tables get, but from data already split into cells.
///
/// Each content line maps back to the grid row it draws, so pressing `e` in the
/// reader drops the editor cursor on the raw CSV line the visible row came from.
pub fn render_grid(rows: &[Vec<String>], width: u16, theme: &Theme) -> Doc {
    let width = width.max(20) as usize;
    let grid: Vec<Row> = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| {
                    // Never more than the cap can draw. A char is at least one
                    // column wide, so this many of them always fills the widest
                    // a grid column may be — and the runaway cell a stray quote
                    // produces is not copied, measured or wrapped in full on
                    // every frame of a resize.
                    let end = cell
                        .char_indices()
                        .nth(MAX_GRID_COL + 1)
                        .map_or(cell.len(), |(i, _)| i);
                    // A quoted cell may hold newlines. One record is one row
                    // here, so they become spaces; the editor has the original.
                    let text = cell[..end].replace('\n', " ");
                    vec![Seg { text, style: Style::default(), link: None }]
                })
                .collect()
        })
        .collect();
    let cols = grid.iter().map(Vec::len).max().unwrap_or(0);
    let aligns = vec![Alignment::None; cols];
    let mut w = Writer::new(width, theme);
    let stops = w.render_table_rows(&grid, &aligns, Fit::Grid, |r| r);
    w.doc.col_stops = stops;
    w.doc
}

fn seg_width(segs: &[Seg]) -> usize {
    segs.iter().map(|s| s.text.width()).sum()
}

/// Drop the whitespace a cell picked up around its markdown.
pub(super) fn trim_segs(mut segs: Vec<Seg>) -> Vec<Seg> {
    if let Some(first) = segs.first_mut() {
        first.text = first.text.trim_start().to_string();
    }
    if let Some(last) = segs.last_mut() {
        last.text = last.text.trim_end().to_string();
    }
    segs.retain(|s| !s.text.is_empty());
    segs
}

/// How a table treats a cell too wide for its column.
///
/// Prose wraps: a markdown table of sentences is all ellipsis otherwise, and a
/// tall table still says something where a clipped one says nothing. A data
/// grid does the opposite — one line per record is what makes it scannable, and
/// a cell wrapped to four lines drags every other column's row down with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Wrap,
    Grid,
}

/// Widest a grid column may grow to. Past this a cell is prose rather than a
/// field: it is truncated here and read in full in the editor. Without a cap a
/// single runaway cell — everything after a stray quote in a CSV lands in one —
/// would make the pan thousands of columns long with nothing in them.
pub(super) const MAX_GRID_COL: usize = 40;

/// Narrowest a grid column is squeezed to before the table gives up on fitting
/// and is panned instead. A fixed width because legibility is a property of the
/// text, not of the terminal: prose scales its floor with the measure, since a
/// wrapped sentence still reads at any width, but a field cut to seven cells is
/// gone whatever the pane is doing.
const MIN_GRID_COL: usize = 12;

/// Shrink the widest column repeatedly until the row fits, never below a width
/// that can still show something.
fn shrink_to(widths: &mut [usize], budget: usize, min: usize) {
    let min = min.max(3);
    let floor = min * widths.len();
    let budget = budget.max(floor);
    while widths.iter().sum::<usize>() > budget {
        let Some(widest) = widths
            .iter()
            .enumerate()
            .filter(|(_, w)| **w > min)
            .max_by_key(|(_, w)| **w)
            .map(|(i, _)| i)
        else {
            break;
        };
        widths[widest] -= 1;
    }
}

/// Pad or truncate a styled cell to an exact width, keeping its styles and its
/// links — a link in a table is still a link to follow.
fn fit_segs(segs: &[Seg], width: usize, align: Alignment, force: Option<Style>) -> Vec<Seg> {
    let restyle = |seg: &Seg| Seg {
        text: seg.text.clone(),
        style: force.unwrap_or(seg.style),
        link: seg.link,
    };
    let total = seg_width(segs);
    let mut out: Vec<Seg> = Vec::with_capacity(segs.len() + 2);
    let blank = |n: usize| Seg { text: " ".repeat(n), style: Style::default(), link: None };

    if total > width {
        let mut used = 0usize;
        for seg in segs {
            let w = seg.text.width();
            if used + w <= width.saturating_sub(1) {
                used += w;
                out.push(restyle(seg));
                continue;
            }
            let (head, _) = split_at_width(&seg.text, width.saturating_sub(used + 1));
            used += head.width();
            out.push(Seg { text: head, ..restyle(seg) });
            break;
        }
        out.push(Seg { text: "…".to_string(), style: force.unwrap_or_default(), link: None });
        used += 1;
        if used < width {
            out.push(blank(width - used));
        }
        return out;
    }

    let slack = width - total;
    let (left, right) = match align {
        Alignment::Right => (slack, 0),
        Alignment::Center => (slack / 2, slack - slack / 2),
        _ => (0, slack),
    };
    if left > 0 {
        out.push(blank(left));
    }
    out.extend(segs.iter().map(restyle));
    if right > 0 {
        out.push(blank(right));
    }
    out
}
