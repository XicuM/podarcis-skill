//! Application state and the command dispatcher.
//!
//! Everything the user can do arrives here as a `Cmd` from the keymap or an
//! overlay, so there is one list of behaviours and one place they are
//! implemented. Rendering reads this state and writes nothing back except the
//! pane geometry it just measured.
//!
//! This file holds the *state* — `App` and the types it is made of — and the
//! handful of methods that are about the whole app rather than one part of it.
//! The behaviour is split across the sibling modules below, which were one
//! 6,000-line file until the sections it had grown were given their own.
//! Inherent `impl App` blocks merge across the crate, so `App` is one type
//! however many files describe it, and a method only needs `pub(super)` when a
//! *sibling* section calls it — plain `fn` still means "private to this part".

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Rect};

use crate::actions::{JobResult, JobRunner, NativeOutcome};
use crate::config::Config;
use crate::control::{Command as Control, Request};
use crate::editor::{Editor, Save};
use crate::event::AppEvent;
use crate::herdr;
use crate::keymap::{self, Cmd, Ctx, Resolved};
use crate::platform;
use crate::search::{self, Hit};
use crate::theme::{Flavor, Theme};
use crate::ui::markdown::{self, Mark};
use crate::vault::git::{GitMap, TrackState};
use crate::vault::index::Index;
use crate::vault::links::LinkKind;
use crate::vault::page::Page;
use crate::vault::tree::Tree;

// Behaviour lives in these; the state and its types stay here.
mod agents;
mod commands;
mod documents;
mod editing;
mod jobs;
mod keys;
mod mouse;
mod overlays;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Tree,
    Doc,
    Sidebar,
}

/// Which box inside `Focus::Doc` currently has the cursor: the reader itself,
/// or the sources/backlinks inspector below it. The two are drawn as separate
/// bordered panes, so Tab needs to be able to stop at each in turn instead of
/// treating the whole column as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocStop {
    Content,
    Sources,
}

/// One Tab-cycle stop: either a specific collection box in the tree column,
/// one of the two boxes in the doc column, or the agents pane. Distinct from
/// `Focus`, which only tracks the coarse column — this is what lets each
/// visible bordered box get its own stop instead of grouping a whole column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    TreeSection(usize),
    DocContent,
    DocSources,
    Sidebar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Good,
    Warn,
    Bad,
}

pub struct Toast {
    pub text: String,
    pub level: Level,
    pub at: Instant,
}

/// How long a toast stays up. Long enough to read a sentence, short enough that
/// it never becomes furniture.
const TOAST_TTL: Duration = Duration::from_secs(6);

/// Narrowest a side pane may be dragged. Below this it is a sliver that can
/// only be widened again by luck.
const MIN_PANE: u16 = 12;

/// How long a single click stays crouched, ready to become a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

pub struct Open {
    pub page: Page,
    pub doc: markdown::Doc,
    pub doc_width: u16,
    pub scroll: usize,
    /// Display columns panned off the left edge. Non-zero only for a table too
    /// wide for the pane — a CSV with more columns than fit, or a broad
    /// markdown table.
    pub hscroll: usize,
    pub inspect_scroll: usize,
    pub link: Option<usize>,
    pub editor: Option<Editor>,
    pub selection: Option<markdown::Selection>,
    /// A citation clicked (or navigated to) in the body, highlighted in the
    /// inspector's sources section.
    pub selected_citation: Option<String>,
    /// Set alongside `selected_citation` when the selection came from a fresh
    /// click, so the inspector scrolls it into view exactly once — a later
    /// manual scroll away from it is left alone.
    pub citation_scroll_pending: bool,
}

/// The in-page find bar — `ctrl+f`. Scoped to the open page, unlike the
/// finder's text mode, which sweeps the whole vault.
///
/// Only the query and which hit is current live here: the hits themselves are
/// recomputed from the rendered `Doc` on every use. A page is small enough that
/// this costs nothing, and it means a resize — which rewraps, moving every
/// rendered column — cannot leave a highlight pointing at the wrong text.
pub struct PageFind {
    pub query: String,
    pub current: usize,
}

/// Files the reader never parses: they are handed to the system's default
/// viewer instead. A PDF and an image are both binary, so rendering either
/// one as markdown would only paint mojibake.
pub fn is_external(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(),
        Some("pdf" | "png" | "jpg" | "jpeg")
    )
}

impl Open {
    fn load(path: &Path, root: &Path, width: u16, theme: &Theme) -> Option<Self> {
        let page = Page::load(path, root).ok()?;
        let doc = if Self::is_csv(path) {
            crate::ui::csv::Grid::parse(&page.body).to_doc(width, theme)
        } else {
            markdown::render(&page.body, width, theme)
        };
        Some(Self {
            page,
            doc,
            doc_width: width,
            scroll: 0,
            hscroll: 0,
            inspect_scroll: 0,
            link: None,
            editor: None,
            selection: None,
            selected_citation: None,
            citation_scroll_pending: false,
        })
    }

    pub(crate) fn is_csv(path: &Path) -> bool {
        path.extension().and_then(|e| e.to_str()) == Some("csv")
    }

    /// A CSV renders as a table rather than markdown — navigating one is the
    /// same reader, but its `doc` is a box-drawn grid of the raw file.
    pub fn csv(&self) -> bool {
        Self::is_csv(&self.page.path)
    }

    pub(crate) fn reflow(&mut self, width: u16, theme: &Theme) {
        if width == self.doc_width {
            return;
        }
        self.selection = None;
        let anchor = self.doc.source_for_line(self.scroll);
        self.doc = if self.csv() {
            crate::ui::csv::Grid::parse(&self.page.body).to_doc(width, theme)
        } else {
            markdown::render(&self.page.body, width, theme)
        };
        self.doc_width = width;
        self.hscroll = 0;
        self.scroll = self.doc.line_for_source(anchor);
    }

    pub fn editing(&self) -> bool {
        self.editor.is_some()
    }

    /// Raw source lines touched by a rendered selection.
    ///
    /// Rendering strips markdown syntax and rewraps prose, so there is no exact
    /// column mapping back to source text; instead this resolves every source
    /// line any part of the selection's rendered lines came from and returns
    /// them verbatim, syntax included.
    pub fn selected_source_text(&self, sel: markdown::Selection) -> String {
        let (start, end) = sel.range();
        if start == end {
            return String::new();
        }
        let start_src = self.doc.source_for_line(start.line);
        let end_src = self.doc.source_for_line(end.line);
        self.page
            .body
            .lines()
            .skip(start_src)
            .take(end_src.saturating_sub(start_src) + 1)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub struct Finder {
    pub mode: search::Mode,
    pub query: String,
    pub hits: Vec<Hit>,
    pub selected: usize,
    pub collection: Option<&'static str>,
    /// Job id of an in-flight semantic search.
    pub running: Option<u64>,
    pub warning: Option<String>,
}

pub struct Palette {
    pub query: String,
    pub items: Vec<(Cmd, &'static str, String)>,
    pub selected: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    Commit,
    NewPage,
    Rename,
    NewProject,
    RenameProject,
    DeleteProject,
}

pub struct Prompt {
    pub kind: PromptKind,
    pub title: String,
    pub value: String,
    /// The file or folder a rename prompt applies to.
    pub path: Option<PathBuf>,
}

pub struct RepoConfig {
    pub name: String,
    /// 0 = apply git URL, 1 = local-only, 2 = gdrive (sources only).
    pub selected: usize,
    pub url: String,
}

impl RepoConfig {
    pub fn option_count(&self) -> usize {
        if self.name == "sources" {
            3
        } else {
            2
        }
    }
}

/// A question an agent put to the user, and the request still waiting on the
/// answer. Holding the request here is what makes the agent's CLI call block
/// until a key is pressed.
pub struct Ask {
    pub question: String,
    pub options: Vec<String>,
    pub selected: usize,
    pub request: Request,
}

pub enum Overlay {
    Finder(Finder),
    Palette(Palette),
    Help { scroll: usize },
    Outline { selected: usize },
    Prompt(Prompt),
    RepoConfig(RepoConfig),
    /// A right-click context menu on a tree row.
    Menu(Menu),
    /// The theme picker. `original` is restored if the picker is cancelled, so
    /// browsing twenty themes never costs you the one you had.
    Themes { selected: usize, original: Flavor },
    /// The project switcher. Name and path only: the registry's description
    /// field is bookkeeping, not something worth a column in a switcher.
    Projects { selected: usize, items: Vec<(String, PathBuf)> },
    /// A question from an agent on the control socket.
    Ask(Ask),
    /// The apm extensions browser — skills and MCP servers.
    Extensions(Extensions),
}

/// One action `apm` can be asked to run on the selected extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionAction {
    Install,
    Update,
    Uninstall,
}

impl ExtensionAction {
    fn verb(self) -> &'static str {
        match self {
            ExtensionAction::Install => "install",
            ExtensionAction::Update => "update",
            ExtensionAction::Uninstall => "uninstall",
        }
    }
}

pub struct Extensions {
    pub selected: usize,
    pub items: Vec<crate::extensions::ExtensionItem>,
    /// Raw stdout/stderr of the last `apm` invocation — shown verbatim, since
    /// apm's own output isn't machine-parseable.
    pub log: Option<String>,
    /// Uninstalling is destructive, so it is gated behind one extra keypress,
    /// the same way tree-row delete is (`MenuAction::Delete`).
    pub confirm: Option<ExtensionAction>,
}

/// One action from the tree's right-click menu. `Delete` opens a confirmation
/// menu whose only committing item is `ConfirmDelete`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Open,
    NewPage,
    Rename,
    Delete,
    ConfirmDelete,
    CopyRelative,
    CopyAbsolute,
    Cancel,
}

/// The context menu shown for the tree row under a right-click. Kept as its own
/// overlay rather than a generic list because it carries the row's path and the
/// popup rect the mouse resolves clicks against.
pub struct Menu {
    /// The row the menu was opened for; a confirmation menu keeps it untouched.
    pub path: PathBuf,
    pub items: Vec<(MenuAction, &'static str)>,
    pub selected: usize,
    /// Popup rectangle, computed from the click position and the layout.
    pub area: Rect,
    /// Row the mouse pressed down on, so a release on the same row activates.
    pub pressed: Option<usize>,
}

/// One collection's box inside the tree column, for drawing and mouse hits.
#[derive(Clone, Copy, Debug, Default)]
pub struct TreePane {
    pub area: Rect,
    pub inner: Rect,
    /// Index of the collection header row in `Tree::rows`.
    pub header: usize,
    /// First child row (header + 1). Equal to `end` when collapsed.
    pub start: usize,
    /// Exclusive end of this collection's rows.
    pub end: usize,
    pub offset: usize,
    /// The one cell that folds this collection away, or brings it back.
    pub fold: Option<Rect>,
    pub folded: bool,
}

/// A clickable backlink in the inspector pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectorBacklink {
    pub row: usize,
    pub col_start: u16,
    pub col_end: u16,
    pub path: PathBuf,
}

/// Geometry from the last frame, so mouse events and the pty know where things
/// are. Written by the renderer, read by the app.
#[derive(Clone, Debug, Default)]
pub struct Areas {
    pub tree: Rect,
    pub doc: Rect,
    pub doc_main: Rect,
    pub doc_body: Rect,
    pub sidebar: Rect,
    pub inspector: Rect,
    /// Inner content rect of the inspector, borders and padding excluded —
    /// what a click's `(x, y)` needs to turn into a row index.
    pub inspector_body: Rect,
    /// The citation id behind each currently visible inspector row (after
    /// wrapping and scrolling), aligned to `inspector_body`, so a click can be
    /// mapped back to a source without re-deriving the layout.
    pub inspector_rows: Vec<Option<String>>,
    /// Clickable backlinks in the visible inspector rows.
    pub inspector_backlinks: Vec<InspectorBacklink>,
    /// Hit-testing for each collection pane inside the tree column.
    pub tree_panes: Vec<TreePane>,
    /// Boundaries between stacked collection boxes, as `(index of the box
    /// above, the row they meet on)`. The last box ends at the column's edge
    /// and so has none.
    pub collection_dividers: Vec<(usize, u16)>,
    /// Column of the tree/document divider, when the tree is shown.
    pub tree_divider: Option<u16>,
    /// Column of the document/agents divider, when the agents pane is shown.
    pub sidebar_divider: Option<u16>,
    /// Row of the document/inspector divider, when the inspector is shown.
    pub inspector_divider: Option<u16>,
    /// The one cell that collapses the tree when shown, or reopens it when
    /// collapsed — always sitting on whichever border is currently visible.
    pub tree_toggle: Option<Rect>,
    /// Same idea, for the agents sidebar.
    pub sidebar_toggle: Option<Rect>,
    /// The one cell that collapses the sources inspector when the document is
    /// open, or reopens it when collapsed — on the divider row when shown, on
    /// the document's bottom border when not.
    pub inspector_toggle: Option<Rect>,
    /// Navigation arrow to go back to the last visited page.
    pub nav_back: Option<Rect>,
    /// Navigation arrow to go forward (reverse) in history.
    pub nav_forward: Option<Rect>,
    /// Clickable edit button on the document header.
    pub doc_edit: Option<Rect>,
    /// Clickable project selector badge on the bottom bar.
    pub project_selector: Option<Rect>,
    /// The narrow-terminal tab strip: one clickable rect per pane, labelled,
    /// replacing the side-by-side layout when there isn't room for it. Empty
    /// on a wide terminal, where the panes sit next to each other instead.
    pub tab_bar: Vec<(Focus, &'static str, Rect)>,
}

/// Which divider the pointer is dragging.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Divider {
    Tree,
    /// The boundary under collection box `usize` in the tree column.
    Collection(usize),
    Sidebar,
    Inspector,
}

/// Which scrollbar the pointer is dragging.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollbarDrag {
    Doc,
    Inspector,
}

/// A place the reader can be.
///
/// `Home` is the screen with the mark on it. It is a destination in its own
/// right, not the absence of one, which is why the history holds this rather
/// than a bare path: while it held paths, opening your first page pushed
/// nothing, so back had nowhere to go and home was unreachable for the rest
/// of the session.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Spot {
    Home,
    Page(PathBuf),
}

pub struct App {
    pub cfg: Config,
    pub theme: Theme,
    pub tree: Tree,
    pub git: GitMap,
    pub git_track: Vec<(PathBuf, TrackState)>,
    pub index: Index,
    pub components: platform::Components,
    pub open: Option<Open>,
    pub focus: Focus,
    pub doc_stop: DocStop,
    pub overlay: Option<Overlay>,
    /// The in-page find bar, open over the reader.
    pub find: Option<PageFind>,
    pub toasts: Vec<Toast>,
    pub jobs: JobRunner,
    pub sidebar: Option<herdr::pty::Pane>,
    pub sidebar_error: Option<String>,
    pub show_tree: bool,
    pub show_sidebar: bool,
    pub show_inspector: bool,
    pub zoom: bool,
    pub back: Vec<Spot>,
    pub forward: Vec<Spot>,
    pub pending: Option<char>,
    pub finder_engine: search::Engine,
    pub quit: bool,
    /// Like `quit`, but the process is replaced with a fresh copy of itself
    /// instead of ending — how the palette's "restart" picks up a rebuild.
    pub restart: bool,
    /// Set by a first `q` while a job is running.
    quit_confirmed: bool,
    /// Set by a first attempt to leave the editor with unsaved changes.
    discard_armed: bool,
    /// Set by a save that found the file changed on disk. The next ctrl+s
    /// overwrites, parking the outside version beside the page.
    overwrite_armed: bool,
    /// The divider currently being dragged, if any.
    pub dragging: Option<Divider>,
    /// The scrollbar currently being dragged, if any.
    pub dragging_scrollbar: Option<ScrollbarDrag>,
    /// Whether mouse drag is currently selecting text in the reader.
    pub selecting_text: bool,
    /// The current selection drag started on the right button: releasing then
    /// pays the selection to the agents pane as a mention instead of copying
    /// it to the clipboard.
    selecting_right: bool,
    pub indexing: bool,
    /// When this instance started. The spinner phase is read off it, so every
    /// spinner on screen turns together regardless of redraw cadence.
    started: Instant,
    /// Source-line ranges an agent asked the reader to mark, by page. In
    /// memory only: an agent pointing at a passage must never edit it.
    pub marks: HashMap<PathBuf, Vec<Mark>>,
    /// Control requests accepted but not yet acted on, because the editor was
    /// open or a question was already on screen.
    pub deferred: Vec<Request>,
    /// Where the control socket is listening, when one could be bound.
    pub control_socket: Option<PathBuf>,
    /// Engine version from `pyproject.toml`, for the status bar.
    pub engine_version: Option<String>,
    /// A splash one-liner from `config.yaml` for the status bar's right corner.
    pub oneline: Option<String>,
    /// Active project name displayed on the bottom bar selector.
    pub project_name: String,
    pub areas: Areas,
    pub tx: Sender<AppEvent>,
    /// Cursor into `index.all_findings()` for `n` / `N`.
    finding_cursor: Option<usize>,
    /// Every mouse `Down` increments this. Two clicks only form a double click
    /// when they are adjacent (one generation apart), so a click in another
    /// pane between the two quietly breaks the pair.
    mouse_down: u64,
    /// The timestamp, row and `mouse_down` generation of the most recent click
    /// in the tree, used to recognise a second click on the same row.
    last_tree_click: Option<(Instant, usize, u64)>,
    /// Same as `last_tree_click`, for a source row in the inspector.
    last_inspector_click: Option<(Instant, usize, u64)>,
}

impl App {
    pub fn new(cfg: Config, tx: Sender<AppEvent>) -> Self {
        let theme = Theme::new(cfg.flavor);
        let engine_version = crate::config::engine_version(&cfg.root);
        let oneline = cfg.oneline();
        let collections: Vec<PathBuf> = cfg.collections().into_iter().map(|(_, p)| p).collect();
        let git = GitMap::scan(&cfg.root, &collections);
        let git_track = collections
            .iter()
            .map(|p| (p.clone(), crate::vault::git::track_state(p, &cfg.root)))
            .collect();
        let reg = crate::project::ProjectRegistry::load();
        let project_name = reg
            .projects
            .iter()
            .find(|(_, entry)| entry.path == cfg.root)
            .map(|(n, _)| n.clone())
            .unwrap_or_else(|| {
                cfg.root
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("project")
                    .to_string()
            });
        let components = platform::discover_components(&cfg.root);
        Self {
            tree: Tree::new(&cfg.root, collections),
            git,
            git_track,
            show_tree: cfg.tree_open,
            show_sidebar: cfg.sidebar_open,
            theme,
            cfg,
            index: Index::default(),
            components,
            open: None,
            focus: Focus::Tree,
            doc_stop: DocStop::Content,
            overlay: None,
            find: None,
            toasts: Vec::new(),
            jobs: JobRunner::default(),
            sidebar: None,
            sidebar_error: None,
            show_inspector: true,
            zoom: false,
            back: Vec::new(),
            forward: Vec::new(),
            pending: None,
            finder_engine: search::Engine::default(),
            quit: false,
            quit_confirmed: false,
            restart: false,
            discard_armed: false,
            overwrite_armed: false,
            dragging: None,
            dragging_scrollbar: None,
            selecting_text: false,
            selecting_right: false,
            engine_version,
            oneline,
            project_name,
            indexing: true,
            started: Instant::now(),
            marks: HashMap::new(),
            deferred: Vec::new(),
            control_socket: None,
            areas: Areas::default(),
            tx,
            finding_cursor: None,
            mouse_down: 0,
            last_tree_click: None,
            last_inspector_click: None,
        }
    }

    fn hit_tree_title(&self, x: u16, y: u16) -> Option<String> {
        for pane in &self.areas.tree_panes {
            if x < pane.area.x || x >= pane.area.x + pane.area.width {
                continue;
            }
            if y != pane.area.y {
                continue;
            }
            return self.tree.rows.get(pane.header).map(|r| r.label.clone());
        }
        None
    }

    /// The collections in the tree column, by name, in the order they are drawn.
    ///
    /// The name is the directory's own — `wiki`, `workspace`, `sources` — which
    /// is what `collection_heights` and `collapsed_collections` are keyed by,
    /// so the geometry survives a checkout moving.
    pub fn collection_names(&self) -> Vec<String> {
        self.tree
            .collection_paths()
            .iter()
            .map(|p| p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string())
            .collect()
    }

    /// Fold or unfold the collection the tree cursor is in.
    fn toggle_collection(&mut self) {
        if let Some(i) = self.tree.section_of(self.tree.selected) {
            self.fold_collection(i);
        }
    }

    /// Fold or unfold collection `i`. The handle and the key both land here.
    pub fn fold_collection(&mut self, i: usize) {
        let names = self.collection_names();
        let Some(name) = names.get(i).cloned() else { return };
        if !self.cfg.collapsed_collections.remove(&name) {
            self.cfg.collapsed_collections.insert(name);
        }
        self.persist_widths();
    }

    fn hit_tree_row(&self, x: u16, y: u16) -> Option<usize> {
        if self.areas.tree_panes.is_empty() {
            let row = (y.saturating_sub(self.areas.tree.y + 1)) as usize;
            return Some(row.min(self.tree.rows.len().saturating_sub(1)));
        }
        for pane in &self.areas.tree_panes {
            if y < pane.area.y || y >= pane.area.y + pane.area.height {
                continue;
            }
            if x < pane.area.x || x >= pane.area.x + pane.area.width {
                continue;
            }
            if pane.inner.is_empty() || y < pane.inner.y {
                return Some(pane.header);
            }
            let row = (y.saturating_sub(pane.inner.y)) as usize;
            let i = pane.start + pane.offset + row;
            if i < pane.end {
                return Some(i);
            }
            return Some(pane.header);
        }
        None
    }

    fn open_repo_config(&mut self, name: &str) {
        let url = self
            .cfg
            .repo_url(name)
            .filter(|u| !u.is_empty() && *u != "local" && *u != "gdrive")
            .unwrap_or("")
            .to_string();
        self.overlay = Some(Overlay::RepoConfig(RepoConfig {
            name: name.to_string(),
            selected: 0,
            url,
        }));
    }

    pub fn collection_dirs(&self) -> Vec<PathBuf> {
        self.cfg.collections().into_iter().map(|(_, p)| p).collect()
    }

    pub fn refresh_git(&mut self) {
        let dirs = self.collection_dirs();
        self.git = GitMap::scan(&self.cfg.root, &dirs);
        self.git_track = dirs
            .iter()
            .map(|p| (p.clone(), crate::vault::git::track_state(p, &self.cfg.root)))
            .collect();
    }

    pub fn track_for(&self, path: &Path) -> &TrackState {
        self.git_track
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, s)| s)
            .unwrap_or(&TrackState::Untracked)
    }

    // ---------------------------------------------------------------- toasts

    pub fn toast(&mut self, level: Level, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), level, at: Instant::now() });
        // Three is as many as can be read at a glance.
        while self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    pub fn expire_toasts(&mut self) {
        self.toasts.retain(|t| t.at.elapsed() < TOAST_TTL);
    }

    // -------------------------------------------------------------- motion

    /// Is anything on screen driven by time rather than by input?
    ///
    /// The answer decides whether a `Tick` costs a frame. A static `◐` beside
    /// a running job reads as a hung process, so background work spins — but
    /// an idle reader has nothing moving and must not repaint four times a
    /// second for the rest of the session.
    pub fn animating(&self) -> bool {
        self.indexing || !self.jobs.labels().is_empty() || !self.toasts.is_empty()
    }

    /// The current frame of the app's one spinner.
    ///
    /// Driven by the wall clock rather than a counter, so every spinner on
    /// screen is in step and none of them depends on how often we happened to
    /// redraw.
    pub fn spinner(&self) -> char {
        const FRAMES: [char; 8] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠇'];
        let ms = self.started.elapsed().as_millis() as usize;
        FRAMES[(ms / 120) % FRAMES.len()]
    }

}

fn suffix(detail: &str) -> String {
    if detail.trim().is_empty() {
        String::new()
    } else {
        format!(" — {}", detail.trim())
    }
}

fn first_line(text: &str) -> Option<String> {
    text.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string)
}

/// Palette entries, filtered by a plain substring match over the title — a
/// command list is short enough that fuzzy ranking would only add surprise.
pub fn palette_items(query: &str) -> Vec<(Cmd, &'static str, String)> {
    let needle = query.trim().to_lowercase();
    let mut seen: Vec<Cmd> = Vec::new();
    let mut out = Vec::new();
    for binding in keymap::BINDINGS {
        if !binding.cmd.palette_visible() || seen.contains(&binding.cmd) {
            continue;
        }
        let hay = format!("{} {}", binding.group, binding.title).to_lowercase();
        if !needle.is_empty() && !hay.contains(&needle) {
            continue;
        }
        seen.push(binding.cmd);
        let keys = keymap::BINDINGS
            .iter()
            .filter(|b| b.cmd == binding.cmd)
            .flat_map(|b| b.keys.iter().copied())
            .collect::<Vec<_>>()
            .join("  ");
        out.push((binding.cmd, binding.title, keys));
    }
    out
}

/// Build a `Config` and `App` for tests without touching a real checkout.
#[cfg(test)]
pub fn test_app(root: &Path) -> (App, std::sync::mpsc::Receiver<AppEvent>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let cfg = Config::load(root);
    let mut app = App::new(cfg, tx);
    app.areas = Areas {
        tree: Rect::new(0, 0, 30, 40),
        doc: Rect::new(30, 0, 60, 40),
        doc_main: Rect::new(30, 0, 60, 30),
        doc_body: Rect::new(30, 0, 60, 30),
        sidebar: Rect::new(90, 0, 40, 40),
        inspector: Rect::new(30, 30, 60, 10),
        inspector_body: Rect::default(),
        inspector_rows: Vec::new(),
        tree_panes: Vec::new(),
        collection_dividers: Vec::new(),
        tree_divider: Some(29),
        sidebar_divider: Some(90),
        inspector_divider: Some(30),
        tree_toggle: Some(Rect::new(29, 20, 1, 1)),
        sidebar_toggle: Some(Rect::new(90, 20, 1, 1)),
        inspector_toggle: Some(Rect::new(75, 30, 1, 1)),
        ..Default::default()
    };
    app.index = Index::build(root, &app.collection_dirs());
    app.indexing = false;
    (app, rx)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests;
