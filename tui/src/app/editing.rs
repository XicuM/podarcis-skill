//! Entering, leaving and saving the editor.
//!
//! Split out of `app.rs` verbatim; inherent `impl App` blocks merge
//! across the crate, so this is the same type seen from another file.

use super::*;

impl App {
    pub(super) fn enter_editor(&mut self) {
        let Some(open) = self.open.as_mut() else { return };
        if open.editing() {
            return;
        }
        let line = open.page.body_start + open.doc.source_for_line(open.scroll);
        let mut editor = Editor::open(&open.page);
        editor.goto_line(line);
        open.editor = Some(editor);
        self.focus = Focus::Doc;
        self.discard_armed = false;
        self.overwrite_armed = false;
    }

    pub(super) fn save(&mut self) {
        self.discard_armed = false;
        let armed = std::mem::take(&mut self.overwrite_armed);
        let Some(open) = self.open.as_mut() else { return };
        let Some(editor) = open.editor.as_mut() else { return };
        let path = editor.path.clone();

        // Armed by a previous ctrl+s that hit a conflict — the same
        // arm-then-confirm shape as discarding an unsaved buffer, so the
        // second press is the answer rather than a new dialog.
        if armed {
            match editor.save_overwriting() {
                Ok(backup) => {
                    let kept = crate::vault::page::rel_path(&backup, &self.cfg.root);
                    self.after_save(&path);
                    self.toast(Level::Warn, format!("saved — their version kept as {kept}"));
                }
                Err(err) => self.toast(Level::Bad, format!("save failed: {err}")),
            }
            return;
        }

        match editor.save() {
            Ok(Save::Written) => {
                self.after_save(&path);
                let rel = crate::vault::page::rel_path(&path, &self.cfg.root);
                self.toast(Level::Good, format!("saved {rel}"));
            }
            Ok(Save::ChangedUnderneath) => {
                self.overwrite_armed = true;
                self.toast(
                    Level::Warn,
                    "changed on disk since you opened it — ctrl+s again to overwrite (a copy is kept), esc esc to discard yours",
                );
            }
            Err(err) => self.toast(Level::Bad, format!("save failed: {err}")),
        }
    }

    /// Re-index and re-parse after a write that actually landed.
    fn after_save(&mut self, path: &Path) {
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            self.index.refresh(path);
        }
        self.refresh_page_after_save();
    }

    /// Re-parse the page so the inspector and findings reflect what was saved,
    /// without closing the editor. A CSV re-renders its table grid the same way.
    fn refresh_page_after_save(&mut self) {
        let root = self.cfg.root.clone();
        if let Some(open) = self.open.as_mut() {
            if let Ok(page) = Page::load(&open.page.path, &root) {
                open.page = page;
                if open.csv() {
                    let width = open.doc_width.max(20);
                    open.doc = crate::ui::csv::Grid::parse(&open.page.body).to_doc(width, &self.theme);
                }
            }
        }
    }

    /// Leaving with unsaved changes asks once, then discards on a second press.
    pub(super) fn leave_editor(&mut self) {
        let dirty = self
            .open
            .as_ref()
            .and_then(|o| o.editor.as_ref())
            .is_some_and(Editor::dirty);

        if dirty && !self.discard_armed {
            self.discard_armed = true;
            self.toast(Level::Warn, "unsaved changes — ctrl+s to save, esc again to discard");
            return;
        }
        self.discard_armed = false;
        self.overwrite_armed = false;
        if let Some(open) = self.open.as_mut() {
            open.editor = None;
        }
        self.reload_open();
    }
}
