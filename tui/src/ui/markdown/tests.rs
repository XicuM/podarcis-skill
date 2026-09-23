use super::*;
use super::table::{render_grid, MAX_GRID_COL};
use super::tex::tex_to_unicode;


    fn doc(body: &str, width: u16) -> Doc {
        render(body, width, &Theme::default())
    }

    fn plain(doc: &Doc) -> Vec<String> {
        doc.lines
            .iter()
            .map(|l| l.segs.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect()
    }

    /// A rendered view does not echo its own syntax: a heading shows its text
    /// and nothing else.
    #[test]
    fn headings_render_their_text_without_the_hash_markers() {
        let d = doc("# One\n\nbody\n\n## Two\n\n### Three\n\n#### Four\n", 40);
        let rows = plain(&d);
        for (level, text) in [(1, "One"), (2, "Two"), (3, "Three"), (4, "Four")] {
            assert!(
                rows.iter().any(|r| r.trim() == text),
                "h{level} renders as bare {text:?}: {rows:?}"
            );
        }
        for row in &rows {
            assert!(!row.contains('#'), "a hash leaked into the render: {row:?}");
        }
    }

    /// With the hash ruler gone, colour and weight are the only thing left to
    /// separate one level from another — so no two levels the corpus actually
    /// uses may share a style, and none may look like body prose.
    #[test]
    fn every_heading_level_is_styled_apart_from_the_others_and_from_body_text() {
        let theme = Theme::default();
        let d = render("# One\n\nbody text\n\n## Two\n\n### Three\n\n#### Four\n", 40, &theme);

        let style_of = |want: &str| {
            d.lines
                .iter()
                .find_map(|l| {
                    let text: String = l.segs.iter().map(|s| s.text.as_str()).collect();
                    (text.trim() == want).then(|| l.segs.iter().find(|s| !s.text.trim().is_empty()).unwrap().style)
                })
                .unwrap_or_else(|| panic!("{want:?} is not in the render"))
        };

        let body = style_of("body text");
        let levels = [style_of("One"), style_of("Two"), style_of("Three"), style_of("Four")];
        for (i, style) in levels.iter().enumerate() {
            assert_ne!(*style, body, "h{} is indistinguishable from body prose", i + 1);
            for (j, other) in levels.iter().enumerate().skip(i + 1) {
                assert_ne!(style, other, "h{} and h{} share a style", i + 1, j + 1);
            }
        }
    }

    #[test]
    fn wraps_at_the_given_width_without_gluing_words() {
        let d = doc("alpha beta gamma delta epsilon zeta", 20);
        for line in plain(&d) {
            assert!(line.width() <= 20, "{line:?} is wider than 20");
        }
        let joined = plain(&d).join(" ");
        assert!(joined.contains("alpha beta"));
        assert!(joined.contains("epsilon zeta"));
        assert!(!joined.contains("gammadelta"));
    }

    #[test]
    fn never_emits_a_line_wider_than_the_viewport() {
        let body = "# A very long heading that certainly does not fit\n\n- a list item that is also rather long indeed\n\n> a quoted passage that goes on and on and on\n\n| a | bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb | c |\n|---|---|---|\n| 1 | 2 | 3 |\n\n```rust\nlet extremely_long_identifier = some_function_with_a_long_name(argument);\n```\n\n1. https://example.com/an/absurdly/long/path/with/no/break/opportunity/at/all\n";
        for width in [20u16, 30, 47, 80] {
            for line in plain(&doc(body, width)) {
                assert!(line.width() <= width as usize, "width {width}: {line:?}");
            }
        }
    }

    #[test]
    fn a_citation_never_starts_a_line_on_its_own() {
        // The citation is its own styled segment; a break between the word and
        // its marker would strand `[1].` at the head of a line.
        let d = doc("adherence varies[^manfredini_2021] over time.\n", 22);
        for line in plain(&d) {
            assert!(!line.trim_start().starts_with('['), "{:?}", plain(&d));
        }
        assert!(plain(&d).iter().any(|l| l.contains("varies[1]")), "{:?}", plain(&d));
    }

    #[test]
    fn a_word_wider_than_the_line_is_broken_rather_than_left_to_overflow() {
        let url = "https://example.com/a/really/long/path/that/nobody/should/have/written";
        let out = plain(&doc(&format!("See {url} here.\n"), 30));
        assert!(out.iter().all(|l| l.width() <= 30), "{out:?}");
        // Broken, not dropped: every character survives somewhere.
        assert!(out.join("").contains("nobody/should/have/written"), "{out:?}");
    }

    #[test]
    fn a_gfm_alert_colours_its_bar_and_keeps_its_title_attached() {
        let theme = Theme::default();
        let d = render("> [!WARNING]\n> Do not do this.\n", 60, &theme);
        let out = plain(&d);
        // Line 0 is the doc's leading blank; the quote follows it.
        assert!(out[1].contains("Warning"), "{out:?}");
        assert!(out[2].contains("Do not do this."), "the title is not orphaned: {out:?}");
        let bars: Vec<&Seg> = d.lines.iter().filter_map(|l| l.segs.first()).collect();
        assert!(bars.iter().all(|s| s.style.fg == Some(theme.warn)), "the bar takes the tone");
    }

    #[test]
    fn a_leading_emoji_tones_a_plain_quote() {
        let theme = Theme::default();
        let warn = render("> \u{26a0}\u{fe0f} single source\n", 60, &theme);
        assert_eq!(warn.lines[1].segs[0].style.fg, Some(theme.warn));
        let plain_quote = render("> just a quote\n", 60, &theme);
        assert_eq!(plain_quote.lines[1].segs[0].style.fg, Some(theme.accent));
    }

    #[test]
    fn a_table_cell_keeps_its_links_and_its_alignment() {
        let d = doc("| a | b |\n|:--|--:|\n| [Doc](../x.md) | 7 |\n", 40);
        assert_eq!(d.links.len(), 1, "a link in a cell is still a link");
        assert_eq!(d.links[0].target, "../x.md");
        let line = &d.lines[d.links[0].line];
        assert!(line.segs.iter().any(|s| s.link == Some(0) && s.text.contains("Doc")), "{line:?}");
        // Right-aligned, so the padding precedes the value.
        let row = line.plain_text();
        assert!(row.trim_end().ends_with("7 │"), "{row:?}");
    }

    #[test]
    fn a_link_records_the_line_it_is_drawn_on() {
        // Not the line its paragraph started on: `gg`-style jumps scroll to
        // the recorded line, and a long paragraph would land pages away.
        let d = doc("one two three four five [target](a.md)\n", 12);
        let line = d.links[0].line;
        assert!(d.lines[line].plain_text().contains("target"), "{:?}", plain(&d));
    }

    #[test]
    fn code_is_highlighted_against_a_full_width_band() {
        let theme = Theme::default();
        let d = render("```rust\nlet x = 1;\n```\n", 40, &theme);
        let band: Vec<&DocLine> =
            d.lines.iter().filter(|l| l.plain_text().contains('▎')).collect();
        assert_eq!(band.len(), 1, "the one line without chip header");
        for line in &band {
            assert_eq!(line.plain_text().width(), 40, "the band is the full width");
            assert!(line.segs.iter().all(|s| s.style.bg == Some(theme.surface)));
        }
        let keyword = band[0].segs.iter().find(|s| s.text == "let").expect("a keyword run");
        assert_eq!(keyword.style.fg, Some(theme.accent));
    }

    #[test]
    fn an_unknown_fence_language_is_banded_but_not_coloured() {
        let theme = Theme::default();
        let d = render("```brainfuck\nlet x = 1;\n```\n", 40, &theme);
        let code = d.lines.iter().find(|l| l.plain_text().contains("let x")).unwrap();
        assert!(code.segs.iter().all(|s| s.style.bg == Some(theme.surface)));
        assert!(code.segs.iter().all(|s| s.style.fg != Some(theme.accent) || s.text == "▎"));
    }

    #[test]
    fn an_image_shows_its_alt_text_and_falls_back_to_its_destination() {
        assert!(plain(&doc("![a diagram](x.png)\n", 60)).join("").contains("🖼 a diagram"));
        assert!(plain(&doc("![](x.png)\n", 60)).join("").contains("🖼 x.png"));
    }

    #[test]
    fn records_links_with_their_kind() {
        let d = doc("See [A](../a.md), [B](https://x.dev) and [C](#s).\n", 80);
        assert_eq!(d.links.len(), 3);
        assert_eq!(d.links[0].target, "../a.md");
        assert_eq!(d.links[0].kind, LinkKind::Relative);
        assert_eq!(d.links[1].kind, LinkKind::External);
        assert_eq!(d.links[2].kind, LinkKind::Anchor);
        assert!(d.lines.iter().any(|l| l.segs.iter().any(|s| s.link == Some(0))));
    }

    #[test]
    fn link_cursor_wraps_in_both_directions() {
        let d = doc("[A](a.md) [B](b.md)\n", 80);
        assert_eq!(d.link_after(None, true), Some(0));
        assert_eq!(d.link_after(Some(1), true), Some(0));
        assert_eq!(d.link_after(Some(0), false), Some(1));
        assert_eq!(doc("no links", 80).link_after(None, true), None);
    }

    #[test]
    fn short_items_stay_tight_and_only_a_multi_line_one_earns_a_gap() {
        // "two" runs to three lines with its sub-list, so the eye needs the
        // break after it; "one" does not.
        let out = plain(&doc("- one\n- two\n  - deep\n  - deeper\n- three\n", 40));
        assert_eq!(
            out,
            vec!["", "• one", "• two", "  ◦ deep", "  ◦ deeper", "", "• three", ""],
            "{out:?}"
        );
    }

    #[test]
    fn a_list_of_one_line_entries_gets_no_gaps_at_all() {
        let out = plain(&doc("- alpha\n- beta\n- gamma\n", 40));
        assert_eq!(out, vec!["", "• alpha", "• beta", "• gamma", ""], "{out:?}");
    }

    #[test]
    fn a_wrapped_item_is_not_split_by_the_gap() {
        let out = plain(&doc("- one that is long enough to wrap twice over\n- two\n", 20));
        // The leading blank, trailing blank, plus the single inter-item gap — never one inside an item.
        let blanks = out.iter().filter(|l| l.is_empty()).count();
        assert_eq!(blanks, 3, "one doc lead, one trailing, one between the items: {out:?}");
        assert_eq!(out[out.len() - 2], "• two");
        assert_eq!(out.last().unwrap(), "");
    }

    #[test]
    fn doc_ends_with_an_empty_line_for_aesthetics() {
        for body in [
            "Just prose.",
            "# Heading only",
            "- List item",
            "```rust\ncode\n```",
            "> Quote",
        ] {
            let out = plain(&doc(body, 40));
            assert_eq!(out.last().unwrap(), "", "must end with blank line for: {body:?}");
            assert_ne!(out[out.len() - 2], "", "must not have duplicate trailing blank line for: {body:?}");
        }
    }

    #[test]
    fn bullets_and_numbers_are_rendered_and_nested() {
        let out = plain(&doc("- one\n- two\n  - deep\n\n1. first\n2. second\n", 80));
        let joined = out.join("\n");
        assert!(joined.contains("• one"), "{joined}");
        assert!(joined.contains("  ◦ deep"), "the depth is in the glyph: {joined}");
        assert!(joined.contains("1. first"), "{joined}");
        assert!(joined.contains("2. second"), "{joined}");
    }

    #[test]
    fn blockquotes_get_a_bar() {
        let out = plain(&doc("> \u{26a0}\u{fe0f} competing hypothesis\n", 80));
        assert!(out.iter().any(|l| l.starts_with('▌')), "{out:?}");
    }

    #[test]
    fn a_footnote_definition_does_not_turn_its_references_into_links() {
        // The failure this guards against is subtle: CommonMark reads
        // `[^id]: …` as a link reference definition, and then renders every
        // `[^id]` as bare `^id`.
        let d = doc(
            "Claim[^burke_2023].\n\n[^burke_2023]: [nutrition/x/raw.md](../x/raw.md)\n",
            90,
        );
        let text = plain(&d).join("\n");
        assert_eq!(d.citations, vec!["burke_2023"]);
        assert!(text.contains("[1]"), "{text:?}");
        assert!(!text.contains("burke_2023"), "the label is not read mid-sentence: {text:?}");
        // The citation itself is a `Footnote`-kind `DocLink` (so it can be
        // clicked); the definition block's `[nutrition/x/raw.md](../x/raw.md)`
        // must not additionally turn into a real, followable link.
        assert_eq!(d.links.len(), 1, "a footnote definition is not a link: {:?}", d.links);
        assert_eq!(d.links[0].kind, LinkKind::Footnote);
        assert_eq!(d.links[0].target, "burke_2023");
        assert!(!text.contains("raw.md"), "the definition block is not prose");
    }

    #[test]
    fn blanking_definitions_keeps_the_line_count() {
        let body = "one\n[^a]: x\ntwo\n";
        assert_eq!(neutralize_footnote_definitions(body).lines().count(), 3);
        assert_eq!(neutralize_footnote_definitions(body), "one\n\ntwo\n");
    }

    #[test]
    fn only_real_definitions_are_blanked() {
        assert!(is_footnote_definition("[^a]: text"));
        assert!(is_footnote_definition("  [^a]: text"));
        assert!(!is_footnote_definition("[^a] not a definition"));
        assert!(!is_footnote_definition("[^]: empty label"));
        assert!(!is_footnote_definition("plain prose"));
    }

    #[test]
    fn footnote_references_are_numbered_in_order_of_citation() {
        let d = doc("Body[^smith2024] and[^jones2020] and again[^smith2024].\n", 80);
        let out = plain(&d).join(" ");
        assert!(out.contains("Body[1]"), "{out}");
        assert!(out.contains("and[2]"), "{out}");
        assert!(out.contains("again[1]"), "a label keeps its number: {out}");
        assert_eq!(d.citations, vec!["smith2024", "jones2020"]);
        assert_eq!(d.citation_number("jones2020"), Some(2));
        assert_eq!(d.citation_number("nobody"), None);
    }

    #[test]
    fn selecting_a_source_highlights_the_corresponding_mark_in_the_body() {
        let d = doc("Body[^smith2024] and[^jones2020].\n", 80);
        let link_idx = d.citation_link("smith2024");
        assert_eq!(link_idx, Some(0), "the first footnote is index 0");
        let theme = Theme::default();
        let lines = d.to_lines(&theme, &Overlays { link: link_idx, ..Overlays::default() }, 0, 40);
        let active = lines.iter().flat_map(|l| &l.spans).any(|s| {
            s.content.contains("[1]")
                && s.style.bg == Some(theme.link)
                && s.style.add_modifier.contains(Modifier::BOLD)
        });
        assert!(active, "[1] must carry the active-link highlight");
    }

    #[test]
    fn tables_render_aligned_with_a_rule() {
        let out = plain(&doc("| a | bbbb |\n|---|------|\n| 1 | 2 |\n", 80));
        let joined = out.join("\n");
        assert!(joined.contains('│'), "{joined}");
        assert!(joined.contains('┼'), "{joined}");
    }

    #[test]
    fn the_table_header_row_is_not_dropped() {
        // pulldown puts the header's cells directly inside TableHead, with no
        // TableRow around them; forgetting that loses the header and promotes
        // the first body row in its place.
        let out = plain(&doc("| Modality | Evidence |\n|---|---|\n| Splints | Strong |\n", 80));
        assert!(out[0].is_empty(), "the doc leads with a blank row: {out:?}");
        assert!(out[1].starts_with('┌'), "the box is closed: {out:?}");
        assert!(out[2].contains("Modality") && out[2].contains("Evidence"), "{out:?}");
        assert!(out[3].contains('┼'), "the rule sits under the header: {out:?}");
        assert!(out[4].contains("Splints"), "{out:?}");
        assert!(out[5].starts_with('└'), "{out:?}");
    }

    #[test]
    fn a_grid_renders_like_a_table_with_per_row_source_mapping() {
        let grid = vec![
            vec!["symbol".to_string(), "price".to_string()],
            vec!["AAPL".to_string(), "232.1".to_string()],
            vec!["MSFT".to_string(), "417.4".to_string()],
        ];
        let d = render_grid(&grid, 80, &Theme::default());
        let out = plain(&d);
        assert!(out[1].starts_with('┌'), "{out:?}");
        assert!(out[2].contains("symbol") && out[2].contains("price"), "{out:?}");
        assert!(out[4].contains("AAPL") && out[5].contains("MSFT"), "{out:?}");

        // Each content line maps back to the grid row it draws, so the editor
        // cursor lands on the same raw CSV line it was reading.
        let header_line = out.iter().position(|l| l.contains("symbol")).unwrap();
        let msft_line = out.iter().position(|l| l.contains("MSFT")).unwrap();
        assert_eq!(d.lines[header_line].src_line, 0);
        assert_eq!(d.lines[msft_line].src_line, 2);
    }

    #[test]
    fn a_wide_grid_keeps_its_columns_and_reports_where_they_end() {
        let cols: Vec<String> = (0..12).map(|i| format!("col_{i}")).collect();
        let rows = vec![cols.clone(), (0..12).map(|i| format!("v{i:02}")).collect()];
        let d = render_grid(&rows, 80, &Theme::default());
        // Twelve columns cannot be squeezed into eighty cells without turning
        // every cell into an ellipsis, so the box is laid out wide and panned.
        assert!(d.width > 80, "the box keeps its natural width: {}", d.width);
        assert!(plain(&d).iter().any(|l| l.contains("col_11")), "every column is drawn");
        assert_eq!(d.col_stops.len(), 12);
        assert_eq!(*d.col_stops.last().unwrap(), d.width - 1, "the last column ends at the frame");

        // With eighty columns in view from the left edge, the tail is hidden
        // and nothing is off to the left yet.
        let (left, right) = d.hidden_columns(0, 80);
        assert_eq!(left, 0);
        assert!(right > 0, "the pane hides the tail columns");
        assert_eq!(d.hidden_columns(d.width, 80), (12, 0), "panned past the end, all are behind you");
    }

    #[test]
    fn a_pan_drops_columns_off_the_left_and_keeps_the_rest_aligned() {
        let theme = Theme::default();
        let rows = vec![
            vec!["alpha".to_string(), "beta".to_string()],
            vec!["one".to_string(), "two".to_string()],
        ];
        let d = render_grid(&rows, 80, &theme);
        let at = |hscroll: usize| -> Vec<String> {
            d.to_lines(&theme, &Overlays { hscroll, ..Overlays::default() }, 0, d.height())
                .iter()
                .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
                .collect()
        };
        let panned = at(4);
        let flush = at(0);
        for (a, b) in panned.iter().zip(&flush).filter(|(_, b)| !b.is_empty()) {
            assert_eq!(a.width() + 4, b.width(), "every drawn line loses exactly four columns: {b:?}");
        }
        assert!(panned.iter().any(|l| l.contains("beta")), "{panned:?}");
        assert!(!panned.iter().any(|l| l.contains("alpha")), "the first column is behind the edge");
    }

    #[test]
    fn a_pan_through_a_wide_glyph_keeps_the_columns_lined_up() {
        let theme = Theme::default();
        // The cut lands in the middle of a two-cell glyph, which cannot be half
        // drawn: it is replaced by a space so the rest stays on its column.
        let rows = vec![vec!["日本".to_string(), "x".to_string()]];
        let d = render_grid(&rows, 80, &theme);
        let line = |hscroll: usize| -> String {
            d.to_lines(&theme, &Overlays { hscroll, ..Overlays::default() }, 2, 1)[0]
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect()
        };
        assert_eq!(line(0).width(), line(3).width() + 3);
        assert!(line(3).starts_with(' '), "the split glyph becomes padding: {:?}", line(3));
    }

    #[test]
    fn a_grid_keeps_one_line_per_record_and_truncates_what_overflows() {
        let rows = vec![
            vec!["date".to_string(), "description".to_string()],
            vec!["2026-01-03".to_string(), "groceries at the market stall by the station".to_string()],
            vec!["2026-01-04".to_string(), "rent".to_string()],
        ];
        let out = plain(&render_grid(&rows, 76, &Theme::default()));
        let records: Vec<&String> = out.iter().filter(|l| l.contains("2026-01-")).collect();
        assert_eq!(records.len(), 2, "one line per record, however long a cell is: {out:?}");
        assert!(records[0].contains('…'), "the long cell is cut, not wrapped: {:?}", records[0]);
        assert!(records[0].contains("groceries at the market"), "{:?}", records[0]);
    }

    #[test]
    fn a_grid_header_still_wraps_because_a_column_must_be_nameable() {
        let rows = vec![
            vec!["a very long column heading indeed".to_string(), "b".to_string()],
            vec!["1".to_string(), "2".to_string()],
        ];
        let out = plain(&render_grid(&rows, 28, &Theme::default()));
        let head = out.iter().filter(|l| l.contains("column") || l.contains("heading")).count();
        assert!(head > 1, "the heading wraps rather than losing its tail: {out:?}");
        assert!(!out.iter().any(|l| l.contains("head…")), "{out:?}");
    }

    #[test]
    fn a_grid_column_stops_growing_at_the_cap() {
        let rows = vec![
            vec!["note".to_string()],
            vec!["z".repeat(500)],
        ];
        let d = render_grid(&rows, 400, &Theme::default());
        // Without a cap the column would be 500 wide and the pan would cross
        // hundreds of columns of one cell.
        assert_eq!(d.width, MAX_GRID_COL + 4, "one column, its frame, and nothing more");
    }

    #[test]
    fn a_narrow_pane_pans_a_grid_rather_than_squeezing_it_to_nothing() {
        let head: Vec<String> = ["date", "category", "amount", "balance", "account", "tags"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let row: Vec<String> = ["2026-01-03", "food", "-42.10", "1932.44", "current", "weekly"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let d = render_grid(&[head, row], 56, &Theme::default());
        let out = plain(&d);
        let record = out.iter().find(|l| l.contains("2026")).expect("the record is drawn");
        assert!(record.contains("2026-01-03"), "a date is not worth showing in part: {record:?}");
        assert!(record.contains("1932.44"), "nor a balance: {record:?}");
        assert!(d.width > 56, "the table outgrows the pane and is panned instead");
        // The prose floor would have squeezed every column toward 56/8 = 7
        // cells, leaving an ellipsis with a hint of date in front of it. The
        // grid floor stops well short of that, so nothing here is cut at all.
        assert!(!record.contains('…'), "every field fits its own column: {record:?}");
    }

    #[test]
    fn a_runaway_cell_costs_the_grid_nothing() {
        // Everything after a stray quote in a CSV lands in a single cell. It
        // used to be copied, measured and wrapped in full on every frame.
        let rows = vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["x".repeat(2_000_000), "y".to_string()],
        ];
        let start = std::time::Instant::now();
        let d = render_grid(&rows, 76, &Theme::default());
        let elapsed = start.elapsed();
        assert_eq!(d.height(), 7, "one record, not forty thousand wrapped lines");
        assert!(elapsed < std::time::Duration::from_millis(500), "{elapsed:?}");
    }

    #[test]
    fn a_long_unbroken_token_wraps_in_linear_time() {
        // The old `wrap` re-collected the remaining text on every line, so this
        // took tens of seconds; the bound is loose enough to be about the shape
        // of the algorithm rather than about this machine.
        let segs = vec![Seg { text: "x".repeat(400_000), style: Style::default(), link: None }];
        let start = std::time::Instant::now();
        let lines = wrap(segs, 49);
        let elapsed = start.elapsed();
        assert_eq!(lines.len(), 8164);
        assert!(elapsed < std::time::Duration::from_secs(2), "{elapsed:?}");
    }

    #[test]
    fn a_quoted_newline_stays_on_its_record() {
        let rows = vec![
            vec!["note".to_string(), "amt".to_string()],
            vec!["refund for the thing\nthat broke".to_string(), "12".to_string()],
        ];
        let out = plain(&render_grid(&rows, 76, &Theme::default()));
        let record = out.iter().find(|l| l.contains("refund")).expect("the record is drawn");
        assert!(record.contains("thing that broke"), "the break becomes a space: {record:?}");
        assert_eq!(out.iter().filter(|l| l.contains("broke")).count(), 1);
    }

    #[test]
    fn a_markdown_table_still_wraps_its_prose() {
        let table = "| when | what |\n| --- | --- |\n| today | a sentence long enough to need more than one line of a narrow pane |\n";
        let out = plain(&doc(table, 46));
        assert!(!out.iter().any(|l| l.contains('…')), "prose wraps rather than truncating: {out:?}");
        let body = out.iter().filter(|l| l.contains('│') && !l.contains("what")).count();
        assert_eq!(body, 3, "the one record spans three lines: {out:?}");
    }

    #[test]
    fn a_mermaid_block_is_drawn_not_printed() {
        let out = plain(&doc("```mermaid\nflowchart TD\n  A[Root] --> B[Leaf]\n```\n", 80)).join("\n");
        assert!(out.contains("Root") && out.contains("Leaf"));
        assert!(out.contains('▼'), "arrows, not source: {out}");
        assert!(!out.contains("flowchart TD"), "the source is not shown: {out}");
        assert!(!out.contains("-->"), "{out}");
    }

    #[test]
    fn an_undrawable_mermaid_block_falls_back_to_its_source() {
        let out = plain(&doc("```mermaid\nsequenceDiagram\n  A->>B: hi\n```\n", 80)).join("\n");
        assert!(out.contains("sequenceDiagram"), "{out}");
        assert!(out.contains("A->>B: hi"), "{out}");
    }

    #[test]
    fn a_mermaid_diagram_respects_the_pane_width() {
        let code = "```mermaid\nflowchart TD\n  A[Root] --> B[A child with a fairly long label]\n  A --> C[Another child with a long label]\n```\n";
        for width in [24u16, 40, 80] {
            for line in plain(&doc(code, width)) {
                assert!(line.width() <= width as usize, "width {width}: {line:?}");
            }
        }
    }

    #[test]
    fn an_xychart_mermaid_block_is_drawn_in_markdown() {
        let md = "```mermaid xychart-beta\n    title \"Area Overhead (% of SoC)\"\n    x-axis [\"Mix-GEMM\", \"SMX-1D\", \"TMU\", \"GMX\", \"CVA6 FPU\"]\n    y-axis \"% of SoC\" 0 --> 35\n    bar [1, 1.37, 1.52, 1.7, 9, 29.66]\n```\n";
        let out = plain(&doc(md, 80)).join("\n");
        assert!(out.contains("Area Overhead"), "{out}");
        assert!(out.contains("Mix-GEMM") && out.contains("TMU"), "{out}");
        assert!(!out.contains("```"), "fences stripped: {out}");
    }

    #[test]
    fn code_blocks_keep_their_lines_verbatim() {
        let out = plain(&doc("```python\nx = 1\ny = 2\n```\n", 80));
        assert!(out.iter().any(|l| l.contains("x = 1")), "{out:?}");
        assert!(out.iter().any(|l| l.contains("y = 2")), "{out:?}");
        // Line numbers are shown in the gutter with separator
        assert!(out.iter().any(|l| l.contains("1 │ x = 1")), "{out:?}");
        assert!(out.iter().any(|l| l.contains("2 │ y = 2")), "{out:?}");
        // Every band row is the same width, gutter included, or the slab has a
        // ragged edge.
        let band: Vec<usize> = out.iter().filter(|l| l.contains('▎')).map(|l| l.width()).collect();
        assert_eq!(band.len(), 2, "two lines: {out:?}");
        assert!(band.windows(2).all(|w| w[0] == w[1]), "{band:?}");
    }

    #[test]
    fn code_blocks_number_lines_and_omit_header_chip() {
        let out = plain(&doc("```python\nx = 1\ny = 2\nz = 3\n```\n", 80));
        // No header chip displaying language or line count
        assert!(!out.iter().any(|l| l.contains("python") || l.contains("lines")), "{out:?}");
        // Line numbers are shown with │ divider
        assert!(out.iter().any(|l| l.contains("1 │ x = 1")), "{out:?}");
        assert!(out.iter().any(|l| l.contains("2 │ y = 2")), "{out:?}");
        assert!(out.iter().any(|l| l.contains("3 │ z = 3")), "{out:?}");
    }

    #[test]
    fn code_blocks_highlight_when_language_is_omitted() {
        let theme = Theme::default();
        let d = render("```\nfn add(x: i32) -> i32 {\n  return x + 1;\n}\n```\n", 80, &theme);
        // "fn" and "return" should be highlighted as keywords even with no fence language
        let has_keyword = d.lines.iter().any(|l| {
            l.segs.iter().any(|s| (s.text == "fn" || s.text == "return") && s.style.fg == Some(theme.accent))
        });
        assert!(has_keyword, "keywords must be highlighted when language is omitted");
    }

    #[test]
    fn code_blocks_wrap_lines_without_truncation() {
        let long_code = "```python\na_very_long_expression_that_will_definitely_exceed_narrow_viewport = 42\n```\n";
        let d = doc(long_code, 30);
        let out = plain(&d);

        // Does not truncate with ellipsis
        assert!(!out.iter().any(|l| l.contains('…')), "must not truncate with ...: {out:?}");

        // All wrapped content survives (stripped of gutter and band padding)
        let content: String = out
            .iter()
            .map(|l| {
                if let Some((_, code_part)) = l.split_once('│') {
                    code_part.trim()
                } else {
                    l.trim()
                }
            })
            .collect();
        let normalized = content.replace(' ', "");
        assert!(normalized.contains("a_very_long_expression_that_will_definitely_exceed_narrow_viewport=42"),
            "got content: {content:?}");

        // Multiple band lines are produced for the wrapped line
        let band: Vec<&DocLine> = d.lines.iter().filter(|l| l.plain_text().contains('▎')).collect();
        assert!(band.len() >= 2, "at least 2 wrapped lines: {out:?}");
        for line in &band {
            assert_eq!(line.plain_text().width(), 30, "every band line maintains full width");
            assert_eq!(line.src_line, 1);
        }
    }

    #[test]
    fn headings_are_collected_for_the_outline() {
        let d = doc("# One\n\ntext\n\n## Two\n\n#### Deep\n", 80);
        let titles: Vec<&str> = d.headings.iter().map(|(_, _, t)| t.as_str()).collect();
        assert_eq!(titles, vec!["One", "Two", "Deep"]);
        // The level is what lets the outline show the shape of the page.
        let levels: Vec<u8> = d.headings.iter().map(|(_, l, _)| *l).collect();
        assert_eq!(levels, vec![1, 2, 4]);
    }

    #[test]
    fn a_code_block_maps_each_line_back_to_its_own_source_line() {
        // Line 0 is the paragraph, 2 the fence, 3 and 4 the code.
        let d = doc("intro\n\n```python\nx = 1\ny = 2\n```\n", 40);
        let at = |needle: &str| {
            d.lines.iter().position(|l| l.plain_text().contains(needle)).unwrap()
        };
        assert_eq!(d.source_for_line(at("x = 1")), 3);
        assert_eq!(d.source_for_line(at("y = 2")), 4);
    }

    #[test]
    fn a_citation_inside_a_table_cell_is_numbered_too() {
        let d = doc("a[^first]\n\n| x |\n|---|\n| see[^second] |\n", 40);
        assert_eq!(d.citations, vec!["first", "second"]);
        assert!(plain(&d).iter().any(|l| l.contains("see[2]")), "{:?}", plain(&d));
    }

    #[test]
    fn source_lines_are_monotonic_and_map_back() {
        let d = doc("# One\n\npara one\n\n## Two\n\npara two\n", 80);
        let srcs: Vec<usize> = d.lines.iter().map(|l| l.src_line).collect();
        assert!(srcs.windows(2).all(|w| w[0] <= w[1]), "{srcs:?}");
        let line = d.line_for_source(6);
        assert!(d.source_for_line(line) >= 4);
    }

    #[test]
    fn consecutive_footnotes_are_all_styled() {
        let d = doc("Claim[^a][^b] end.\n", 80);
        let cited: Vec<&str> = d
            .lines
            .iter()
            .flat_map(|l| l.segs.iter())
            .filter(|s| s.style.add_modifier.contains(Modifier::DIM))
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(cited, vec!["[1]", "[2]"]);
    }

    #[test]
    fn a_lone_bracket_caret_is_left_alone() {
        let out = plain(&doc("array[^ ] and [^] here\n", 80)).join(" ");
        assert!(out.contains("[^"), "{out}");
    }

    #[test]
    fn empty_input_renders_nothing_and_does_not_panic() {
        let empty = doc("", 80);
        assert_eq!(empty.lines.len(), 1, "just the leading blank");
        assert!(doc("", 80).lines.iter().all(|l| l.segs.is_empty()));
        assert!(doc("\n\n\n", 80).lines.iter().all(|l| l.segs.is_empty()));
    }

    #[test]
    fn a_very_narrow_viewport_still_produces_output() {
        let d = doc("some prose that must fit somewhere", 1);
        assert!(!d.lines.is_empty());
    }

    #[test]
    fn text_selection_extracts_substring_and_multiline() {
        let d = doc("First line of text\n\nSecond line of text\n", 80);
        // Line 0 is the doc's leading blank, so the prose starts at line 1.
        let sel1 = Selection::new(TextPos { line: 1, col: 6 }, TextPos { line: 1, col: 10 });
        assert_eq!(d.selected_text(sel1), "line");

        // Select backward (anchor after cursor)
        let sel1_back = Selection::new(TextPos { line: 1, col: 10 }, TextPos { line: 1, col: 6 });
        assert_eq!(d.selected_text(sel1_back), "line");

        // Multiline selection
        let sel2 = Selection::new(TextPos { line: 1, col: 14 }, TextPos { line: 3, col: 6 });
        let text = d.selected_text(sel2);
        assert_eq!(text, "text\n\nSecond");
    }

    #[test]
    fn to_lines_highlights_selected_range() {
        let theme = Theme::default();
        let d = doc("Alpha **Beta** Gamma", 80);
        let sel = Selection::new(TextPos { line: 1, col: 3 }, TextPos { line: 1, col: 10 });
        let lines = d.to_lines(&theme, &Overlays { selection: Some(sel), ..Overlays::default() }, 1, 1);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        // Check that some spans have the selection style (accent background)
        let has_sel_span = line.spans.iter().any(|s| s.style.bg == Some(theme.accent));
        assert!(has_sel_span, "selected range must have theme.accent background");
    }

    #[test]
    fn find_matches_case_insensitively_and_never_across_a_wrap() {
        let d = doc("Alpha alpha ALPHA", 80);
        let hits = d.find("alpha");
        assert_eq!(hits.len(), 3);
        assert!(hits.windows(2).all(|w| w[0].line == w[1].line), "one rendered line");
        assert_eq!(hits[0].end - hits[0].start, 5);
        assert!(d.find("").is_empty(), "an empty query matches nothing, not everything");
        // The wrap is where the reader broke the prose, so a phrase spanning it
        // has no single span to tint.
        let wrapped = doc("alpha beta gamma delta", 12);
        assert!(wrapped.find("gamma delta").is_empty());
    }

    #[test]
    fn to_lines_tints_the_current_find_hit_apart_from_the_others() {
        let theme = Theme::default();
        let d = doc("alpha beta alpha", 80);
        let hits = d.find("alpha");
        assert_eq!(hits.len(), 2);
        let lines = d.to_lines(&theme, &Overlays { finds: &hits, current_find: 1, ..Overlays::default() }, 1, 1);
        let spans = &lines[0].spans;
        let current: Vec<&str> =
            spans.iter().filter(|s| s.style.bg == Some(theme.warn)).map(|s| s.content.as_ref()).collect();
        assert_eq!(current, ["alpha"], "only the hit you are on gets a background");
        assert!(
            spans.iter().any(|s| s.style.bg.is_none()
                && s.style.add_modifier.contains(Modifier::UNDERLINED)
                && s.content.as_ref() == "alpha"),
            "the other hit is underlined instead"
        );
    }

    #[test]
    fn plain_text_extracts_entire_rendered_body() {
        let d = doc("First\n\nSecond", 80);
        assert!(d.plain_text().contains("First"));
        assert!(d.plain_text().contains("Second"));
    }

    #[test]
    fn inline_math_renders_known_commands_and_hides_the_delimiters() {
        let d = doc("## 1. The Climax $\\rightarrow$ Calm Pill\n", 80);
        let out = plain(&d).join("\n");
        assert!(out.contains("The Climax → Calm Pill"), "{out}");
        assert!(!out.contains('$'), "delimiters are syntax: {out}");
        assert!(!out.contains("\\rightarrow"), "{out}");
        assert!(!out.contains('\\'), "{out}");
    }

    #[test]
    fn inline_math_is_styled_like_math_not_body_text() {
        let theme = Theme::default();
        let d = render("eat $\\alpha\\beta$ more\n", 80, &theme);
        let span = d
            .lines
            .iter()
            .flat_map(|l| l.segs.iter())
            .find(|s| s.text == "αβ")
            .expect("the greek makes one run");
        assert_eq!(span.style.fg, Some(theme.literal));
        assert!(span.style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn math_translates_commands_lists_fractions_and_fonts() {
        assert_eq!(tex_to_unicode(r"\frac{a}{b}"), "a/b");
        assert_eq!(tex_to_unicode(r"x^{\frac{1}{2}}"), "x^1/2");
        assert_eq!(tex_to_unicode(r"\text{Climax} \to \mathbb{R}"), "Climax → R");
        assert_eq!(tex_to_unicode(r"\left(x\right)^{2} \leq \alpha"), "(x)^2 ≤ α");
    }

    #[test]
    fn unknown_math_keeps_its_source_verbatim() {
        let out = plain(&doc("a $\\frobnicate{y}$ z\n", 80)).join(" ");
        assert!(out.contains("\\frobnicate"), "{out}");
    }

    #[test]
    fn a_priced_dollar_is_not_treated_as_math() {
        // `$` only opens a span when followed by non-whitespace and needs a
        // closing `$` that is not preceded by whitespace, so currency survives.
        let out = plain(&doc("It costs $40 to $50 on a good day.\n", 80)).join(" ");
        assert!(out.contains("$40 to $50"), "{out}");
        assert_eq!(plain(&doc("It costs $5.\n", 80)).join(" ").trim(), "It costs $5.");
    }

    #[test]
    fn display_math_is_a_centered_block() {
        let d = doc("before\n\n$$\\frac{a}{b} \\rightarrow c$$\n\nafter\n", 60);
        let out = plain(&d);
        let line = out.iter().find(|l| l.trim() == "a/b → c").expect("the formula renders");
        let lead = line.width() - line.trim().width();
        assert_eq!(lead, (60 - line.trim().width()) / 2, "centered in the pane: {out:?}");
        assert!(out.iter().any(|l| l.trim() == "before"), "prose before: {out:?}");
        assert!(out.iter().any(|l| l.trim() == "after"), "prose after: {out:?}");
    }

    #[test]
    fn over_wide_display_math_is_cut_not_broken() {
        for width in [20u16, 40] {
            let d = doc("$$\\sum_{i=1}^{n} i = \\frac{n(n+1)}{2} \\text{ really quite long}$$\n", width);
            for line in plain(&d) {
                assert!(line.width() <= width as usize, "width {width}: {line:?}");
            }
        }
    }
