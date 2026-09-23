//! Tests for `app`, moved out of `app.rs` unchanged but for one level of
//! indentation. `super` is still the `app` module, so every use is as it was.

use super::*;

struct Vault(PathBuf);

impl Vault {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("podarcis-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".podarcis")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "# agents\n").unwrap();
        std::fs::write(dir.join(".podarcis/config.yaml"), "engines:\n  qmd: true\n").unwrap();
        for (rel, body) in [
            ("wiki/_index.md", "# Wiki\n"),
            (
                "wiki/a.md",
                "---\ntitle: Alpha\ntype: concept\ncategory: c\nrationale: r\nsources:\n  - id: s1\n    resource: ../sources/s1/metadata.md\n---\n# Alpha\n\nSee [Beta](b.md) and [gone](nope.md).\n\n## Section two\n\nCites[^s1].\n",
            ),
            ("wiki/b.md", "---\ntitle: Beta\ntype: concept\ncategory: c\nrationale: r\n---\n# Beta\n\nLeaf page.\n"),
            ("workspace/p.md", "---\ntitle: Protocol\ntype: protocol\ncategory: c\nrationale: r\n---\n# Protocol\n"),
        ] {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        Self(dir)
    }
    fn app(&self) -> App {
        test_app(&self.0).0
    }
    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
    fn write(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ch(c: char) -> KeyEvent {
    let mods = if c.is_uppercase() { KeyModifiers::SHIFT } else { KeyModifiers::NONE };
    KeyEvent::new(KeyCode::Char(c), mods)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn control(cmd: Control) -> (Request, std::sync::mpsc::Receiver<serde_json::Value>) {
    Request::for_test(cmd)
}

#[test]
fn an_agent_can_open_a_page_at_a_line() {
    let v = Vault::new("control-open");
    let mut app = v.app();
    // File line 12 is body line 2: nine lines of frontmatter precede it.
    let (req, rx) = control(Control::Open { path: "wiki/a.md".into(), line: Some(12) });
    app.on_control(req);

    let open = app.open.as_ref().unwrap();
    assert_eq!(open.page.title(), "Alpha");
    assert_eq!(open.scroll, open.doc.line_for_source(2), "agents count file lines");
    assert_eq!(rx.try_recv().unwrap()["ok"], serde_json::json!(true));
}

#[test]
fn an_agent_cannot_reach_outside_the_checkout() {
    let v = Vault::new("control-escape");
    let mut app = v.app();
    for path in ["../../etc/passwd", "/etc/passwd"] {
        let (req, rx) = control(Control::Open { path: path.into(), line: None });
        app.on_control(req);
        assert!(app.open.is_none(), "{path} was opened");
        assert_eq!(rx.try_recv().unwrap()["ok"], serde_json::json!(false));
    }
}

#[test]
fn an_open_never_interrupts_the_editor_and_runs_once_it_closes() {
    let v = Vault::new("control-defer");
    let mut app = v.app();
    app.open_path(&v.path("wiki/b.md"), true);
    app.run(Cmd::Edit);

    let (req, rx) = control(Control::Open { path: "wiki/a.md".into(), line: None });
    app.on_control(req);
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Beta", "the editor kept the page");
    assert_eq!(rx.try_recv().unwrap()["deferred"], serde_json::json!(true));
    assert_eq!(app.deferred.len(), 1);

    app.run(Cmd::LeaveEdit);
    app.flush_control();
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Alpha");
    assert!(app.deferred.is_empty());
}

#[test]
fn a_highlight_tints_the_lines_it_names_and_labels_the_first_one() {
    let v = Vault::new("control-mark");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    let (req, _rx) = control(Control::Highlight {
        path: Some("wiki/a.md".into()),
        ranges: vec![(12, 12)],
        label: Some("unsourced".into()),
        clear: false,
    });
    app.on_control(req);

    // File line 12 is body line 2: nine lines of frontmatter precede it.
    let marks = app.marks_for(&v.path("wiki/a.md"));
    assert_eq!(marks, [Mark { start: 2, end: 2, label: Some("unsourced".into()) }]);

    let open = app.open.as_ref().unwrap();
    let rendered = open
        .doc
        .to_lines(&app.theme, &markdown::Overlays { marks, ..Default::default() }, 0, open.doc.height())
        .iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("◂ unsourced"), "{rendered}");

    app.run(Cmd::ClearMarks);
    assert!(app.marks_for(&v.path("wiki/a.md")).is_empty());
}

#[test]
fn a_question_blocks_until_a_key_answers_it() {
    let v = Vault::new("control-ask");
    let mut app = v.app();
    let (req, rx) = control(Control::Ask {
        question: "Merge these two pages?".into(),
        options: vec!["yes".into(), "no".into()],
    });
    app.on_control(req);
    assert!(matches!(app.overlay, Some(Overlay::Ask(_))));
    assert!(rx.try_recv().is_err(), "nothing is answered before a keypress");

    app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let reply = rx.try_recv().unwrap();
    assert_eq!(reply["answer"], serde_json::json!("no"));
    assert_eq!(reply["index"], serde_json::json!(1));
    assert!(app.overlay.is_none());
}

#[test]
fn a_digit_answers_a_question_outright() {
    let v = Vault::new("control-ask-digit");
    let mut app = v.app();
    let (req, rx) = control(Control::Ask {
        question: "Which?".into(),
        options: vec!["alpha".into(), "beta".into(), "gamma".into()],
    });
    app.on_control(req);
    app.on_key(ch('3'));
    assert_eq!(rx.try_recv().unwrap()["answer"], serde_json::json!("gamma"));
}

#[test]
fn a_dismissed_question_is_answered_as_a_dismissal_rather_than_left_hanging() {
    let v = Vault::new("control-ask-esc");
    let mut app = v.app();
    let (req, rx) = control(Control::Ask {
        question: "Merge?".into(),
        options: vec!["yes".into()],
    });
    app.on_control(req);
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let reply = rx.try_recv().unwrap();
    assert_eq!(reply["cancelled"], serde_json::json!(true));
    assert!(reply["answer"].is_null());
}

#[test]
fn opening_a_page_renders_it_and_moves_focus() {
    let v = Vault::new("open");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    let open = app.open.as_ref().unwrap();
    assert_eq!(open.page.title(), "Alpha");
    assert!(open.doc.height() > 0);
    assert_eq!(app.focus, Focus::Doc);
}

#[test]
fn a_csv_opens_as_a_table_and_edits_as_text() {
    let v = Vault::new("csv");
    v.write("workspace/finance/quotes.csv", "symbol,price\nAAPL,232.1\nMSFT,417.4\n");
    let mut app = v.app();
    app.open_path(&v.path("workspace/finance/quotes.csv"), true);
    let open = app.open.as_ref().unwrap();
    assert!(open.csv(), "a csv is a csv open, not a markdown one");
    let text = open.doc.lines.iter().map(|l| l.plain_text()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("symbol") && text.contains("AAPL") && text.contains("MSFT"), "{text}");

    // The editor opens on the same raw file line the reader was showing.
    let msft = open.doc.lines.iter().position(|l| l.plain_text().contains("MSFT")).unwrap();
    app.open.as_mut().unwrap().scroll = msft;
    app.run(Cmd::Edit);
    let editor = app.open.as_ref().unwrap().editor.as_ref().unwrap();
    assert_eq!(editor.cursor_line(), 2, "editor lands on the MSFT row");
}

#[test]
fn a_wide_csv_pans_to_its_last_column_instead_of_clipping_it() {
    let v = Vault::new("csv-wide");
    let cols: Vec<String> = (0..14).map(|i| format!("col_{i}")).collect();
    let vals: Vec<String> = (0..14).map(|i| format!("v{i:02}")).collect();
    v.write("workspace/finance/wide.csv", &format!("{}\n{}\n", cols.join(","), vals.join(",")));
    let mut app = v.app();
    app.open_path(&v.path("workspace/finance/wide.csv"), true);

    let view = app.viewport_width();
    let visible = |app: &App| -> String {
        let open = app.open.as_ref().unwrap();
        let over = markdown::Overlays { hscroll: open.hscroll, ..Default::default() };
        open.doc
            .to_lines(&app.theme, &over, 0, open.doc.height())
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    .chars()
                    .take(view)
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    // The box is wider than the pane — that is the bug: the tail columns
    // were drawn past the right edge and clipped away with no way back.
    assert!(app.open.as_ref().unwrap().doc.width > view, "the table overflows the pane");
    assert!(!visible(&app).contains("col_13"), "the last column starts off screen");

    // Panning right reaches it, and never scrolls past the right edge.
    for _ in 0..40 {
        app.run(Cmd::PanRight);
    }
    let open = app.open.as_ref().unwrap();
    assert_eq!(open.hscroll, open.doc.width - view, "the pan stops at the right edge");
    let shown = visible(&app);
    assert!(shown.contains("col_13") && shown.contains("v13"), "{shown}");

    // And back again, clamped at zero rather than going negative.
    for _ in 0..40 {
        app.run(Cmd::PanLeft);
    }
    assert_eq!(app.open.as_ref().unwrap().hscroll, 0);
    assert!(visible(&app).contains("col_0"));
}

#[test]
fn prose_never_pans_and_a_reflow_drops_the_pan() {
    let v = Vault::new("csv-pan-reset");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::PanRight);
    assert_eq!(
        app.open.as_ref().unwrap().hscroll,
        0,
        "prose is wrapped to the measure, so there is nothing to pan to"
    );

    let cols: Vec<String> = (0..14).map(|i| format!("col_{i}")).collect();
    v.write("workspace/finance/wide.csv", &format!("{}\n", cols.join(",")));
    app.open_path(&v.path("workspace/finance/wide.csv"), true);
    app.run(Cmd::PanRight);
    assert!(app.open.as_ref().unwrap().hscroll > 0);
    // A narrower pane re-lays the table out, so the old offset addresses
    // columns that no longer sit there.
    app.areas.doc.width = 40;
    app.areas.doc_body.width = 36;
    app.reflow();
    assert_eq!(app.open.as_ref().unwrap().hscroll, 0, "a reflow starts from the left edge");
}

#[test]
fn a_csv_is_laid_out_across_the_whole_pane_not_the_reading_measure() {
    let v = Vault::new("csv-measure");
    v.write("workspace/finance/quotes.csv", "symbol,price\nAAPL,232.1\n");
    let mut app = v.app();
    app.areas.doc.width = crate::ui::markdown::MAX_WIDTH + 40;
    app.open_path(&v.path("workspace/finance/quotes.csv"), true);
    assert_eq!(app.doc_width(), crate::ui::markdown::MAX_WIDTH + 36);
    app.open_path(&v.path("wiki/a.md"), true);
    assert_eq!(app.doc_width(), crate::ui::markdown::MAX_WIDTH, "prose keeps the measure");
}

#[test]
fn opening_a_pdf_hands_off_to_the_system_viewer_instead_of_rendering_it() {
    let v = Vault::new("pdf");
    v.write("sources/literature/smith2024/original.pdf", "%PDF-1.4");
    let mut app = v.app();
    app.open_path(&v.path("sources/literature/smith2024/original.pdf"), true);
    assert!(app.open.is_none(), "a pdf is never opened in the reader pane");
    assert_eq!(app.toasts.len(), 1);
}

#[test]
fn opening_an_image_hands_off_to_the_system_viewer_too() {
    let v = Vault::new("image");
    v.write("wiki/biology/figure.png", "\u{89}PNG");
    v.write("wiki/biology/photo.JPEG", "jpegdata");
    let mut app = v.app();
    for rel in ["wiki/biology/figure.png", "wiki/biology/photo.JPEG"] {
        app.open_path(&v.path(rel), true);
        assert!(app.open.is_none(), "{rel} is never opened in the reader pane");
    }
}

#[test]
fn renaming_keeps_a_non_markdown_extension() {
    let v = Vault::new("rename-ext");
    v.write("wiki/biology/figure.png", "x");
    let mut app = v.app();
    app.rename_path(&v.path("wiki/biology/figure.png"), "diagram");
    assert!(v.path("wiki/biology/diagram.png").is_file(), "the .png survives a rename");
}

#[test]
fn opening_a_directory_opens_its_index_page() {
    let v = Vault::new("dir");
    let mut app = v.app();
    app.open_path(&v.path("wiki"), true);
    assert_eq!(app.open.as_ref().unwrap().page.path, v.path("wiki/_index.md"));
}

#[test]
fn selecting_the_wiki_folder_in_the_tree_opens_its_index_page() {
    let v = Vault::new("tree-dir");
    let mut app = v.app();
    app.tree.move_to(0); // the "wiki" collection row
    app.run(Cmd::TreeExpand);
    assert_eq!(app.open.as_ref().unwrap().page.path, v.path("wiki/_index.md"));
}

#[test]
fn following_a_link_navigates_and_history_goes_back() {
    let v = Vault::new("follow");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::NextLink);
    app.run(Cmd::FollowLink);
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Beta");

    app.run(Cmd::Back);
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Alpha");
    app.run(Cmd::Forward);
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Beta");
}

#[test]
fn clicking_a_link_navigates_to_page() {
    let v = Vault::new("click-link");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    // In wiki/a.md:
    // Line 0: the doc's leading blank row
    // Line 1: "# Alpha"
    // Line 2: the rule the reader draws under an h1
    // Line 3: ""
    // Line 4: "See Beta and gone."
    // doc_body starts at x=30, y=0. "Beta" link is on line 4, around col 4..8.
    let x = 30 + 5;
    let y = 4;
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Beta");
}

#[test]
fn clicking_navigation_arrows_goes_back_and_forward() {
    let v = Vault::new("click-nav-arrows");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.open_path(&v.path("wiki/b.md"), true);
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Beta");

    app.areas.nav_back = Some(Rect::new(31, 0, 3, 1));
    app.areas.nav_forward = Some(Rect::new(34, 0, 3, 1));

    // Click back arrow
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 32,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Alpha");

    // Click forward arrow
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 35,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Beta");
}

#[test]
fn clicking_edit_button_enters_editor() {
    let v = Vault::new("click-edit-btn");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    assert!(!app.open.as_ref().unwrap().editing());

    app.areas.doc_edit = Some(Rect::new(85, 0, 3, 1));
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 86,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.open.as_ref().unwrap().editing());
}

#[test]
fn a_single_click_on_a_source_row_selects_it_and_a_double_one_opens_it() {
    let v = Vault::new("citation");
    v.write(
        "sources/s1/metadata.md",
        "---\ntitle: Source One\nauthors: [\"A. One\", \"B. Two\"]\nyear: 2024\n---\n# Source One\n\nAbstract.\n",
    );
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.show_inspector = false;

    // Links in wiki/a.md, in order: Beta, gone, then the `[^s1]` citation.
    app.run(Cmd::NextLink);
    app.run(Cmd::NextLink);
    app.run(Cmd::NextLink);
    app.run(Cmd::FollowLink);
    assert_eq!(app.open.as_ref().unwrap().selected_citation.as_deref(), Some("s1"));
    assert!(app.show_inspector, "following a citation reveals the inspector");

    // The renderer would normally fill these in; set them directly to
    // isolate the click-handling logic from layout.
    app.areas.inspector = Rect::new(30, 30, 60, 10);
    app.areas.inspector_body = Rect::new(31, 31, 58, 8);
    app.areas.inspector_divider = None;
    app.areas.inspector_rows = vec![Some("s1".to_string())];
    let down = |row: u16| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 32,
        row,
        modifiers: KeyModifiers::NONE,
    };

    // A single click selects the source: its `[n]` stays highlighted in the
    // body and the page is untouched.
    app.on_mouse(down(31));
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Alpha");
    assert_eq!(app.open.as_ref().unwrap().selected_citation.as_deref(), Some("s1"));
    assert_eq!(app.open.as_ref().unwrap().link, Some(2), "the body [n] is the active link");

    // A second click on the same row within the double-click window opens it.
    app.on_mouse(down(31));
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Source One");
}

#[test]
fn clicking_a_backlink_in_inspector_opens_the_page() {
    let v = Vault::new("backlinks-click");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Alpha");

    app.areas.inspector = Rect::new(30, 30, 60, 10);
    app.areas.inspector_body = Rect::new(31, 31, 58, 8);
    app.areas.inspector_divider = None;
    app.areas.inspector_rows = vec![None];
    app.areas.inspector_backlinks = vec![
        InspectorBacklink {
            row: 0,
            col_start: 2,
            col_end: 20,
            path: v.path("wiki/b.md"),
        },
    ];

    let click = |col: u16, row: u16| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    };

    // Clicking on "← " (column 31 or 32, i.e. col 0 or 1 inside body) does not open the page
    app.on_mouse(click(31, 31));
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Alpha");

    // Clicking inside the backlink title (column 34, i.e. col 3 inside body) opens wiki/b.md
    app.on_mouse(click(34, 31));
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Beta");
}

#[test]
fn following_a_broken_link_says_so_instead_of_navigating() {
    let v = Vault::new("broken");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::NextLink);
    app.run(Cmd::NextLink); // the second link is the broken one
    app.run(Cmd::FollowLink);
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Alpha");
    assert!(app.toasts.last().unwrap().text.contains("broken link"));
}

#[test]
fn navigating_to_the_page_already_open_does_not_stack_history() {
    let v = Vault::new("dedupe");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    // Leaving home is a move like any other, so the first open records it.
    assert_eq!(app.back, vec![Spot::Home]);
    app.open_path(&v.path("wiki/a.md"), true);
    assert_eq!(app.back, vec![Spot::Home], "re-opening the same page adds nothing");
}

/// Dragging the boundary under a collection resizes it, and the size
/// survives a reload — geometry the user set is theirs, not the session's.
#[test]
fn a_collection_boundary_drags_and_the_height_persists() {
    let v = Vault::new("collection-drag");
    let mut app = v.app();
    app.areas.tree = Rect::new(0, 0, 30, 30);
    app.areas.tree_panes = vec![
        TreePane { area: Rect::new(0, 0, 30, 10), ..Default::default() },
        TreePane { area: Rect::new(0, 10, 30, 10), ..Default::default() },
        TreePane { area: Rect::new(0, 20, 30, 10), ..Default::default() },
    ];
    app.areas.collection_dividers = vec![(0, 10), (1, 20)];

    // The boundary under the first box is grabbable where it is drawn.
    assert_eq!(app.divider_at(4, 10), Some(Divider::Collection(0)));
    assert_eq!(app.divider_at(4, 20), Some(Divider::Collection(1)));

    app.resize_to(Divider::Collection(0), 4, 16);
    assert_eq!(app.cfg.collection_heights.get("wiki").copied(), Some(16));

    app.persist_widths();
    let reloaded = crate::config::Config::load_with_herdr_theme(&v.0, None);
    assert_eq!(reloaded.collection_heights.get("wiki").copied(), Some(16));
}

/// A folded box is one row by definition, so its boundary is not a handle.
/// Writing a height there would resize it behind your back on unfold.
#[test]
fn dragging_a_folded_collections_boundary_does_nothing() {
    let v = Vault::new("collection-drag-folded");
    let mut app = v.app();
    app.areas.tree = Rect::new(0, 0, 30, 30);
    app.areas.tree_panes = vec![
        TreePane { area: Rect::new(0, 0, 30, 1), folded: true, ..Default::default() },
        TreePane { area: Rect::new(0, 1, 30, 29), ..Default::default() },
    ];
    app.resize_to(Divider::Collection(0), 4, 12);
    assert!(app.cfg.collection_heights.is_empty());
}

/// Folding is remembered too, and both the handle and the key reach it.
#[test]
fn folding_a_collection_persists_and_the_key_toggles_it() {
    let v = Vault::new("collection-fold");
    let mut app = v.app();
    app.fold_collection(0);
    assert!(app.cfg.collapsed_collections.contains("wiki"));
    let reloaded = crate::config::Config::load_with_herdr_theme(&v.0, None);
    assert!(reloaded.collapsed_collections.contains("wiki"));

    // The key acts on whichever collection the cursor is in.
    app.tree.selected = 0;
    app.run(Cmd::ToggleCollection);
    assert!(!app.cfg.collapsed_collections.contains("wiki"), "the same key unfolds it");
}

/// Home is reachable again once you have left it — by the `home` command,
/// by walking the history back, and by the arrows that drive both.
#[test]
fn home_is_a_destination_you_can_navigate_back_to() {
    let v = Vault::new("home");
    let mut app = v.app();
    assert_eq!(app.here(), Spot::Home, "the session opens on the mark");

    app.open_path(&v.path("wiki/a.md"), true);
    assert!(matches!(app.here(), Spot::Page(_)));

    // Back from the first page lands on home rather than doing nothing.
    app.run(Cmd::Back);
    assert_eq!(app.here(), Spot::Home);
    assert!(app.open.is_none(), "the home screen is what gets drawn");

    // And forward returns to the page that was left.
    app.run(Cmd::Forward);
    assert_eq!(app.here(), Spot::Page(v.path("wiki/a.md")));

    // The command gets there in one step. Like opening a page, it is a
    // fresh navigation and so clears the forward stack — `back` is what
    // returns you, exactly as it would after following a link.
    app.run(Cmd::Home);
    assert_eq!(app.here(), Spot::Home);
    assert!(app.forward.is_empty(), "a new navigation drops the forward stack");
    app.run(Cmd::Back);
    assert_eq!(app.here(), Spot::Page(v.path("wiki/a.md")));
}

/// `home` while already home must not stack a history entry, or repeated
/// presses bury the page you came from under copies of the mark.
#[test]
fn home_from_home_is_a_no_op() {
    let v = Vault::new("home-idempotent");
    let mut app = v.app();
    app.run(Cmd::Home);
    app.run(Cmd::Home);
    assert!(app.back.is_empty());
    assert_eq!(app.here(), Spot::Home);
}

#[test]
fn the_same_key_reaches_the_right_pane() {
    let v = Vault::new("focus");
    let mut app = v.app();
    app.areas.doc = Rect::new(30, 0, 60, 6);
    app.areas.doc_main = app.areas.doc;
    app.areas.doc_body = Rect::new(32, 1, 56, 4);
    app.open_path(&v.path("wiki/a.md"), true);

    app.focus = Focus::Doc;
    app.on_key(ch('j'));
    assert_eq!(app.open.as_ref().unwrap().scroll, 1);

    app.focus = Focus::Tree;
    let before = app.tree.selected;
    app.on_key(ch('j'));
    assert_eq!(app.tree.selected, before + 1);
    assert_eq!(app.open.as_ref().unwrap().scroll, 1, "the reader did not also scroll");
}

#[test]
fn a_two_key_sequence_is_held_then_applied() {
    let v = Vault::new("sequence");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::ScrollDown);

    app.on_key(ch('g'));
    assert_eq!(app.pending, Some('g'));
    app.on_key(ch('g'));
    assert_eq!(app.pending, None);
    assert_eq!(app.open.as_ref().unwrap().scroll, 0);
}

#[test]
fn scrolling_is_clamped_to_the_document() {
    let v = Vault::new("scroll");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::DocTop);
    app.run(Cmd::ScrollUp);
    assert_eq!(app.open.as_ref().unwrap().scroll, 0);
    app.run(Cmd::DocBottom);
    let at = app.open.as_ref().unwrap().scroll;
    app.run(Cmd::ScrollDown);
    assert_eq!(app.open.as_ref().unwrap().scroll, at);
}

#[test]
fn the_last_lines_are_reachable_with_the_sources_pane_open() {
    let v = Vault::new("scroll-inspector");
    let body: String = (1..=200).map(|i| format!("line {i}\n")).collect();
    let long = v.write("wiki/long.md", &format!("---\ntitle: Long\ntype: concept\ncategory: c\nrationale: r\n---\n{body}"));
    let mut app = v.app();
    app.open_path(&long, true);
    app.run(Cmd::DocBottom);
    let open = app.open.as_ref().unwrap();
    // The reader body is 30 rows of the 40-row column; the other 10 are
    // the sources pane and are not room the document can scroll into.
    assert_eq!(open.scroll, open.doc.height() - 30, "the tail of the page must scroll into the reader");
}

#[test]
fn reflow_keeps_the_reading_position() {
    let v = Vault::new("reflow");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::DocBottom);
    let source = {
        let open = app.open.as_ref().unwrap();
        open.doc.source_for_line(open.scroll)
    };
    app.areas.doc = Rect::new(30, 0, 30, 40);
    app.reflow();
    let open = app.open.as_ref().unwrap();
    assert_eq!(open.doc.source_for_line(open.scroll), source);
}

#[test]
fn focus_cycling_skips_hidden_panes() {
    let v = Vault::new("cycle");
    let mut app = v.app();
    app.show_sidebar = false;
    app.focus = Focus::Tree;
    assert_eq!(app.tree.collection_paths().len(), 2, "fixture has a wiki and a workspace collection");

    // Each collection box is its own stop before Tab moves on to the doc.
    app.run(Cmd::CycleFocus);
    assert_eq!(app.focus, Focus::Tree, "steps into the second collection box first");
    let (second_start, _) = app.tree.section_span(1).unwrap();
    assert_eq!(app.tree.selected, second_start);

    app.run(Cmd::CycleFocus);
    assert_eq!(app.focus, Focus::Doc);
    app.run(Cmd::CycleFocus);
    assert_eq!(app.focus, Focus::Tree, "no sidebar to visit, wraps back to the first collection");

    app.show_tree = false;
    app.focus = Focus::Doc;
    app.run(Cmd::CycleFocus);
    assert_eq!(app.focus, Focus::Doc);
}

#[test]
fn cycling_stops_at_the_sources_box_when_the_inspector_is_open() {
    let v = Vault::new("cycle-sources");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.show_sidebar = false;
    app.show_tree = false;
    app.show_inspector = true;
    // The renderer sizes this normally; set it directly so `stops()` sees
    // the inspector as actually on screen without a full layout pass.
    app.areas.inspector = Rect::new(0, 30, 60, 10);
    app.focus = Focus::Doc;
    app.doc_stop = DocStop::Content;

    app.run(Cmd::CycleFocus);
    assert_eq!(app.focus, Focus::Doc);
    assert_eq!(app.doc_stop, DocStop::Sources, "the sources box gets its own stop");

    app.run(Cmd::CycleFocus);
    assert_eq!(app.focus, Focus::Doc);
    assert_eq!(app.doc_stop, DocStop::Content, "wraps back with tree and sidebar both hidden");

    app.run(Cmd::CycleFocusBack);
    assert_eq!(app.doc_stop, DocStop::Sources, "shift+tab steps backward through the same stops");
}

#[test]
fn hiding_the_focused_pane_moves_focus_out_of_it() {
    let v = Vault::new("hide");
    let mut app = v.app();
    app.focus = Focus::Tree;
    app.run(Cmd::ToggleTree);
    assert!(!app.show_tree);
    assert_eq!(app.focus, Focus::Doc);
}

#[test]
fn focusing_the_sidebar_is_refused_when_there_is_none() {
    let v = Vault::new("no-sidebar");
    let mut app = v.app();
    app.focus = Focus::Doc;
    app.run(Cmd::FocusSidebar);
    assert_eq!(app.focus, Focus::Doc);
}

#[test]
fn leaving_the_sidebar_advances_instead_of_stepping_back() {
    let v = Vault::new("leave-sidebar");
    let mut app = v.app();
    app.show_sidebar = true;
    let cmd = portable_pty::CommandBuilder::new("/bin/sh");
    let (tx, _rx) = std::sync::mpsc::channel();
    app.sidebar = Some(crate::herdr::pty::Pane::spawn_command(cmd, 10, 40, tx, None).unwrap());
    app.focus = Focus::Sidebar;

    // F12 ("leave agents pane") used to always land on Doc, one step
    // *back* in Tree → Doc → Sidebar order. It should behave like Tab:
    // advance forward, wrapping to Tree.
    app.run(Cmd::LeaveSidebar);
    assert_eq!(app.focus, Focus::Tree, "f12 advances forward, wrapping past doc to the tree");
}

#[test]
fn the_editor_opens_at_the_line_being_read_and_saves() {
    let v = Vault::new("edit");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::DocBottom);
    app.run(Cmd::Edit);

    let open = app.open.as_ref().unwrap();
    let editor = open.editor.as_ref().unwrap();
    assert!(editor.cursor_line() >= open.page.body_start);
    assert_eq!(app.ctx(), Ctx::Edit);

    app.run(Cmd::Save);
    assert!(app.toasts.last().unwrap().text.starts_with("saved"));
}

#[test]
fn a_paste_into_the_editor_lands_verbatim() {
    let v = Vault::new("paste-edit");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::Edit);
    let row = app.open.as_ref().unwrap().editor.as_ref().unwrap().cursor_line();

    // A list, which as a burst of key events would have had every marker
    // doubled by `continue_block`, and a tab, which would have indented.
    app.on_paste("- one\n- two\n\ttail");

    let editor = app.open.as_ref().unwrap().editor.as_ref().unwrap();
    let lines: Vec<String> = editor.text().lines().map(str::to_string).collect();
    assert_eq!(lines[row], "- one");
    assert_eq!(lines[row + 1], "- two");
    // What the cursor was sitting in front of rides on the last pasted
    // line, the way splitting a line always works.
    assert_eq!(lines[row + 2], "\ttail# Alpha");
    assert_eq!(editor.cursor_line(), row + 2);
    assert_eq!(editor.cursor_col(), "\ttail".chars().count(), "cursor sits at the end of the paste");
}

/// Before bracketed paste, a paste reached the reader as one key event per
/// character, so pasting prose containing `L` ran the linter and `P` opened
/// the project switcher.
#[test]
fn undo_and_redo_walk_the_buffer_back_and_forward() {
    let v = Vault::new("undo");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::Edit);
    let before = app.open.as_ref().unwrap().editor.as_ref().unwrap().text();

    app.on_key(ch('x'));
    assert!(app.open.as_ref().unwrap().editor.as_ref().unwrap().dirty());

    app.on_key(ctrl('z'));
    assert_eq!(app.open.as_ref().unwrap().editor.as_ref().unwrap().text(), before, "undo restored the buffer");

    app.on_key(ctrl('r'));
    assert_ne!(app.open.as_ref().unwrap().editor.as_ref().unwrap().text(), before, "redo put the edit back");
}

#[test]
fn a_paste_into_the_reader_runs_no_commands() {
    let v = Vault::new("paste-reader");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.focus = Focus::Doc;

    app.on_paste("Lint P / text");

    assert!(app.overlay.is_none(), "no command ran");
    assert!(app.find.is_none());
    assert!(!app.open.as_ref().unwrap().editing());
}

#[test]
fn a_paste_into_a_prompt_is_typed_into_it() {
    let v = Vault::new("paste-prompt");
    let mut app = v.app();
    app.run(Cmd::FindFiles);
    app.on_paste("alpha\nbeta");
    match app.overlay.as_ref() {
        // The newline is dropped rather than submitting the finder midway.
        Some(Overlay::Finder(f)) => assert_eq!(f.query, "alphabeta"),
        _ => panic!("expected the finder to still be open"),
    }
}

#[test]
fn reader_keys_do_not_fire_while_editing() {
    let v = Vault::new("edit-keys");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::Edit);
    let before = app.open.as_ref().unwrap().scroll;
    app.on_key(ch('j'));
    assert_eq!(app.open.as_ref().unwrap().scroll, before, "j is a motion, not a scroll");
    assert!(app.open.as_ref().unwrap().editing());
}

#[test]
fn creating_a_page_scaffolds_okf_frontmatter_and_opens_the_editor() {
    let v = Vault::new("new");
    let mut app = v.app();
    app.run(Cmd::NewPage);
    for c in "wiki/health/sleep".chars() {
        app.on_key(ch(c));
    }
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let created = v.path("wiki/health/sleep.md");
    assert!(created.is_file());
    let text = std::fs::read_to_string(&created).unwrap();
    assert!(text.contains("type: concept"));
    assert!(text.contains("category: health"));
    assert!(text.contains("title: sleep"));
    assert_eq!(app.open.as_ref().unwrap().page.path, created);
    assert!(app.open.as_ref().unwrap().editing());
}

#[test]
fn a_new_page_path_cannot_escape_the_checkout() {
    let v = Vault::new("escape");
    let mut app = v.app();
    app.create_page("../../etc/evil", None);
    assert!(app.toasts.last().unwrap().text.contains("escapes"));
    assert!(app.open.is_none());
}

#[test]
fn the_finder_filters_live_and_opens_the_hit() {
    let v = Vault::new("finder");
    let mut app = v.app();
    app.run(Cmd::FindFiles);
    assert!(matches!(app.overlay, Some(Overlay::Finder(_))));
    for c in "beta".chars() {
        app.on_key(ch(c));
    }
    let Some(Overlay::Finder(finder)) = app.overlay.as_ref() else { panic!() };
    assert_eq!(finder.hits.len(), 1);
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.overlay.is_none());
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Beta");
}

#[test]
fn the_finder_cycles_modes_and_scopes() {
    let v = Vault::new("finder-modes");
    let mut app = v.app();
    app.run(Cmd::FindFiles);
    app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    let Some(Overlay::Finder(f)) = app.overlay.as_ref() else { panic!() };
    assert_eq!(f.mode, search::Mode::Text);

    app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL));
    let Some(Overlay::Finder(f)) = app.overlay.as_ref() else { panic!() };
    assert_eq!(f.collection, Some("wiki"));
}

#[test]
fn semantic_search_is_never_run_while_typing() {
    let v = Vault::new("semantic");
    let mut app = v.app();
    app.run(Cmd::FindSemantic);
    for c in "creatine".chars() {
        app.on_key(ch(c));
    }
    assert!(!app.jobs.is_busy(), "typing must not spawn the engine");
    let Some(Overlay::Finder(f)) = app.overlay.as_ref() else { panic!() };
    assert!(f.hits.is_empty());
    assert!(f.running.is_none());
}

#[test]
fn semantic_search_refuses_when_qmd_is_disabled() {
    let v = Vault::new("no-qmd");
    std::fs::write(v.path(".podarcis/config.yaml"), "engines:\n  qmd: false\n").unwrap();
    let mut app = v.app();
    app.cfg = Config::load(&v.0);
    app.run(Cmd::FindSemantic);
    for c in "x".chars() {
        app.on_key(ch(c));
    }
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let Some(Overlay::Finder(f)) = app.overlay.as_ref() else { panic!() };
    assert!(f.warning.as_ref().unwrap().contains("engines.qmd"));
    assert!(!app.jobs.is_busy());
}

#[test]
fn the_palette_filters_by_title_and_runs_the_command() {
    let v = Vault::new("palette");
    let mut app = v.app();
    app.run(Cmd::Palette);
    for c in "restart".chars() {
        app.on_key(ch(c));
    }
    let Some(Overlay::Palette(p)) = app.overlay.as_ref() else { panic!() };
    assert_eq!(p.items.len(), 1);
    assert_eq!(p.items[0].0, Cmd::Restart);

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.overlay.is_none());
}

#[test]
fn the_palette_never_offers_a_movement_command() {
    assert!(palette_items("").iter().all(|(cmd, _, _)| cmd.palette_visible()));
    assert!(palette_items("down").is_empty());
}

#[test]
fn quitting_with_a_job_in_flight_asks_once() {
    let v = Vault::new("quit-busy");
    let mut app = v.app();
    app.jobs.running.push(crate::actions::Running { id: 1, label: "sync".into() });
    app.on_key(ctrl('q'));
    assert!(!app.quit);
    assert!(app.toasts.last().unwrap().text.contains("sync"));
    app.on_key(ctrl('q'));
    assert!(app.quit);
}

#[test]
fn any_other_key_resets_the_quit_confirmation() {
    let v = Vault::new("quit-reset");
    let mut app = v.app();
    app.jobs.running.push(crate::actions::Running { id: 1, label: "sync".into() });
    app.on_key(ctrl('q'));
    app.on_key(ch('j'));
    app.on_key(ctrl('q'));
    assert!(!app.quit, "the warning has to be re-earned");
}

/// The reachable way to quit on top of a dirty buffer: `ctrl+q` is text
/// while the editor holds focus, but clicking into the tree leaves the
/// editor open and puts `ctrl+q` back in reach.
#[test]
fn quitting_with_unsaved_changes_asks_once() {
    let v = Vault::new("quit-dirty");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::Edit);
    app.editor_key(ch('x'));
    assert!(app.open.as_ref().unwrap().editor.as_ref().unwrap().dirty());
    app.focus = Focus::Tree;

    app.on_key(ctrl('q'));
    assert!(!app.quit, "an unsaved buffer is not discarded by reflex");
    assert!(app.toasts.last().unwrap().text.contains("unsaved changes"));
    app.on_key(ctrl('q'));
    assert!(app.quit, "the second press means it");
}

#[test]
fn a_clean_buffer_quits_on_the_first_press() {
    let v = Vault::new("quit-clean");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::Edit);
    app.focus = Focus::Tree;
    app.on_key(ctrl('q'));
    assert!(app.quit, "the guard is about unsaved work, not about editing");
}

#[test]
fn restarting_with_unsaved_changes_instructs_instead_of_prompting() {
    let v = Vault::new("restart-dirty");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::Edit);
    app.editor_key(ch('x'));

    // Twice, because the palette would reset a "choose it again" prompt.
    app.run(Cmd::Restart);
    app.run(Cmd::Restart);
    assert!(!app.restart);
    assert!(app.toasts.last().unwrap().text.contains("ctrl+s to save"));
}

#[test]
fn q_does_not_quit_ctrl_q_does() {
    let v = Vault::new("quit-keys");
    let mut app = v.app();
    app.on_key(ch('q'));
    assert!(!app.quit);
    app.on_key(ch('q'));
    assert!(!app.quit, "double q does not quit either");

    app.on_key(ctrl('q'));
    assert!(app.quit);
}

#[test]
fn a_single_click_in_the_tree_selects_but_does_not_open() {
    let v = Vault::new("mouse");
    let mut app = v.app();
    let target = app.tree.rows.iter().position(|r| r.label == "a").unwrap();
    let (x, y) = (app.areas.tree.x + 2, app.areas.tree.y + 1 + target as u16);
    press(&mut app, x, y);
    assert_eq!(app.tree.selected, target, "the click selects the row");
    assert!(app.open.is_none(), "a single click must not open the page");
    assert_eq!(app.focus, Focus::Tree);
}

#[test]
fn a_double_click_in_the_tree_opens_the_page() {
    let v = Vault::new("mouse-double");
    let mut app = v.app();
    let target = app.tree.rows.iter().position(|r| r.label == "a").unwrap();
    let (x, y) = (app.areas.tree.x + 2, app.areas.tree.y + 1 + target as u16);
    press(&mut app, x, y);
    assert!(app.open.is_none());
    press(&mut app, x, y);
    assert_eq!(app.open.as_ref().unwrap().page.title(), "Alpha");
    assert_eq!(app.focus, Focus::Doc, "opening a page focuses it");
}

#[test]
fn two_clicks_on_different_rows_never_open() {
    let v = Vault::new("mouse-diff-rows");
    let mut app = v.app();
    let a = app.tree.rows.iter().position(|r| r.label == "a").unwrap();
    let b = app.tree.rows.iter().position(|r| r.label == "b").unwrap();
    let (x, ya) = (app.areas.tree.x + 2, app.areas.tree.y + 1 + a as u16);
    let yb = app.areas.tree.y + 1 + b as u16;
    press(&mut app, x, ya);
    press(&mut app, x, yb);
    assert!(app.open.is_none(), "fast clicks on different rows are singles");
    assert_eq!(app.tree.selected, b, "the last row is the one selected");
}

#[test]
fn a_single_click_on_a_folder_toggles_it_without_opening() {
    let v = Vault::new("mouse-folder");
    v.write("wiki/health/_index.md", "# Health\n");
    let mut app = v.app();
    app.tree.rebuild();
    let folder = app.tree.rows.iter().position(|r| r.label == "health").unwrap();
    let (x, y) = (app.areas.tree.x + 2, app.areas.tree.y + 1 + folder as u16);
    press(&mut app, x, y);
    assert!(app.tree.rows[folder].expanded, "a single click expands a folder");
    assert!(app.open.is_none(), "and does not open its index");
}

#[test]
fn a_double_click_on_a_folder_opens_its_index_page() {
    let v = Vault::new("mouse-folder-double");
    v.write("wiki/health/_index.md", "# Health\n");
    let mut app = v.app();
    app.tree.rebuild();
    let folder = app.tree.rows.iter().position(|r| r.label == "health").unwrap();
    let (x, y) = (app.areas.tree.x + 2, app.areas.tree.y + 1 + folder as u16);
    press(&mut app, x, y);
    press(&mut app, x, y);
    assert!(app.tree.rows[folder].expanded, "the folder stays expanded");
    assert_eq!(
        app.open.as_ref().unwrap().page.path,
        v.path("wiki/health/_index.md"),
        "the double click opens the folder's index"
    );
}

#[test]
fn clicking_a_collection_title_opens_repo_config() {
    let v = Vault::new("repo-config");
    let mut app = v.app();
    app.areas.tree_panes = vec![crate::app::TreePane {
        area: Rect::new(0, 0, 30, 12),
        inner: Rect::new(1, 1, 28, 10),
        header: 0,
        start: 1,
        end: app.tree.rows.len(),
        offset: 0,
        fold: None,
        folded: false,
    }];
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    match app.overlay {
        Some(Overlay::RepoConfig(ref state)) => {
            assert_eq!(state.name, app.tree.rows[0].label);
            assert_eq!(state.selected, 0);
        }
        _ => panic!("expected repo config overlay"),
    }
    assert!(app.tree.rows[0].expanded, "the collection stays expanded");
}

fn right_press(app: &mut App, x: u16, y: u16) {
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

/// Right-click the tree row labelled `label` and return its row index.
fn right_click(app: &mut App, label: &str) -> usize {
    let (i, y) = {
        let x0 = app.areas.tree.x;
        let y0 = app.areas.tree.y;
        let i = app.tree.rows.iter().position(|r| r.label == label).unwrap();
        (i, (x0 + 2, y0 + 1 + i as u16))
    };
    right_press(app, y.0, y.1);
    i
}

#[test]
fn right_click_opens_a_context_menu_on_that_row() {
    let v = Vault::new("menu-open");
    let mut app = v.app();
    let i = right_click(&mut app, "a");
    assert_eq!(app.tree.selected, i, "the right-click selects the row");
    assert_eq!(app.focus, Focus::Tree);
    let Some(Overlay::Menu(menu)) = app.overlay.as_ref() else { panic!("expected menu overlay") };
    assert_eq!(menu.path, v.path("wiki/a.md"));
    let labels: Vec<&str> = menu.items.iter().map(|(_, l)| *l).collect();
    assert_eq!(labels, vec!["open", "rename…", "delete…", "copy relative path", "copy absolute path"]);
}

#[test]
fn a_collection_root_offers_no_rename_or_delete() {
    let v = Vault::new("menu-collection");
    let mut app = v.app();
    right_click(&mut app, "wiki");
    let Some(Overlay::Menu(menu)) = app.overlay.as_ref() else { panic!("expected menu overlay") };
    let labels: Vec<&str> = menu.items.iter().map(|(_, l)| *l).collect();
    assert_eq!(labels, vec!["open", "new page…", "copy relative path", "copy absolute path"]);
}

#[test]
fn the_menu_creates_a_page_inside_the_clicked_folder() {
    let v = Vault::new("menu-new-page");
    let mut app = v.app();
    right_click(&mut app, "wiki");
    app.on_key(ch('j')); // new page…
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let Some(Overlay::Prompt(prompt)) = app.overlay.as_ref() else { panic!("expected new-page prompt") };
    assert_eq!(prompt.kind, PromptKind::NewPage);
    assert_eq!(prompt.value, "");
    assert_eq!(prompt.path.as_deref(), Some(v.path("wiki").as_path()));
    app.on_key(ch('c'));
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.overlay.is_none());
    assert!(v.path("wiki/c.md").exists(), "the page was created in the clicked folder");
    assert!(app.tree.rows.iter().any(|r| r.label == "c"), "the tree sees the new page");
    assert!(app.open.is_some(), "the new page is opened");
}

#[test]
fn a_normal_folder_also_offers_new_page() {
    let v = Vault::new("menu-new-page-dir");
    v.write("wiki/nested/x.md", "# X\n");
    let mut app = v.app();
    right_click(&mut app, "nested");
    let Some(Overlay::Menu(menu)) = app.overlay.as_ref() else { panic!("expected menu overlay") };
    let labels: Vec<&str> = menu.items.iter().map(|(_, l)| *l).collect();
    assert_eq!(labels, vec!["open", "new page…", "rename…", "delete…", "copy relative path", "copy absolute path"]);
}

#[test]
fn the_menu_scrolls_by_keypress_and_activates_on_enter() {
    let v = Vault::new("menu-keys");
    let mut app = v.app();
    right_click(&mut app, "a");
    app.on_key(ch('j'));
    app.on_key(ch('j'));
    let Some(Overlay::Menu(menu)) = app.overlay.as_ref() else { panic!("expected menu") };
    assert_eq!(menu.selected, 2, "j moves past open and rename…");
    app.on_key(ch('k'));
    let Some(Overlay::Menu(menu)) = app.overlay.as_ref() else { panic!("expected menu") };
    assert_eq!(menu.selected, 1);
    // Enter on rename… hands the slug to the prompt.
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let Some(Overlay::Prompt(prompt)) = app.overlay.as_ref() else { panic!("expected prompt") };
    assert_eq!(prompt.kind, PromptKind::Rename);
    assert_eq!(prompt.value, "a");
    assert_eq!(prompt.path.as_deref(), Some(v.path("wiki/a.md").as_path()));
}

#[test]
fn esc_closes_the_menu_without_running_anything() {
    let v = Vault::new("menu-esc");
    let mut app = v.app();
    right_click(&mut app, "b");
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.overlay.is_none(), "esc dismisses");
    assert!(app.open.is_none());
}

#[test]
fn the_menu_renames_a_page_on_disk() {
    let v = Vault::new("menu-rename");
    let mut app = v.app();
    right_click(&mut app, "a");
    app.on_key(ch('j')); // rename…
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    // Backspace the slug "a", type "z", confirm.
    app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    app.on_key(ch('z'));
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.overlay.is_none());
    assert!(!v.path("wiki/a.md").exists(), "the old file is gone");
    assert!(v.path("wiki/z.md").exists(), "the new file is there");
    assert!(app.tree.rows.iter().any(|r| r.label == "z"), "the tree sees the new name");
    assert!(app.toasts.last().unwrap().text.contains("renamed wiki/a.md → wiki/z.md"));
}

#[test]
fn the_menu_deletes_a_page_only_after_confirmation() {
    let v = Vault::new("menu-delete");
    let mut app = v.app();
    right_click(&mut app, "b");
    app.on_key(ch('j'));
    app.on_key(ch('j')); // delete…
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let Some(Overlay::Menu(confirm)) = app.overlay.as_ref() else { panic!("expected confirm menu") };
    assert_eq!(confirm.path, v.path("wiki/b.md"));
    assert_eq!(confirm.items.iter().map(|(_, l)| *l).collect::<Vec<_>>(), vec!["cancel", "delete"]);
    // Esc on the confirm leaves the file alone.
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.overlay.is_none());
    assert!(v.path("wiki/b.md").exists());

    // Back through the whole flow and confirm this time.
    right_click(&mut app, "b");
    app.on_key(ch('j'));
    app.on_key(ch('j'));
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.on_key(ch('j')); // move onto "delete"
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.overlay.is_none());
    assert!(!v.path("wiki/b.md").exists());
    assert!(!app.tree.rows.iter().any(|r| r.label == "b"));
}

#[test]
fn renaming_or_deleting_a_collection_root_is_refused() {
    let v = Vault::new("menu-rename-root");
    let mut app = v.app();
    app.prompt_rename(&v.path("wiki"));
    assert!(app.overlay.is_none(), "no rename prompt opens for a collection root");
    assert!(app.toasts.last().unwrap().text.contains("cannot be renamed"));

    app.delete_path(&v.path("workspace"));
    assert!(v.path("workspace").is_dir(), "the collection survives");
    assert!(app.toasts.last().unwrap().text.contains("cannot be deleted"));
}

#[test]
fn clicking_outside_the_menu_dismisses_it() {
    let v = Vault::new("menu-dismiss");
    let mut app = v.app();
    right_click(&mut app, "a");
    assert!(app.overlay.is_some());
    // Click in the document area, well away from the popup.
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: app.areas.doc.x + 5,
        row: app.areas.doc.y + 5,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.overlay.is_none(), "a press outside the popup dismisses it");
}

#[test]
fn releasing_on_a_menu_item_activates_it() {
    let v = Vault::new("menu-mouse");
    let mut app = v.app();
    right_click(&mut app, "a");
    let Some(Overlay::Menu(menu)) = app.overlay.as_ref() else { panic!("expected menu") };
    let area = menu.area;
    // Down on the second item, up on the same row -> rename prompt.
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x + 2,
        row: area.y + 1 + 1,
        modifiers: KeyModifiers::NONE,
    });
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: area.x + 2,
        row: area.y + 1 + 1,
        modifiers: KeyModifiers::NONE,
    });
    let Some(Overlay::Prompt(prompt)) = app.overlay.as_ref() else { panic!("expected rename prompt") };
    assert_eq!(prompt.kind, PromptKind::Rename);
}

#[test]
fn the_menu_copies_relative_and_absolute_paths() {
    let v = Vault::new("menu-copy");
    let mut app = v.app();
    right_click(&mut app, "a");
    app.on_key(ch('j'));
    app.on_key(ch('j'));
    app.on_key(ch('j')); // copy relative path
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.overlay.is_none());
    assert!(app.toasts.last().unwrap().text.contains("wiki/a.md"));

    right_click(&mut app, "a");
    app.on_key(ch('j'));
    app.on_key(ch('j'));
    app.on_key(ch('j'));
    app.on_key(ch('j')); // copy absolute path
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.toasts.last().unwrap().text.contains("wiki/a.md"));
}

#[test]
fn the_wheel_scrolls_the_pane_under_the_pointer() {
    let v = Vault::new("wheel");
    let mut app = v.app();
    app.areas.doc = Rect::new(30, 0, 60, 6);
    app.areas.doc_main = app.areas.doc;
    app.areas.doc_body = Rect::new(32, 1, 56, 4);
    app.open_path(&v.path("wiki/a.md"), true);
    app.focus = Focus::Tree;
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 40,
        row: 3,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.open.as_ref().unwrap().scroll > 0, "the pointer decides, not focus");
    assert_eq!(app.focus, Focus::Tree, "scrolling does not steal focus");
}

fn right_press_at(app: &mut App, x: u16, y: u16) {
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

fn right_drag_to(app: &mut App, x: u16, y: u16) {
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Right),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

fn right_release_at(app: &mut App, x: u16, y: u16) {
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Right),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

#[test]
fn right_drag_sends_a_file_mention_into_the_agent_session() {
    let v = Vault::new("right-send");
    let mut app = laid_out(&v);
    app.open_path(&v.path("wiki/a.md"), true);

    // A real child that echoes what it reads, so the test proves the bytes
    // actually leave the app rather than only updating state.
    let (tx, rx) = std::sync::mpsc::channel();
    let mut cmd = portable_pty::CommandBuilder::new("/bin/sh");
    cmd.args(["-c", "read line; printf 'GOT:%s\\n' \"$line\""]);
    cmd.env("TERM", "xterm-256color");
    let pane = crate::herdr::pty::Pane::spawn_command(cmd, 10, 40, tx, None).unwrap();
    app.sidebar = Some(pane);

    // Right-drag across "See Beta and gone." (wiki/a.md line 4, the one
    // body source line it renders from, starting at file line 9): the
    // mention must name file line 12 — not the rendered line 4.
    right_press_at(&mut app, 32, 4);
    right_drag_to(&mut app, 50, 4);
    right_release_at(&mut app, 50, 4);

    for _ in 0..50 {
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(crate::event::AppEvent::PtyExited) | Err(_) => break,
            Ok(_) => {}
        }
    }
    app.sidebar.as_ref().unwrap().with_screen(|s| {
        let text = s.contents();
        assert!(text.contains("GOT:@wiki/a.md:12"), "{text:?}");
    });
    assert!(app.toasts.last().unwrap().text.contains("sent selection to the agent"));
    assert!(
        !app.toasts.last().unwrap().text.contains("@wiki/a.md"),
        "the toast must not leak the file path: {:?}",
        app.toasts.last().unwrap().text
    );
    assert!(app.open.as_ref().unwrap().selection.is_some(), "the selection stays visible");
}

#[test]
fn right_drag_without_an_agent_session_warns_and_keeps_the_selection() {
    let v = Vault::new("right-send-none");
    let mut app = laid_out(&v);
    app.open_path(&v.path("wiki/a.md"), true);
    app.sidebar = None;

    right_press_at(&mut app, 32, 4);
    right_drag_to(&mut app, 50, 4);
    right_release_at(&mut app, 50, 4);

    assert!(app.toasts.last().unwrap().text.contains("no agent session"));
    assert!(app.open.as_ref().unwrap().selection.is_some());
}

#[test]
fn a_right_click_without_a_drag_sends_nothing() {
    let v = Vault::new("right-click-none");
    let mut app = laid_out(&v);
    app.open_path(&v.path("wiki/a.md"), true);

    right_press_at(&mut app, 32, 4);
    right_release_at(&mut app, 32, 4);

    assert!(app.toasts.is_empty(), "a click without a drag sends nothing");
    assert!(app.open.as_ref().unwrap().selection.is_none());
}

fn press(app: &mut App, x: u16, y: u16) {
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

fn drag(app: &mut App, x: u16, y: u16) {
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

fn release(app: &mut App, x: u16, y: u16) {
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

fn laid_out(v: &Vault) -> App {
    let mut app = v.app();
    app.areas = Areas {
        tree: Rect::new(0, 0, 30, 40),
        doc: Rect::new(30, 0, 90, 40),
        doc_main: Rect::new(30, 0, 90, 30),
        doc_body: Rect::new(30, 0, 90, 30),
        sidebar: Rect::new(120, 0, 40, 40),
        inspector: Rect::new(30, 30, 90, 10),
        inspector_body: Rect::default(),
        inspector_rows: Vec::new(),
        tree_panes: Vec::new(),
        tree_divider: Some(29),
        sidebar_divider: Some(120),
        inspector_divider: Some(30),
        tree_toggle: Some(Rect::new(29, 20, 1, 1)),
        sidebar_toggle: Some(Rect::new(120, 20, 1, 1)),
        inspector_toggle: Some(Rect::new(75, 30, 1, 1)),
        ..Default::default()
    };
    app
}

#[test]
fn clicking_the_tree_handle_collapses_it_instead_of_dragging() {
    let v = Vault::new("toggle-tree");
    let mut app = laid_out(&v);
    assert!(app.show_tree);
    press(&mut app, 29, 20);
    assert!(!app.show_tree, "the handle toggles, it does not start a drag");
    assert!(app.dragging.is_none());
}

#[test]
fn clicking_a_tab_focuses_its_pane_and_reopens_it_if_hidden() {
    let v = Vault::new("tab-click");
    let mut app = laid_out(&v);
    app.areas.tab_bar = vec![
        (Focus::Tree, "Tree", Rect::new(0, 0, 27, 1)),
        (Focus::Doc, "Doc", Rect::new(27, 0, 26, 1)),
        (Focus::Sidebar, "Agents", Rect::new(53, 0, 27, 1)),
    ];
    app.show_tree = false;
    app.focus = Focus::Doc;

    press(&mut app, 10, 0);
    assert_eq!(app.focus, Focus::Tree, "clicking a hidden pane's tab still switches focus to it");
    assert!(app.show_tree, "and reopens it, the same way a hand-hidden pane is meant to come back");
}

#[test]
fn clicking_the_sidebar_handle_collapses_it_instead_of_dragging() {
    let v = Vault::new("toggle-sidebar");
    let mut app = laid_out(&v);
    assert!(app.show_sidebar);
    press(&mut app, 120, 20);
    assert!(!app.show_sidebar);
    assert!(app.dragging.is_none());
}

#[test]
fn clicking_the_inspector_handle_hides_the_sources_section() {
    let v = Vault::new("toggle-inspector");
    let mut app = laid_out(&v);
    assert!(app.show_inspector);
    press(&mut app, 75, 30);
    assert!(!app.show_inspector, "the handle toggles the inspector, it does not start a drag");
    assert!(app.dragging.is_none());
    // And it comes back from the same spot.
    press(&mut app, 75, 30);
    assert!(app.show_inspector);
}

#[test]
fn dragging_the_tree_divider_resizes_it() {
    let v = Vault::new("drag-tree");
    let mut app = laid_out(&v);
    press(&mut app, 29, 5);
    assert_eq!(app.dragging, Some(Divider::Tree));
    drag(&mut app, 49, 5);
    assert_eq!(app.cfg.tree_width, 50);
    release(&mut app, 49, 5);
    assert!(app.dragging.is_none());
}

#[test]
fn dragging_the_agents_divider_resizes_from_the_right() {
    let v = Vault::new("drag-sidebar");
    let mut app = laid_out(&v);
    press(&mut app, 120, 5);
    assert_eq!(app.dragging, Some(Divider::Sidebar));
    drag(&mut app, 100, 5);
    assert_eq!(app.cfg.sidebar_width, 60);
}

#[test]
fn a_divider_can_be_grabbed_from_a_column_either_side() {
    let v = Vault::new("drag-grab");
    let mut app = laid_out(&v);
    press(&mut app, 30, 5);
    assert_eq!(app.dragging, Some(Divider::Tree), "one column of slack each way");
}

#[test]
fn a_drag_never_squeezes_the_document_away() {
    let v = Vault::new("drag-clamp");
    let mut app = laid_out(&v);
    press(&mut app, 29, 5);
    drag(&mut app, 500, 5);
    assert!(app.cfg.tree_width <= 160 - 30 - 40, "the document keeps 30 columns");
    drag(&mut app, 0, 5);
    assert_eq!(app.cfg.tree_width, 12, "and the tree keeps a usable minimum");
}

#[test]
fn a_drag_that_wanders_off_the_divider_still_resizes() {
    let v = Vault::new("drag-wander");
    let mut app = laid_out(&v);
    press(&mut app, 29, 5);
    // Straying into the tree pane must not be read as a click in the tree.
    drag(&mut app, 40, 5);
    assert_eq!(app.cfg.tree_width, 41);
    assert!(app.open.is_none(), "no page was opened by the wandering drag");
}

#[test]
fn a_click_away_from_a_divider_is_an_ordinary_click() {
    let v = Vault::new("drag-none");
    let mut app = laid_out(&v);
    press(&mut app, 60, 5);
    assert!(app.dragging.is_none());
    assert_eq!(app.focus, Focus::Doc);
}

#[test]
fn dragging_the_inspector_divider_resizes_height() {
    let v = Vault::new("drag-inspector");
    let mut app = laid_out(&v);
    press(&mut app, 60, 30);
    assert_eq!(app.dragging, Some(Divider::Inspector));
    drag(&mut app, 60, 25);
    assert_eq!(app.cfg.inspector_height, 15);
    drag(&mut app, 60, 35);
    assert_eq!(app.cfg.inspector_height, 5);
    release(&mut app, 60, 35);
    assert!(app.dragging.is_none());
}

#[test]
fn inspector_drag_clamps_to_min_and_max() {
    let v = Vault::new("drag-inspector-clamp");
    let mut app = laid_out(&v);
    press(&mut app, 60, 30);
    drag(&mut app, 60, 0);
    assert_eq!(app.cfg.inspector_height, 32);
    drag(&mut app, 60, 50);
    assert_eq!(app.cfg.inspector_height, 4);
}

#[test]
fn scrolling_inspector_with_mouse_wheel() {
    let v = Vault::new("scroll-inspect");
    let mut app = laid_out(&v);
    let path = v.write(
        "wiki/sources.md",
        "---\ntitle: S\ntype: concept\ncategory: c\nsources:\n  - id: s1\n  - id: s2\n  - id: s3\n  - id: s4\n  - id: s5\n  - id: s6\n  - id: s7\n  - id: s8\n  - id: s9\n  - id: s10\n---\nbody\n",
    );
    app.open_path(&path, false);
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 60,
        row: 35,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.open.as_ref().unwrap().inspect_scroll > 0);
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: 60,
        row: 35,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.open.as_ref().unwrap().inspect_scroll, 0);
}

#[test]
fn clicking_and_dragging_inspector_scrollbar() {
    let v = Vault::new("scrollbar-inspect");
    let mut app = laid_out(&v);
    let sources_yaml = (0..20).map(|i| format!("  - id: s{i}\n")).collect::<String>();
    let path = v.write(
        "wiki/long_sources.md",
        &format!("---\ntitle: LS\ntype: concept\ncategory: c\nsources:\n{sources_yaml}---\nbody\n"),
    );
    app.open_path(&path, false);
    press(&mut app, 119, 37);
    assert_eq!(app.dragging_scrollbar, Some(ScrollbarDrag::Inspector));
    let scroll_after_press = app.open.as_ref().unwrap().inspect_scroll;
    assert!(scroll_after_press > 0);
    drag(&mut app, 119, 32);
    assert!(app.open.as_ref().unwrap().inspect_scroll < scroll_after_press);
    release(&mut app, 119, 32);
    assert!(app.dragging_scrollbar.is_none());
}

#[test]
fn clicking_and_dragging_doc_scrollbar() {
    let v = Vault::new("scrollbar-doc");
    let mut app = laid_out(&v);
    let long_body = (0..100).map(|i| format!("Line {i}\n\n")).collect::<String>();
    let path = v.write(
        "wiki/long_doc.md",
        &format!("---\ntitle: LD\ntype: concept\ncategory: c\n---\n{long_body}"),
    );
    app.open_path(&path, false);
    press(&mut app, 119, 20);
    assert_eq!(app.dragging_scrollbar, Some(ScrollbarDrag::Doc));
    assert!(app.open.as_ref().unwrap().scroll > 0);
    release(&mut app, 119, 20);
    assert!(app.dragging_scrollbar.is_none());
}

#[test]
fn clicking_doc_scrollbar_at_inner_columns() {
    let v = Vault::new("scrollbar-inner");
    let mut app = laid_out(&v);
    let long_body = (0..100).map(|i| format!("Line {i}\n\n")).collect::<String>();
    let path = v.write(
        "wiki/long_doc.md",
        &format!("---\ntitle: LD\ntype: concept\ncategory: c\n---\n{long_body}"),
    );
    app.open_path(&path, false);
    // Column 118 is right_border - 1 (inner thumb column)
    press(&mut app, 118, 20);
    assert_eq!(app.dragging_scrollbar, Some(ScrollbarDrag::Doc));
    release(&mut app, 118, 20);

    // Column 117 is right_border - 2 (reach leeway column)
    press(&mut app, 117, 20);
    assert_eq!(app.dragging_scrollbar, Some(ScrollbarDrag::Doc));
    release(&mut app, 117, 20);
}

#[test]
fn a_click_in_an_empty_pane_is_ignored() {
    let v = Vault::new("mouse-empty");
    let mut app = v.app();
    app.areas.sidebar = Rect::ZERO;
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 200,
        row: 200,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.open.is_none());
}

#[test]
fn keyboard_resize_adjusts_active_pane_and_persists() {
    let v = Vault::new("key-resize");
    let mut app = laid_out(&v);
    app.focus = Focus::Tree;
    let initial_tree = app.cfg.tree_width;
    app.run(Cmd::WidenPane);
    assert_eq!(app.cfg.tree_width, initial_tree + 4);
    app.run(Cmd::ShrinkPane);
    assert_eq!(app.cfg.tree_width, initial_tree);

    // Focusing sidebar resizes sidebar
    app.focus = Focus::Sidebar;
    let initial_sidebar = app.cfg.sidebar_width;
    app.run(Cmd::WidenSidebar);
    assert_eq!(app.cfg.sidebar_width, initial_sidebar + 4);
    app.run(Cmd::ShrinkSidebar);
    assert_eq!(app.cfg.sidebar_width, initial_sidebar);

    // When in Doc, WidenPane shrinks the sidebar so its divider follows
    // the arrow; the tree grows in its place.
    app.focus = Focus::Doc;
    app.show_sidebar = true;
    app.run(Cmd::WidenPane);
    assert_eq!(app.cfg.sidebar_width, initial_sidebar - 4);

    // And verifies persistence to config.yaml
    let reloaded = Config::load_with_herdr_theme(&app.cfg.root, None);
    assert_eq!(reloaded.sidebar_width, initial_sidebar - 4);
}

#[test]
fn mouse_drag_release_persists_new_widths() {
    let v = Vault::new("drag-persist");
    let mut app = laid_out(&v);
    press(&mut app, 29, 5);
    drag(&mut app, 49, 5);
    release(&mut app, 49, 5);
    assert_eq!(app.cfg.tree_width, 50);

    let reloaded = Config::load_with_herdr_theme(&app.cfg.root, None);
    assert_eq!(reloaded.tree_width, 50);
}

#[test]
fn drag_in_reader_selects_and_copies_text() {
    let v = Vault::new("drag-select");
    let mut app = laid_out(&v);
    app.open_path(&v.path("wiki/a.md"), true);
    app.areas.doc_body = Rect::new(32, 3, 50, 30);
    // Press at row 4 (line 1, the "Alpha" title — line 0 is the doc's
    // leading blank), col 32
    press(&mut app, 32, 4);
    assert!(app.selecting_text);
    assert!(app.open.as_ref().unwrap().selection.is_some());

    // Drag to col 40, past the end of the rendered heading, which clamps
    // to its 5 columns. The `#` markers are not drawn, so the selection is
    // the heading's text and nothing else — dragging across a title yields
    // "Alpha", not "# Alpha".
    drag(&mut app, 40, 4);
    let sel = app.open.as_ref().unwrap().selection.unwrap();
    assert_eq!(sel.anchor.col, 0);
    assert_eq!(sel.cursor.col, 5);

    // Release to copy
    release(&mut app, 40, 4);
    assert!(!app.selecting_text);
    assert!(app.open.as_ref().unwrap().selection.is_some());
    let toast = app.toasts.last().unwrap();
    assert!(toast.text.contains("copied"), "toast must announce copy: {}", toast.text);
}

#[test]
fn click_without_drag_clears_selection() {
    let v = Vault::new("click-clear");
    let mut app = laid_out(&v);
    app.open_path(&v.path("wiki/a.md"), true);
    app.areas.doc_body = Rect::new(32, 3, 50, 30);
    // First, drag to select
    press(&mut app, 32, 4);
    drag(&mut app, 40, 4);
    release(&mut app, 40, 4);
    assert!(app.open.as_ref().unwrap().selection.is_some());

    // Click without dragging
    press(&mut app, 35, 4);
    release(&mut app, 35, 4);
    assert!(app.open.as_ref().unwrap().selection.is_none());
}

#[test]
fn cmd_copy_copies_selection_then_page() {
    let v = Vault::new("cmd-copy");
    let mut app = laid_out(&v);
    app.open_path(&v.path("wiki/a.md"), true);
    app.areas.doc_body = Rect::new(32, 3, 50, 30);

    // With selection
    press(&mut app, 32, 3);
    drag(&mut app, 40, 3);
    release(&mut app, 40, 3);
    app.toasts.clear();
    app.run(Cmd::Copy);
    assert!(app.toasts.last().unwrap().text.contains("copied"));

    // Clear selection and copy page
    app.run(Cmd::ClearSelection);
    assert!(app.open.as_ref().unwrap().selection.is_none());
    app.toasts.clear();
    app.run(Cmd::Copy);
    assert!(app.toasts.last().unwrap().text.contains("copied page"));
}

#[test]
fn selecting_rendered_text_still_copies_raw_markdown_syntax() {
    // "See Beta and gone." renders from a body line that reads
    // "See [Beta](b.md) and [gone](nope.md)." in the source — a partial
    // selection over the rendered prose must still yield that markdown
    // syntax, not the stripped display text.
    let v = Vault::new("copy-raw-source");
    let mut app = laid_out(&v);
    app.open_path(&v.path("wiki/a.md"), true);
    let open = app.open.as_ref().unwrap();
    let line = open
        .doc
        .lines
        .iter()
        .position(|l| l.plain_text().contains("See Beta and gone."))
        .expect("rendered line with the stripped link text");
    let sel = markdown::Selection::new(
        markdown::TextPos { line, col: 0 },
        markdown::TextPos { line, col: open.doc.lines[line].plain_text().chars().count() },
    );
    let text = open.selected_source_text(sel);
    assert_eq!(text, "See [Beta](b.md) and [gone](nope.md).");
}

#[test]
fn uncited_sources_are_listed_from_the_citation_graph() {
    let v = Vault::new("uncited");
    std::fs::create_dir_all(v.path("sources/lit/x")).unwrap();
    std::fs::write(v.path("sources/lit/x/metadata.md"), "m\n").unwrap();
    let mut app = v.app();
    app.index = crate::vault::index::Index::build(&v.0, &app.collection_dirs());
    app.run(Cmd::Uncited);
    let Some(Overlay::Finder(f)) = app.overlay.as_ref() else { panic!("no finder") };
    assert_eq!(f.hits.len(), 1);
    assert_eq!(f.hits[0].rel, "sources/lit/x/metadata.md");
    assert!(f.warning.as_ref().unwrap().contains("not cited"));
}

#[test]
fn overlays_swallow_keys_that_would_otherwise_quit() {
    let v = Vault::new("swallow");
    let mut app = v.app();
    app.run(Cmd::FindFiles);
    app.on_key(ch('q'));
    assert!(!app.quit, "q typed into the finder is a query, not a quit");
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.overlay.is_none());
    app.on_key(ctrl('q'));
    assert!(app.quit);
}

#[test]
fn jumping_between_findings_visits_the_broken_link() {
    let v = Vault::new("findings");
    let mut app = v.app();
    let findings = app.index.all_findings();
    assert!(!findings.is_empty(), "expected findings, got none");
    app.run(Cmd::NextFinding);
    assert_eq!(app.open.as_ref().unwrap().page.path, v.path("wiki/a.md"));
    assert!(
        app.toasts.last().unwrap().text.contains("finding 1/"),
        "{:?}",
        app.toasts.last().unwrap().text
    );
}

#[test]
fn a_clean_vault_says_so_rather_than_jumping_nowhere() {
    let v = Vault::new("clean");
    std::fs::write(v.path("wiki/a.md"), "---\ntitle: A\ntype: concept\ncategory: c\nrationale: r\n---\n# A\n").unwrap();
    let mut app = v.app();
    app.run(Cmd::NextFinding);
    assert_eq!(app.toasts.last().unwrap().level, Level::Good);
}

#[test]
fn a_filesystem_change_refreshes_the_index_and_the_open_page() {
    let v = Vault::new("fs");
    let mut app = v.app();
    let path = v.path("wiki/b.md");
    app.open_path(&path, true);

    std::fs::write(&path, "---\ntitle: Beta Renamed\ntype: concept\ncategory: c\nrationale: r\n---\n# Beta Renamed\n").unwrap();
    app.on_fs_changed(vec![path.clone()]);

    assert_eq!(app.open.as_ref().unwrap().page.title(), "Beta Renamed");
    assert_eq!(app.index.get("wiki/b.md").unwrap().title, "Beta Renamed");
}

#[test]
fn lint_results_are_summarised_not_dumped() {
    let v = Vault::new("lint");
    let mut app = v.app();
    app.on_job_done(JobResult {
        id: 1,
        label: "lint".into(),
        args: vec!["lint".into(), "--json".into()],
        code: 1,
        stdout: "{\"ok\": false, \"files\": {\"wiki/a.md\": [{\"code\": \"broken_link\", \"detail\": \"x\"}]}}".into(),
        stderr: String::new(),
    });
    let toast = app.toasts.last().unwrap();
    assert_eq!(toast.level, Level::Warn);
    assert!(toast.text.contains("1 findings across 1 files"), "{}", toast.text);
}

#[test]
fn a_clean_lint_is_reported_as_good() {
    let v = Vault::new("lint-clean");
    let mut app = v.app();
    app.on_job_done(JobResult {
        id: 1,
        label: "lint".into(),
        args: vec!["lint".into(), "--json".into()],
        code: 0,
        stdout: "{\"ok\": true, \"files\": {}}".into(),
        stderr: String::new(),
    });
    assert_eq!(app.toasts.last().unwrap().level, Level::Good);
}

#[test]
fn toasts_expire_and_never_pile_up() {
    let v = Vault::new("toasts");
    let mut app = v.app();
    for i in 0..6 {
        app.toast(Level::Info, format!("message {i}"));
    }
    assert_eq!(app.toasts.len(), 3);
    assert!(app.toasts[0].text.contains('3'));

    app.toasts[0].at = Instant::now() - TOAST_TTL - Duration::from_secs(1);
    app.expire_toasts();
    assert_eq!(app.toasts.len(), 2);
}

#[test]
fn ctrl_f_searches_the_open_page_rather_than_the_whole_vault() {
    let v = Vault::new("find-in-page");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.on_key(ctrl('f'));
    assert!(app.find.is_some(), "ctrl+f opens the in-page bar");
    assert!(app.overlay.is_none(), "and never the vault-wide finder");

    for c in "beta".chars() {
        app.on_key(ch(c));
    }
    assert_eq!(app.finds().hits.len(), 1, "case-insensitive, and only this page");

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.find.is_none(), "esc closes it");
}

#[test]
fn enter_walks_the_hits_and_wraps_round() {
    let v = Vault::new("find-cycle");
    let mut app = v.app();
    let path = v.write(
        "wiki/c.md",
        "---\ntitle: Gamma\ntype: concept\ncategory: c\nrationale: r\n---\n# Gamma\n\nfirst hit\n\nsecond hit\n\nthird hit\n",
    );
    app.open_path(&path, true);
    app.run(Cmd::FindInPage);
    for c in "hit".chars() {
        app.on_key(ch(c));
    }
    assert_eq!(app.finds().hits.len(), 3);
    assert_eq!(app.finds().current, 0, "typing lands on the first hit");

    let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    app.on_key(enter);
    assert_eq!(app.finds().current, 1);
    app.on_key(enter);
    app.on_key(enter);
    assert_eq!(app.finds().current, 0, "wraps past the last hit");
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
    assert_eq!(app.finds().current, 2, "shift+enter walks back");
}

#[test]
fn the_find_bar_swallows_keys_that_would_otherwise_be_commands() {
    let v = Vault::new("find-swallows");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::FindInPage);
    // `q` quits and `e` edits when the reader has focus; in the bar they
    // are two characters of a query.
    app.on_key(ch('q'));
    app.on_key(ch('e'));
    assert!(!app.quit);
    assert!(!app.open.as_ref().unwrap().editing());
    assert_eq!(app.find.as_ref().unwrap().query, "qe");
    app.on_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(app.find.as_ref().unwrap().query, "q");
}

#[test]
fn a_hit_below_the_fold_is_scrolled_into_view() {
    let v = Vault::new("find-scroll");
    let mut app = v.app();
    let mut body =
        String::from("---\ntitle: Long\ntype: concept\ncategory: c\nrationale: r\n---\n# Long\n\n");
    for i in 0..120 {
        body.push_str(&format!("filler line {i}\n\n"));
    }
    body.push_str("the needle\n");
    let path = v.write("wiki/long.md", &body);
    app.open_path(&path, true);
    // The reader has not been drawn, so give it the viewport a draw would.
    app.areas.doc_body = Rect::new(0, 0, 80, 20);
    app.run(Cmd::FindInPage);
    for c in "needle".chars() {
        app.on_key(ch(c));
    }
    let finds = app.finds();
    assert_eq!(finds.hits.len(), 1);
    let open = app.open.as_ref().unwrap();
    let line = finds.hits[0].line;
    assert!(line >= open.scroll && line < open.scroll + 20, "hit {line} off screen at {}", open.scroll);
}

#[test]
fn find_declines_politely_when_nothing_is_open() {
    let v = Vault::new("find-empty");
    let mut app = v.app();
    app.run(Cmd::FindInPage);
    assert!(app.find.is_none());
    assert!(app.toasts.last().unwrap().text.contains("no page open"));
}

#[test]
fn the_outline_jumps_to_a_heading() {
    let v = Vault::new("outline");
    let mut app = v.app();
    app.open_path(&v.path("wiki/a.md"), true);
    app.run(Cmd::Outline);
    assert!(matches!(app.overlay, Some(Overlay::Outline { .. })));
    app.on_key(ch('j'));
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.overlay.is_none());
    assert!(app.open.as_ref().unwrap().scroll > 0);
}
