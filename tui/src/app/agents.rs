//! The herdr pane and the agents running inside it.
//!
//! Split out of `app.rs` verbatim; inherent `impl App` blocks merge
//! across the crate, so this is the same type seen from another file.

use super::*;

impl App {
    /// Marks an agent left on `path`, for the reader to tint.
    pub fn marks_for(&self, path: &Path) -> &[Mark] {
        self.marks.get(path).map_or(&[], Vec::as_slice)
    }

    /// A control request, straight off the socket.
    ///
    /// Nothing here steals the terminal from someone mid-sentence: while the
    /// editor is open — or a question is already on screen — the request is
    /// parked and acknowledged, and runs the moment the way is clear.
    pub fn on_control(&mut self, request: Request) {
        if self.control_blocked(&request.cmd) {
            request.deferred();
            self.announce_deferred(&request.cmd);
            self.deferred.push(request);
            return;
        }
        self.apply_control(request);
    }

    /// Run whatever was parked, oldest first, for as long as the way is clear.
    pub fn flush_control(&mut self) {
        while let Some(i) = self.deferred.iter().position(|r| !self.control_blocked(&r.cmd)) {
            let request = self.deferred.remove(i);
            self.apply_control(request);
        }
    }

    fn control_blocked(&self, cmd: &Control) -> bool {
        let editing = self.open.as_ref().is_some_and(Open::editing);
        match cmd {
            // Passive: a tint repaints under whatever you are doing.
            Control::Highlight { .. } => false,
            Control::Open { .. } => editing,
            Control::Ask { .. } => editing || self.overlay.is_some(),
        }
    }

    fn announce_deferred(&mut self, cmd: &Control) {
        let what = match cmd {
            Control::Open { path, .. } => format!("agent wants to open {path}"),
            Control::Ask { .. } => "agent is waiting on a question".to_string(),
            Control::Highlight { .. } => return,
        };
        self.toast(Level::Info, format!("{what} — after you leave the editor"));
    }

    fn apply_control(&mut self, request: Request) {
        match request.cmd.clone() {
            Control::Open { path, line } => match self.resolve_control_path(&path) {
                Ok(target) => {
                    self.open_path(&target, true);
                    self.tree.reveal(&target);
                    // Agents count file lines, the way `Read` and `sed` show
                    // them; the reader counts body lines.
                    if let (Some(open), Some(line)) = (self.open.as_mut(), line) {
                        let body = line.saturating_sub(1).saturating_sub(open.page.body_start);
                        open.scroll = open.doc.line_for_source(body);
                    }
                    self.toast(Level::Info, format!("agent opened {path}"));
                    request.ok(serde_json::json!({"path": target.display().to_string()}));
                }
                Err(err) => request.err(err),
            },
            Control::Highlight { path, ranges, label, clear } => {
                let target = match path.as_deref().map(|p| self.resolve_control_path(p)).transpose() {
                    Ok(target) => target,
                    Err(err) => return request.err(err),
                };
                match (&target, clear) {
                    (None, _) => self.marks.clear(),
                    (Some(target), true) => {
                        self.marks.remove(target);
                    }
                    (Some(_), false) => {}
                }
                let count = ranges.len();
                if let Some(target) = target.filter(|_| !ranges.is_empty()) {
                    let offset = self.body_start(&target);
                    let marks = ranges.into_iter().map(|(start, end)| Mark {
                        start: start.saturating_sub(1).saturating_sub(offset),
                        end: end.saturating_sub(1).saturating_sub(offset),
                        label: label.clone(),
                    });
                    self.marks.entry(target).or_default().extend(marks);
                    let what = label.unwrap_or_else(|| "marked".to_string());
                    self.toast(Level::Warn, format!("{what} — {count} passage(s) flagged by an agent"));
                }
                request.ok(serde_json::json!({"marks": count}));
            }
            Control::Ask { question, options } => {
                self.overlay = Some(Overlay::Ask(Ask { question, options, selected: 0, request }));
                self.focus = Focus::Doc;
            }
        }
    }

    /// Where `path`'s body begins, so a file line an agent quoted can be
    /// turned into the body line the reader renders. The open page already
    /// knows; any other one costs a read, which a highlight can afford.
    fn body_start(&self, path: &Path) -> usize {
        match self.open.as_ref().filter(|o| o.page.path == path) {
            Some(open) => open.page.body_start,
            None => Page::load(path, &self.cfg.root).map_or(0, |page| page.body_start),
        }
    }

    /// Resolve an agent-supplied path against the checkout.
    ///
    /// Absolute paths and `..` escapes are refused rather than followed: the
    /// control socket reaches this project's collections, not the filesystem.
    fn resolve_control_path(&self, path: &str) -> Result<PathBuf, String> {
        let candidate = Path::new(path);
        let rel = candidate.strip_prefix(&self.cfg.root).unwrap_or(candidate);
        if rel.is_absolute() || rel.components().any(|c| c == std::path::Component::ParentDir) {
            return Err(format!("{path} is outside the checkout"));
        }
        let target = self.cfg.root.join(rel);
        if !target.exists() {
            return Err(format!("{path} does not exist"));
        }
        Ok(target)
    }

    pub(super) fn ask_key(&mut self, key: KeyEvent) {
        let Some(Overlay::Ask(ask)) = self.overlay.as_mut() else { return };
        match key.code {
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                ask.selected = (ask.selected + 1) % ask.options.len();
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                ask.selected = (ask.selected + ask.options.len() - 1) % ask.options.len();
            }
            KeyCode::Esc => {
                let Some(Overlay::Ask(ask)) = self.overlay.take() else { return };
                // A dismissed question is an answer: "not now", not silence.
                ask.request.ok(serde_json::json!({"cancelled": true, "answer": null}));
                self.toast(Level::Info, "question dismissed");
            }
            KeyCode::Enter => {
                let Some(Overlay::Ask(ask)) = self.overlay.take() else { return };
                let answer = ask.options[ask.selected].clone();
                ask.request.ok(serde_json::json!({
                    "answer": answer,
                    "index": ask.selected,
                    "cancelled": false,
                }));
                self.toast(Level::Good, format!("answered: {answer}"));
            }
            // A digit picks its option outright, so a two-way question is one
            // keystroke rather than an arrow and a return.
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                let pick = c.to_digit(10).unwrap() as usize - 1;
                if pick < ask.options.len() {
                    ask.selected = pick;
                    self.ask_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                }
            }
            _ => {}
        }
    }
}
