//! Long-running native work and what to do with its result.
//!
//! Split out of `app.rs` verbatim; inherent `impl App` blocks merge
//! across the crate, so this is the same type seen from another file.

use super::*;

impl App {
    /// The whole vault is already scanned and kept fresh by the file watcher
    /// (`self.index`), so this needs no subprocess: assemble the same payload
    /// `podarcis lint --json` used to produce and report it immediately.
    pub(super) fn run_lint(&mut self) {
        let payload = crate::vault::lint::to_json_payload(&self.index);
        let ok = payload.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        let result = JobResult {
            id: 0,
            label: "lint".into(),
            args: vec!["lint".into(), "--json".into()],
            code: if ok { 0 } else { 1 },
            stdout: payload.to_string(),
            stderr: String::new(),
        };
        self.on_job_done(result);
    }

    pub(super) fn run_sync_repos(&mut self) {
        let root = self.cfg.root.clone();
        let repos = self.cfg.repositories.clone();
        let tx = self.tx.clone();
        self.jobs.spawn_native(
            "sync",
            vec!["repo".into(), "sync".into()],
            move || {
                let results = crate::vault::git::sync_repos(&root, &repos);
                let ok = results.iter().all(|r| r.status == "ok");
                let stdout = serde_json::to_string(&results).unwrap_or_default();
                NativeOutcome { code: if ok { 0 } else { 1 }, stdout, stderr: String::new() }
            },
            tx,
        );
    }

    /// Lint-gate on the in-memory index, then `git add -A` + commit every
    /// dirty configured repo. Mirrors `audit.py::audit_and_commit`, run
    /// through `podarcis repo commit -m` until now.
    pub(super) fn run_commit(&mut self, message: String) {
        let lint_ok = !self.index.entries.iter().any(|e| e.worst() == crate::vault::lint::Severity::Error);
        let root = self.cfg.root.clone();
        let repos = self.cfg.repositories.clone();
        let tx = self.tx.clone();
        self.jobs.spawn_native(
            "commit",
            vec!["repo".into(), "commit".into(), "-m".into(), message.clone()],
            move || {
                let outcome = crate::vault::git::commit_dirty(&root, &repos, lint_ok, &message);
                if outcome.ok {
                    NativeOutcome { code: 0, stdout: outcome.message, stderr: String::new() }
                } else {
                    NativeOutcome { code: 1, stdout: String::new(), stderr: outcome.message }
                }
            },
            tx,
        );
    }

    pub fn start_indexing(&mut self) {
        self.indexing = true;
        let root = self.cfg.root.clone();
        let dirs = self.collection_dirs();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let index = Index::build(&root, &dirs);
            let _ = tx.send(AppEvent::IndexReady(Box::new(index)));
        });
    }

    pub fn on_job_done(&mut self, result: JobResult) {
        self.jobs.finish(result.id);

        let is_semantic = matches!(
            self.overlay.as_ref(),
            Some(Overlay::Finder(f)) if f.running == Some(result.id)
        );
        if is_semantic {
            let payload = result.json();
            let hits = payload
                .as_ref()
                .map(|v| search::parse_semantic(v, &self.cfg.root))
                .unwrap_or_default();
            let warning = payload.as_ref().and_then(search::semantic_warning);
            if let Some(Overlay::Finder(finder)) = self.overlay.as_mut() {
                finder.running = None;
                finder.selected = 0;
                finder.warning = warning.or_else(|| {
                    (!result.ok()).then(|| first_line(&result.stderr).unwrap_or_else(|| "search failed".into()))
                });
                finder.hits = hits;
            }
            return;
        }

        if result.args.first().map(String::as_str) == Some("lint") {
            return self.report_lint(&result);
        }

        if result.args.first().map(String::as_str) == Some("apm") {
            return self.report_extension_action(&result);
        }

        let detail = first_line(&result.stderr)
            .or_else(|| result.stdout.lines().rev().find(|l| !l.trim().is_empty()).map(str::to_string))
            .unwrap_or_default();
        if result.ok() {
            self.toast(Level::Good, format!("{} ok{}", result.label, suffix(&detail)));
            self.reload_everything();
        } else {
            self.toast(Level::Bad, format!("{} failed{}", result.label, suffix(&detail)));
        }
    }

    /// Refresh the browse list from `apm.yml`/`apm.lock.yaml` and show the raw
    /// `apm` output — never parsed, since apm prints Rich tables, not JSON.
    fn report_extension_action(&mut self, result: &JobResult) {
        let items = crate::extensions::load(&self.cfg.root);
        let log = if result.stdout.trim().is_empty() { result.stderr.clone() } else { result.stdout.clone() };
        if let Some(Overlay::Extensions(state)) = self.overlay.as_mut() {
            state.items = items;
            state.selected = state.selected.min(state.items.len().saturating_sub(1));
            state.log = Some(log);
        }
        if result.ok() {
            self.toast(Level::Good, format!("{} ok", result.label));
        } else {
            let detail = first_line(&result.stderr).unwrap_or_default();
            self.toast(Level::Bad, format!("{} failed{}", result.label, suffix(&detail)));
        }
    }

    fn report_lint(&mut self, result: &JobResult) {
        let Some(payload) = result.json() else {
            self.toast(Level::Bad, "lint produced no JSON payload");
            return;
        };
        let files = payload.get("files").and_then(|f| f.as_object());
        let count: usize = files.map(|f| f.values().filter_map(|v| v.as_array()).map(Vec::len).sum()).unwrap_or(0);
        if count == 0 {
            self.toast(Level::Good, "lint: clean");
        } else {
            let n = files.map(|f| f.len()).unwrap_or(0);
            self.toast(Level::Warn, format!("lint: {count} findings across {n} files"));
        }
    }

    pub fn on_index_ready(&mut self, index: Index) {
        self.index = index;
        self.indexing = false;
        self.finding_cursor = None;
        self.components = platform::discover_components(&self.cfg.root);
    }

    pub fn on_fs_changed(&mut self, paths: Vec<PathBuf>) {
        let mut structural = false;
        for path in &paths {
            structural |= self.index.refresh(path);
        }
        if structural {
            self.tree.rebuild();
            self.components = platform::discover_components(&self.cfg.root);
        }
        self.refresh_git();
        let open_changed = self
            .open
            .as_ref()
            .map(|o| paths.contains(&o.page.path))
            .unwrap_or(false);
        if open_changed {
            self.reload_open();
        }
    }
}
