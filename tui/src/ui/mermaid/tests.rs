use super::*;


    fn text(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|row| row.iter().map(|(t, _)| t.as_str()).collect()).collect()
    }

    const TREE: &str = "flowchart TD\n    A[Root] --> B[Left]\n    A --> C[Right]\n";

    #[test]
    fn an_unsupported_diagram_declines_rather_than_guessing() {
        assert!(render("sequenceDiagram\n  A->>B: hi\n", 80).is_none());
        assert!(render("", 80).is_none());
        assert!(render("pie title X\n  \"a\": 1\n", 80).is_none());
    }

    #[test]
    fn a_flowchart_is_drawn_with_boxes_and_arrows() {
        let rows = render(TREE, 80).unwrap();
        let out = text(&rows).join("\n");
        assert!(out.contains("Root"), "{out}");
        assert!(out.contains("Left") && out.contains("Right"));
        assert!(out.contains('┌') && out.contains('└'), "{out}");
        assert!(out.contains('▼'), "{out}");
    }

    #[test]
    fn both_flowchart_and_graph_headers_are_accepted() {
        assert!(render("graph LR\n  A[x] --> B[y]\n", 80).is_some());
        assert!(render("flowchart TD\n  A[x] --> B[y]\n", 80).is_some());
    }

    #[test]
    fn nothing_is_drawn_wider_than_the_pane() {
        for width in [20usize, 40, 80, 120] {
            for row in render(TREE, width).unwrap() {
                let line: String = row.iter().map(|(t, _)| t.as_str()).collect();
                assert!(line.width() <= width, "width {width}: {line:?}");
            }
        }
    }

    #[test]
    fn shapes_are_recognised() {
        let cases = [
            ("A[r]", Shape::Rect),
            ("A(r)", Shape::Round),
            ("A([r])", Shape::Round),
            ("A{d}", Shape::Diamond),
            ("A{{d}}", Shape::Diamond),
            ("A((c))", Shape::Circle),
            ("A", Shape::Rect),
        ];
        for (text, want) in cases {
            let (_, _, shape) = parse_node(text).unwrap();
            assert_eq!(shape, want, "{text}");
        }
    }

    #[test]
    fn line_breaks_inside_a_node_are_honoured() {
        let (_, lines, _) = parse_node("A[Myofascial trigger<br>point release]").unwrap();
        assert_eq!(lines, vec!["Myofascial trigger", "point release"]);
    }

    #[test]
    fn edge_labels_are_parsed_in_both_spellings() {
        let g = parse("flowchart TD\n A --> |Yes| B\n C -- No --> D\n E --> F\n").unwrap();
        let labels: Vec<Option<&str>> = g.edges.iter().map(|e| e.label.as_deref()).collect();
        assert_eq!(labels, vec![Some("Yes"), Some("No"), None]);
    }

    #[test]
    fn ampersands_fan_in_and_out() {
        let g = parse("flowchart TD\n E & F & G --> H\n A --> B & C\n").unwrap();
        let into_h = g.edges.iter().filter(|e| e.to == g.by_id["H"]).count();
        assert_eq!(into_h, 3);
        let from_a = g.edges.iter().filter(|e| e.from == g.by_id["A"]).count();
        assert_eq!(from_a, 2);
    }

    #[test]
    fn a_label_given_later_replaces_a_bare_reference() {
        let g = parse("flowchart TD\n A --> B\n B[Proper Label] --> C\n").unwrap();
        let b = &g.nodes[g.by_id["B"]];
        assert_eq!(b.lines, vec!["Proper Label"]);
    }

    #[test]
    fn directives_and_subgraphs_are_skipped_not_drawn() {
        let g = parse(
            "flowchart TD\n  %% a comment\n  classDef big fill:#f00\n  subgraph One\n  A --> B\n  end\n  style A fill:#fff\n",
        )
        .unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn ranks_follow_the_longest_path_and_survive_a_cycle() {
        let mut g = parse("flowchart TD\n A --> B\n B --> C\n A --> C\n").unwrap();
        let depth = assign_ranks(&mut g);
        assert_eq!(depth, 2, "C is below B, not beside it");
        assert_eq!(g.nodes[g.by_id["C"]].rank, 2);

        let mut cyclic = parse("flowchart TD\n A --> B\n B --> A\n").unwrap();
        assert!(assign_ranks(&mut cyclic) < 100, "a cycle must not loop forever");
    }

    #[test]
    fn a_parent_is_centred_over_its_children() {
        let mut g = parse("flowchart TD\n A[Root] --> B[Wide left child]\n A --> C[C]\n").unwrap();
        let depth = assign_ranks(&mut g);
        place(&mut g, depth);
        let (a, b, c) = (&g.nodes[g.by_id["A"]], &g.nodes[g.by_id["B"]], &g.nodes[g.by_id["C"]]);
        let span = (b.x + (c.x + c.width)) / 2;
        assert!(a.centre().abs_diff(span) <= 2, "root at {} span centre {span}", a.centre());
    }

    #[test]
    fn siblings_never_overlap() {
        let mut g = parse("flowchart TD\n A --> B[Some label]\n A --> C[Another label]\n A --> D[Third]\n")
            .unwrap();
        let depth = assign_ranks(&mut g);
        place(&mut g, depth);
        let mut row: Vec<&Node> = g.nodes.iter().filter(|n| n.rank == 1).collect();
        row.sort_by_key(|n| n.x);
        for pair in row.windows(2) {
            assert!(
                pair[0].x + pair[0].width <= pair[1].x,
                "{:?} overlaps {:?}",
                pair[0].lines,
                pair[1].lines
            );
        }
    }

    #[test]
    fn a_diagram_too_wide_for_boxes_becomes_an_outline() {
        let wide = "flowchart TD\n A[Root] --> B[A rather long child label here]\n A --> C[Another quite long child label]\n";
        let rows = render(wide, 30).unwrap();
        let out = text(&rows).join("\n");
        assert!(out.contains("outline"), "{out}");
        assert!(out.contains("└─") || out.contains("├─"), "{out}");
        assert!(!out.contains('┌'), "no boxes in the outline: {out}");
    }

    #[test]
    fn the_outline_names_a_shared_node_again_but_does_not_expand_it() {
        let diamond = "flowchart TD\n A[Start of a long chain] --> B[Left branch label]\n A --> C[Right branch label]\n B --> D[Shared destination]\n C --> D\n D --> E[Only child of D]\n";
        let rows = render(diamond, 24).unwrap();
        let out = text(&rows).join("\n");
        assert_eq!(out.matches("Shared destination").count(), 2, "named on both branches: {out}");
        assert_eq!(out.matches("Only child of D").count(), 1, "expanded once: {out}");
        assert!(out.contains("↺ shown above"), "{out}");
    }

    #[test]
    fn the_outline_carries_edge_labels() {
        let code = "flowchart TD\n A[A decision point in the tree] -->|Yes| B[The affirmative branch]\n A -->|No| C[The negative branch]\n";
        let out = text(&render(code, 28).unwrap()).join("\n");
        assert!(out.contains("Yes") && out.contains("No"), "{out}");
    }

    #[test]
    fn the_real_wiki_diagram_renders() {
        let code = "flowchart TD\n    A[Bruxism Diagnosis] --> B{Mild / Self-limiting?}\n    B -->|Yes| C[Education + Self-care<br>+ Habit Reversal]\n    B -->|No / Persistent| D[Conservative First-Line]\n    D --> E[Occlusal Splint<br>protects teeth]\n    E & D --> H{Adequate response?}\n";
        let rows = render(code, 120).unwrap();
        let out = text(&rows).join("\n");
        assert!(out.contains("Bruxism Diagnosis"), "{out}");
        assert!(out.contains("Yes"), "edge labels are drawn: {out}");
        assert!(out.contains("Habit Reversal"), "line breaks are drawn: {out}");
    }

    #[test]
    fn an_xychart_renders_with_bars_and_axes() {
        let code = r#"xychart-beta
    title "Area Overhead (% of SoC)"
    x-axis ["Mix-GEMM", "SMX-1D", "TMU", "GMX", "CVA6 FPU"]
    y-axis "% of SoC" 0 --> 35
    bar [1, 1.37, 1.52, 1.7, 9, 29.66]
"#;
        let rows = render(code, 80).unwrap();
        let out = text(&rows).join("\n");
        assert!(out.contains("Area Overhead"), "{out}");
        assert!(out.contains("Mix-GEMM"), "{out}");
        assert!(out.contains("TMU"), "{out}");
        assert!(out.contains("CVA6"), "{out}");
        assert!(out.contains('└') && out.contains('┤'), "{out}");
        assert!(out.contains('█') || out.contains('▂') || out.contains('▄'), "{out}");
    }

    #[test]
    fn an_xychart_respects_pane_width_and_switches_to_horizontal_when_narrow() {
        let code = r#"xychart-beta
    title "Area Overhead (% of SoC)"
    x-axis ["Mix-GEMM", "SMX-1D", "TMU", "GMX", "CVA6 FPU"]
    y-axis "% of SoC" 0 --> 35
    bar [1, 1.37, 1.52, 1.7, 9, 29.66]
"#;
        for width in [25usize, 40, 80, 120] {
            let rows = render(code, width).unwrap();
            for row in rows {
                let line: String = row.iter().map(|(t, _)| t.as_str()).collect();
                assert!(line.width() <= width, "width {width}: {line:?}");
            }
        }
    }

    #[test]
    fn an_xychart_with_line_series_renders_markers() {
        let code = r#"xychart-beta
    x-axis [Q1, Q2, Q3, Q4]
    y-axis 0 --> 100
    line [20, 45, 60, 90]
"#;
        let rows = render(code, 60).unwrap();
        let out = text(&rows).join("\n");
        assert!(out.contains('●'), "{out}");
        assert!(out.contains("Q1") && out.contains("Q4"), "{out}");
    }
