use super::*;
use super::chrome::*;
use super::document::*;
use super::inspector::*;
use super::tree::*;
use super::wrap::*;


    /// The home screen is a history position, so it draws the same arrows a
    /// page does and publishes the same hit rects. Without them there is no
    /// way to click your way back out of it.
    #[test]
    fn the_home_screen_carries_the_navigation_arrows_too() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let count = INSP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("podarcis-homenav-{}-{count}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".podarcis")).unwrap();
        std::fs::create_dir_all(dir.join("wiki")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "").unwrap();
        std::fs::write(dir.join(".podarcis/config.yaml"), "").unwrap();
        std::fs::write(dir.join("wiki/a.md"), "---\ntitle: Alpha\n---\nbody\n").unwrap();

        let (mut app, _rx) = crate::app::test_app(&dir);
        let mut terminal = Terminal::new(TestBackend::new(48, 12)).unwrap();
        let top = |app: &mut App, terminal: &mut Terminal<TestBackend>| -> String {
            terminal.draw(|f| document(f, app, f.area())).unwrap();
            let b = terminal.backend().buffer().clone();
            (0..b.area.width).map(|x| b[(x, 0)].symbol().to_string()).collect()
        };

        let row = top(&mut app, &mut terminal);
        assert!(row.contains('←') && row.contains('→'), "home draws both arrows: {row}");
        let (back, forward) = (app.areas.nav_back, app.areas.nav_forward);
        assert!(back.is_some() && forward.is_some(), "and publishes both hit rects");

        // The rects are where the glyphs actually are, so a click lands on
        // what it looks like it is hitting.
        let glyph_at = |rect: Rect, terminal: &Terminal<TestBackend>| -> String {
            let b = terminal.backend().buffer();
            (rect.x..rect.x + rect.width).map(|x| b[(x, rect.y)].symbol().to_string()).collect()
        };
        assert_eq!(glyph_at(back.unwrap(), &terminal).trim(), "←");
        assert_eq!(glyph_at(forward.unwrap(), &terminal).trim(), "→");

        // Leaving and coming back keeps them in the same place on both screens.
        app.open_path(&dir.join("wiki/a.md"), true);
        let row = top(&mut app, &mut terminal);
        assert!(row.contains("Alpha"), "the reader names the page: {row}");
        assert_eq!(app.areas.nav_back, back, "the arrows do not move between screens");

        app.run(crate::keymap::Cmd::Home);
        let row = top(&mut app, &mut terminal);
        assert!(!row.contains("Alpha"), "g h leaves the page: {row}");
        assert_eq!(app.areas.nav_back, back);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// You must be able to tell what you are reading without leaving the
    /// reader, and the prose must sit in a centred measure rather than hard
    /// against the left border of a wide pane.
    #[test]
    fn the_reader_names_the_page_and_centres_its_measure() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let count = INSP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("podarcis-reader-{}-{count}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".podarcis")).unwrap();
        std::fs::create_dir_all(dir.join("wiki")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "").unwrap();
        std::fs::write(dir.join(".podarcis/config.yaml"), "").unwrap();
        let page = dir.join("wiki/glycogen.md");
        std::fs::write(
            &page,
            "---\ntitle: Glycogen Resynthesis\ntype: concept\ncategory: c\nrationale: r\nstatus: verified\n---\nbody text\n",
        )
        .unwrap();

        let (mut app, _rx) = crate::app::test_app(&dir);
        app.open_path(&page, true);

        // Far wider than MAX_WIDTH, so the measure has surplus to centre in.
        let width = crate::ui::markdown::MAX_WIDTH + 60;
        let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
        terminal.draw(|f| document(f, &mut app, f.area())).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect())
            .collect();
        let screen = rows.join("\n");

        assert!(rows[0].contains("Glycogen Resynthesis"), "the title bar names the page: {:?}", rows[0]);
        assert!(rows[0].contains("wiki"), "and the collection it lives in: {:?}", rows[0]);
        assert!(rows[0].contains("verified"), "and its OKF status: {:?}", rows[0]);

        let body = rows.iter().find(|r| r.contains("body text")).expect("the body is drawn");
        let left = body.find("body text").unwrap();
        let right = body.len() - (left + "body text".len());
        assert!(
            left > 20,
            "the measure is centred, not flush left (text starts at column {left}):\n{screen}"
        );
        assert!(right > 20, "and there is surplus on the right too:\n{screen}");
        assert_eq!(
            app.areas.doc_body.width,
            crate::ui::markdown::MAX_WIDTH,
            "the hit region is the text region, so clicks still land where the words are"
        );

        if std::env::var("SHOW").is_ok() {
            println!("{screen}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The front door carries the brand. It is the one screen whose whole job
    /// is to say what this is, so the mark, the wordmark and the tagline are
    /// all load-bearing.
    #[test]
    fn the_empty_document_shows_the_mark_the_wordmark_and_the_tagline() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let count = INSP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("podarcis-splash-{}-{count}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".podarcis")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "").unwrap();
        std::fs::write(dir.join(".podarcis/config.yaml"), "").unwrap();
        let (mut app, _rx) = crate::app::test_app(&dir);
        assert!(app.open.is_none(), "nothing is open, so this is the splash");

        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| document(f, &mut app, f.area())).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect())
            .collect();
        let screen = rows.join("\n");

        assert!(screen.contains(crate::brand::name()), "the wordmark is on screen:\n{screen}");
        assert!(screen.contains(crate::brand::tagline()), "the tagline is on screen:\n{screen}");
        for row in crate::brand::logo_lines().iter().filter(|r| !r.trim().is_empty()) {
            assert!(screen.contains(row.trim_end()), "logo row {row:?} is drawn:\n{screen}");
        }
        assert!(screen.contains("command palette"), "the key hints survive:\n{screen}");

        // The mark is centred as a block, so every one of its rows starts at
        // the same column — a per-row centring would shear the drawing.
        let starts: Vec<usize> = rows
            .iter()
            .filter(|r| r.contains('\u{2588}'))
            .map(|r| r.find(|c: char| c != ' ' && c != '\u{2502}' && c != '\u{256d}' && c != '\u{2570}').unwrap())
            .collect();
        assert!(starts.len() >= 3, "several logo rows are drawn");

        if std::env::var("SHOW").is_ok() {
            println!("{screen}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn navigation_arrows_rendered_in_block_title() {
        use ratatui::buffer::Buffer;
        use ratatui::widgets::Widget;
        let area = Rect::new(0, 0, 20, 3);
        let mut buf = Buffer::empty(area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::from(vec![Span::raw(" ← "), Span::raw(" → ")]));
        block.render(area, &mut buf);
        assert_eq!(buf[(0, 0)].symbol(), "╭");
        assert_eq!(buf[(1, 0)].symbol(), " ");
        assert_eq!(buf[(2, 0)].symbol(), "←");
        assert_eq!(buf[(3, 0)].symbol(), " ");
        assert_eq!(buf[(4, 0)].symbol(), " ");
        assert_eq!(buf[(5, 0)].symbol(), "→");
        assert_eq!(buf[(6, 0)].symbol(), " ");
        assert_eq!(buf[(7, 0)].symbol(), "─");
    }

    /// The fold handle is on screen, folding hides the box's rows, and the
    /// handle stays reachable so it can be brought back.
    #[test]
    fn a_collection_folds_to_its_header_and_unfolds_from_the_same_handle() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let count = INSP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("podarcis-fold-{}-{count}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".podarcis")).unwrap();
        for c in ["wiki", "workspace", "sources"] {
            std::fs::create_dir_all(dir.join(c)).unwrap();
            std::fs::write(dir.join(c).join("a.md"), "---\ntitle: A\n---\nbody\n").unwrap();
        }
        std::fs::write(dir.join("AGENTS.md"), "").unwrap();
        std::fs::write(dir.join(".podarcis/config.yaml"), "").unwrap();

        let (mut app, _rx) = crate::app::test_app(&dir);
        let mut terminal = Terminal::new(TestBackend::new(34, 30)).unwrap();
        let screen = |app: &mut App, terminal: &mut Terminal<TestBackend>| -> String {
            let area = Rect::new(0, 0, 34, 30);
            terminal.draw(|f| tree(f, app, area)).unwrap();
            let b = terminal.backend().buffer().clone();
            (0..b.area.height)
                .map(|y| (0..b.area.width).map(|x| b[(x, y)].symbol().to_string()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        };

        let before = screen(&mut app, &mut terminal);
        assert!(before.contains('▾'), "an open box offers a fold handle:\n{before}");
        let panes = app.areas.tree_panes.clone();
        assert_eq!(panes.len(), 3);
        let handle = panes[0].fold.expect("the first box has a handle");
        let open_rows = panes[0].area.height;
        assert!(open_rows > FOLDED_ROWS);

        // Clicking the handle folds it, and the box shrinks to its rule.
        app.fold_collection(0);
        let after = screen(&mut app, &mut terminal);
        assert_eq!(app.areas.tree_panes[0].area.height, FOLDED_ROWS, "folded to a rule:\n{after}");
        assert!(after.contains('▸'), "and the handle now points at hidden content:\n{after}");
        assert_eq!(
            app.areas.tree_panes[0].fold.map(|r| (r.x, r.y)),
            Some((handle.x, handle.y)),
            "the handle stays where it was, so it can be clicked back"
        );
        // The boxes below take the freed rows; the column stays exactly full.
        let total: u16 = app.areas.tree_panes.iter().map(|p| p.area.height).sum();
        assert_eq!(total, 30);
        assert!(app.areas.tree_panes[2].area.height > FOLDED_ROWS);

        app.fold_collection(0);
        assert_eq!(app.areas.tree_panes.len(), 3);
        let back = screen(&mut app, &mut terminal);
        assert!(back.contains('▾'), "unfolding restores the handle:\n{back}");
        assert_eq!(app.areas.tree_panes[0].area.height, open_rows, "and the height it had");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The column must be exactly full at every terminal height, whatever the
    /// stored preferences say — a row of slack shows as a gap under the tree,
    /// and a row of overflow silently clips the last box.
    #[test]
    fn collection_rows_always_add_up_to_the_column() {
        for total in 0..60u16 {
            for folded in [
                [false, false, false],
                [true, false, false],
                [false, true, false],
                [false, false, true],
                [true, true, false],
                [true, true, true],
            ] {
                for wanted in [
                    [None, None, None],
                    [Some(10), None, None],
                    [Some(4), Some(4), Some(4)],
                    [Some(200), Some(200), Some(200)],
                    [Some(0), Some(1), Some(2)],
                ] {
                    let rows = collection_rows(&wanted, &folded, total);
                    assert_eq!(rows.len(), 3);
                    assert_eq!(
                        rows.iter().sum::<u16>(),
                        total,
                        "total={total} folded={folded:?} wanted={wanted:?} -> {rows:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_folded_collection_costs_one_row_and_the_rest_is_shared() {
        let rows = collection_rows(&[None, None, None], &[false, true, false], 31);
        assert_eq!(rows[1], FOLDED_ROWS, "the folded box is a header rule: {rows:?}");
        assert_eq!(rows[0] + rows[2], 30);
        assert_eq!(rows.iter().sum::<u16>(), 31);
    }

    #[test]
    fn a_stored_height_is_honoured_and_the_last_open_box_absorbs_the_rest() {
        let rows = collection_rows(&[Some(8), Some(5), None], &[false; 3], 40);
        assert_eq!(rows[0], 8, "the first box gets what it asked for");
        assert_eq!(rows[1], 5);
        assert_eq!(rows[2], 27, "the last open box takes the remainder");

        // The last box's own stored height is deliberately ignored — it is the
        // absorber, so honouring it could only leave the column short.
        let rows = collection_rows(&[Some(8), Some(5), Some(3)], &[false; 3], 40);
        assert_eq!(rows[2], 27);
    }

    /// A box may never be squeezed below the point where it can show a row,
    /// however greedy the box above it is.
    #[test]
    fn an_oversized_preference_cannot_starve_the_boxes_below_it() {
        let rows = collection_rows(&[Some(200), Some(200), None], &[false; 3], 20);
        assert_eq!(rows.iter().sum::<u16>(), 20);
        for (i, r) in rows.iter().enumerate() {
            assert!(*r >= MIN_COLLECTION_ROWS, "box {i} was starved: {rows:?}");
        }
    }

    /// Below the point where everyone can have a minimum there is no good
    /// answer, only a defined one: share evenly and stay exactly full.
    #[test]
    fn a_column_too_short_for_every_box_still_fills_exactly() {
        for total in 0..(MIN_COLLECTION_ROWS * 3) {
            let rows = collection_rows(&[Some(9), Some(9), Some(9)], &[false; 3], total);
            assert_eq!(rows.iter().sum::<u16>(), total, "total={total} -> {rows:?}");
        }
    }

    #[test]
    fn the_cursor_stays_a_third_down_once_the_list_scrolls() {
        assert_eq!(scroll_offset(0, 100, 30), 0);
        assert_eq!(scroll_offset(5, 100, 30), 0, "no scroll until the cursor passes the line");
        assert_eq!(scroll_offset(40, 100, 30), 30);
        assert_eq!(scroll_offset(99, 100, 30), 70, "clamped at the end");
        assert_eq!(scroll_offset(5, 10, 30), 0, "a short list never scrolls");
    }

    #[test]
    fn the_inspector_never_takes_more_than_half_the_pane() {
        let dir = std::env::temp_dir().join(format!("podarcis-panes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("wiki")).unwrap();
        let raw = format!(
            "---\ntitle: T\ntype: concept\ncategory: c\nrationale: r\nsources:\n{}---\nbody\n",
            (0..20).map(|i| format!("  - id: s{i}\n")).collect::<String>()
        );
        let path = dir.join("wiki/a.md");
        std::fs::write(&path, raw).unwrap();
        let page = crate::vault::page::Page::load(&path, &dir).unwrap();
        let open = Open {
            doc: crate::ui::markdown::render(&page.body, 40, &Theme::default()),
            page,
            doc_width: 40,
            hscroll: 0,
            scroll: 0,
            inspect_scroll: 0,
            link: None,
            editor: None,
            selection: None,
            selected_citation: None,
            citation_scroll_pending: false,
        };
        assert!(inspector_height(&open, 40) <= 20);
        assert!(inspector_height(&open, 12) >= 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    static INSP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn open_doc(body: &str) -> Open {
        let count = INSP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("podarcis-insp-{}-{count}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("wiki")).unwrap();
        let path = dir.join("wiki/a.md");
        std::fs::write(&path, format!("---\ntitle: T\n---\n{body}\n")).unwrap();
        let page = crate::vault::page::Page::load(&path, &dir).unwrap();
        let doc = crate::ui::markdown::render(&page.body, 40, &Theme::default());
        Open {
            doc,
            page,
            doc_width: 40,
            hscroll: 0,
            scroll: 0,
            inspect_scroll: 0,
            link: None,
            editor: None,
            selection: None,
            selected_citation: None,
            citation_scroll_pending: false,
        }
    }

    #[test]
    fn the_sources_section_lists_full_citations_in_citation_order_with_the_id_last() {
        let open = open_doc(
            "Cites beta first, then alpha[^b][^a].\n\n## Sources\n[^a]: [Alpha paper (2020). A deliberately long title so the truncation that used to eat it would be visible in the row. *Journal of Letters*, 1(1).](../lit/a/metadata.md)\n[^b]: [Beta paper (2019).](../lit/b/metadata.md)\n",
        );
        let root = open.page.path.parent().unwrap().to_path_buf();
        let index = crate::vault::index::Index::default();
        let lines = inspector_lines(&open, &index, &root, &Theme::default());

        let rows: Vec<String> = lines.iter().map(|l| format!("{l}")).collect();
        assert_eq!(rows.len(), 5, "two citations with top, middle, and bottom blank lines");
        assert!(rows[0].is_empty(), "line on top of first source");
        assert!(
            rows[1].starts_with("[1]") && rows[3].starts_with("[2]"),
            "numbered in body citation order, not definition order: {:?}",
            rows
        );
        assert!(rows[1].contains("Beta paper (2019)"));
        assert!(rows[2].is_empty(), "extra line between sources");
        assert!(rows[3].to_lowercase().contains("long title"), "full text, not a truncated copy: {:?}", rows);
        assert!(rows[3].to_lowercase().contains("journal of letters"));
        assert!(rows[1].ends_with("b"), "the id hangs at the row's end: {:?}", rows[1]);
        assert!(rows[3].ends_with("a"), "the id hangs at the row's end: {:?}", rows[3]);
        assert!(rows[4].is_empty(), "line after last source");
    }

    #[test]
    fn an_uncited_source_rows_after_the_numbered_ones() {
        let open = open_doc("One cited[^a].\n\n## Sources\n[^a]: [A paper (2021).](../lit/a/metadata.md)\n[^ghost]: [Never cited (2022).](../lit/g/metadata.md)\n");
        let root = open.page.path.parent().unwrap().to_path_buf();
        let index = crate::vault::index::Index::default();
        let lines = inspector_lines(&open, &index, &root, &Theme::default());

        let rows: Vec<String> = lines.iter().map(|l| format!("{l}")).collect();
        assert_eq!(rows.len(), 5);
        assert!(rows[0].is_empty(), "line on top of first source");
        assert!(rows[1].starts_with("[1]") && rows[1].contains("A paper (2021)"));
        assert!(rows[2].is_empty(), "extra line between sources");
        assert!(rows[3].starts_with("·") && rows[3].contains("Never cited (2022)"));
        assert!(rows[4].is_empty(), "line after last source");
    }

    #[test]
    fn biblio_wrapping_hangs_continuation_lines_under_the_citation() {
        let row = Line::from(vec![
            Span::raw("[2] "),
            Span::raw("A paper title long enough that it must wrap onto another line here."),
            Span::raw("  fame1970"),
        ]);
        let wrapped = wrap_biblio(row, 20);
        let rows: Vec<String> = wrapped.iter().map(|l| format!("{l}")).collect();
        assert!(rows.len() > 1, "long citation wraps: {:?}", rows);
        assert!(rows[0].starts_with("[2] "), "first line keeps the mark: {:?}", rows);
        for row in rows.iter().skip(1) {
            assert!(row.starts_with("    "), "continuations hang 4 under the mark: {:?}", row);
        }
    }

    #[test]
    fn biblio_wrapping_handles_separated_mark_and_space_without_tinting_spacer() {
        use ratatui::style::Color;
        let row = Line::from(vec![
            Span::styled("[1]", Style::default().bg(Color::Blue)),
            Span::raw(" "),
            Span::raw("A paper title long enough that it must wrap onto another line here."),
            Span::raw("  fame1970"),
        ]);
        let wrapped = wrap_biblio(row, 20);
        let rows: Vec<String> = wrapped.iter().map(|l| format!("{l}")).collect();
        assert!(rows.len() > 1, "long citation wraps: {:?}", rows);
        assert!(rows[0].starts_with("[1] "), "first line keeps the mark and space: {:?}", rows);
        assert_eq!(wrapped[0].spans[0].content, "[1]");
        assert_eq!(wrapped[0].spans[0].style.bg, Some(Color::Blue));
        assert_eq!(wrapped[0].spans[1].content, " ");
        assert_eq!(wrapped[0].spans[1].style.bg, None);
        for (i, line) in wrapped.iter().enumerate().skip(1) {
            assert!(rows[i].starts_with("    "), "continuations hang 4 under the mark: {:?}", rows[i]);
            assert_eq!(line.spans[0].content, "    ");
            assert_eq!(line.spans[0].style.bg, None, "spacer must not inherit mark background");
        }
    }

    #[test]
    fn selecting_a_citation_highlights_its_number_mark_not_its_id() {
        let mut open = open_doc(
            "Cites beta first, then alpha[^b][^a].\n\n## Sources\n[^a]: [Alpha paper (2020).](../lit/a/metadata.md)\n[^b]: [Beta paper (2019).](../lit/b/metadata.md)\n",
        );
        let root = open.page.path.parent().unwrap().to_path_buf();
        let index = crate::vault::index::Index::default();
        let theme = Theme::default();

        // When nothing is selected, [1] has ok colour, no bg; and id 'b' has faint style.
        let rows = inspector_rows(&open, &index, &root, &theme);
        assert!(rows[0].0.spans.is_empty(), "row 0 is blank top line");
        let (first_line, first_id) = &rows[1];
        assert_eq!(first_id.as_deref(), Some("b"));
        assert_eq!(first_line.spans[0].content, "[1]");
        assert_eq!(first_line.spans[0].style.fg, Some(theme.ok));
        assert_eq!(first_line.spans[0].style.bg, None);
        // The id span (index 3) is faint, not highlighted
        assert_eq!(first_line.spans[3].content, "  b");
        assert_eq!(first_line.spans[3].style.bg, None);

        // When citation "b" is selected, [1] is highlighted with accent bg, while the id stays faint.
        open.selected_citation = Some("b".to_string());
        let rows = inspector_rows(&open, &index, &root, &theme);
        let (first_line, first_id) = &rows[1];
        assert_eq!(first_id.as_deref(), Some("b"));
        assert_eq!(first_line.spans[0].content, "[1]");
        assert_eq!(first_line.spans[0].style.bg, Some(theme.accent));
        assert_eq!(first_line.spans[0].style.fg, Some(theme.bg));
        // id 'b' is still not highlighted with accent bg
        assert_eq!(first_line.spans[3].content, "  b");
        assert_eq!(first_line.spans[3].style.bg, None);

        // When the active link in the reader is [^b]
        open.selected_citation = None;
        open.link = open.doc.citation_link("b");
        let rows = inspector_rows(&open, &index, &root, &theme);
        let (first_line, _) = &rows[1];
        assert_eq!(first_line.spans[0].style.bg, Some(theme.accent));
        assert_eq!(first_line.spans[3].style.bg, None);
    }

    #[test]
    fn backlinks_render_with_dot_separator_and_link_style() {
        let entries = vec![
            ("Agent Memory Index".to_string(), std::path::PathBuf::from("/vault/wiki/index.md")),
            ("Agent Memory Taxonomies".to_string(), std::path::PathBuf::from("/vault/wiki/tax.md")),
        ];
        let theme = Theme::default();
        let (lines, links) = wrap_backlinks(&entries, 80, &theme);

        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(format!("{line}"), "← Agent Memory Index · Agent Memory Taxonomies");

        assert_eq!(line.spans[0].content, "← Agent Memory Index");
        assert_eq!(line.spans[0].style.fg, Some(theme.link));

        assert_eq!(line.spans[1].content, " · ");
        assert_eq!(line.spans[1].style, theme.dim());

        assert_eq!(line.spans[2].content, "Agent Memory Taxonomies");
        assert_eq!(line.spans[2].style.fg, Some(theme.link));

        assert_eq!(links.len(), 2);
        assert_eq!(links[0].row, 0);
        assert_eq!(links[0].col_start, 2);
        assert_eq!(links[0].col_end, 20);
        assert_eq!(links[0].path, std::path::PathBuf::from("/vault/wiki/index.md"));

        assert_eq!(links[1].row, 0);
        assert_eq!(links[1].col_start, 23);
        assert_eq!(links[1].col_end, 46);
        assert_eq!(links[1].path, std::path::PathBuf::from("/vault/wiki/tax.md"));
    }

    #[test]
    fn wrap_backlinks_handles_multiline_wrapping() {
        let entries = vec![
            ("Agent Memory Index".to_string(), std::path::PathBuf::from("/vault/wiki/index.md")),
            ("Agent Memory Taxonomies".to_string(), std::path::PathBuf::from("/vault/wiki/tax.md")),
        ];
        let theme = Theme::default();
        // Width 25 forces wrap between the first and second entry
        let (lines, links) = wrap_backlinks(&entries, 25, &theme);

        assert_eq!(lines.len(), 2);
        assert_eq!(format!("{}", lines[0]), "← Agent Memory Index ·");
        assert_eq!(format!("{}", lines[1]), "Agent Memory Taxonomies");

        assert_eq!(links.len(), 2);
        assert_eq!(links[0].row, 0);
        assert_eq!(links[0].col_start, 2);
        assert_eq!(links[0].col_end, 20);

        assert_eq!(links[1].row, 1);
        assert_eq!(links[1].col_start, 0);
        assert_eq!(links[1].col_end, 23);
        assert_eq!(links[1].path, std::path::PathBuf::from("/vault/wiki/tax.md"));
    }

    #[test]
    fn status_bar_renders_project_selector_instead_of_read() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use crate::config::Config;

        let count = INSP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("podarcis-status-{}-{count}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".podarcis")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "").unwrap();
        std::fs::write(dir.join(".podarcis/config.yaml"), "").unwrap();

        let cfg = Config::load(&dir);
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::new(cfg, tx);
        app.project_name = "test-project".to_string();

        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                status(f, &app, f.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let content: String = (0..buffer.area.width)
            .map(|x| buffer[(x, 0)].symbol().to_string())
            .collect();

        assert!(content.contains("test-project ▾"), "status has project selector: {content}");

        // The bar sits on `surface`, so nothing in it may use `faint` — that
        // is the gutter token, and text painted with it on a raised
        // background is what made the bar hard to read.
        for x in 0..buffer.area.width {
            assert_ne!(
                buffer[(x, 0)].fg,
                app.theme.faint,
                "column {x} of the status bar is painted in the gutter colour: {content}"
            );
        }
        assert_eq!(
            buffer[(1, 0)].fg,
            app.theme.on_accent(),
            "the badge takes its foreground from the palette, never a hardcoded white"
        );
        assert_eq!(buffer[(1, 0)].bg, app.theme.accent);
        assert!(!content.contains("READ"), "status does not contain READ: {content}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The sources pane sits below the reader, not on top of it: the reader's
    /// own bottom border survives, and the end of the page can be scrolled
    /// into the rows above it.
    #[test]
    fn the_sources_pane_does_not_cover_the_end_of_the_page() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let count = INSP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("podarcis-inspect-overlap-{}-{count}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".podarcis")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "").unwrap();
        std::fs::write(dir.join(".podarcis/config.yaml"), "").unwrap();
        std::fs::create_dir_all(dir.join("wiki")).unwrap();
        let body: String = (1..=60).map(|i| format!("para {i}\n\n")).collect();
        let page = dir.join("wiki/long.md");
        std::fs::write(
            &page,
            format!("---\ntitle: Long\ntype: concept\ncategory: c\nrationale: r\n---\n{body}"),
        )
        .unwrap();

        let (mut app, _rx) = crate::app::test_app(&dir);
        app.open_path(&page, true);

        let mut terminal = Terminal::new(TestBackend::new(60, 30)).unwrap();
        let draw = |app: &mut App, terminal: &mut Terminal<TestBackend>| {
            terminal.draw(|f| document(f, app, f.area())).unwrap();
            let buffer = terminal.backend().buffer().clone();
            let rows: Vec<String> = (0..buffer.area.height)
                .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect())
                .collect();
            rows
        };

        // First frame measures the panes; the second scrolls with those
        // measurements in hand, as the event loop does.
        draw(&mut app, &mut terminal);
        let reader_bottom = app.areas.inspector.y.saturating_sub(1) as usize;
        app.run(crate::keymap::Cmd::DocBottom);
        let rows = draw(&mut app, &mut terminal);

        assert!(app.areas.inspector.y > app.areas.doc_main.y, "the sources pane is below the reader");
        assert!(
            rows[reader_bottom].chars().all(|c| "╰╯─".contains(c)),
            "the reader keeps its own bottom border: {:?}",
            rows[reader_bottom]
        );
        assert!(
            rows[..reader_bottom].iter().any(|r| r.contains("para 60")),
            "the last paragraph scrolls into the reader:\n{}",
            rows.join("\n")
        );
    }

    /// The editor resolves clicks against the text area it records while
    /// rendering, so this needs a real frame before the click means anything.
    #[test]
    fn clicking_in_the_editor_places_the_cursor() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let count = INSP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("podarcis-editor-click-{}-{count}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".podarcis")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "").unwrap();
        std::fs::write(dir.join(".podarcis/config.yaml"), "").unwrap();
        std::fs::create_dir_all(dir.join("wiki")).unwrap();
        let body: String = (1..=12).map(|i| format!("line {i} with some words\n")).collect();
        let page = dir.join("wiki/click.md");
        std::fs::write(
            &page,
            format!("---\ntitle: Click\ntype: concept\ncategory: c\nrationale: r\n---\n{body}"),
        )
        .unwrap();

        let (mut app, _rx) = crate::app::test_app(&dir);
        app.open_path(&page, true);
        app.run(crate::keymap::Cmd::Edit);
        // `draw` calls `sync_sidebar`, which would spawn `herdr --session
        // podarcis` in this temp dir and steal the live TUI's workspace.
        app.show_sidebar = false;

        // The whole frame, not just this pane: `App::on_mouse` resolves the
        // pointer against `areas.doc`, which only the top-level layout sets.
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, &mut app)).unwrap();

        let before = {
            let editor = app.open.as_ref().unwrap().editor.as_ref().unwrap();
            (editor.cursor_line(), editor.cursor_col())
        };
        let area = app.areas.doc_main;
        // Four rows down and ten columns in, well clear of the gutter.
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + 10,
            row: area.y + 4,
            modifiers: crossterm::event::KeyModifiers::NONE,
        });

        let editor = app.open.as_ref().unwrap().editor.as_ref().unwrap();
        assert_ne!((editor.cursor_line(), editor.cursor_col()), before, "the click moved the cursor");
        assert!(app.open.as_ref().unwrap().editing(), "and left the editor open");
    }

    #[test]
    fn tree_renders_repo_branch_and_omits_non_git() {
        use std::process::Command;
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use crate::config::Config;

        let count = INSP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("podarcis-tree-branch-{}-{count}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".podarcis")).unwrap();
        std::fs::create_dir_all(dir.join("wiki")).unwrap();
        std::fs::create_dir_all(dir.join("sources")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "").unwrap();
        std::fs::write(
            dir.join(".podarcis/config.yaml"),
            "repositories:\n  wiki: local\n  sources: local\n",
        ).unwrap();

        // init git in wiki on branch "test-branch"
        Command::new("git").args(["init"]).current_dir(dir.join("wiki")).output().unwrap();
        Command::new("git").args(["checkout", "-b", "test-branch"]).current_dir(dir.join("wiki")).output().unwrap();

        // sources is not a git repo

        let cfg = Config::load(&dir);
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::new(cfg, tx);

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                tree(f, &mut app, f.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                rendered.push_str(buffer[(x, y)].symbol());
            }
            rendered.push('\n');
        }

        assert!(rendered.contains("wiki · test-branch"), "tree contains directory and branch separated by ·: {rendered}");
        assert!(!rendered.contains("git repo"), "tree does not contain 'git repo': {rendered}");
        assert!(!rendered.contains("no git"), "tree does not contain 'no git': {rendered}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_wide_csv_says_how_many_columns_are_off_screen() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let dir = std::env::temp_dir().join(format!("podarcis-csv-chip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".podarcis")).unwrap();
        std::fs::create_dir_all(dir.join("workspace/finance")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "").unwrap();
        std::fs::write(dir.join(".podarcis/config.yaml"), "").unwrap();
        let cols: Vec<String> = (0..14).map(|i| format!("col_{i}")).collect();
        let page = dir.join("workspace/finance/wide.csv");
        std::fs::write(&page, format!("{}\n", cols.join(","))).unwrap();

        let (mut app, _rx) = crate::app::test_app(&dir);
        app.open_path(&page, true);
        let mut terminal = Terminal::new(TestBackend::new(60, 12)).unwrap();
        let screen = |app: &mut App, terminal: &mut Terminal<TestBackend>| -> String {
            terminal.draw(|f| document(f, app, f.area())).unwrap();
            let buffer = terminal.backend().buffer().clone();
            (0..buffer.area.height)
                .map(|y| {
                    (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let first = screen(&mut app, &mut terminal);
        assert!(first.contains('▸'), "the frame says the table continues right:\n{first}");
        assert!(!first.contains('◂'), "nothing is off to the left yet:\n{first}");

        for _ in 0..40 {
            app.run(crate::keymap::Cmd::PanRight);
        }
        let last = screen(&mut app, &mut terminal);
        assert!(last.contains("col_13"), "panning right reaches the final column:\n{last}");
        assert!(last.contains('◂'), "and the frame says what is now behind you:\n{last}");
        assert!(!last.contains('▸'), "the right edge is the end of the pan:\n{last}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tree_renders_a_csv_badge_to_the_left_of_the_label() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        use crate::config::Config;

        let count = INSP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("podarcis-tree-csv-badge-{}-{count}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".podarcis")).unwrap();
        std::fs::create_dir_all(dir.join("workspace/finance")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "").unwrap();
        std::fs::write(dir.join(".podarcis/config.yaml"), "").unwrap();
        std::fs::write(dir.join("workspace/finance/transactions.csv"), "a,b\n1,2\n").unwrap();

        let cfg = Config::load(&dir);
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::new(cfg, tx);
        app.tree.expand(); // workspace
        app.tree.rebuild();
        app.tree.expand(); // finance
        app.tree.rebuild();

        for flavor in [crate::theme::Flavor::Terminal, crate::theme::Flavor::Latte] {
            app.theme = crate::theme::Theme::new(flavor);
            let backend = TestBackend::new(80, 20);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal.draw(|f| tree(f, &mut app, f.area())).unwrap();

            let buffer = terminal.backend().buffer();
            let mut rendered = String::new();
            let mut badge_bg = None;
            for y in 0..buffer.area.height {
                for x in 0..buffer.area.width {
                    let cell = &buffer[(x, y)];
                    // The badge is now bare letters on a colour block, so the
                    // `C` of `CSV` is what identifies it rather than a bracket.
                    let is_csv_head = cell.symbol() == "C"
                        && x + 2 < buffer.area.width
                        && buffer[(x + 1, y)].symbol() == "S"
                        && buffer[(x + 2, y)].symbol() == "V";
                    if is_csv_head && badge_bg.is_none() {
                        badge_bg = Some(cell.bg);
                    }
                    rendered.push_str(cell.symbol());
                }
                rendered.push('\n');
            }

            assert!(rendered.contains(" CSV "), "{flavor:?} tree shows the csv badge: {rendered}");
            assert!(!rendered.contains("[CSV]"), "{flavor:?} badge is a highlight, not brackets: {rendered}");
            assert_eq!(
                badge_bg,
                Some(app.theme.link),
                "{flavor:?} badge keeps its highlight background (not reset by the flavor's `Color::Reset` bg): {rendered}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_reader_shows_edit_button_in_top_right_corner() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let count = INSP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("podarcis-edit-btn-{}-{count}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".podarcis")).unwrap();
        std::fs::create_dir_all(dir.join("wiki")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "").unwrap();
        std::fs::write(dir.join(".podarcis/config.yaml"), "").unwrap();
        let page = dir.join("wiki/doc.md");
        std::fs::write(
            &page,
            "---\ntitle: Document\ntype: concept\ncategory: c\nrationale: r\n---\nbody\n",
        )
        .unwrap();

        let (mut app, _rx) = crate::app::test_app(&dir);
        let mut terminal = Terminal::new(TestBackend::new(50, 10)).unwrap();

        // On empty document (splash), there is no edit button.
        terminal.draw(|f| document(f, &mut app, f.area())).unwrap();
        assert!(app.areas.doc_edit.is_none(), "splash screen has no edit button");

        // When a page is open in reader, edit button appears in top-right.
        app.open_path(&page, true);
        terminal.draw(|f| document(f, &mut app, f.area())).unwrap();
        assert!(app.areas.doc_edit.is_some(), "reader has an edit button");
        let edit_rect = app.areas.doc_edit.unwrap();
        assert_eq!(edit_rect.y, 0);
        assert_eq!(edit_rect.width, 3);
        assert_eq!(edit_rect.height, 1);
        assert_eq!(edit_rect.x, 50 - 4);

        let buffer = terminal.backend().buffer();
        let glyphs: String = (edit_rect.x..edit_rect.x + edit_rect.width)
            .map(|x| buffer[(x, edit_rect.y)].symbol().to_string())
            .collect();
        assert!(glyphs.contains('✎'), "edit button contains pencil symbol: {glyphs:?}");

        // In editor mode, doc_edit is None.
        app.run(crate::keymap::Cmd::Edit);
        terminal.draw(|f| document(f, &mut app, f.area())).unwrap();
        assert!(app.areas.doc_edit.is_none(), "editor has no edit button");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scrollbar_renders_half_block_thumb() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let dir = std::env::temp_dir().join(format!("podarcis-scrollbar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("wiki")).unwrap();
        let (mut app, _rx) = crate::app::test_app(&dir);
        let path = dir.join("wiki/long.md");
        let body = (0..100).map(|i| format!("Line {i}\n\n")).collect::<String>();
        std::fs::write(&path, format!("---\ntitle: Long\ntype: concept\ncategory: c\n---\n{body}")).unwrap();
        app.open_path(&path, false);

        let mut terminal = Terminal::new(TestBackend::new(40, 15)).unwrap();
        terminal.draw(|f| document(f, &mut app, f.area())).unwrap();
        let b = terminal.backend().buffer();
        // The right border of the doc pane is column 39. The thumb should render on column 39 as '▌'.
        let mut found_thumb = false;
        for y in 1..14 {
            let col_outer = b[(39, y)].symbol();
            if col_outer == "▌" {
                found_thumb = true;
                break;
            }
        }
        assert!(found_thumb, "the scrollbar thumb is rendered as '▌' on the border column");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tree_highlights_open_page_only_when_content_panel_is_focused() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let count = INSP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("podarcis-tree-highlight-{}-{count}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".podarcis")).unwrap();
        std::fs::create_dir_all(dir.join("wiki")).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "").unwrap();
        std::fs::write(dir.join(".podarcis/config.yaml"), "").unwrap();

        let page_a = dir.join("wiki/alpha.md");
        let page_b = dir.join("wiki/beta.md");
        std::fs::write(&page_a, "---\ntitle: Alpha\ntype: concept\ncategory: c\nrationale: r\n---\nalpha\n").unwrap();
        std::fs::write(&page_b, "---\ntitle: Beta\ntype: concept\ncategory: c\nrationale: r\n---\nbeta\n").unwrap();

        let (mut app, _rx) = crate::app::test_app(&dir);
        app.tree.expand(); // wiki
        app.tree.rebuild();

        // Open page_b in content window.
        app.open_path(&page_b, true);
        // Position tree.selected on page_a explicitly.
        let pos_a = app.tree.rows.iter().position(|r| r.path == page_a).unwrap();
        app.tree.selected = pos_a;

        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();

        // 1. Content panel is focused:
        app.focus = Focus::Doc;
        app.doc_stop = DocStop::Content;
        terminal.draw(|f| tree(f, &mut app, f.area())).unwrap();
        let buffer = terminal.backend().buffer().clone();

        let row_bg = |needle: &str| -> Option<Color> {
            for y in 0..buffer.area.height {
                let row_str: String = (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect();
                if let Some(pos) = row_str.find(needle) {
                    return Some(buffer[(pos as u16, y)].bg);
                }
            }
            None
        };

        // When content panel is focused, open page (beta) must have surface highlight background,
        // while alpha (selected in tree) must NOT be highlighted.
        assert_eq!(
            row_bg("beta"),
            Some(app.theme.surface),
            "open page is highlighted with surface background when content panel is focused"
        );
        assert_ne!(
            row_bg("alpha"),
            Some(app.theme.surface),
            "unfocused tree cursor row is not highlighted when content panel is focused"
        );

        // 2. Tree is focused:
        app.focus = Focus::Tree;
        terminal.draw(|f| tree(f, &mut app, f.area())).unwrap();
        let buffer2 = terminal.backend().buffer().clone();
        let row_bg2 = |needle: &str| -> Option<Color> {
            for y in 0..buffer2.area.height {
                let row_str: String = (0..buffer2.area.width).map(|x| buffer2[(x, y)].symbol()).collect();
                if let Some(pos) = row_str.find(needle) {
                    return Some(buffer2[(pos as u16, y)].bg);
                }
            }
            None
        };

        // Tree cursor (alpha) is now focused (accent background).
        // Open page (beta) is NOT highlighted.
        assert_eq!(
            row_bg2("alpha"),
            Some(app.theme.accent),
            "tree cursor is highlighted with accent background when tree is focused"
        );
        assert_ne!(
            row_bg2("beta"),
            Some(app.theme.surface),
            "open page is not highlighted when tree is focused"
        );
        assert_ne!(
            row_bg2("beta"),
            Some(app.theme.accent),
            "open page does not have accent background when tree is focused"
        );

        // 3. Sidebar is focused:
        app.focus = Focus::Sidebar;
        terminal.draw(|f| tree(f, &mut app, f.area())).unwrap();
        let buffer3 = terminal.backend().buffer().clone();
        let row_bg3 = |needle: &str| -> Option<Color> {
            for y in 0..buffer3.area.height {
                let row_str: String = (0..buffer3.area.width).map(|x| buffer3[(x, y)].symbol()).collect();
                if let Some(pos) = row_str.find(needle) {
                    return Some(buffer3[(pos as u16, y)].bg);
                }
            }
            None
        };

        // Open page (beta) is NOT highlighted when sidebar is focused.
        assert_ne!(
            row_bg3("beta"),
            Some(app.theme.surface),
            "open page is not highlighted when sidebar is focused"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
