//! The finder, palette, prompts and every other overlay.
//!
//! Split out of `app.rs` verbatim; inherent `impl App` blocks merge
//! across the crate, so this is the same type seen from another file.

use super::*;

impl App {
    pub(super) fn open_finder(&mut self, mode: search::Mode) {
        let mut finder = Finder {
            mode,
            query: String::new(),
            hits: Vec::new(),
            selected: 0,
            collection: None,
            running: None,
            warning: None,
        };
        if mode.is_live() {
            finder.hits = self.finder_engine.files(&self.index, "", None);
        }
        self.overlay = Some(Overlay::Finder(finder));
    }

    /// Sources ingested but never cited by a `wiki/` page. Derived live from
    /// the citation graph, which is the same thing `literature_status` does —
    /// there is no manifest to fall out of date.
    pub(super) fn show_uncited(&mut self) {
        let hits: Vec<Hit> = self
            .index
            .uncited_sources()
            .into_iter()
            .map(|entry| Hit {
                path: entry.path.clone(),
                rel: entry.rel.clone(),
                title: entry.title.clone(),
                line: None,
                snippet: "not cited by any wiki page".into(),
                matched: Vec::new(),
                score: 0,
            })
            .collect();
        if hits.is_empty() {
            return self.toast(Level::Good, "every ingested source is cited");
        }
        let count = hits.len();
        self.overlay = Some(Overlay::Finder(Finder {
            mode: search::Mode::Files,
            query: String::new(),
            hits,
            selected: 0,
            collection: Some("sources"),
            running: None,
            warning: Some(format!("{count} ingested sources are not cited by any wiki page")),
        }));
    }

    /// Switch theme: repaint, re-wrap the page, and hand the new flavour to
    /// the agents pane so the two never disagree.
    fn apply_flavor(&mut self, flavor: Flavor) {
        if self.cfg.flavor == flavor {
            return;
        }
        self.cfg.flavor = flavor;
        self.theme = Theme::new(flavor);
        // The document is re-rendered rather than re-wrapped: every span
        // carries a colour from the old theme.
        let (width, theme) = (self.doc_width(), self.theme);
        if let Some(open) = self.open.as_mut() {
            open.doc_width = 0;
            open.reflow(width, &theme);
        }
        herdr::pty::reload_config(flavor);
    }

    pub(super) fn open_palette(&mut self) {
        let items = palette_items("");
        self.overlay = Some(Overlay::Palette(Palette { query: String::new(), items, selected: 0 }));
    }

    pub(super) fn prompt(&mut self, kind: PromptKind, title: &str) {
        self.overlay = Some(Overlay::Prompt(Prompt {
            kind,
            title: title.to_string(),
            value: String::new(),
            path: None,
        }));
    }

    pub(super) fn overlay_key(&mut self, key: KeyEvent) {
        match self.overlay.as_mut() {
            Some(Overlay::Help { scroll }) => match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => self.overlay = None,
                KeyCode::Down | KeyCode::Char('j') => *scroll += 1,
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                _ => {}
            },
            Some(Overlay::Outline { selected }) => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.overlay = None,
                KeyCode::Down | KeyCode::Char('j') => *selected += 1,
                KeyCode::Up | KeyCode::Char('k') => *selected = selected.saturating_sub(1),
                KeyCode::Enter => {
                    let at = *selected;
                    self.overlay = None;
                    if let Some(open) = self.open.as_mut() {
                        if let Some((line, _, _)) = open.doc.headings.get(at) {
                            open.scroll = *line;
                        }
                    }
                }
                _ => {}
            },
            Some(Overlay::Themes { .. }) => self.theme_key(key),
            Some(Overlay::Projects { .. }) => self.project_key(key),
            Some(Overlay::Palette(_)) => self.palette_key(key),
            Some(Overlay::Finder(_)) => self.finder_key(key),
            Some(Overlay::Prompt(_)) => self.prompt_key(key),
            Some(Overlay::Menu(_)) => self.menu_key(key),
            Some(Overlay::RepoConfig(_)) => self.repo_config_key(key),
            Some(Overlay::Ask(_)) => self.ask_key(key),
            Some(Overlay::Extensions(_)) => self.extensions_key(key),
            None => {}
        }
    }

    /// Say why up front rather than opening an overlay whose every action
    /// would fail — same posture as `run_semantic_search`'s `qmd_off_reason`
    /// check.
    pub(super) fn open_extensions(&mut self) {
        if let Some(reason) = self.cfg.apm_off_reason {
            self.toast(Level::Warn, reason);
            return;
        }
        let items = crate::extensions::load(&self.cfg.root);
        self.overlay = Some(Overlay::Extensions(Extensions { selected: 0, items, log: None, confirm: None }));
    }

    fn extensions_key(&mut self, key: KeyEvent) {
        let Some(Overlay::Extensions(state)) = self.overlay.as_mut() else { return };
        if let Some(action) = state.confirm {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    state.confirm = None;
                    self.run_extension_action(action);
                }
                _ => state.confirm = None,
            }
            return;
        }
        let n = state.items.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.overlay = None,
            KeyCode::Down | KeyCode::Char('j') if n > 0 => state.selected = (state.selected + 1).min(n - 1),
            KeyCode::Up | KeyCode::Char('k') => state.selected = state.selected.saturating_sub(1),
            KeyCode::Char('i') if n > 0 => self.run_extension_action(ExtensionAction::Install),
            KeyCode::Char('u') if n > 0 => self.run_extension_action(ExtensionAction::Update),
            KeyCode::Char('d') | KeyCode::Char('x') if n > 0 => state.confirm = Some(ExtensionAction::Uninstall),
            _ => {}
        }
    }

    /// Every action shells out to the real `apm` binary — this never touches
    /// `apm.yml`/`apm.lock.yaml` itself. Runs as a native job (`extensions::run`
    /// on its own thread) so a slow install never blocks the UI, mirroring
    /// `run_semantic_search`.
    fn run_extension_action(&mut self, action: ExtensionAction) {
        let Some(Overlay::Extensions(state)) = self.overlay.as_ref() else { return };
        let Some(item) = state.items.get(state.selected) else { return };
        let key = item.key.clone();
        let root = self.cfg.root.clone();
        let tx = self.tx.clone();
        let args: Vec<String> = match action {
            ExtensionAction::Install => vec!["install".into(), key],
            ExtensionAction::Update => vec!["update".into(), key, "-y".into()],
            ExtensionAction::Uninstall => vec!["uninstall".into(), key],
        };
        let mut job_args = vec!["apm".to_string()];
        job_args.extend(args.clone());
        self.jobs.spawn_native(
            format!("apm {}", action.verb()),
            job_args,
            move || {
                let refs: Vec<&str> = args.iter().map(String::as_str).collect();
                let (code, stdout, stderr) = crate::extensions::run(&root, &refs);
                NativeOutcome { code, stdout, stderr }
            },
            tx,
        );
    }

    pub(super) fn open_projects(&mut self) {
        let reg = crate::project::ProjectRegistry::load();
        let mut items = Vec::new();
        let mut sorted_names: Vec<&String> = reg.projects.keys().collect();
        sorted_names.sort();
        for name in sorted_names {
            let entry = &reg.projects[name];
            items.push((name.clone(), entry.path.clone()));
        }
        let selected = items.iter().position(|(n, _)| *n == reg.active).unwrap_or(0);
        self.overlay = Some(Overlay::Projects { selected, items });
    }

    fn project_key(&mut self, key: KeyEvent) {
        // Creating a project is valid even with an empty registry, so handle it
        // before the "no items" bail-out below closes the overlay outright.
        if key.code == KeyCode::Char('n') {
            self.overlay = None;
            self.prompt_new_project();
            return;
        }
        let Some(Overlay::Projects { selected, items }) = self.overlay.as_mut() else { return };
        let n = items.len();
        if n == 0 {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                self.overlay = None;
            }
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.overlay = None;
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                *selected = (*selected + 1) % n;
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                *selected = (*selected + n - 1) % n;
            }
            KeyCode::Enter => {
                let (name, path) = items[*selected].clone();
                self.overlay = None;
                self.switch_project(&name, &path);
            }
            KeyCode::Char('r') => {
                let (name, path) = items[*selected].clone();
                self.overlay = None;
                self.prompt_rename_project(&name, &path);
            }
            KeyCode::Char('d') => {
                let (name, path) = items[*selected].clone();
                self.overlay = None;
                self.prompt_delete_project(&name, &path);
            }
            _ => {}
        }
    }

    fn prompt_new_project(&mut self) {
        self.overlay = Some(Overlay::Prompt(Prompt {
            kind: PromptKind::NewProject,
            title: "new project name".to_string(),
            value: String::new(),
            path: None,
        }));
    }

    fn prompt_rename_project(&mut self, name: &str, path: &Path) {
        self.overlay = Some(Overlay::Prompt(Prompt {
            kind: PromptKind::RenameProject,
            title: format!("rename project '{name}'"),
            value: name.to_string(),
            path: Some(path.to_path_buf()),
        }));
    }

    fn prompt_delete_project(&mut self, name: &str, path: &Path) {
        let is_in_projects_dir = path.starts_with(crate::project::projects_dir());
        let title = if is_in_projects_dir {
            format!("type '{name}' to delete it (files will be removed from disk)")
        } else {
            format!("type '{name}' to unregister it (external files kept on disk)")
        };
        self.overlay = Some(Overlay::Prompt(Prompt {
            kind: PromptKind::DeleteProject,
            title,
            value: String::new(),
            path: Some(path.to_path_buf()),
        }));
    }

    /// Create a project in the default XDG projects directory, register it, and switch to it.
    fn create_new_project(&mut self, raw_name: &str) {
        let name = raw_name.trim().replace(' ', "_");
        if name.is_empty() {
            return;
        }
        let reg = crate::project::ProjectRegistry::load();
        if reg.projects.contains_key(&name) {
            return self.toast(Level::Bad, format!("project '{name}' already exists"));
        }
        match crate::project::create_project(&name, None, "", "", "", "", "local") {
            Ok(proj) => {
                let _ = herdr::space::create_workspace_if_server_running(&proj.name, &proj.root);
                self.switch_project(&proj.name, &proj.root);
                self.toast(Level::Good, format!("created project '{name}'"));
            }
            Err(err) => self.toast(Level::Bad, format!("could not create project: {err}")),
        }
    }

    /// Rename a registered project's key, leaving its files untouched.
    fn rename_project(&mut self, path: Option<&Path>, value: &str) {
        let Some(path) = path else { return };
        let mut reg = crate::project::ProjectRegistry::load();
        let Some(old_name) = reg.projects.iter().find(|(_, e)| e.path == path).map(|(n, _)| n.clone()) else {
            return self.toast(Level::Bad, "project no longer registered — reload");
        };
        let new_name = value.trim().replace(' ', "_");
        if new_name.is_empty() || new_name == old_name {
            return;
        }
        if reg.projects.contains_key(&new_name) {
            return self.toast(Level::Bad, format!("project '{new_name}' already exists"));
        }
        let entry = reg.projects.remove(&old_name).unwrap();
        let was_active = reg.active == old_name;
        reg.projects.insert(new_name.clone(), entry);
        if was_active {
            reg.active = new_name.clone();
        }
        if let Err(err) = reg.save() {
            return self.toast(Level::Bad, format!("could not save registry: {err}"));
        }
        if was_active {
            self.project_name = new_name.clone();
        }
        if let Ok(workspaces) = herdr::space::list_workspaces() {
            if let Some(ws) = workspaces.iter().find(|w| w.label == old_name) {
                let _ = herdr::space::rename_workspace(&ws.id, &new_name);
            }
        }
        self.toast(Level::Good, format!("renamed project '{old_name}' → '{new_name}'"));
    }

    /// Unregister or delete a project after the user types its name back to confirm.
    /// Projects located in the default XDG projects directory are deleted from disk
    /// so they are not immediately re-discovered by registry scanning.
    fn delete_project(&mut self, path: Option<&Path>, typed: &str) {
        let Some(path) = path else { return };
        let mut reg = crate::project::ProjectRegistry::load();
        let Some(name) = reg.projects.iter().find(|(_, e)| e.path == path).map(|(n, _)| n.clone()) else {
            return self.toast(Level::Bad, "project no longer registered — reload");
        };
        if typed.trim() != name {
            return self.toast(Level::Warn, "name did not match — deletion cancelled");
        }
        reg.projects.remove(&name);
        let was_active = reg.active == name;
        if was_active {
            reg.active = reg.projects.keys().next().cloned().unwrap_or_else(|| "default".to_string());
        }

        if let Ok(workspaces) = herdr::space::list_workspaces() {
            if let Some(ws) = workspaces.iter().find(|w| w.label == name) {
                let _ = herdr::space::close_workspace(&ws.id);
            }
        }

        let is_in_projects_dir = path.starts_with(crate::project::projects_dir());
        if is_in_projects_dir && path.is_dir() {
            let _ = std::fs::remove_dir_all(path);
            self.toast(Level::Good, format!("deleted project '{name}'"));
        } else {
            self.toast(Level::Good, format!("unregistered project '{name}' (files kept at {})", path.display()));
        }

        if let Err(err) = reg.save() {
            return self.toast(Level::Bad, format!("could not save registry: {err}"));
        }

        if was_active {
            if let Some(entry) = reg.projects.get(&reg.active) {
                let new_path = entry.path.clone();
                let new_name = reg.active.clone();
                self.switch_project(&new_name, &new_path);
            }
        }
    }

    pub fn switch_project(&mut self, name: &str, path: &Path) {
        if !path.is_dir() {
            self.toast(Level::Warn, format!("Project directory does not exist: {}", path.display()));
            return;
        }

        let mut reg = crate::project::ProjectRegistry::load();
        reg.active = name.to_string();
        let _ = reg.save();

        self.cfg = Config::load(path);
        self.project_name = name.to_string();
        let collections: Vec<PathBuf> = self.cfg.collections().into_iter().map(|(_, p)| p).collect();
        self.tree = Tree::new(path, collections);
        self.index = Index::build(path, &self.collection_dirs());
        self.components = platform::discover_components(path);
        self.open = None;
        self.doc_stop = DocStop::Content;

        if let Some(pane) = self.sidebar.as_mut() {
            pane.switch_project(name, path);
        } else {
            let _ = herdr::space::ensure_project_space(name, path);
        }

        self.toast(Level::Good, format!("Switched to project '{name}'"));
    }

    fn theme_key(&mut self, key: KeyEvent) {
        let Some(Overlay::Themes { selected, original }) = self.overlay.as_mut() else { return };
        let original = *original;
        let n = Flavor::ALL.len();
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                self.apply_flavor(original);
            }
            KeyCode::Enter => {
                self.overlay = None;
                let _ = self.cfg.save_tui();
                let name = self.cfg.flavor.display_name();
                self.toast(Level::Good, format!("theme: {name}"));
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                *selected = (*selected + 1) % n;
                let flavor = Flavor::ALL[*selected];
                self.apply_flavor(flavor);
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                *selected = (*selected + n - 1) % n;
                let flavor = Flavor::ALL[*selected];
                self.apply_flavor(flavor);
            }
            _ => {}
        }
    }

    fn palette_key(&mut self, key: KeyEvent) {
        let Some(Overlay::Palette(palette)) = self.overlay.as_mut() else { return };
        match key.code {
            KeyCode::Esc => self.overlay = None,
            KeyCode::Down | KeyCode::Tab => {
                palette.selected = (palette.selected + 1).min(palette.items.len().saturating_sub(1))
            }
            KeyCode::Up | KeyCode::BackTab => palette.selected = palette.selected.saturating_sub(1),
            KeyCode::Enter => {
                let cmd = palette.items.get(palette.selected).map(|(c, _, _)| *c);
                self.overlay = None;
                if let Some(cmd) = cmd {
                    self.run(cmd);
                }
            }
            KeyCode::Backspace => {
                palette.query.pop();
                palette.items = palette_items(&palette.query);
                palette.selected = 0;
            }
            KeyCode::Char(c) => {
                palette.query.push(c);
                palette.items = palette_items(&palette.query);
                palette.selected = 0;
            }
            _ => {}
        }
    }

    fn finder_key(&mut self, key: KeyEvent) {
        let Some(Overlay::Finder(finder)) = self.overlay.as_mut() else { return };
        let mut requery = false;
        let mut run_semantic = false;
        match key.code {
            KeyCode::Esc => {
                self.overlay = None;
                return;
            }
            KeyCode::Down | KeyCode::Tab => {
                finder.selected = (finder.selected + 1).min(finder.hits.len().saturating_sub(1))
            }
            KeyCode::Up | KeyCode::BackTab => finder.selected = finder.selected.saturating_sub(1),
            KeyCode::Enter => {
                if finder.mode == search::Mode::Semantic && finder.hits.is_empty() {
                    run_semantic = true;
                } else {
                    let hit = finder.hits.get(finder.selected).cloned();
                    self.overlay = None;
                    if let Some(hit) = hit {
                        self.open_path(&hit.path.clone(), true);
                        self.tree.reveal(&hit.path);
                        if let (Some(line), Some(open)) = (hit.line, self.open.as_mut()) {
                            let body_line = line.saturating_sub(open.page.body_start);
                            open.scroll = open.doc.line_for_source(body_line);
                        }
                    }
                    return;
                }
            }
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                finder.mode = finder.mode.next();
                finder.hits.clear();
                finder.warning = None;
                requery = finder.mode.is_live();
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                finder.collection = match finder.collection {
                    None => Some("wiki"),
                    Some("wiki") => Some("workspace"),
                    Some("workspace") => Some("sources"),
                    _ => None,
                };
                requery = finder.mode.is_live();
            }
            KeyCode::Backspace => {
                finder.query.pop();
                requery = finder.mode.is_live();
            }
            KeyCode::Char(c) => {
                finder.query.push(c);
                requery = finder.mode.is_live();
            }
            _ => {}
        }

        if run_semantic {
            return self.run_semantic_search();
        }
        if requery {
            self.requery_finder();
        }
    }

    fn requery_finder(&mut self) {
        let Some(Overlay::Finder(finder)) = self.overlay.as_ref() else { return };
        let (mode, query, collection) = (finder.mode, finder.query.clone(), finder.collection);
        let hits = match mode {
            search::Mode::Files => self.finder_engine.files(&self.index, &query, collection),
            search::Mode::Text => self.finder_engine.text(&self.index, &query, collection, false),
            search::Mode::Semantic => return,
        };
        if let Some(Overlay::Finder(finder)) = self.overlay.as_mut() {
            finder.hits = hits;
            finder.selected = 0;
        }
    }

    fn run_semantic_search(&mut self) {
        let Some(Overlay::Finder(finder)) = self.overlay.as_ref() else { return };
        if finder.query.trim().is_empty() {
            return;
        }
        if let Some(reason) = self.cfg.qmd_off_reason {
            if let Some(Overlay::Finder(finder)) = self.overlay.as_mut() {
                finder.warning = Some(reason.to_string());
            }
            return;
        }
        let collection = finder.collection.unwrap_or("all").to_string();
        let query = finder.query.clone();
        let root = self.cfg.root.clone();
        let tx = self.tx.clone();
        let id = self.jobs.spawn_native(
            "semantic search",
            vec!["wiki".into(), "search".into(), query.clone(), "--json".into(), "--collection".into(), collection.clone()],
            move || match search::semantic_search(&root, &query, &collection) {
                Ok(stdout) => NativeOutcome { code: 0, stdout, stderr: String::new() },
                Err(err) => NativeOutcome { code: 1, stdout: String::new(), stderr: err },
            },
            tx,
        );
        if let Some(Overlay::Finder(finder)) = self.overlay.as_mut() {
            finder.running = Some(id);
            finder.warning = None;
        }
    }

    fn menu_key(&mut self, key: KeyEvent) {
        let Some(Overlay::Menu(menu)) = self.overlay.as_ref() else { return };
        let last = menu.items.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.overlay = None,
            KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
                if let Some(Overlay::Menu(menu)) = self.overlay.as_mut() {
                    menu.selected = (menu.selected + 1).min(last);
                }
            }
            KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => {
                if let Some(Overlay::Menu(menu)) = self.overlay.as_mut() {
                    menu.selected = menu.selected.saturating_sub(1);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.activate_menu(),
            _ => {}
        }
    }

    pub(super) fn activate_menu(&mut self) {
        let (action, path, area) = match self.overlay.as_ref() {
            Some(Overlay::Menu(menu)) => (
                menu.items.get(menu.selected).map(|(a, _)| *a),
                menu.path.clone(),
                menu.area,
            ),
            _ => return,
        };
        let rel = crate::vault::page::rel_path(&path, &self.cfg.root);
        self.overlay = None;
        match action {
            Some(MenuAction::Open) => self.open_path(&path, true),
            Some(MenuAction::NewPage) => self.prompt_new_page(&path),
            Some(MenuAction::Rename) => self.prompt_rename(&path),
            Some(MenuAction::Delete) => self.confirm_delete(path, area),
            Some(MenuAction::ConfirmDelete) => self.delete_path(&path),
            Some(MenuAction::CopyRelative) => {
                crate::clipboard::copy(&rel);
                self.toast(Level::Good, format!("copied {rel}"));
            }
            Some(MenuAction::CopyAbsolute) => {
                let abs = std::fs::canonicalize(&path)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .to_string();
                crate::clipboard::copy(&abs);
                self.toast(Level::Good, format!("copied {abs}"));
            }
            _ => {}
        }
    }

    /// Ask for the new basename. The tree label is a spaced rendering of the
    /// snake_case filename, so the prompt opens on the slug without extension.
    pub(super) fn prompt_rename(&mut self, path: &Path) {
        if self.tree.collection_paths().iter().any(|c| c == path) {
            return self.toast(Level::Bad, "a collection root cannot be renamed");
        }
        let rel = crate::vault::page::rel_path(path, &self.cfg.root);
        let mut name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if path.is_file() {
            let ext = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
            name = name.trim_end_matches(&ext).trim_end_matches('.').to_string();
        }
        self.overlay = Some(Overlay::Prompt(Prompt {
            kind: PromptKind::Rename,
            title: format!("rename {rel}"),
            value: name,
            path: Some(path.to_path_buf()),
        }));
    }

    /// Ask for the name of a page to create inside `dir`. The prompt is a
    /// basename joined onto the folder, so the menu can't create elsewhere.
    fn prompt_new_page(&mut self, dir: &Path) {
        let rel = crate::vault::page::rel_path(dir, &self.cfg.root);
        self.overlay = Some(Overlay::Prompt(Prompt {
            kind: PromptKind::NewPage,
            title: format!("new page in {rel}"),
            value: String::new(),
            path: Some(dir.to_path_buf()),
        }));
    }

    pub(super) fn rename_path(&mut self, path: &Path, value: &str) {
        let Some(parent) = path.parent() else { return self.toast(Level::Bad, "cannot rename the checkout root") };
        if !path.exists() {
            return self.toast(Level::Bad, "no longer exists — reload");
        }
        let mut name = value.trim().replace(' ', "_");
        if name.is_empty() {
            return;
        }
        if path.is_file() {
            // Keep the file's own extension unless the new name already
            // carries it — so a `.png` stays a `.png` without this having to
            // know the set of extensions the tree lists.
            let ext = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
            if !ext.is_empty() && !name.to_ascii_lowercase().ends_with(&format!(".{}", ext.to_ascii_lowercase())) {
                name = format!("{name}.{ext}");
            }
        }
        let target = parent.join(&name);
        if target == path {
            return self.toast(Level::Info, "name unchanged");
        }
        if target.exists() {
            return self.toast(Level::Bad, "a page with that name already exists");
        }
        let was_open = self.open.as_ref().map(|o| o.page.path == *path).unwrap_or(false);
        match std::fs::rename(path, &target) {
            Ok(()) => {
                let rel = crate::vault::page::rel_path(path, &self.cfg.root);
                let rel2 = crate::vault::page::rel_path(&target, &self.cfg.root);
                self.open = None;
                self.reload_everything();
                if was_open {
                    self.open_path(&target, false);
                }
                self.toast(Level::Good, format!("renamed {rel} → {rel2}"));
            }
            Err(err) => self.toast(Level::Bad, format!("could not rename: {err}")),
        }
    }

    /// Deleting is destructive and never confirmed anywhere else in the app, so
    /// the menu first narrows to a confirm overlay carrying the same path.
    fn confirm_delete(&mut self, path: PathBuf, area: Rect) {
        self.overlay = Some(Overlay::Menu(Menu {
            path,
            items: vec![(MenuAction::Cancel, "cancel"), (MenuAction::ConfirmDelete, "delete")],
            selected: 0,
            area,
            pressed: None,
        }));
    }

    pub(super) fn delete_path(&mut self, path: &Path) {
        if self.tree.collection_paths().iter().any(|c| c == path) {
            return self.toast(Level::Bad, "a collection root cannot be deleted");
        }
        let rel = crate::vault::page::rel_path(path, &self.cfg.root);
        let editing_target = self.open.as_ref().is_some_and(|open| {
            let under = if path.is_dir() {
                open.page.path.starts_with(path)
            } else {
                open.page.path == *path
            };
            under && open.editing()
        });
        if editing_target {
            return self.toast(Level::Warn, "save or leave the editor first");
        }
        let result = if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        match result {
            Ok(()) => {
                let open_under = self.open.as_ref().is_some_and(|open| {
                    if path.is_dir() {
                        open.page.path.starts_with(path)
                    } else {
                        open.page.path == *path
                    }
                });
                if open_under {
                    self.open = None;
                    self.doc_stop = DocStop::Content;
                }
                self.reload_everything();
                self.toast(Level::Good, format!("deleted {rel}"));
            }
            Err(err) => self.toast(Level::Bad, format!("could not delete {rel}: {err}")),
        }
    }

    fn repo_config_key(&mut self, key: KeyEvent) {
        let Some(Overlay::RepoConfig(state)) = self.overlay.as_mut() else { return };
        let last = state.option_count().saturating_sub(1);
        match key.code {
            KeyCode::Esc => self.overlay = None,
            KeyCode::Down | KeyCode::Tab => state.selected = (state.selected + 1).min(last),
            KeyCode::Up | KeyCode::BackTab => state.selected = state.selected.saturating_sub(1),
            KeyCode::Backspace if state.selected == 0 => {
                state.url.pop();
            }
            KeyCode::Char(c) if state.selected == 0 && !key.modifiers.contains(KeyModifiers::CONTROL) => {
                state.url.push(c);
            }
            KeyCode::Enter => {
                let name = state.name.clone();
                let selected = state.selected;
                let url = state.url.trim().to_string();
                self.overlay = None;
                self.apply_repo_config(&name, selected, &url);
            }
            _ => {}
        }
    }

    /// Set `repositories.<name>` and, for a git URL or `local`, make sure the
    /// checkout has a repo to match. Local, fast operations — no network call
    /// happens here (a fresh remote is only pulled by `SyncRepos`), so this
    /// runs synchronously rather than through a native job.
    fn apply_repo_config(&mut self, name: &str, selected: usize, url: &str) {
        let new_url = match selected {
            1 => "local".to_string(),
            2 => "gdrive".to_string(),
            _ if url.is_empty() => {
                self.toast(Level::Warn, "enter a git URL, or pick local");
                self.open_repo_config(name);
                return;
            }
            _ => url.to_string(),
        };

        self.cfg.repositories.insert(name.to_string(), new_url.clone());
        if let Err(err) = self.cfg.save_repositories() {
            self.toast(Level::Bad, format!("config {name} failed: {err}"));
            return;
        }

        if new_url != "gdrive" {
            let repo_dir = self.cfg.root.join(name);
            if repo_dir.join(".git").is_dir() {
                crate::vault::git::set_remote(&repo_dir, &new_url);
            } else {
                crate::vault::git::ensure_local_repo(&self.cfg.root, name);
                if new_url != "local" {
                    crate::vault::git::set_remote(&self.cfg.root.join(name), &new_url);
                }
            }
        }

        self.toast(Level::Good, format!("{name}: configured"));
        self.refresh_git();
        self.reload_everything();
    }

    fn prompt_key(&mut self, key: KeyEvent) {
        let Some(Overlay::Prompt(prompt)) = self.overlay.as_mut() else { return };
        match key.code {
            KeyCode::Esc => self.overlay = None,
            KeyCode::Backspace => {
                prompt.value.pop();
            }
            KeyCode::Char(c) => prompt.value.push(c),
            KeyCode::Enter => {
                let path = prompt.path.clone();
                let kind = prompt.kind;
                let value = prompt.value.trim().to_string();
                self.overlay = None;
                if value.is_empty() {
                    return;
                }
                match kind {
                    PromptKind::Commit => self.run_commit(value),
                    PromptKind::NewPage => self.create_page(&value, path.as_deref()),
                    PromptKind::Rename => {
                        if let Some(path) = path {
                            self.rename_path(&path, &value);
                        }
                    }
                    PromptKind::NewProject => self.create_new_project(&value),
                    PromptKind::RenameProject => self.rename_project(path.as_deref(), &value),
                    PromptKind::DeleteProject => self.delete_project(path.as_deref(), &value),
                }
            }
            _ => {}
        }
    }

    pub(super) fn create_page(&mut self, rel: &str, base: Option<&Path>) {
        let rel = if rel.ends_with(".md") { rel.to_string() } else { format!("{rel}.md") };
        let root = &self.cfg.root;
        let base = base.unwrap_or(root);
        let path = crate::vault::links::normalize(&base.join(&rel));
        if !path.starts_with(root) {
            return self.toast(Level::Bad, "path escapes the checkout");
        }
        if path.exists() {
            self.open_path(&path, true);
            return self.toast(Level::Info, "page already exists");
        }
        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().replace('_', " "))
            .unwrap_or_default();
        let category = path
            .parent()
            .and_then(|p| p.strip_prefix(&self.cfg.root).ok())
            .map(|p| p.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"))
            .unwrap_or_default();
        let category = category.split_once('/').map(|(_, rest)| rest).unwrap_or(&category);
        let scaffold = format!(
            "---\ntitle: {title}\ntype: concept\ncategory: {category}\nrationale: \nstatus: draft\nsources: []\n---\n# {title}\n\n"
        );
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&path, scaffold) {
            Ok(()) => {
                self.index.refresh(&path);
                self.tree.rebuild();
                self.open_path(&path, true);
                self.tree.reveal(&path);
                self.run(Cmd::Edit);
                self.toast(Level::Good, format!("created {rel}"));
            }
            Err(err) => self.toast(Level::Bad, format!("could not create {rel}: {err}")),
        }
    }
}
