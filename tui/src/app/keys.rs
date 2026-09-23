//! Key events, resolved against the keymap into a `Cmd`.
//!
//! Split out of `app.rs` verbatim; inherent `impl App` blocks merge
//! across the crate, so this is the same type seen from another file.

use super::*;

impl App {
    pub fn ctx(&self) -> Ctx {
        match self.focus {
            Focus::Sidebar if self.sidebar.is_some() => Ctx::Sidebar,
            Focus::Tree => Ctx::Tree,
            _ if self.open.as_ref().is_some_and(Open::editing) => Ctx::Edit,
            Focus::Doc | Focus::Sidebar => Ctx::Doc,
        }
    }

    /// A paste, delivered whole by bracketed paste rather than as a burst of
    /// key events. Where it lands depends on what has focus, because only some
    /// of these surfaces take text at all.
    pub fn on_paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        // Text inputs read `Char` keys, which is exactly what they were being
        // fed before bracketed paste was enabled. Control characters are
        // dropped: a newline in a pasted query would submit the prompt
        // halfway through the paste.
        if self.overlay.is_some() || self.find.is_some() {
            for ch in text.chars().filter(|c| !c.is_control()) {
                self.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
            }
            return;
        }
        match self.ctx() {
            Ctx::Sidebar => {
                let bracketed = self.sidebar.as_ref().is_some_and(herdr::pty::Pane::bracketed_paste);
                let bytes = if bracketed {
                    format!("\x1b[200~{text}\x1b[201~").into_bytes()
                } else {
                    text.as_bytes().to_vec()
                };
                if let Some(pane) = self.sidebar.as_mut() {
                    pane.send(&bytes);
                }
            }
            Ctx::Edit => {
                self.with_editor(|editor| editor.paste(text));
                // Typing again means the warning has to be re-earned.
                self.discard_armed = false;
                self.overwrite_armed = false;
                self.refresh_completion();
            }
            // Nothing in the reader or the tree takes text, and replaying the
            // characters as keys would run each one as a command.
            _ => self.toast(Level::Info, "nothing here takes a paste — press e to edit"),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if self.overlay.is_some() {
            return self.overlay_key(key);
        }

        // The find bar is a text input sitting over the reader: while it is
        // open every key is its own, or nothing.
        if self.find.is_some() {
            return self.find_key(key);
        }

        let ctx = self.ctx();

        // The sidebar owns its keys: only its escape hatch is ours.
        if ctx == Ctx::Sidebar {
            if let Resolved::Run(cmd) = keymap::resolve(&key, ctx, None) {
                return self.run(cmd);
            }
            let application_cursor = self.sidebar.as_ref().is_some_and(|p| p.application_cursor());
            let bytes = herdr::keys::encode(&key, application_cursor);
            if let Some(pane) = self.sidebar.as_mut() {
                pane.send(&bytes);
            }
            return;
        }

        if ctx == Ctx::Edit {
            return self.editor_key(key);
        }

        match keymap::resolve(&key, ctx, self.pending.take()) {
            Resolved::Run(cmd) => {
                if cmd != Cmd::Quit {
                    self.quit_confirmed = false;
                }
                self.run(cmd)
            }
            Resolved::Pending(prefix) => self.pending = Some(prefix),
            Resolved::None => {}
        }
    }

    /// Hits for the live find query against the open page.
    ///
    /// Derived, never stored: see `PageFind`.
    pub fn finds(&self) -> markdown::Finds {
        let (Some(find), Some(open)) = (self.find.as_ref(), self.open.as_ref()) else {
            return markdown::Finds::default();
        };
        let hits = open.doc.find(&find.query);
        let current = find.current.min(hits.len().saturating_sub(1));
        markdown::Finds { hits, current }
    }

    pub(super) fn open_find(&mut self) {
        if self.open.is_none() {
            return self.toast(Level::Warn, "no page open to search");
        }
        self.focus = Focus::Doc;
        self.find = Some(PageFind { query: String::new(), current: 0 });
    }

    fn find_key(&mut self, key: KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) => self.find = None,
            (KeyCode::Enter, m) if m.contains(KeyModifiers::SHIFT) => self.step_find(false),
            (KeyCode::Enter | KeyCode::Down, _) => self.step_find(true),
            (KeyCode::Up, _) => self.step_find(false),
            (KeyCode::Char('n' | 'f'), KeyModifiers::CONTROL) => self.step_find(true),
            (KeyCode::Char('p'), KeyModifiers::CONTROL) => self.step_find(false),
            (KeyCode::Backspace, _) => {
                if let Some(find) = self.find.as_mut() {
                    find.query.pop();
                    find.current = 0;
                }
                self.scroll_to_find();
            }
            (KeyCode::Char(c), m) if (m - KeyModifiers::SHIFT).is_empty() => {
                if let Some(find) = self.find.as_mut() {
                    find.query.push(c);
                    find.current = 0;
                }
                self.scroll_to_find();
            }
            _ => {}
        }
    }

    fn step_find(&mut self, forward: bool) {
        let n = self.finds().hits.len();
        if n == 0 {
            return;
        }
        if let Some(find) = self.find.as_mut() {
            let cur = find.current.min(n - 1);
            find.current = if forward { (cur + 1) % n } else { (cur + n - 1) % n };
        }
        self.scroll_to_find();
    }

    /// Bring the current hit on screen, leaving the scroll alone if it already
    /// is — a find that jumps the page on every keystroke is unreadable.
    fn scroll_to_find(&mut self) {
        let finds = self.finds();
        let Some(hit) = finds.hits.get(finds.current).copied() else { return };
        let height = self.areas.doc_body.height as usize;
        let Some(open) = self.open.as_mut() else { return };
        if hit.line < open.scroll {
            open.scroll = hit.line;
        } else if height > 0 && hit.line >= open.scroll + height {
            open.scroll = hit.line + 1 - height;
        }
    }

    /// The editor is modeless: every key is text unless it is one of the few
    /// the editor itself claims. A visible completion popup claims its own
    /// navigation keys first.
    pub(super) fn editor_key(&mut self, key: KeyEvent) {
        let has_completion = self
            .open
            .as_ref()
            .and_then(|o| o.editor.as_ref())
            .is_some_and(|e| e.completion.is_some());

        if has_completion {
            let editor = self.open.as_mut().unwrap().editor.as_mut().unwrap();
            let handled = match (key.code, key.modifiers) {
                (KeyCode::Tab, _) | (KeyCode::Down, _) => {
                    editor.completion.as_mut().unwrap().move_by(1);
                    true
                }
                (KeyCode::BackTab, _) | (KeyCode::Up, _) => {
                    editor.completion.as_mut().unwrap().move_by(-1);
                    true
                }
                (KeyCode::Enter, _) => {
                    editor.accept_completion();
                    true
                }
                (KeyCode::Esc, _) => {
                    editor.dismiss_completion();
                    true
                }
                _ => false,
            };
            if handled {
                return;
            }
        }

        if let Resolved::Run(cmd) = keymap::resolve(&key, Ctx::Edit, None) {
            return self.run(cmd);
        }

        // Markdown-aware keys the editor handles before edtui sees them.
        match (key.code, key.modifiers) {
            (KeyCode::Enter, KeyModifiers::NONE) => {
                let continued = self
                    .open
                    .as_mut()
                    .and_then(|o| o.editor.as_mut())
                    .is_some_and(Editor::continue_block);
                if continued {
                    return;
                }
            }
            (KeyCode::Tab, _) => {
                self.with_editor(|editor| editor.indent(false));
                return;
            }
            (KeyCode::BackTab, _) => {
                self.with_editor(|editor| editor.indent(true));
                return;
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                self.with_editor(|editor| editor.move_visual(true));
                return;
            }
            (KeyCode::Up, KeyModifiers::NONE) => {
                self.with_editor(|editor| editor.move_visual(false));
                return;
            }
            _ => {}
        }

        if let Some(editor) = self.open.as_mut().and_then(|o| o.editor.as_mut()) {
            editor.events.on_key_event(key, &mut editor.state);
            editor.keep_modeless();
        }
        // Typing again means the warning has to be re-earned.
        self.discard_armed = false;
        self.overwrite_armed = false;
        self.refresh_completion();
    }

    pub(super) fn with_editor(&mut self, action: impl FnOnce(&mut Editor)) {
        if let Some(editor) = self.open.as_mut().and_then(|o| o.editor.as_mut()) {
            action(editor);
        }
    }

    pub(super) fn refresh_completion(&mut self) {
        let (root, index) = (self.cfg.root.clone(), &self.index);
        if let Some(open) = self.open.as_mut() {
            let page = &open.page;
            if let Some(editor) = open.editor.as_mut() {
                editor.refresh_completion(page, index, &root);
            }
        }
    }
}
