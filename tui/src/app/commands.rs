//! The `Cmd` dispatcher — one arm per behaviour the app has.
//!
//! Split out of `app.rs` verbatim; inherent `impl App` blocks merge
//! across the crate, so this is the same type seen from another file.

use super::*;

impl App {
    /// Whether leaving the app should be held back for one press.
    ///
    /// A sync mid-flight and an unsaved buffer are both things not to discard
    /// by reflex; say so once, then take the second press as meaning it. The
    /// editor already guards `esc` this way — quitting is the same loss by a
    /// different key, so it asks the same question.
    fn exit_blocked(&mut self, again: &str) -> bool {
        if self.quit_confirmed {
            return false;
        }
        let warning = if self.jobs.is_busy() {
            let running = self.jobs.labels().join(", ");
            Some(format!("still running: {running} — {again} to quit"))
        } else if self.open.as_ref().and_then(|o| o.editor.as_ref()).is_some_and(Editor::dirty) {
            Some(format!("unsaved changes — ctrl+s to save, {again} to discard"))
        } else {
            None
        };
        match warning {
            Some(message) => {
                self.quit_confirmed = true;
                self.toast(Level::Warn, message);
                true
            }
            None => false,
        }
    }

    pub fn run(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::ClearMarks => {
                let count: usize = self.marks.values().map(Vec::len).sum();
                self.marks.clear();
                self.toast(Level::Info, format!("cleared {count} agent mark(s)"));
            }
            Cmd::Quit => {
                if !self.exit_blocked("press ctrl+q again") {
                    self.quit = true;
                }
            }
            Cmd::Help => self.overlay = Some(Overlay::Help { scroll: 0 }),
            Cmd::Palette => self.open_palette(),
            Cmd::Restart => {
                // A restart is a quit with a re-exec on the way out, so it
                // carries the same guard — except for the unsaved buffer.
                // Restart is reachable only from the palette, and opening the
                // palette resets the confirmation, so "choose it again" is a
                // prompt that can never be satisfied. An instruction can be.
                if self.open.as_ref().and_then(|o| o.editor.as_ref()).is_some_and(Editor::dirty) {
                    self.toast(
                        Level::Warn,
                        "unsaved changes — ctrl+s to save or esc to discard, then restart",
                    );
                } else if !self.exit_blocked("choose restart again") {
                    self.restart = true;
                    self.quit = true;
                }
            }
            Cmd::Theme => {
                let selected = Flavor::ALL.iter().position(|f| *f == self.cfg.flavor).unwrap_or(0);
                self.overlay = Some(Overlay::Themes { selected, original: self.cfg.flavor });
            }
            Cmd::Projects => self.open_projects(),

            Cmd::FindFiles => self.open_finder(search::Mode::Files),
            Cmd::FindText => self.open_finder(search::Mode::Text),
            Cmd::FindSemantic => self.open_finder(search::Mode::Semantic),
            Cmd::FindInPage => self.open_find(),

            Cmd::FocusTree => self.set_focus(Focus::Tree),
            Cmd::FocusDoc => self.set_focus(Focus::Doc),
            Cmd::FocusSidebar => self.set_focus(Focus::Sidebar),
            Cmd::CycleFocus => self.cycle_focus(1),
            Cmd::CycleFocusBack => self.cycle_focus(-1),
            Cmd::ToggleTree => {
                self.show_tree = !self.show_tree;
                if !self.show_tree && self.focus == Focus::Tree {
                    self.focus = Focus::Doc;
                }
            }
            Cmd::ToggleSidebar => {
                self.show_sidebar = !self.show_sidebar;
                if self.show_sidebar {
                    // Showing it again is also how you retry after a failure.
                    self.sidebar_error = None;
                } else if self.focus == Focus::Sidebar {
                    self.focus = Focus::Doc;
                }
            }
            Cmd::ToggleInspector => {
                self.show_inspector = !self.show_inspector;
                if !self.show_inspector {
                    self.doc_stop = DocStop::Content;
                }
            }
            Cmd::ShrinkInspector => self.adjust_inspector_height(-2),
            Cmd::GrowInspector => self.adjust_inspector_height(2),
            Cmd::ZoomPane => self.zoom = !self.zoom,
            Cmd::ShrinkPane => self.shrink_active_pane(),
            Cmd::WidenPane => self.widen_active_pane(),
            Cmd::ShrinkTree => self.adjust_tree_width(-4),
            Cmd::WidenTree => self.adjust_tree_width(4),
            Cmd::ShrinkSidebar => self.adjust_sidebar_width(-4),
            Cmd::WidenSidebar => self.adjust_sidebar_width(4),
            Cmd::LeaveSidebar => self.cycle_focus(1),

            Cmd::Back => self.go_back(),
            Cmd::Forward => self.go_forward(),
            Cmd::Home => self.go_home(),
            Cmd::ToggleCollection => self.toggle_collection(),
            Cmd::RevealInTree => self.reveal_open(),
            Cmd::NextFinding => self.jump_finding(1),
            Cmd::PrevFinding => self.jump_finding(-1),

            Cmd::TreeDown => self.tree.move_by(1),
            Cmd::TreeUp => self.tree.move_by(-1),
            Cmd::TreeExpand => {
                if let Some(path) = self.tree.expand() {
                    self.open_path(&path.clone(), true);
                }
            }
            Cmd::TreeCollapse => self.tree.collapse(),
            Cmd::TreeToggle => {
                if let Some(path) = self.tree.toggle() {
                    self.open_path(&path.clone(), true);
                }
            }
            Cmd::TreeNextSibling => self.tree.move_sibling(true),
            Cmd::TreePrevSibling => self.tree.move_sibling(false),
            Cmd::TreeCollapseAll => self.tree.collapse_all(),
            Cmd::TreeTop => self.tree.move_to(0),
            Cmd::TreeBottom => self.tree.move_to(usize::MAX),

            Cmd::ScrollDown => self.scroll(1),
            Cmd::ScrollUp => self.scroll(-1),
            Cmd::HalfPageDown => self.scroll(self.page_step() / 2),
            Cmd::HalfPageUp => self.scroll(-(self.page_step() / 2)),
            Cmd::PageDown => self.scroll(self.page_step()),
            Cmd::PageUp => self.scroll(-self.page_step()),
            Cmd::DocTop => self.scroll(isize::MIN / 2),
            Cmd::DocBottom => self.scroll(isize::MAX / 2),
            Cmd::PanLeft => self.pan(-Self::PAN_STEP),
            Cmd::PanRight => self.pan(Self::PAN_STEP),
            Cmd::NextLink => self.move_link(true),
            Cmd::PrevLink => self.move_link(false),
            Cmd::FollowLink => self.follow_link(),
            Cmd::Outline => {
                if self.open.as_ref().is_some_and(|o| !o.doc.headings.is_empty()) {
                    self.overlay = Some(Overlay::Outline { selected: 0 });
                }
            }
            Cmd::Copy => self.copy_selection_or_page(),
            Cmd::ClearSelection => self.clear_selection(),

            Cmd::Edit => self.enter_editor(),
            Cmd::Save => self.save(),
            Cmd::LeaveEdit => self.leave_editor(),
            Cmd::Bold => self.with_editor(|editor| editor.wrap_emphasis("**")),
            Cmd::Italic => self.with_editor(|editor| editor.wrap_emphasis("_")),
            Cmd::Link => {
                self.with_editor(crate::editor::Editor::insert_link);
                self.refresh_completion();
            }
            Cmd::Undo => self.with_editor(crate::editor::Editor::undo),
            Cmd::Redo => self.with_editor(crate::editor::Editor::redo),

            Cmd::Lint => self.run_lint(),
            Cmd::SyncRepos => self.run_sync_repos(),
            Cmd::Commit => self.prompt(PromptKind::Commit, "commit message"),
            Cmd::NewPage => self.prompt(PromptKind::NewPage, "new page path (relative to the checkout)"),
            Cmd::Uncited => self.show_uncited(),
            Cmd::Extensions => self.open_extensions(),
        }
    }

    /// Tab-bar click: unlike `set_focus`, this may reopen a pane the user had
    /// hidden — clicking a tab is asking for that pane, not just its focus.
    pub(super) fn select_tab(&mut self, focus: Focus) {
        match focus {
            Focus::Tree => self.show_tree = true,
            Focus::Sidebar => {
                self.show_sidebar = true;
                self.sidebar_error = None;
            }
            Focus::Doc => {}
        }
        self.set_focus(focus);
    }

    pub(super) fn set_focus(&mut self, focus: Focus) {
        let next = match focus {
            Focus::Tree if !self.show_tree => Focus::Doc,
            Focus::Sidebar if !self.show_sidebar || self.sidebar.is_none() => self.focus,
            other => other,
        };
        if next != self.focus {
            if self.focus == Focus::Sidebar {
                if let Some(pane) = self.sidebar.as_mut() {
                    pane.send(b"\x1b[O");
                }
            }
            if next == Focus::Sidebar {
                if let Some(pane) = self.sidebar.as_mut() {
                    pane.send(b"\x1b[I");
                }
            }
            self.focus = next;
        }
        if next == Focus::Doc {
            // A direct jump (a number key, a tab-bar click, opening a page)
            // always means the reader, never the sources box a previous
            // Tab-cycle happened to leave selected.
            self.doc_stop = DocStop::Content;
        }
        self.zoom = false;
    }

    /// Every Tab-cycle stop currently on screen, in order: each tree
    /// collection box, the reader, the sources box (only once it is actually
    /// rendered — the layout can collapse it on a short terminal even with
    /// `show_inspector` set), then the agents pane.
    fn stops(&self) -> Vec<Stop> {
        let mut out = Vec::new();
        if self.show_tree {
            out.extend((0..self.tree.collection_paths().len()).map(Stop::TreeSection));
        }
        out.push(Stop::DocContent);
        if self.open.is_some() && self.show_inspector && !self.areas.inspector.is_empty() {
            out.push(Stop::DocSources);
        }
        if self.show_sidebar && self.sidebar.is_some() {
            out.push(Stop::Sidebar);
        }
        out
    }

    fn current_stop(&self) -> Stop {
        match self.focus {
            Focus::Tree => {
                let i = (0..self.tree.collection_paths().len())
                    .find(|&i| {
                        self.tree
                            .section_span(i)
                            .is_some_and(|(start, end)| self.tree.selected >= start && self.tree.selected < end)
                    })
                    .unwrap_or(0);
                Stop::TreeSection(i)
            }
            Focus::Sidebar => Stop::Sidebar,
            Focus::Doc => match self.doc_stop {
                DocStop::Content => Stop::DocContent,
                DocStop::Sources => Stop::DocSources,
            },
        }
    }

    fn goto_stop(&mut self, stop: Stop) {
        match stop {
            Stop::TreeSection(i) => {
                self.focus = Focus::Tree;
                if let Some((start, end)) = self.tree.section_span(i) {
                    if !(self.tree.selected >= start && self.tree.selected < end) {
                        self.tree.selected = start;
                    }
                }
            }
            Stop::DocContent => {
                self.focus = Focus::Doc;
                self.doc_stop = DocStop::Content;
            }
            Stop::DocSources => {
                self.focus = Focus::Doc;
                self.doc_stop = DocStop::Sources;
            }
            Stop::Sidebar => self.focus = Focus::Sidebar,
        }
    }

    fn cycle_focus(&mut self, delta: isize) {
        let stops = self.stops();
        if stops.is_empty() {
            return;
        }
        let cur = self.current_stop();
        let at = stops.iter().position(|s| *s == cur).unwrap_or(0) as isize;
        let n = stops.len() as isize;
        let next = stops[(((at + delta) % n + n) % n) as usize];
        self.goto_stop(next);
    }

    /// Rows of document the reader can actually show. The sources pane sits
    /// *below* the reader inside the same column, so measuring the column
    /// would count its rows as places content could scroll into — and the
    /// last screenful would stay unreachable, reading as if the pane were
    /// drawn over the end of the page.
    fn viewport_height(&self) -> usize {
        let rows = if self.areas.doc_body.height > 0 {
            self.areas.doc_body.height
        } else if !self.areas.doc_main.is_empty() {
            self.areas.doc_main.height.saturating_sub(2)
        } else {
            self.areas.doc.height.saturating_sub(2)
        };
        rows.max(1) as usize
    }

    fn page_step(&self) -> isize {
        self.viewport_height() as isize
    }

    /// Columns a single pan moves. Wide enough to make progress across a broad
    /// table, narrow enough that you can still line a column up under the edge.
    pub(super) const PAN_STEP: isize = 8;

    /// Pan a table wider than the pane. Clamped to the doc's own width, so the
    /// right edge of the box is as far as it goes and a prose page — never
    /// wider than its measure — cannot be panned at all.
    pub(super) fn pan(&mut self, delta: isize) {
        let view = self.viewport_width();
        if let Some(open) = self.open.as_mut() {
            let last = open.doc.width.saturating_sub(view);
            open.hscroll = (open.hscroll as isize + delta).clamp(0, last as isize) as usize;
        }
    }

    pub(crate) fn viewport_width(&self) -> usize {
        let cols = if self.areas.doc_body.width > 0 {
            self.areas.doc_body.width
        } else {
            self.areas.doc.width.saturating_sub(4)
        };
        cols.max(1) as usize
    }

    pub(super) fn scroll(&mut self, delta: isize) {
        if self.focus == Focus::Doc && self.doc_stop == DocStop::Sources {
            // The inspector clamps this against its own row count at render
            // time, so an out-of-range value here is harmless.
            if let Some(open) = self.open.as_mut() {
                open.inspect_scroll = (open.inspect_scroll as isize + delta).max(0) as usize;
            }
            return;
        }
        let height = self.viewport_height();
        if let Some(open) = self.open.as_mut() {
            let last = open.doc.height().saturating_sub(height.min(open.doc.height()));
            open.scroll = (open.scroll as isize + delta).clamp(0, last as isize) as usize;
        }
    }

    fn move_link(&mut self, forward: bool) {
        let height = self.viewport_height();
        if let Some(open) = self.open.as_mut() {
            open.link = open.doc.link_after(open.link, forward);
            if let Some(link) = open.link.and_then(|i| open.doc.links.get(i)) {
                // Keep the highlighted link on screen.
                if link.line < open.scroll || link.line >= open.scroll + height {
                    open.scroll = link.line.saturating_sub(height / 3);
                }
                if link.kind == LinkKind::Footnote {
                    open.selected_citation = Some(link.target.clone());
                    open.citation_scroll_pending = true;
                } else {
                    open.selected_citation = None;
                }
            } else {
                open.selected_citation = None;
            }
        }
    }

    fn follow_link(&mut self) {
        let Some(open) = self.open.as_ref() else { return };
        let Some(link) = open.link.and_then(|i| open.doc.links.get(i)).cloned() else {
            self.toast(Level::Info, "no link selected — use l / h to pick one");
            return;
        };
        match link.kind {
            LinkKind::Relative => {
                let dir = open.page.path.parent().unwrap_or(&self.cfg.root).to_path_buf();
                let bare = link.target.split(['#', '?']).next().unwrap_or(&link.target);
                let target = crate::vault::links::normalize(&dir.join(bare));
                if target.starts_with(&self.cfg.root) && target.exists() {
                    self.open_path(&target, true);
                    self.reveal_open();
                } else {
                    self.toast(Level::Bad, format!("broken link: {}", link.target));
                }
            }
            LinkKind::External => self.toast(Level::Info, format!("external: {}", link.target)),
            LinkKind::Anchor => self.jump_to_anchor(&link.target),
            LinkKind::Wiki => self.toast(Level::Warn, "wikilinks are forbidden — use a relative link"),
            LinkKind::Footnote => self.select_citation(&link.target),
        }
    }

    /// Highlight a citation in the inspector's sources section — from a click
    /// or keyboard nav on its `[n]` mark in the body — and queue it to be
    /// scrolled into view on the next inspector render.
    fn select_citation(&mut self, id: &str) {
        self.show_inspector = true;
        if let Some(open) = self.open.as_mut() {
            open.selected_citation = Some(id.to_string());
            open.citation_scroll_pending = true;
        }
    }

    /// The mirror of `select_citation`: a source picked in the inspector's
    /// sources section highlights the `[n]` mark that cites it in the body and
    /// scrolls it into view.
    pub(super) fn select_source(&mut self, id: &str) {
        self.show_inspector = true;
        let Some(open) = self.open.as_mut() else { return };
        open.selected_citation = Some(id.to_string());
        open.citation_scroll_pending = true;
        if let Some(link) = open.doc.citation_link(id) {
            open.link = Some(link);
            let line = open.doc.links[link].line;
            let height = self.areas.doc_body.height.saturating_sub(1).max(1) as usize;
            if line < open.scroll || line >= open.scroll + height {
                open.scroll = line.saturating_sub(height / 3);
            }
        }
    }

    /// Open a source's own file (its `resource:` path) in the main document
    /// pane — the reader is the only "visualizer" this app has, so opening a
    /// source means reading it the same way as any wiki page.
    pub fn open_source(&mut self, id: &str) {
        let Some(open) = self.open.as_ref() else { return };
        // A frontmatter `sources:` entry names its own `resource:` path; a
        // footnote-only citation carries its link target on the definition line.
        let dir = open.page.path.parent().unwrap_or(&self.cfg.root).to_path_buf();
        let resource = open
            .page
            .okf
            .sources
            .iter()
            .find(|s| s.id == id)
            .and_then(|s| s.resource.as_deref().map(str::to_string))
            .or_else(|| {
                open.page
                    .footnote_defs()
                    .into_iter()
                    .find(|(fid, _, _)| fid == id)
                    .and_then(|(_, _, target)| target)
            });
        let Some(resource) = resource else {
            self.toast(Level::Info, format!("{id} has no resource path"));
            return;
        };
        let target = crate::vault::links::normalize(&dir.join(&resource));
        if target.starts_with(&self.cfg.root) && target.exists() {
            self.open_path(&target, true);
        } else {
            self.toast(Level::Bad, format!("broken source: {resource}"));
        }
    }

    pub fn reader_body(&self) -> Rect {
        if !self.areas.doc_body.is_empty() {
            return self.areas.doc_body;
        }
        let area = self.areas.doc;
        if area.is_empty() {
            return Rect::default();
        }
        let min_main = 8u16;
        let min_inspect = 4u16;
        let main = if self.show_inspector && area.height >= min_main + min_inspect {
            let max_inspect = area.height.saturating_sub(min_main);
            let height = self.cfg.inspector_height.clamp(min_inspect, max_inspect);
            let [main, _] = Layout::vertical([Constraint::Min(min_main), Constraint::Length(height)]).areas(area);
            main
        } else {
            area
        };
        let inner_x = main.x.saturating_add(2);
        let inner_y = main.y.saturating_add(1);
        let inner_w = main.width.saturating_sub(4);
        let inner_h = main.height.saturating_sub(2);
        Rect::new(
            inner_x,
            inner_y.saturating_add(2),
            inner_w,
            inner_h.saturating_sub(2),
        )
    }

    pub fn char_pos_at(&self, x: u16, y: u16) -> Option<markdown::TextPos> {
        let open = self.open.as_ref()?;
        if open.doc.lines.is_empty() {
            return None;
        }
        let body = self.reader_body();
        if body.is_empty() || body.height == 0 {
            return None;
        }

        let rel_y = if y < body.y {
            0
        } else if y >= body.y + body.height {
            body.height.saturating_sub(1) as usize
        } else {
            (y - body.y) as usize
        };

        let line_idx = (open.scroll + rel_y).min(open.doc.lines.len().saturating_sub(1));
        let line = &open.doc.lines[line_idx];
        let plain = line.plain_text();

        // The pan moved the text under the pointer, so the clicked cell is
        // that many columns further into the line than the pane suggests.
        let rel_x = (x as isize - body.x as isize).max(0) as usize + open.hscroll;
        let mut cur_col = 0usize;
        let mut char_idx = 0usize;
        for c in plain.chars() {
            let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
            if cur_col + w > rel_x {
                break;
            }
            cur_col += w;
            char_idx += 1;
        }

        Some(markdown::TextPos { line: line_idx, col: char_idx })
    }

    pub fn start_selection(&mut self, x: u16, y: u16) {
        if let Some(pos) = self.char_pos_at(x, y) {
            self.selecting_text = true;
            if let Some(open) = self.open.as_mut() {
                open.selection = Some(markdown::Selection::new(pos, pos));
            }
        }
    }

    pub fn extend_selection(&mut self, x: u16, y: u16) {
        let body = self.reader_body();
        if !body.is_empty() {
            if y < body.y {
                self.scroll(-1);
            } else if y >= body.y + body.height {
                self.scroll(1);
            }
        }

        if let Some(pos) = self.char_pos_at(x, y) {
            if let Some(open) = self.open.as_mut() {
                if let Some(sel) = open.selection.as_mut() {
                    sel.cursor = pos;
                }
            }
        }
    }

    pub fn finish_selection(&mut self, x: u16, y: u16) {
        self.selecting_text = false;
        let click_pos = self.char_pos_at(x, y);
        if let Some(pos) = click_pos {
            if let Some(open) = self.open.as_mut() {
                if let Some(sel) = open.selection.as_mut() {
                    sel.cursor = pos;
                }
            }
        }
        let Some(open) = self.open.as_mut() else { return };
        let Some(sel) = open.selection else { return };
        if sel.is_empty() {
            open.selection = None;
            // A click without dragging on a link activates and follows that link
            let mut clicked_link = None;
            if let Some(pos) = click_pos {
                if let Some(line) = open.doc.lines.get(pos.line) {
                    let mut cur = 0;
                    for seg in &line.segs {
                        let len = seg.text.chars().count();
                        if pos.col >= cur && pos.col < cur + len {
                            if let Some(link_idx) = seg.link {
                                open.link = Some(link_idx);
                                clicked_link = Some(link_idx);
                            }
                            break;
                        }
                        cur += len;
                    }
                }
            }
            if clicked_link.is_some() {
                self.follow_link();
            }
            return;
        }
        let text = open.selected_source_text(sel);
        if !text.is_empty() {
            crate::clipboard::copy(&text);
            let n = text.lines().count().max(1);
            let label = if n == 1 { "copied 1 line".to_string() } else { format!("copied {n} lines") };
            self.toast(Level::Good, label);
        } else {
            open.selection = None;
        }
    }

    pub fn clear_selection(&mut self) {
        if let Some(open) = self.open.as_mut() {
            open.selection = None;
        }
    }

    /// Right-button release ends the drag the same way as a left one, but the
    /// selection is paid to the agents pane as a `@path:lines` mention rather
    /// than copied to the clipboard. A click without a drag selects nothing and
    /// sends nothing — and never follows a link, which is a left-button habit.
    pub fn finish_selection_right(&mut self, x: u16, y: u16) {
        self.selecting_text = false;
        self.selecting_right = false;
        if let Some(pos) = self.char_pos_at(x, y) {
            if let Some(open) = self.open.as_mut() {
                if let Some(sel) = open.selection.as_mut() {
                    sel.cursor = pos;
                }
            }
        }
        let Some(mention) = self.selection_mention() else {
            self.clear_selection();
            return;
        };
        self.send_to_agent(&mention);
    }

    /// The `@<rel>:<first>-<last>` mention for the current selection, if there
    /// is a non-empty one. Line numbers are the file's real 1-based numbers:
    /// `source_for_line` indexes into the body, which starts at `body_start`.
    fn selection_mention(&self) -> Option<String> {
        let open = self.open.as_ref()?;
        let sel = open.selection.filter(|s| !s.is_empty())?;
        let (start, end) = sel.range();
        let first = open.page.body_start + open.doc.source_for_line(start.line) + 1;
        let last = open.page.body_start + open.doc.source_for_line(end.line) + 1;
        let range = if first == last { format!("{first}") } else { format!("{first}-{last}") };
        Some(format!("@{}:{range}", open.page.rel))
    }

    /// Type `text` into the herdr pane and press enter, so the mention arrives
    /// in the agent session exactly as if the user had typed and submitted it.
    fn send_to_agent(&mut self, text: &str) {
        let Some(pane) = self.sidebar.as_mut() else {
            self.toast(Level::Warn, "no agent session — open the agents pane (ctrl+g)");
            return;
        };
        if !pane.is_alive() {
            self.toast(Level::Warn, "the agent session has exited — ctrl+g to restart");
            return;
        }
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(b'\r');
        pane.send(&bytes);
        self.toast(Level::Good, "sent selection to the agent");
    }

    pub fn copy_selection_or_page(&mut self) {
        let Some(open) = self.open.as_ref() else { return };
        if let Some(sel) = open.selection {
            if !sel.is_empty() {
                let text = open.selected_source_text(sel);
                if !text.is_empty() {
                    crate::clipboard::copy(&text);
                    let n = text.lines().count().max(1);
                    let label = if n == 1 { "copied 1 line".to_string() } else { format!("copied {n} lines") };
                    self.toast(Level::Good, label);
                    return;
                }
            }
        }
        crate::clipboard::copy(&open.page.body);
        self.toast(Level::Good, "copied page to clipboard");
    }

    fn jump_to_anchor(&mut self, anchor: &str) {
        let needle = anchor.trim_start_matches('#').replace('-', " ").to_lowercase();
        let Some(open) = self.open.as_mut() else { return };
        if let Some((line, _, _)) = open
            .doc
            .headings
            .iter()
            .find(|(_, _, text)| text.to_lowercase().replace('-', " ") == needle)
        {
            open.scroll = *line;
        }
    }

    fn reveal_open(&mut self) {
        if let Some(path) = self.open.as_ref().map(|o| o.page.path.clone()) {
            self.tree.reveal(&path);
        }
    }

    /// Where the reader is right now.
    pub fn here(&self) -> Spot {
        match self.open.as_ref() {
            Some(open) => Spot::Page(open.page.path.clone()),
            None => Spot::Home,
        }
    }

    /// Go to a destination without touching the history — the caller has
    /// already recorded whatever move it is making.
    fn go_to(&mut self, spot: Spot) {
        match spot {
            Spot::Home => self.show_home(),
            Spot::Page(path) => self.open_path(&path, false),
        }
    }

    /// Close the open page and show the home screen.
    ///
    /// The find bar belongs to the page that was open, so it closes with it;
    /// leaving it up would search a page that is no longer on screen.
    fn show_home(&mut self) {
        self.open = None;
        self.find = None;
        self.focus = Focus::Doc;
        self.doc_stop = DocStop::Content;
    }

    /// The `home` command: go to the mark, recording the move so `forward`
    /// brings you back to the page you left.
    fn go_home(&mut self) {
        if self.open.is_none() {
            return;
        }
        self.back.push(self.here());
        self.forward.clear();
        self.show_home();
    }

    fn go_back(&mut self) {
        let Some(previous) = self.back.pop() else { return };
        self.forward.push(self.here());
        self.go_to(previous);
    }

    fn go_forward(&mut self) {
        let Some(next) = self.forward.pop() else { return };
        self.back.push(self.here());
        self.go_to(next);
    }

    fn jump_finding(&mut self, delta: isize) {
        let findings: Vec<(String, Option<usize>)> = self
            .index
            .all_findings()
            .into_iter()
            .map(|(rel, f)| (rel, f.line))
            .collect();
        if findings.is_empty() {
            self.toast(Level::Good, "no findings — the vault is clean");
            return;
        }
        let n = findings.len() as isize;
        // The first `n` lands on the first finding rather than the second.
        let at = match self.finding_cursor {
            None if delta > 0 => 0,
            None => n - 1,
            Some(at) => ((at as isize + delta) % n + n) % n,
        };
        self.finding_cursor = Some(at as usize);
        let (rel, line) = findings[at as usize].clone();
        let path = self.cfg.root.join(&rel);
        if path.is_file() {
            self.open_path(&path, true);
            if let (Some(open), Some(line)) = (self.open.as_mut(), line) {
                open.scroll = open.doc.line_for_source(line.saturating_sub(open.page.body_start));
            }
        }
        self.tree.reveal(&path);
        self.toast(
            Level::Info,
            format!("finding {}/{}: {rel}", at + 1, findings.len()),
        );
    }

    pub(super) fn reload_everything(&mut self) {
        self.tree.rebuild();
        self.refresh_git();
        self.reload_open();
        self.start_indexing();
        self.toast(Level::Info, "reloading");
    }
}
