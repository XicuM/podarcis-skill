//! The tree pane: collections of pages, foldable and resizable.

use super::*;

// ------------------------------------------------------------------- tree

/// A folded collection shows its header rule and nothing else.
pub const FOLDED_ROWS: u16 = 1;
/// Two borders and one row of content: below this a box cannot show anything.
pub const MIN_COLLECTION_ROWS: u16 = 3;

/// How many rows each collection's box gets.
///
/// Sizes are the user's, not the content's: an empty collection keeps whatever
/// height it was dragged to. A folded one costs a single row, and the last
/// *open* one absorbs the remainder — that is what keeps the column exactly
/// full at any terminal height without having to store fractions, and it is
/// why its own stored height is never consulted.
pub fn collection_rows(heights: &[Option<u16>], folded: &[bool], total: u16) -> Vec<u16> {
    let n = heights.len();
    if n == 0 || total == 0 {
        return vec![0; n];
    }
    // Fewer rows than boxes. Even a header rule costs one, so the boxes that
    // fit get theirs and the rest get nothing — a box drawn into zero rows
    // draws nothing, which is the honest outcome at this size. Without this
    // the folded boxes alone could claim more rows than the column has.
    if total < n as u16 {
        return (0..n).map(|i| u16::from((i as u16) < total)).collect();
    }

    let open: Vec<usize> = (0..n).filter(|i| !folded[*i]).collect();
    let mut out = vec![FOLDED_ROWS; n];

    // Everything folded: the rows still have to add up, so the last one takes
    // the slack rather than leaving a gap under the column.
    let Some((&last, rest)) = open.split_last() else {
        out[n - 1] = total.saturating_sub(FOLDED_ROWS * (n as u16 - 1));
        return out;
    };

    let reserved = FOLDED_ROWS * (n - open.len()) as u16;
    let budget = total.saturating_sub(reserved);
    // Too cramped to honour anyone's preference: share what there is evenly and
    // let the last take the remainder.
    if budget < MIN_COLLECTION_ROWS * open.len() as u16 {
        let each = budget / open.len() as u16;
        for &i in rest {
            out[i] = each;
        }
        out[last] = budget - each * rest.len() as u16;
        return out;
    }

    let even = budget / open.len() as u16;
    let mut spent = 0u16;
    for (k, &i) in rest.iter().enumerate() {
        // Whatever is left must still seat every open box after this one.
        let after = (rest.len() - k) as u16 * MIN_COLLECTION_ROWS;
        let ceiling = budget.saturating_sub(spent + after).max(MIN_COLLECTION_ROWS);
        out[i] = heights[i].unwrap_or(even).clamp(MIN_COLLECTION_ROWS, ceiling);
        spent += out[i];
    }
    out[last] = budget - spent;
    out
}

pub fn tree(frame: &mut Frame, app: &mut App, area: Rect) {
    let names: Vec<String> = app.collection_names();
    let n = names.len().max(1);
    let folded: Vec<bool> = names.iter().map(|c| app.cfg.collapsed_collections.contains(c)).collect();
    let wanted: Vec<Option<u16>> =
        names.iter().map(|c| app.cfg.collection_heights.get(c).copied()).collect();
    let rows = collection_rows(&wanted, &folded, area.height);
    let constraints: Vec<Constraint> = if rows.is_empty() {
        (0..n).map(|_| Constraint::Fill(1)).collect()
    } else {
        rows.iter().map(|r| Constraint::Length(*r)).collect()
    };
    let chunks = Layout::vertical(constraints).split(area);

    let focused = app.focus == Focus::Tree;
    let open_path = app.open.as_ref().map(|o| o.page.path.clone());
    let selected = app.tree.selected;

    let mut panes = Vec::with_capacity(n);
    for i in 0..app.tree.collection_paths().len() {
        let Some((header, end)) = app.tree.section_span(i) else {
            continue;
        };
        let chunk = chunks[i];
        let row = &app.tree.rows[header];
        let track = app.track_for(&row.path);
        let section_focus = focused && selected >= header && selected < end;

        let mut spans = vec![
            Span::styled(format!(" {}", row.label), app.theme.title(section_focus)),
        ];
        if let Some(branch) = track.branch() {
            spans.push(Span::styled(" · ", app.theme.faint_style()));
            spans.push(Span::styled(
                format!("{branch} "),
                Style::default().fg(app.theme.ok).add_modifier(Modifier::BOLD),
            ));
        } else {
            spans[0] = Span::styled(format!(" {} ", row.label), app.theme.title(section_focus));
        }
        let title = Line::from(spans);
        // A folded collection keeps its header rule and gives up everything
        // else, so the column reads as a stack of sections rather than a set
        // of boxes that vanish.
        let borders = if folded[i] { Borders::TOP } else { Borders::ALL };
        let block = Block::default()
            .borders(borders)
            .border_type(BorderType::Rounded)
            .border_style(app.theme.border(section_focus))
            .title(title)
            .style(Style::default().bg(app.theme.bg));
        let inner = if folded[i] { Rect::ZERO } else { block.inner(chunk) };
        frame.render_widget(block, chunk);

        // The fold handle, on the header rule at the right-hand end. `\u{25be}`
        // points at the content it would hide; `\u{25b8}` points at content
        // waiting to come back.
        let fold = if chunk.width >= 6 {
            let rect = Rect::new(chunk.x + chunk.width - 3, chunk.y, 1, 1);
            frame.render_widget(
                Paragraph::new(Span::styled(
                    if folded[i] { "\u{25b8}" } else { "\u{25be}" },
                    Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD),
                )),
                rect,
            );
            Some(rect)
        } else {
            None
        };

        let content_focused = app.focus == Focus::Doc && app.doc_stop == DocStop::Content;
        let open_row = if content_focused {
            open_path.as_ref().and_then(|p| app.tree.rows.iter().position(|r| &r.path == p))
        } else {
            None
        };
        let active_row = if content_focused && open_row.is_some() {
            open_row.unwrap()
        } else {
            selected
        };

        let start = header + 1;
        let height = inner.height as usize;
        let local_selected = if active_row >= start && active_row < end {
            active_row - start
        } else {
            0
        };
        let child_count = end.saturating_sub(start);
        let offset = if active_row >= start && active_row < end {
            scroll_offset(local_selected, child_count, height)
        } else {
            0
        };

        if !folded[i] && !inner.is_empty() && start < end {
            let lines: Vec<Line> = app
                .tree
                .rows
                .iter()
                .enumerate()
                .skip(start + offset)
                .take(end.saturating_sub(start + offset).min(height))
                .map(|(i, row)| {
                    tree_line(app, i, row, selected, focused, open_path.as_deref(), inner.width as usize)
                })
                .collect();
            frame.render_widget(Paragraph::new(lines), inner);
        }

        panes.push(TreePane {
            area: chunk,
            inner,
            header,
            // A folded collection draws none of its rows, so it owns none:
            // `start == end` is what tells hit-testing to answer with the
            // header rather than a row that is not on screen.
            start: if folded[i] { end } else { start },
            end,
            offset,
            fold,
            folded: folded[i],
        });
    }
    // The boundary between two stacked boxes, for dragging. The last box ends
    // at the column's own edge, so it has none below it — which is also why it
    // is the one that absorbs the remainder.
    app.areas.collection_dividers = panes
        .windows(2)
        .enumerate()
        .map(|(i, pair)| (i, pair[1].area.y))
        .collect();
    app.areas.tree_panes = panes;
}

/// Left-of-label badge for a non-markdown file. The colours come from the
/// theme's neutral range, not its alert range — see `Theme::file_badge`.
pub(super) fn external_badge(theme: &Theme, path: &std::path::Path, is_dir: bool) -> Option<(&'static str, Color)> {
    if is_dir {
        return None;
    }
    let ext = path.extension().and_then(|e| e.to_str())?.to_ascii_lowercase();
    theme.file_badge(&ext)
}

pub(super) fn tree_line<'a>(
    app: &'a App,
    i: usize,
    row: &'a crate::vault::tree::Row,
    selected: usize,
    focused: bool,
    open_path: Option<&std::path::Path>,
    width: usize,
) -> Line<'a> {
    let is_open = open_path == Some(row.path.as_path());
    let content_focused = app.focus == Focus::Doc && app.doc_stop == DocStop::Content;
    let marker = if row.is_dir {
        if row.expanded {
            "▾ "
        } else {
            "▸ "
        }
    } else {
        "  "
    };

    let mut style = if row.is_dir {
        Style::default().fg(app.theme.text)
    } else {
        Style::default().fg(app.theme.subtext)
    };

    let is_highlighted = if content_focused {
        if open_path.is_some() {
            is_open
        } else {
            i == selected
        }
    } else {
        i == selected
    };

    if is_highlighted {
        style = if content_focused && is_open {
            app.theme.selection(false).add_modifier(Modifier::BOLD)
        } else {
            app.theme.selection(focused)
        };
    }

    // Guide runs rather than bare spaces: one space per level left a ragged
    // edge that got harder to read the deeper the tree went, and depth is the
    // one thing a tree exists to show. The guides stay `faint` on a selected
    // row too, so the selection wash reads as one block.
    let indent = row.depth.saturating_sub(1);
    let guide = if is_highlighted {
        style
    } else {
        app.theme.faint_style()
    };
    let mut spans = vec![
        // One cell per level, exactly as the plain spaces were, so adding the
        // guides costs the label no columns in a narrow pane.
        Span::styled("│".repeat(indent), guide),
        Span::styled(marker, style),
    ];

    if let Some((tag, colour)) = external_badge(&app.theme, &row.path, row.is_dir) {
        // The colour block itself is the badge — one space of padding inside
        // it on each side so the three letters are not flush against the edge.
        spans.push(Span::styled(
            format!(" {tag} "),
            Style::default().fg(app.theme.overlay).bg(colour).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
    }

    spans.push(Span::styled(row.label.clone(), style));

    // The git glyph and the lint mark used to trail the label, so their column
    // moved with every filename and the eye had to re-find them on each row.
    // They are collected here and pinned to the right edge instead, and the
    // gap between is padded in the row's own style — which also turns the
    // selection into a full-width bar rather than a ragged stub.
    let mut marks: Vec<Span<'a>> = Vec::new();
    if let Some(status) = app.git.status_for(&row.path) {
        let colour = match status {
            GitStatus::Conflict => app.theme.err,
            GitStatus::Modified | GitStatus::Deleted => app.theme.warn,
            GitStatus::Added | GitStatus::Renamed => app.theme.ok,
            GitStatus::Untracked => app.theme.subtext,
        };
        marks.push(Span::styled(
            format!(" {}", status.glyph()),
            Style::default().fg(colour),
        ));
    }

    let badge = app
        .index
        .get(&row.rel)
        .map(|e| e.worst())
        .filter(|_| !row.is_dir)
        .or_else(|| app.index.dir_findings.contains_key(&row.rel).then_some(Severity::Warn));
    if let Some(severity) = badge {
        // A weight ramp, lightest to heaviest: a triangle warns, a filled dot
        // is an error. The warning used to be an emoji-presentation codepoint
        // that some terminals draw two cells wide, which shears a
        // column-aligned tree.
        let (mark, colour) = match severity {
            Severity::Clean => ("", app.theme.ok),
            Severity::Warn => (" ▲", app.theme.warn),
            Severity::Error => (" ●", app.theme.err),
        };
        if !mark.is_empty() {
            marks.push(Span::styled(mark, Style::default().fg(colour)));
        }
    }

    let used: usize = spans.iter().map(|s| s.content.as_ref().width()).sum();
    let mark_w: usize = marks.iter().map(|s| s.content.as_ref().width()).sum();
    if used + mark_w > width {
        // Too narrow for both: the label gives up cells, because the marks are
        // the part that cannot be inferred from anything else on the row.
        let over = used + mark_w - width;
        if let Some(label) = spans.last_mut() {
            let kept = label.content.as_ref().width().saturating_sub(over);
            label.content = truncate(label.content.as_ref(), kept).into();
        }
    }
    let used: usize = spans.iter().map(|s| s.content.as_ref().width()).sum();
    spans.push(Span::styled(
        " ".repeat(width.saturating_sub(used + mark_w)),
        style,
    ));
    spans.extend(marks);
    Line::from(spans)
}

