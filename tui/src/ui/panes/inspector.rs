//! The inspector pane: citations, backlinks, and lint findings for the open page.

use super::*;
use super::wrap::{wrap_biblio, wrap_line};

// -------------------------------------------------------------- inspector

#[allow(dead_code)]
pub(super) fn inspector_height(open: &Open, available: u16) -> u16 {
    let citations = open.page.okf.sources.len() + open.page.footnote_definitions().len();
    let rows = 2 + citations.min(4) + usize::from(!open.page.okf.related.is_empty());
    (rows as u16 + 2).min(available / 2).max(5)
}

pub fn inspector_lines(
    open: &Open,
    index: &crate::vault::index::Index,
    root: &std::path::Path,
    theme: &Theme,
) -> Vec<Line<'static>> {
    inspector_rows(open, index, root, theme).into_iter().map(|(line, _)| line).collect()
}

/// A citation's display label, built from the source's own metadata (its
/// `resource` file's frontmatter — real author lists, not the abbreviated
/// `Author et al.` copy a wiki page keeps in its own `sources:` entry) and
/// falling back to that copy when the resource is missing or unreadable.
pub(super) fn citation_label(source: &crate::vault::page::SourceRef, page_dir: &std::path::Path) -> String {
    let meta = source
        .resource
        .as_deref()
        .map(|r| crate::vault::links::normalize(&page_dir.join(r)))
        .and_then(|p| crate::vault::page::SourceMeta::load(&p))
        .filter(|m| !m.is_empty());

    let (title, authors, year) = match meta {
        Some(m) => (
            m.title.or_else(|| source.title.clone()),
            (!m.authors.is_empty()).then(|| m.authors.join(", ")).or_else(|| source.author.clone()),
            m.year.or_else(|| source.year.clone()),
        ),
        None => (source.title.clone(), source.author.clone(), source.year.clone()),
    };

    let mut label = authors.unwrap_or_default();
    if let Some(year) = year {
        label = if label.is_empty() { format!("({year})") } else { format!("{label} ({year})") };
    }
    if let Some(title) = title {
        label = if label.is_empty() { title } else { format!("{label} — {title}") };
    }
    label
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InspectorTarget {
    Source(String),
    Backlinks(Vec<(String, std::path::PathBuf)>),
    None,
}

impl InspectorTarget {
    #[allow(dead_code)]
    pub fn as_deref(&self) -> Option<&str> {
        match self {
            Self::Source(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

/// Each inspector line paired with the source id it belongs to, if any — a
/// citation clicked in the body needs this to scroll to its row, and a click
/// on the row itself needs it to know what to open.
pub(super) fn inspector_rows(
    open: &Open,
    index: &crate::vault::index::Index,
    root: &std::path::Path,
    theme: &Theme,
) -> Vec<(Line<'static>, InspectorTarget)> {
    let rel = crate::vault::page::rel_path(&open.page.path, root);
    let entry = index.by_path(&open.page.path);
    let backlinks = index.backlinks(&rel);
    let page_dir = open.page.path.parent().unwrap_or(root);

    let mut rows: Vec<(Line<'static>, InspectorTarget)> = Vec::new();

    if let Some(entry) = entry {
        for finding in &entry.findings {
            rows.push((
                Line::from(vec![
                    Span::styled("▲ ", Style::default().fg(theme.warn)),
                    Span::styled(finding.code, Style::default().fg(theme.err)),
                    Span::styled(format!("  {}", finding.detail), theme.dim()),
                ]),
                InspectorTarget::None,
            ));
        }
    }

    let cited = open.page.footnote_refs();
    let defs = open.page.footnote_defs();

    // Candidates first, each tagged with the order its `[N]` is cited in the
    // body — numbered sources sort by that number, the uncited trail after.
    let mut candidates: Vec<(usize, String, Line<'static>)> = Vec::new();

    let active_link_target = open
        .link
        .and_then(|i| open.doc.links.get(i))
        .filter(|l| l.kind == crate::vault::links::LinkKind::Footnote)
        .map(|l| l.target.as_str());

    // A footnote definition carries the full citation the wiki wrote — prefer
    // it over the abbreviated `sources:` copy.
    for (id, text, _) in &defs {
        let used = cited.contains(id);
        let mark = match open.doc.citation_number(id) {
            Some(n) => format!("[{n}]"),
            None if used => "✓".to_string(),
            None => "·".to_string(),
        };
        let selected = open.selected_citation.as_deref() == Some(id.as_str())
            || active_link_target == Some(id.as_str());
        let order = open.doc.citation_number(id).unwrap_or(usize::MAX);
        let colour = if used { theme.ok } else { theme.faint };
        let mark_style = if selected {
            Style::default().fg(theme.bg).bg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(colour)
        };
        candidates.push((
            order,
            id.clone(),
            Line::from(vec![
                Span::styled(mark, mark_style),
                Span::raw(" "),
                Span::styled(text.clone(), Style::default().fg(theme.text)),
                Span::styled(format!("  {id}"), theme.faint_style()),
            ]),
        ));
    }
    for source in &open.page.okf.sources {
        if defs.iter().any(|(id, _, _)| id == &source.id) {
            continue;
        }
        let label = citation_label(source, page_dir);
        let used = cited.contains(&source.id);
        let mark = match open.doc.citation_number(&source.id) {
            Some(n) => format!("[{n}]"),
            None if used => "✓".to_string(),
            None => "·".to_string(),
        };
        let selected = open.selected_citation.as_deref() == Some(source.id.as_str())
            || active_link_target == Some(source.id.as_str());
        let order = open.doc.citation_number(&source.id).unwrap_or(usize::MAX);
        let colour = if used { theme.ok } else { theme.faint };
        let mark_style = if selected {
            Style::default().fg(theme.bg).bg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(colour)
        };
        candidates.push((
            order,
            source.id.clone(),
            Line::from(vec![
                Span::styled(mark, mark_style),
                Span::raw(" "),
                Span::styled(label, Style::default().fg(theme.text)),
                Span::styled(format!("  {}", source.id), theme.faint_style()),
            ]),
        ));
    }
    candidates.sort_by_key(|(order, _, _)| *order);
    if !candidates.is_empty() {
        rows.push((Line::default(), InspectorTarget::None));
        for (i, (_, id, line)) in candidates.into_iter().enumerate() {
            if i > 0 {
                rows.push((Line::default(), InspectorTarget::None));
            }
            rows.push((line, InspectorTarget::Source(id)));
        }
        rows.push((Line::default(), InspectorTarget::None));
    }

    if !backlinks.is_empty() {
        let mut spans = vec![Span::styled("← ", Style::default().fg(theme.link))];
        for (i, entry) in backlinks.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" · ", theme.dim()));
            }
            spans.push(Span::styled(entry.title.clone(), Style::default().fg(theme.link)));
        }
        let entries = backlinks.iter().map(|e| (e.title.clone(), e.path.clone())).collect();
        rows.push((Line::from(spans), InspectorTarget::Backlinks(entries)));
    }

    if rows.is_empty() {
        rows.push((
            Line::from(Span::styled("no sources, no backlinks, nothing to fix", theme.faint_style())),
            InspectorTarget::None,
        ));
    }

    rows
}

pub fn wrap_backlinks(
    entries: &[(String, std::path::PathBuf)],
    width: usize,
    theme: &Theme,
) -> (Vec<Line<'static>>, Vec<InspectorBacklink>) {
    use unicode_width::UnicodeWidthChar;

    if width == 0 || entries.is_empty() {
        return (vec![Line::default()], Vec::new());
    }

    let mut chars: Vec<(char, Style, Option<std::path::PathBuf>)> = Vec::new();
    chars.push(('←', Style::default().fg(theme.link), None));
    chars.push((' ', Style::default().fg(theme.link), None));

    for (i, (title, path)) in entries.iter().enumerate() {
        if i > 0 {
            chars.push((' ', theme.dim(), None));
            chars.push(('·', theme.dim(), None));
            chars.push((' ', theme.dim(), None));
        }
        for c in title.chars() {
            chars.push((c, Style::default().fg(theme.link), Some(path.clone())));
        }
    }

    let mut rows: Vec<&[(char, Style, Option<std::path::PathBuf>)]> = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let mut col = 0usize;
        let mut end = start;
        let mut last_space = None;
        while end < chars.len() {
            let w = chars[end].0.width().unwrap_or(1);
            if col + w > width {
                break;
            }
            if chars[end].0 == ' ' {
                last_space = Some(end);
            }
            col += w;
            end += 1;
        }
        if end == chars.len() {
            rows.push(&chars[start..end]);
            break;
        }
        let break_at = last_space.filter(|&pos| pos >= start).unwrap_or(end.max(start + 1));
        rows.push(&chars[start..break_at]);
        start = if chars.get(break_at).is_some_and(|(c, _, _)| *c == ' ') { break_at + 1 } else { break_at };
        while start < chars.len() && chars[start].0 == ' ' {
            start += 1;
        }
    }

    let mut wrapped_lines: Vec<Line<'static>> = Vec::new();
    let mut links: Vec<InspectorBacklink> = Vec::new();

    for (row_idx, row) in rows.into_iter().enumerate() {
        let end = row.iter().rposition(|(c, _, _)| *c != ' ').map_or(0, |i| i + 1);
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut cur_col = 0u16;
        let mut cur_link: Option<(u16, u16, std::path::PathBuf)> = None;

        for &(c, style, ref target) in &row[..end] {
            let w = c.width().unwrap_or(1) as u16;

            match target {
                Some(path) => match &mut cur_link {
                    Some((_, end_col, cur_path)) if cur_path == path => {
                        *end_col += w;
                    }
                    _ => {
                        if let Some((start_col, end_col, p)) = cur_link.take() {
                            links.push(InspectorBacklink {
                                row: row_idx,
                                col_start: start_col,
                                col_end: end_col,
                                path: p,
                            });
                        }
                        cur_link = Some((cur_col, cur_col + w, path.clone()));
                    }
                },
                None => {
                    if let Some((start_col, end_col, p)) = cur_link.take() {
                        links.push(InspectorBacklink {
                            row: row_idx,
                            col_start: start_col,
                            col_end: end_col,
                            path: p,
                        });
                    }
                }
            }

            cur_col += w;

            match spans.last_mut() {
                Some(last) if last.style == style => last.content.to_mut().push(c),
                _ => spans.push(Span::styled(c.to_string(), style)),
            }
        }

        if let Some((start_col, end_col, p)) = cur_link {
            links.push(InspectorBacklink {
                row: row_idx,
                col_start: start_col,
                col_end: end_col,
                path: p,
            });
        }

        wrapped_lines.push(Line::from(spans));
    }

    (wrapped_lines, links)
}

pub(super) fn inspector(frame: &mut Frame, app: &mut App, area: Rect) {
    let open = app.open.as_ref().unwrap();
    let rel = crate::vault::page::rel_path(&open.page.path, &app.cfg.root);
    let entry = app.index.by_path(&open.page.path);
    let backlinks = app.index.backlinks(&rel);

    let sources = open.page.okf.sources.len() + open.page.footnote_definitions().len();
    let findings = entry.map(|e| e.findings.len()).unwrap_or(0);
    // Same marks the pane body uses for each thing — a citation bracket, `←`
    // for backlinks, `▲` for findings — so the title reads as a summary of
    // what's below rather than a second vocabulary to learn. A count of zero
    // means "nothing to see here", so the segment is dropped instead of
    // printed as `0`.
    let title = [
        (sources > 0).then(|| format!("[{sources}]")),
        (!backlinks.is_empty()).then(|| format!("← {}", backlinks.len())),
        (findings > 0).then(|| format!("▲ {findings}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("  ");
    let title = if title.is_empty() { "empty".to_string() } else { title };
    let focused = app.focus == Focus::Doc && app.doc_stop == DocStop::Sources;
    let block = pane(&app.theme, &title, focused).padding(ratatui::widgets::Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    app.areas.inspector_body = inner;
    if inner.is_empty() {
        app.areas.inspector_rows.clear();
        app.areas.inspector_backlinks.clear();
        return;
    }

    // Wrapped one raw row at a time (rather than via `wrap_lines` over the
    // whole batch) so each wrapped screen row keeps the citation id its raw
    // row carried — the mapping a click and a scroll-to-selection both need.
    let open = app.open.as_ref().unwrap();
    let raw_rows = inspector_rows(open, &app.index, &app.cfg.root, &app.theme);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut ids: Vec<Option<String>> = Vec::new();
    let mut all_backlinks: Vec<InspectorBacklink> = Vec::new();
    for (line, target) in raw_rows {
        match target {
            InspectorTarget::Source(id) => {
                let wrapped = wrap_biblio(line, inner.width as usize);
                ids.extend(std::iter::repeat_n(Some(id), wrapped.len()));
                lines.extend(wrapped);
            }
            InspectorTarget::Backlinks(entries) => {
                let (wrapped, backlinks) = wrap_backlinks(&entries, inner.width as usize, &app.theme);
                let base_row = lines.len();
                for mut bl in backlinks {
                    bl.row += base_row;
                    all_backlinks.push(bl);
                }
                ids.extend(std::iter::repeat_n(None, wrapped.len()));
                lines.extend(wrapped);
            }
            InspectorTarget::None => {
                let wrapped = wrap_line(line, inner.width as usize);
                ids.extend(std::iter::repeat_n(None, wrapped.len()));
                lines.extend(wrapped);
            }
        }
    }

    let total = lines.len();
    let height = inner.height as usize;
    let max_scroll = total.saturating_sub(height);

    let open = app.open.as_mut().unwrap();
    if open.citation_scroll_pending {
        open.citation_scroll_pending = false;
        if let Some(row) = open
            .selected_citation
            .as_deref()
            .and_then(|sel| ids.iter().position(|id| id.as_deref() == Some(sel)))
        {
            if row < open.inspect_scroll || row >= open.inspect_scroll + height {
                open.inspect_scroll = row.saturating_sub(height / 2);
            }
        }
    }
    let scroll = if total <= height { 0 } else { open.inspect_scroll.min(max_scroll) };

    let visible: Vec<Line> = lines.into_iter().skip(scroll).take(height).collect();
    app.areas.inspector_rows = ids.into_iter().skip(scroll).take(height).collect();
    app.areas.inspector_backlinks = all_backlinks
        .into_iter()
        .filter(|l| l.row >= scroll && l.row < scroll + height)
        .map(|mut l| {
            l.row -= scroll;
            l
        })
        .collect();
    frame.render_widget(Paragraph::new(visible), inner);

    scrollbar(frame, app, area, scroll, total, height);
}

