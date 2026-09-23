//! Mermaid diagrams, drawn with box characters.
//!
//! There is no mermaid renderer for the terminal, and shelling out to
//! `mmdc` for a PNG would need a terminal graphics protocol, a Node
//! toolchain and a round trip per diagram. The diagrams that actually appear in
//! this wiki are small `flowchart TD` trees, so they are laid out and drawn
//! here: tidy-tree placement, real boxes, real arrows, edge labels.
//!
//! Two renderings are produced, and the caller gets whichever fits: boxes when
//! the diagram is no wider than the pane, and an indented outline when it is
//! not. An outline is not a consolation prize — for a deep tree in a narrow
//! column it is the more readable of the two.

use std::collections::HashMap;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod xychart;
#[cfg(test)]
mod tests;


/// What a run of characters means, so the caller can colour it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// Box borders.
    Frame,
    /// Text inside a node.
    Node,
    /// Connector lines and arrowheads.
    Edge,
    /// An edge label.
    Label,
    /// The diagram kind, shown above it.
    Title,
}

pub type Row = Vec<(String, Ink)>;

/// Gap between sibling boxes.
const GAP: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Rect,
    Round,
    Diamond,
    Circle,
}

#[derive(Clone, Debug)]
struct Node {
    lines: Vec<String>,
    shape: Shape,
    rank: usize,
    x: usize,
    width: usize,
}

impl Node {
    fn centre(&self) -> usize {
        self.x + self.width / 2
    }
}

#[derive(Clone, Debug)]
struct Edge {
    from: usize,
    to: usize,
    label: Option<String>,
}

/// Render a mermaid block, or `None` when the diagram kind is not supported —
/// in which case the caller shows the source, which is better than a wrong
/// picture.
pub fn render(code: &str, width: usize) -> Option<Vec<Row>> {
    let kind = code
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("%%"))?;

    if kind.starts_with("flowchart") || kind.starts_with("graph") {
        return flowchart(code, width);
    }
    if kind.starts_with("xychart-beta") || kind.starts_with("xychart") {
        return xychart::xychart(code, width);
    }
    None
}

// ── parsing ────────────────────────────────────────────────────────────────

fn statements(code: &str) -> Vec<String> {
    code.lines()
        .skip(1)
        .flat_map(|line| line.split(';'))
        .map(|line| line.trim().to_string())
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("%%")
                && !line.starts_with("classDef")
                && !line.starts_with("class ")
                && !line.starts_with("style ")
                && !line.starts_with("click ")
                && !line.starts_with("linkStyle")
                && !line.starts_with("subgraph")
                && *line != "end"
                && !line.starts_with("direction ")
        })
        .collect()
}

/// `A[Label]`, `A(Label)`, `A{Label}`, `A((Label))`, `A([Label])`, or a bare id.
fn parse_node(text: &str) -> Option<(String, Vec<String>, Shape)> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let open = text.find(['[', '(', '{', '>']);
    let Some(open) = open else {
        let id = text.trim().to_string();
        return valid_id(&id).then(|| (id.clone(), vec![id], Shape::Rect));
    };
    let id = text[..open].trim().to_string();
    if !valid_id(&id) {
        return None;
    }
    let rest = &text[open..];
    let (shape, label) = match rest.chars().next()? {
        '[' if rest.starts_with("[[") => (Shape::Rect, strip(rest, "[[", "]]")),
        '[' if rest.starts_with("[(") => (Shape::Round, strip(rest, "[(", ")]")),
        '[' => (Shape::Rect, strip(rest, "[", "]")),
        '(' if rest.starts_with("((") => (Shape::Circle, strip(rest, "((", "))")),
        '(' if rest.starts_with("([") => (Shape::Round, strip(rest, "([", "])")),
        '(' => (Shape::Round, strip(rest, "(", ")")),
        '{' if rest.starts_with("{{") => (Shape::Diamond, strip(rest, "{{", "}}")),
        '{' => (Shape::Diamond, strip(rest, "{", "}")),
        '>' => (Shape::Rect, strip(rest, ">", "]")),
        _ => return None,
    };
    let label = label.unwrap_or_else(|| id.clone());
    Some((id, split_breaks(&label), shape))
}

fn strip(text: &str, open: &str, close: &str) -> Option<String> {
    let inner = text.strip_prefix(open)?.strip_suffix(close)?;
    Some(inner.trim().trim_matches('"').trim_matches('\'').to_string())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

/// `<br>`, `<br/>` and `\n` all mean a new line inside a node.
fn split_breaks(label: &str) -> Vec<String> {
    let normalized = label
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<br>", "\n")
        .replace("\\n", "\n");
    normalized
        .split('\n')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

struct Graph {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    by_id: HashMap<String, usize>,
}

impl Graph {
    fn intern(&mut self, text: &str) -> Option<usize> {
        let (id, lines, shape) = parse_node(text)?;
        if let Some(at) = self.by_id.get(&id) {
            // A later mention with a real label wins over a bare reference.
            if self.nodes[*at].lines == vec![id.clone()] && lines != vec![id.clone()] {
                self.nodes[*at].lines = lines;
                self.nodes[*at].shape = shape;
            }
            return Some(*at);
        }
        let at = self.nodes.len();
        self.by_id.insert(id.clone(), at);
        self.nodes.push(Node { lines, shape, rank: 0, x: 0, width: 0 });
        Some(at)
    }
}

/// A link between two nodes: where it sits in the statement and its label.
#[derive(Debug, PartialEq, Eq)]
struct Link {
    start: usize,
    end: usize,
    label: Option<String>,
}

fn is_shaft(c: u8) -> bool {
    matches!(c, b'-' | b'=' | b'.')
}

/// Find the first link in a statement.
///
/// Hand-written rather than a regex because mermaid's link syntax has three
/// independent parts — shaft, optional inline label, optional `|label|` — and a
/// pattern that covers all of them is one nobody can read or debug.
fn find_link(statement: &str) -> Option<Link> {
    let bytes = statement.as_bytes();
    let mut i = 1; // a link never starts a statement
    while i + 1 < bytes.len() {
        let starts = (bytes[i] == b'-' && (bytes[i + 1] == b'-' || bytes[i + 1] == b'.'))
            || (bytes[i] == b'=' && bytes[i + 1] == b'=');
        if !starts {
            i += 1;
            continue;
        }

        let mut shaft_end = i;
        while shaft_end < bytes.len() && is_shaft(bytes[shaft_end]) {
            shaft_end += 1;
        }
        let mut end = shaft_end;
        let has_head = end < bytes.len() && matches!(bytes[end], b'>' | b'x' | b'o');
        if has_head {
            end += 1;
        }

        let mut label = None;
        // `-- label -->`: no arrowhead yet, then text, then a second shaft.
        if !has_head {
            let rest = &statement[end..];
            if let Some(k) = rest.find(|c: char| is_shaft(c as u8)) {
                let mid = rest[..k].trim();
                let rb = rest.as_bytes();
                let mut m = k;
                while m < rb.len() && is_shaft(rb[m]) {
                    m += 1;
                }
                let mut second = m;
                if second < rb.len() && matches!(rb[second], b'>' | b'x' | b'o') {
                    second += 1;
                }
                // A node id containing a dash is not a label.
                let looks_like_label =
                    !mid.is_empty() && !mid.contains(['[', '(', '{', '|']) && m - k >= 2;
                if looks_like_label {
                    label = Some(mid.trim_matches('"').to_string());
                    end += second;
                }
            }
        }

        // `-->|label|`
        let rest = &statement[end..];
        let trimmed = rest.trim_start();
        let skipped = rest.len() - trimmed.len();
        if let Some(after) = trimmed.strip_prefix('|') {
            if let Some(close) = after.find('|') {
                label = Some(after[..close].trim().trim_matches('"').to_string());
                end += skipped + 1 + close + 1;
            }
        }

        return Some(Link { start: i, end, label: label.filter(|l| !l.is_empty()) });
    }
    None
}

fn parse(code: &str) -> Option<Graph> {
    let mut graph = Graph { nodes: Vec::new(), edges: Vec::new(), by_id: HashMap::new() };

    for statement in statements(code) {
        let mut rest = statement.as_str();
        let mut previous: Option<Vec<usize>> = None;

        while let Some(link) = find_link(rest) {
            // `A --> B & C` fans out; `E & F & G --> H` fans in.
            let sources = match previous.take() {
                Some(carried) => carried,
                None => rest[..link.start].split('&').filter_map(|p| graph.intern(p)).collect(),
            };
            let tail = &rest[link.end..];
            // A chain `A --> B --> C` continues past this link.
            let next = find_link(tail);
            let head = next.as_ref().map(|l| &tail[..l.start]).unwrap_or(tail);
            let targets: Vec<usize> = head.split('&').filter_map(|p| graph.intern(p)).collect();

            for from in &sources {
                for to in &targets {
                    if from != to {
                        graph.edges.push(Edge { from: *from, to: *to, label: link.label.clone() });
                    }
                }
            }
            if next.is_none() {
                break;
            }
            previous = Some(targets);
            rest = tail;
        }

        if previous.is_none() && find_link(&statement).is_none() {
            graph.intern(&statement);
        }
    }

    (!graph.nodes.is_empty()).then_some(graph)
}

// ── layout ─────────────────────────────────────────────────────────────────

/// Longest-path layering. A cycle stops growing rather than looping forever.
fn assign_ranks(graph: &mut Graph) -> usize {
    let n = graph.nodes.len();
    for _ in 0..n {
        let mut changed = false;
        for edge in &graph.edges {
            let want = graph.nodes[edge.from].rank + 1;
            if graph.nodes[edge.to].rank < want {
                graph.nodes[edge.to].rank = want;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    graph.nodes.iter().map(|n| n.rank).max().unwrap_or(0)
}

fn box_width(node: &Node) -> usize {
    let text = node.lines.iter().map(|l| l.width()).max().unwrap_or(1);
    match node.shape {
        Shape::Diamond | Shape::Circle => text + 6,
        _ => text + 4,
    }
}

/// Tidy-tree placement: deepest rank packs left to right, then every shallower
/// node is centred over its children and pushed right only if it would collide.
fn place(graph: &mut Graph, depth: usize) -> usize {
    for node in graph.nodes.iter_mut() {
        node.width = box_width(node);
    }

    let order: Vec<Vec<usize>> = (0..=depth)
        .map(|rank| (0..graph.nodes.len()).filter(|i| graph.nodes[*i].rank == rank).collect())
        .collect();

    for rank in (0..=depth).rev() {
        let mut cursor = 0usize;
        for &node in &order[rank] {
            let children: Vec<usize> = graph
                .edges
                .iter()
                .filter(|e| e.from == node && graph.nodes[e.to].rank > rank)
                .map(|e| e.to)
                .collect();
            let desired = if children.is_empty() || rank == depth {
                cursor
            } else {
                let left = children.iter().map(|c| graph.nodes[*c].x).min().unwrap_or(cursor);
                let right = children
                    .iter()
                    .map(|c| graph.nodes[*c].x + graph.nodes[*c].width)
                    .max()
                    .unwrap_or(cursor);
                let centre = (left + right) / 2;
                centre.saturating_sub(graph.nodes[node].width / 2)
            };
            let x = desired.max(cursor);
            graph.nodes[node].x = x;
            cursor = x + graph.nodes[node].width + GAP;
        }
    }

    graph.nodes.iter().map(|n| n.x + n.width).max().unwrap_or(0)
}

// ── drawing ────────────────────────────────────────────────────────────────

/// A character canvas that remembers what each cell means.
struct Canvas {
    cells: Vec<Vec<(char, Ink)>>,
    width: usize,
}

impl Canvas {
    fn new(width: usize) -> Self {
        Self { cells: Vec::new(), width }
    }

    fn ensure(&mut self, row: usize) {
        while self.cells.len() <= row {
            self.cells.push(vec![(' ', Ink::Edge); self.width]);
        }
    }

    fn put(&mut self, row: usize, col: usize, ch: char, ink: Ink) {
        if col >= self.width {
            return;
        }
        self.ensure(row);
        self.cells[row][col] = (ch, ink);
    }

    /// Draw a connector without erasing a crossing one.
    fn connect(&mut self, row: usize, col: usize, ch: char) {
        if col >= self.width {
            return;
        }
        self.ensure(row);
        let existing = self.cells[row][col].0;
        let merged = match (existing, ch) {
            (' ', c) => c,
            (a, b) if a == b => a,
            ('─', '│') | ('│', '─') => '┼',
            ('─', '┌') | ('┌', '─') => '┬',
            ('─', '┐') | ('┐', '─') => '┬',
            ('┘', '─') | ('─', '┘') => '┴',
            ('└', '─') | ('─', '└') => '┴',
            (_, b) => b,
        };
        self.cells[row][col] = (merged, Ink::Edge);
    }

    fn write(&mut self, row: usize, col: usize, text: &str, ink: Ink) {
        for (i, ch) in text.chars().enumerate() {
            self.put(row, col + i, ch, ink);
        }
    }

    fn write_str(&mut self, row: usize, mut col: usize, text: &str, ink: Ink) {
        for ch in text.chars() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(1);
            if col + w > self.width {
                break;
            }
            self.put(row, col, ch, ink);
            col += w;
        }
    }

    fn rows(self) -> Vec<Row> {
        self.cells
            .into_iter()
            .map(|cells| {
                let mut row: Row = Vec::new();
                // Trailing blanks are noise in a terminal; drop them.
                let end = cells.iter().rposition(|(c, _)| *c != ' ').map(|i| i + 1).unwrap_or(0);
                for (ch, ink) in cells.into_iter().take(end) {
                    match row.last_mut() {
                        Some((text, last)) if *last == ink => text.push(ch),
                        _ => row.push((ch.to_string(), ink)),
                    }
                }
                row
            })
            .collect()
    }
}

fn flowchart(code: &str, width: usize) -> Option<Vec<Row>> {
    let mut graph = parse(code)?;
    let depth = assign_ranks(&mut graph);
    let total = place(&mut graph, depth);

    if total > width {
        return Some(outline(&graph));
    }

    let mut canvas = Canvas::new(width);
    let mut row = 0usize;

    for rank in 0..=depth {
        let nodes: Vec<usize> = (0..graph.nodes.len()).filter(|i| graph.nodes[*i].rank == rank).collect();
        if nodes.is_empty() {
            continue;
        }
        if rank > 0 {
            row = channel(&mut canvas, &graph, rank, row);
        }
        let height = nodes.iter().map(|i| graph.nodes[*i].lines.len()).max().unwrap_or(1) + 2;
        for &i in &nodes {
            draw_box(&mut canvas, &graph.nodes[i], row);
        }
        row += height;
    }

    let mut rows = canvas.rows();
    rows.insert(0, vec![(format!("flowchart · {} nodes", graph.nodes.len()), Ink::Title)]);
    Some(rows)
}

fn draw_box(canvas: &mut Canvas, node: &Node, top: usize) {
    let (left, right, tl, tr, bl, br) = match node.shape {
        Shape::Diamond => ('◄', '►', '┌', '┐', '└', '┘'),
        Shape::Circle => ('(', ')', '╭', '╮', '╰', '╯'),
        Shape::Round => ('│', '│', '╭', '╮', '╰', '╯'),
        Shape::Rect => ('│', '│', '┌', '┐', '└', '┘'),
    };
    let inner = node.width.saturating_sub(2);

    canvas.put(top, node.x, tl, Ink::Frame);
    canvas.put(top, node.x + node.width - 1, tr, Ink::Frame);
    for i in 1..node.width - 1 {
        canvas.put(top, node.x + i, '─', Ink::Frame);
    }

    for (i, line) in node.lines.iter().enumerate() {
        let y = top + 1 + i;
        canvas.put(y, node.x, left, Ink::Frame);
        canvas.put(y, node.x + node.width - 1, right, Ink::Frame);
        let pad = inner.saturating_sub(line.width()) / 2;
        canvas.write(y, node.x + 1 + pad, line, Ink::Node);
    }

    let bottom = top + node.lines.len() + 1;
    canvas.put(bottom, node.x, bl, Ink::Frame);
    canvas.put(bottom, node.x + node.width - 1, br, Ink::Frame);
    for i in 1..node.width - 1 {
        canvas.put(bottom, node.x + i, '─', Ink::Frame);
    }
}

/// Draw the connectors feeding into `rank`, returning the row its boxes start on.
fn channel(canvas: &mut Canvas, graph: &Graph, rank: usize, top: usize) -> usize {
    let incoming: Vec<&Edge> = graph.edges.iter().filter(|e| graph.nodes[e.to].rank == rank).collect();
    if incoming.is_empty() {
        return top + 1;
    }
    let has_label = incoming.iter().any(|e| e.label.is_some());
    // stub · bus · (label) · arrowhead
    let height = if has_label { 4 } else { 3 };

    let bus = top + 1;
    for edge in &incoming {
        let from = graph.nodes[edge.from].centre();
        let to = graph.nodes[edge.to].centre();

        canvas.connect(top, from, '│');
        let (lo, hi) = (from.min(to), from.max(to));
        for col in lo..=hi {
            canvas.connect(bus, col, '─');
        }
        canvas.connect(bus, from, if from == to { '│' } else { '┴' });
        canvas.connect(bus, to, if from == to { '│' } else { '┬' });

        for row in bus + 1..top + height - 1 {
            canvas.connect(row, to, '│');
        }
        canvas.put(top + height - 1, to, '▼', Ink::Edge);

        if let Some(label) = &edge.label {
            canvas.write(bus + 1, to + 2, label, Ink::Label);
        }
    }
    top + height
}

/// The fallback when the boxes do not fit: an indented outline, which for a
/// deep tree in a narrow column reads better than a squeezed diagram anyway.
fn outline(graph: &Graph) -> Vec<Row> {
    let mut rows: Vec<Row> =
        vec![vec![(format!("flowchart · {} nodes · outline", graph.nodes.len()), Ink::Title)]];

    let roots: Vec<usize> = (0..graph.nodes.len())
        .filter(|i| !graph.edges.iter().any(|e| e.to == *i))
        .collect();
    let roots = if roots.is_empty() { vec![0] } else { roots };

    let mut seen = vec![false; graph.nodes.len()];
    for root in roots {
        walk(graph, root, "", None, &mut seen, &mut rows);
    }
    rows
}

fn walk(
    graph: &Graph,
    node: usize,
    indent: &str,
    stem: Option<&str>,
    seen: &mut Vec<bool>,
    rows: &mut Vec<Row>,
) {
    let mut row: Row = Vec::new();
    if let Some(stem) = stem {
        row.push((format!("{indent}{stem}"), Ink::Edge));
    }
    row.push((graph.nodes[node].lines.join(" "), Ink::Node));

    if seen[node] {
        // A shared node is named again but not expanded again, so a diamond in
        // the graph does not become an infinite tree on the page.
        row.push(("  ↺ shown above".to_string(), Ink::Label));
        rows.push(row);
        return;
    }
    seen[node] = true;
    rows.push(row);

    let children: Vec<&Edge> = graph.edges.iter().filter(|e| e.from == node).collect();
    let child_indent = match stem {
        None => String::new(),
        Some(s) if s.starts_with('└') => format!("{indent}   "),
        Some(_) => format!("{indent}│  "),
    };
    for (i, edge) in children.iter().enumerate() {
        let last = i == children.len() - 1;
        if let Some(label) = &edge.label {
            rows.push(vec![
                (format!("{child_indent}│  "), Ink::Edge),
                (label.clone(), Ink::Label),
            ]);
        }
        walk(
            graph,
            edge.to,
            &child_indent,
            Some(if last { "└─ " } else { "├─ " }),
            seen,
            rows,
        );
    }
}

