//! Chrome around the three panes: the tab bar, pane toggles, the agent
//! sidebar, and the status line.

use super::*;

// -------------------------------------------------------------- collapsers

/// The collapse/expand handles on the tree and sidebar borders — the only
/// visual sign that either pane can be tucked away, and the only way back
/// in once it has been. Each glyph points the way the pane would travel:
/// outward, into the pane, to collapse it; inward, into the document, to
/// pull a collapsed one back.
/// The narrow-terminal tab strip standing in for the side-by-side panes.
pub fn tab_bar(frame: &mut Frame, app: &App) {
    for (focus, label, rect) in &app.areas.tab_bar {
        let focused = *focus == app.focus;
        let style = if focused {
            Style::default().fg(app.theme.bg).bg(app.theme.accent).add_modifier(Modifier::BOLD)
        } else {
            app.theme.faint_style()
        };
        let text = format!("{label:^width$}", width = rect.width as usize);
        frame.render_widget(Paragraph::new(Span::styled(text, style)), *rect);
    }
}

pub fn toggles(frame: &mut Frame, app: &App) {
    let style = Style::default().fg(app.theme.accent).add_modifier(Modifier::BOLD);
    if let Some(rect) = app.areas.tree_toggle {
        let glyph = if app.show_tree { "◂" } else { "▸" };
        frame.render_widget(Paragraph::new(Span::styled(glyph, style)), rect);
    }
    if let Some(rect) = app.areas.sidebar_toggle {
        let glyph = if app.show_sidebar { "▸" } else { "◂" };
        frame.render_widget(Paragraph::new(Span::styled(glyph, style)), rect);
    }
    if let Some(rect) = app.areas.inspector_toggle {
        let glyph = if app.show_inspector { "▾" } else { "▴" };
        frame.render_widget(Paragraph::new(Span::styled(glyph, style)), rect);
    }
}

// ---------------------------------------------------------------- sidebar

pub fn sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Sidebar;
    let mut block = pane(&app.theme, "agent", focused);
    if focused && area.width >= 28 {
        block = block.title(
            Line::from(Span::styled(" f12 to unfocus ", app.theme.dim()))
                .alignment(Alignment::Right),
        );
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() {
        return;
    }

    if let Some(pane) = app.sidebar.as_ref() {
        let drawn = pane.with_screen(|screen| {
            let widget = PseudoTerminal::new(screen);
            frame.render_widget(widget, inner);
        });
        if drawn.is_some() {
            return;
        }
    }

    let message = app
        .sidebar_error
        .clone()
        .unwrap_or_else(|| "starting the agent session…".to_string());
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(format!(" {message}"), Style::default().fg(app.theme.warn))),
        Line::from(""),
        Line::from(Span::styled(" ctrl+g hides this pane", app.theme.faint_style())),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

// ----------------------------------------------------------------- status

/// The bottom bar.
///
/// Everything here is `subtext`, never `faint`: the bar sits on `surface` and
/// `faint` is the token for inactive borders and gutters, so using it for text
/// on a raised background left the whole bar close to unreadable. The splash
/// keeps the one-liner; the bar does not, because a line of flavour text is
/// not status and it crowded out the things that are.
pub fn status(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;
    let mut left: Vec<Span> = Vec::new();

    // When editing, show the active edit state (green or amber if dirty).
    // In reading mode, render the project selector badge instead of "READ".
    let (badge_text, badge_style) = match app.open.as_ref().and_then(|o| o.editor.as_ref()) {
        Some(editor) if editor.dirty() => (" EDIT ⏺ ".to_string(), Style::default().fg(theme.bg).bg(theme.warn)),
        Some(_) => (" EDIT ".to_string(), Style::default().fg(theme.bg).bg(theme.ok)),
        None => (
            format!(" {} ▾ ", app.project_name),
            // The theme's own contrast pole, not a hardcoded white: this was
            // the one widget in the app naming a colour the palette did not
            // hand it, so a light flavour got white-on-light here.
            Style::default().fg(theme.on_accent()).bg(theme.accent),
        ),
    };
    left.push(Span::styled(badge_text, badge_style.add_modifier(Modifier::BOLD)));

    if let Some(prefix) = app.pending {
        left.push(Span::styled(format!(" {prefix}…"), Style::default().fg(theme.literal)));
    }

    if app.indexing {
        left.push(Span::styled(format!("  {} indexing…", app.spinner()), Style::default().fg(theme.accent)));
    } else {
        left.push(Span::styled(
            format!(
                "  {} skills  {} agents  {} mcp",
                app.components.skills.len(),
                app.components.agents.len(),
                app.components.mcp.len()
            ),
            theme.dim(),
        ));
        let pages = app.index.entries.len();
        left.push(Span::styled(
            format!("  {pages} page{}", if pages == 1 { "" } else { "s" }),
            theme.dim(),
        ));

        // Vault health. The counts above never move while you work; this is
        // the number that does, and it was the one thing you had to open the
        // lint overlay to see.
        let (errors, warns) = app.index.finding_counts();
        if errors > 0 {
            left.push(Span::styled(format!("  ● {errors}"), Style::default().fg(theme.err)));
        }
        if warns > 0 {
            left.push(Span::styled(format!("  ▲ {warns}"), Style::default().fg(theme.warn)));
        }
        if errors == 0 && warns == 0 && !app.index.entries.is_empty() {
            left.push(Span::styled("  ✓ clean", Style::default().fg(theme.ok)));
        }
    }

    for job in app.jobs.labels() {
        left.push(Span::styled(
            format!("  {} {job}", app.spinner()),
            Style::default().fg(theme.link),
        ));
    }

    if let Some(editor) = app.open.as_ref().and_then(|o| o.editor.as_ref()) {
        left.push(Span::styled(
            format!("  {}:{}", editor.cursor_line() + 1, editor.cursor_col() + 1),
            theme.dim(),
        ));
        left.push(Span::styled("  ctrl+s save  esc leave", theme.dim()));
    } else if let Some(open) = app.open.as_ref() {
        // How far through the page you are. The scrollbar shows it as a
        // position; this says it as a number, which is what you want when you
        // are deciding whether to keep reading.
        let height = app.areas.doc_body.height as usize;
        let total = open.doc.height();
        if total > height && height > 0 {
            let pct = ((open.scroll + height) * 100 / total).min(100);
            left.push(Span::styled(format!("  {pct}%"), theme.dim()));
        }
    }

    let mut right = Vec::new();
    if let Some(version) = app.engine_version.as_deref() {
        right.push(Span::styled(format!("v{version}  "), theme.dim()));
    }
    // The flavour's own name. This used to be hardcoded to "catppuccin", so
    // picking Nord put someone else's brand in the one corner where the app
    // names itself.
    right.push(Span::styled(
        format!("{}  ", theme.flavor.display_name()),
        theme.dim(),
    ));

    let bar = Style::default().bg(theme.surface).fg(theme.text);
    frame.render_widget(Block::default().style(bar), area);

    let right_line = Line::from(right);
    let right_width = right_line.width() as u16;
    let right_area = Rect::new(
        area.x + area.width.saturating_sub(right_width),
        area.y,
        right_width.min(area.width),
        area.height,
    );
    let left_width = area.width.saturating_sub(right_width);
    let left_area = Rect::new(area.x, area.y, left_width, area.height);

    frame.render_widget(Paragraph::new(Line::from(left)), left_area);
    frame.render_widget(Paragraph::new(right_line), right_area);
}

