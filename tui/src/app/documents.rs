//! Opening, navigating and reloading pages, and the tree sidebar.
//!
//! Split out of `app.rs` verbatim; inherent `impl App` blocks merge
//! across the crate, so this is the same type seen from another file.

use super::*;

impl App {
    pub fn doc_width(&self) -> u16 {
        self.measure(self.open.as_ref().is_some_and(Open::csv))
    }

    /// Width a page is laid out at. Two columns of border plus one of padding
    /// on each side; prose is then capped at the reading measure, but a CSV is
    /// a table and takes the whole pane — capping it would pan columns that
    /// had room to be drawn.
    pub fn measure(&self, csv: bool) -> u16 {
        let inner = self.areas.doc.width.saturating_sub(4).max(20);
        if csv {
            inner
        } else {
            inner.min(crate::ui::markdown::MAX_WIDTH)
        }
    }

    pub fn open_path(&mut self, path: &Path, push_history: bool) {
        if is_external(path) {
            return self.open_external(path);
        }
        if path.is_dir() {
            let index = path.join("_index.md");
            if index.is_file() {
                return self.open_path(&index, push_history);
            }
            return;
        }
        let here = self.here();
        if here == Spot::Page(path.to_path_buf()) {
            return;
        }
        // Pushed even when leaving home, so the first page you open has
        // somewhere to go back to.
        if push_history {
            self.back.push(here);
            self.forward.clear();
        }
        match Open::load(path, &self.cfg.root, self.measure(Open::is_csv(path)), &self.theme) {
            Some(open) => {
                self.open = Some(open);
                self.focus = Focus::Doc;
                self.doc_stop = DocStop::Content;
            }
            None => self.toast(Level::Bad, format!("could not read {}", path.display())),
        }
    }

    /// Hand a file off to the system's default viewer instead of rendering it
    /// in-pane — used for PDFs and images, which the tree lists but never
    /// tries to parse as markdown.
    fn open_external(&mut self, path: &Path) {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        match std::process::Command::new(opener)
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(_) => self.toast(Level::Info, format!("opened {} externally", path.display())),
            Err(_) => self.toast(Level::Bad, format!("no viewer found for {}", path.display())),
        }
    }

    /// Re-read the open page from disk, keeping the reading position.
    pub fn reload_open(&mut self) {
        let Some(open) = self.open.as_ref() else { return };
        if open.editing() {
            return;
        }
        let (path, anchor, inspect_scroll) =
            (open.page.path.clone(), open.doc.source_for_line(open.scroll), open.inspect_scroll);
        let width = self.measure(Open::is_csv(&path));
        if let Some(mut fresh) = Open::load(&path, &self.cfg.root, width, &self.theme) {
            fresh.scroll = fresh.doc.line_for_source(anchor);
            fresh.inspect_scroll = inspect_scroll;
            self.open = Some(fresh);
        }
    }

    pub fn reflow(&mut self) {
        let (width, theme) = (self.doc_width(), self.theme);
        if let Some(open) = self.open.as_mut() {
            open.reflow(width, &theme);
        }
    }

    pub fn inspector_total_lines(&self) -> usize {
        let Some(open) = self.open.as_ref() else { return 0 };
        let lines = crate::ui::panes::inspector_lines(open, &self.index, &self.cfg.root, &self.theme);
        // 2 columns of border, 2 of padding — matches the block the inspector
        // is actually drawn into (`ui::panes::inspector`).
        let width = self.areas.inspector.width.saturating_sub(4) as usize;
        crate::ui::panes::wrap_lines(lines, width).len()
    }

    pub fn scroll_inspector(&mut self, delta: isize) {
        let total = self.inspector_total_lines();
        let height = self.areas.inspector.height.saturating_sub(2) as usize;
        let max_scroll = total.saturating_sub(height);
        let Some(open) = self.open.as_mut() else { return };
        if delta < 0 {
            open.inspect_scroll = open.inspect_scroll.saturating_sub(delta.unsigned_abs());
        } else {
            open.inspect_scroll = open.inspect_scroll.saturating_add(delta as usize).min(max_scroll);
        }
    }

    pub fn scroll_inspector_to_y(&mut self, y: u16) {
        let area = self.areas.inspector;
        if area.height < 4 {
            return;
        }
        let total = self.inspector_total_lines();
        let height = area.height.saturating_sub(2) as usize;
        if total <= height || height == 0 {
            if let Some(open) = self.open.as_mut() {
                open.inspect_scroll = 0;
            }
            return;
        }
        let max_scroll = total.saturating_sub(height);
        let track_y = area.y + 1;
        let track_len = area.height.saturating_sub(2) as usize;
        let span = ((height * track_len) / total).max(1);
        let scroll = Self::scroll_from_track_y(y, track_y, track_len, span, max_scroll);
        if let Some(open) = self.open.as_mut() {
            open.inspect_scroll = scroll;
        }
    }

    pub fn scroll_doc_to_y(&mut self, y: u16) {
        let area = if !self.areas.doc_main.is_empty() {
            self.areas.doc_main
        } else {
            self.areas.doc
        };
        if area.height < 4 {
            return;
        }
        let Some(open) = self.open.as_mut() else { return };
        let total = open.doc.height();
        let height = self.areas.doc_body.height as usize;
        if total <= height || height == 0 {
            open.scroll = 0;
            return;
        }
        let max_scroll = total.saturating_sub(height);
        let track_y = area.y + 1;
        let track_len = area.height.saturating_sub(2) as usize;
        let span = ((height * track_len) / total).max(1);
        open.scroll = Self::scroll_from_track_y(y, track_y, track_len, span, max_scroll);
    }

    /// Start or resize the agents pane to match the area it was drawn into.
    pub fn sync_sidebar(&mut self) {
        if !self.show_sidebar {
            self.sidebar = None;
            return;
        }
        let area = self.areas.sidebar;
        if area.width < 4 || area.height < 3 {
            return;
        }
        let (rows, cols) = (area.height.saturating_sub(2), area.width.saturating_sub(2));

        if let Some(pane) = self.sidebar.as_mut() {
            if pane.is_alive() {
                pane.resize(rows, cols);
                return;
            }
            self.sidebar = None;
            self.sidebar_error = Some("the agent session exited — ctrl+g twice to restart".into());
            if self.focus == Focus::Sidebar {
                self.focus = Focus::Doc;
            }
            return;
        }
        if self.sidebar_error.is_some() {
            return;
        }
        if !herdr::pty::available() {
            self.sidebar_error = Some("herdr is not installed — the agents pane needs it (herdr.dev)".into());
            return;
        }
        if let Err(err) = herdr::config::provision(self.cfg.flavor) {
            self.sidebar_error = Some(format!("agents pane config: {err}"));
            return;
        }
        match herdr::pty::Pane::spawn(&self.project_name, &self.cfg.root, rows, cols, self.tx.clone()) {
            Ok(pane) => self.sidebar = Some(pane),
            Err(err) => self.sidebar_error = Some(format!("herdr: {err}")),
        }
    }
}
