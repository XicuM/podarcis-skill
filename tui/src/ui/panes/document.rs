//! The document pane: the reader, the editor, and the empty-state screen.

use super::*;

pub fn document(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.open.is_none() {
        app.areas.doc_main = area;
        app.areas.doc_body = Rect::ZERO;
        app.areas.inspector = Rect::ZERO;
        app.areas.inspector_divider = None;
        app.areas.inspector_toggle = None;
        app.areas.doc_edit = None;
        empty_document(frame, app, area);
        return;
    }
    let min_main = 8u16;
    let min_inspect = 4u16;
    let (main, inspect) = if app.show_inspector && area.height >= min_main + min_inspect {
        let max_inspect = area.height.saturating_sub(min_main);
        let height = app.cfg.inspector_height.clamp(min_inspect, max_inspect);
        let [main, inspect] = Layout::vertical([Constraint::Min(min_main), Constraint::Length(height)]).areas(area);
        (main, inspect)
    } else {
        (area, Rect::default())
    };

    app.areas.doc_main = main;
    app.areas.inspector = inspect;
    app.areas.inspector_divider = (!inspect.is_empty()).then_some(inspect.y);
    // The collapse handle: on the divider row while the sources section is
    // open, on the document's own bottom border once it is gone — the same
    // reopen affordance the side panes use on their closed edges.
    app.areas.inspector_toggle = if main.is_empty() {
        None
    } else if !inspect.is_empty() {
        Some(Rect::new(main.x + main.width / 2, inspect.y, 1, 1))
    } else {
        Some(Rect::new(main.x + main.width / 2, main.y + main.height - 1, 1, 1))
    };

    let open = app.open.as_mut().unwrap();
    // 2 columns for borders, 2 for padding; capped so prose on an ultra-wide
    // pane doesn't run edge to edge.
    let width = main.width.saturating_sub(4).min(crate::ui::markdown::MAX_WIDTH);
    open.reflow(width, &app.theme);

    if open.editing() {
        app.areas.doc_body = Rect::ZERO;
        app.areas.nav_back = None;
        app.areas.nav_forward = None;
        app.areas.doc_edit = None;
        editor(frame, app, main);
    } else {
        reader(frame, app, main);
    }
    if !inspect.is_empty() {
        inspector::inspector(frame, app, inspect);
    }
}

/// The app's front door, and the only screen that exists purely to say what
/// this thing is. It gets the mark, the wordmark and the tagline; the key
/// hints come after, because someone who has read them once wants the brand
/// to be the thing they see, not a menu.
pub(super) fn empty_document(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Doc && app.doc_stop == DocStop::Content;
    // Theme is `Copy`; taking it by value here releases the borrow so the nav
    // arrows can record their hit rects on the same `app`.
    let theme = app.theme;
    // The arrows, and no title text: the body carries the wordmark, and
    // printing the name twice on one screen is how a splash starts to look
    // like chrome.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.border(focused))
        .style(Style::default().bg(theme.bg))
        .title(Line::from(nav_arrows(app, area, focused)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() {
        return;
    }

    let keys: [(&str, &str); 5] = [
        ("f", "find a page"),
        ("ctrl+f", "find in the open page"),
        ("/", "search text across the vault"),
        ("ctrl+p", "command palette"),
        ("?", "every key"),
    ];
    // The hint block is laid out as one unit so the keys form a column rather
    // than a ragged left edge.
    let key_w = keys.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    let hint_w = keys.iter().map(|(_, l)| key_w + 3 + l.len()).max().unwrap_or(0);

    let mut lines: Vec<Line> = Vec::new();

    let logo_w = crate::brand::logo_width();
    // The mark is drawn as one block, so its own internal alignment survives:
    // every row gets the same leading pad, not a per-row centring that would
    // shear the drawing.
    let logo_pad = " ".repeat(centre_pad(inner.width as usize, logo_w));
    // Two tones: the animal in the accent, the strokes it sits against quieter
    // behind it, so the lizard reads first.
    let body_style = Style::default().fg(theme.accent);
    let frame_style = theme.faint_style();
    for row in crate::brand::logo_lines() {
        let mut spans = vec![Span::raw(logo_pad.clone())];
        for ch in row.chars() {
            let style = if crate::brand::is_body(ch) { body_style } else { frame_style };
            match spans.last_mut() {
                // Runs of one tone coalesce into a single span rather than one
                // span per cell.
                Some(last) if last.style == style => last.content.to_mut().push(ch),
                _ => spans.push(Span::styled(ch.to_string(), style)),
            }
        }
        lines.push(Line::from(spans));
    }

    let version = app
        .engine_version
        .as_deref()
        .map(|v| format!("{} v{v}", crate::brand::name()))
        .unwrap_or_else(|| crate::brand::name().to_string());
    lines.push(Line::from(""));
    lines.push(centred(
        &version,
        inner.width,
        Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
    ));
    lines.push(centred(
        crate::brand::tagline(),
        inner.width,
        Style::default().fg(theme.subtext),
    ));

    if let Some(oneline) = app.oneline.as_deref() {
        lines.push(Line::from(""));
        lines.push(centred(
            oneline,
            inner.width,
            theme.faint_style().add_modifier(Modifier::ITALIC),
        ));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));
    let pad = " ".repeat(centre_pad(inner.width as usize, hint_w));
    for (key, label) in keys {
        lines.push(Line::from(vec![
            Span::styled(format!("{pad}{key:<key_w$}"), Style::default().fg(theme.accent)),
            Span::styled(format!("   {label}"), theme.dim()),
        ]));
    }

    // Vertically centred, and clipped from the top when the pane is short so
    // the hints — the useful half — are what survives.
    let top = centre_pad(inner.height as usize, lines.len());
    let body = Rect {
        y: inner.y + top as u16,
        height: inner.height.saturating_sub(top as u16),
        ..inner
    };
    let overflow = lines.len().saturating_sub(body.height as usize);
    frame.render_widget(Paragraph::new(lines.split_off(overflow)), body);
}

/// Left pad that centres `width` cells of content in `total`, never negative.
pub(super) fn centre_pad(total: usize, width: usize) -> usize {
    total.saturating_sub(width) / 2
}

/// One centred line of plain text.
pub(super) fn centred(text: &str, width: u16, style: Style) -> Line<'static> {
    use unicode_width::UnicodeWidthStr;
    let pad = " ".repeat(centre_pad(width as usize, text.width()));
    Line::from(Span::styled(format!("{pad}{text}"), style))
}

/// The back/forward arrows a pane title opens with, and the hit rects behind
/// them.
///
/// Shared by the reader and the home screen: home is a history position like
/// any other, so it carries the same affordance — without arrows of its own
/// there was no way to click forward out of it.
pub(super) fn nav_arrows(app: &mut App, area: Rect, focused: bool) -> Vec<Span<'static>> {
    let style = |enabled: bool| {
        if enabled {
            app.theme.title(focused)
        } else {
            app.theme.faint_style()
        }
    };
    let spans = vec![
        Span::styled(" \u{2190} ", style(!app.back.is_empty())),
        Span::styled(" \u{2192} ", style(!app.forward.is_empty())),
    ];
    if area.width >= 8 {
        app.areas.nav_back = Some(Rect::new(area.x + 1, area.y, 3, 1));
        app.areas.nav_forward = Some(Rect::new(area.x + 4, area.y, 3, 1));
    } else {
        app.areas.nav_back = None;
        app.areas.nav_forward = None;
    }
    spans
}

pub(super) fn reader(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Doc && app.doc_stop == DocStop::Content;

    let show_edit = area.width >= 12;
    if show_edit {
        app.areas.doc_edit = Some(Rect::new(area.x + area.width.saturating_sub(4), area.y, 3, 1));
    } else {
        app.areas.doc_edit = None;
    }

    let mut title = nav_arrows(app, area, focused);
    let open = app.open.as_ref().unwrap();
    let spent = 8u16 + if show_edit { 4 } else { 0 };
    if area.width > spent + 12 {
        let budget = (area.width - spent - 4) as usize;
        let name = open
            .page
            .okf
            .title
            .clone()
            .unwrap_or_else(|| open.page.rel.rsplit('/').next().unwrap_or("").to_string());
        let collection = open.page.rel.split('/').next().unwrap_or("");
        let status = open.page.okf.status.clone();
        // The status chip is the first thing dropped when the pane narrows,
        // then the collection: the page's own name is what must survive.
        let chip_w = status.as_deref().map_or(0, |s| s.len() + 3);
        let coll_w = collection.len() + 3;
        let (show_chip, show_coll) = (
            chip_w > 0 && name.len() + chip_w <= budget,
            name.len() + coll_w <= budget,
        );
        let room = budget
            .saturating_sub(if show_chip { chip_w } else { 0 })
            .saturating_sub(if show_coll { coll_w } else { 0 });
        title.push(Span::styled(truncate(&name, room), app.theme.title(focused)));
        if show_coll {
            title.push(Span::styled(format!("  {collection}"), app.theme.faint_style()));
        }
        if let Some(status) = status.filter(|_| show_chip) {
            title.push(Span::styled(
                format!(" {status} "),
                Style::default().fg(app.theme.bg).bg(app.theme.status_of(&status)),
            ));
        }
        title.push(Span::raw(" "));
    }
    let title = Line::from(title);

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(app.theme.border(focused))
        .style(Style::default().bg(app.theme.bg))
        .padding(ratatui::widgets::Padding::horizontal(1))
        .title(title);

    if show_edit {
        block = block.title(
            Line::from(Span::styled(" \u{270e} ", app.theme.title(focused)))
                .alignment(Alignment::Right),
        );
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() {
        app.areas.doc_body = Rect::ZERO;
        return;
    }
    // The find bar eats the bottom row of the reader. `doc_body` stays the text
    // region alone: it is what mouse clicks and scroll arithmetic are measured
    // against, so it must never include chrome.
    let (body, bar) = if app.find.is_some() && inner.height > 1 {
        let [body, bar] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
        (body, Some(bar))
    } else {
        (inner, None)
    };
    // Prose is reflowed to at most `MAX_WIDTH`, so on a wide pane the text used
    // to sit hard against the left border with the surplus columns left empty.
    // Centring the measure is what makes it read as a page instead of a log.
    // `doc_body` moves with it: it is what clicks and scroll maths measure
    // against, so the text region and the hit region must be the same rect.
    // A table is data rather than prose: it takes the whole pane, and is
    // panned when even that is not enough.
    let measure =
        if open.csv() { body.width } else { body.width.min(crate::ui::markdown::MAX_WIDTH) };
    let body = Rect {
        x: body.x + (body.width - measure) / 2,
        width: measure,
        ..body
    };
    app.areas.doc_body = body;

    let finds = app.finds();
    let marks = app.marks_for(&open.page.path);
    let view = body.width as usize;
    // Clamped here rather than trusted: the pane can narrow under a pan that
    // was legal at the old width.
    let hscroll = open.hscroll.min(open.doc.width.saturating_sub(view));
    let mut over = Overlays::new(open.link, open.selection, &finds, marks);
    over.hscroll = hscroll;
    let lines = open.doc.to_lines(&app.theme, &over, open.scroll, body.height as usize);
    frame.render_widget(Paragraph::new(lines), body);
    pan_edges(frame, app, area, body, open.doc.width, hscroll);

    if let Some(bar) = bar {
        find_bar(frame, app, &finds, bar);
    }

    scrollbar(frame, app, area, open.scroll, open.doc.height(), body.height as usize);
}

/// Mark the sides of a page that runs wider than the pane.
///
/// A clipped table says nothing about what it is hiding, which is the whole of
/// the bug this exists for: the arrow says there is more, and for a CSV — whose
/// doc is one table with known column stops — it says how many columns.
pub(super) fn pan_edges(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    body: Rect,
    doc_width: usize,
    hscroll: usize,
) {
    let view = body.width as usize;
    if doc_width <= view || area.height < 2 || area.width < 8 {
        return;
    }
    let open = app.open.as_ref().unwrap();
    let (left, right) = open.doc.hidden_columns(hscroll, view);
    let style = Style::default().fg(app.theme.bg).bg(app.theme.accent).add_modifier(Modifier::BOLD);
    // The chips sit on the bottom border, where the vertical scrollbar sits on
    // the right one: chrome belongs in the frame, not over the text.
    let row = area.y + area.height - 1;
    if hscroll > 0 {
        let tag = count_tag("◂", left, true);
        let w = tag.chars().count() as u16;
        frame.render_widget(
            Paragraph::new(Span::styled(tag, style)),
            Rect::new(area.x + 1, row, w, 1),
        );
    }
    if hscroll + view < doc_width {
        let tag = count_tag("▸", right, false);
        let w = tag.chars().count() as u16;
        frame.render_widget(
            Paragraph::new(Span::styled(tag, style)),
            Rect::new(area.x + area.width.saturating_sub(w + 1), row, w, 1),
        );
    }
}

/// ` ◂ 4 ` when the doc knows its columns, a bare ` ◂ ` when it does not — a
/// markdown page can hold several tables, so there is no column count to give.
pub(super) fn count_tag(arrow: &str, columns: usize, leading: bool) -> String {
    match (columns, leading) {
        (0, _) => format!(" {arrow} "),
        (n, true) => format!(" {arrow} {n} "),
        (n, false) => format!(" {n} {arrow} "),
    }
}

/// The in-page find input: query, hit counter, and the two keys that move.
pub(super) fn find_bar(frame: &mut Frame, app: &App, finds: &Finds, area: Rect) {
    let Some(find) = app.find.as_ref() else { return };
    let theme = &app.theme;
    let count = if find.query.is_empty() {
        String::new()
    } else if finds.hits.is_empty() {
        "  no matches".to_string()
    } else {
        format!("  {}/{}", finds.current + 1, finds.hits.len())
    };
    let line = Line::from(vec![
        Span::styled("⌕ ", Style::default().fg(theme.accent)),
        Span::styled(find.query.clone(), Style::default().fg(theme.text)),
        Span::styled("▌", Style::default().fg(theme.accent)),
        Span::styled(count, Style::default().fg(theme.warn)),
        Span::styled("   enter next · shift+enter previous · esc close", theme.faint_style()),
    ]);
    frame.render_widget(Paragraph::new(line).style(Style::default().bg(theme.surface)), area);
}


pub(super) fn editor(frame: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme;
    let root = app.cfg.root.clone();
    let Some(open) = app.open.as_mut() else { return };
    let title = crate::vault::page::rel_path(&open.page.path, &root);
    let dirty = open.editor.as_ref().is_some_and(crate::editor::Editor::dirty);
    let label = if dirty { format!("{title}  ⏺") } else { title };
    let block = pane(&theme, &label, true);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(ed) = open.editor.as_mut() else { return };
    // Mirrors `EditorView::line_number_width` (`LineNumbers::Absolute`), so
    // wrapped-line cursor movement (`Editor::move_visual`) sees the same
    // content width the view itself wraps at.
    let gutter = (ed.state.lines.len().max(1).to_string().len() + 1) as u16;
    ed.render_width = inner.width.saturating_sub(gutter) as usize;
    ed.refresh_syntax(&theme);
    let editor_theme = EditorTheme::default()
        .base(Style::default().fg(theme.text).bg(theme.bg))
        .cursor_style(Style::default().fg(theme.bg).bg(theme.accent))
        .selection_style(Style::default().fg(theme.bg).bg(theme.link))
        .line_numbers_style(Style::default().fg(theme.faint))
        .status_line(
            EditorStatusLine::default()
                .style_mode(Style::default().fg(theme.bg).bg(theme.accent).add_modifier(Modifier::BOLD))
                .style_line(Style::default().fg(theme.subtext).bg(theme.surface)),
        );
    frame.render_widget(
        EditorView::new(&mut ed.state)
            .theme(editor_theme)
            .wrap(true)
            .line_numbers(LineNumbers::Absolute),
        inner,
    );

    completion_popup(frame, app, inner);
}

pub(super) fn completion_popup(frame: &mut Frame, app: &App, area: Rect) {
    let Some(editor) = app.open.as_ref().and_then(|o| o.editor.as_ref()) else { return };
    let Some(completion) = editor.completion.as_ref() else { return };

    let rows = completion.candidates.len().min(7) as u16;
    let width = completion
        .candidates
        .iter()
        .map(|c| c.value.len() + c.note.len() + 6)
        .max()
        .unwrap_or(20)
        .clamp(24, area.width.saturating_sub(4).max(24) as usize) as u16;

    let cursor_row = editor.state.cursor.row;
    let offset = editor.state.viewport_offset().1;
    let y = area.y + (cursor_row.saturating_sub(offset) as u16).min(area.height.saturating_sub(1)) + 1;
    // Flip above the cursor when there is no room below.
    let y = if y + rows + 2 > area.y + area.height { y.saturating_sub(rows + 3) } else { y };
    let x = (area.x + 6).min(area.x + area.width.saturating_sub(width));
    let popup = Rect { x, y, width, height: rows + 2 }.intersection(area);
    if popup.is_empty() {
        return;
    }

    let label = match &completion.what {
        crate::editor::Completing::Footnote { .. } => "sources",
        crate::editor::Completing::Link { .. } => "pages",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(app.theme.link))
        .title(Span::styled(format!(" {label} "), Style::default().fg(app.theme.link)))
        .style(Style::default().bg(app.theme.overlay));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);

    let start = scroll_offset(completion.selected, completion.candidates.len(), inner.height as usize);
    let lines: Vec<Line> = completion
        .candidates
        .iter()
        .enumerate()
        .skip(start)
        .take(inner.height as usize)
        .map(|(i, candidate)| {
            let selected = i == completion.selected;
            let style = if selected {
                app.theme.selection(true)
            } else {
                Style::default().fg(app.theme.text)
            };
            Line::from(vec![
                Span::styled(format!(" {} ", candidate.value), style),
                Span::styled(candidate.note.clone(), app.theme.faint_style()),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

