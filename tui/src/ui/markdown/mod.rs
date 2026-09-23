//! Markdown → styled terminal lines.
//!
//! Produces a `Doc` of width-wrapped segments rather than a ratatui `Text`
//! directly, because the reader needs two things a finished `Text` throws away:
//! which segments belong to which link (so one can be highlighted and followed)
//! and which source line each rendered line came from (so `e` drops the editor
//! cursor where you were reading).

use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme::Theme;
use crate::vault::links::LinkKind;

mod table;
mod tex;
#[cfg(test)]
mod tests;

pub use table::render_grid;
use table::trim_segs;
use tex::tex_to_unicode;


/// Prose past this column count is harder to track line-to-line, so the
/// reader and edit preview never wrap wider than this even on an ultra-wide
/// pane. Typography's usual advice is 45-90 characters a line; 89 sits at the
/// top of that range, which suits reference prose full of long identifiers
/// without letting the eye lose its place on the return sweep.
pub const MAX_WIDTH: u16 = 89;

#[derive(Clone, Debug)]
pub struct Seg {
    pub text: String,
    pub style: Style,
    /// Index into `Doc::links` when this segment is part of a link.
    pub link: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextPos {
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: TextPos,
    pub cursor: TextPos,
}

impl Selection {
    pub fn new(anchor: TextPos, cursor: TextPos) -> Self {
        Self { anchor, cursor }
    }

    pub fn range(&self) -> (TextPos, TextPos) {
        if (self.anchor.line, self.anchor.col) <= (self.cursor.line, self.cursor.col) {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.cursor
    }
}

/// A source-line range an agent asked the reader to draw attention to.
///
/// Ranges are over *source* lines, not rendered ones: rendering rewraps prose,
/// so a rendered range would move whenever the pane is resized. Marks are
/// in-memory only — nothing an agent points at is written back to the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    /// 0-based, inclusive.
    pub start: usize,
    /// 0-based, inclusive.
    pub end: usize,
    pub label: Option<String>,
}

impl Mark {
    pub fn covers(&self, src_line: usize) -> bool {
        src_line >= self.start && src_line <= self.end
    }
}

/// One literal hit for the in-page find, in *rendered* coordinates: character
/// columns on one rendered line.
///
/// Rendered, not source, unlike `Mark` — find highlights exactly what is on
/// screen, and a hit is recomputed from the current `Doc` rather than kept
/// across a resize, so rewrapping cannot strand it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Match {
    pub line: usize,
    /// 0-based character column, inclusive.
    pub start: usize,
    /// 0-based character column, exclusive.
    pub end: usize,
}

/// Every hit for the live find query, and which one the reader is sitting on.
#[derive(Clone, Debug, Default)]
pub struct Finds {
    pub hits: Vec<Match>,
    pub current: usize,
}

/// Everything the reader paints *over* the rendered text. Bundled because they
/// are one concept — "what is highlighted right now" — and because four more
/// positional arguments on `to_lines` would be four more chances to pass them
/// in the wrong order.
#[derive(Clone, Copy, Default)]
pub struct Overlays<'a> {
    /// The link the reader is sitting on.
    pub link: Option<usize>,
    pub selection: Option<Selection>,
    pub finds: &'a [Match],
    /// Index into `finds` of the hit the find bar is on.
    pub current_find: usize,
    pub marks: &'a [Mark],
    /// Display columns scrolled off the left edge. A table wider than the pane
    /// is laid out at its true width and panned, rather than squeezed until
    /// every cell is an ellipsis.
    pub hscroll: usize,
}

impl<'a> Overlays<'a> {
    pub fn new(link: Option<usize>, selection: Option<Selection>, finds: &'a Finds, marks: &'a [Mark]) -> Self {
        Self { link, selection, finds: &finds.hits, current_find: finds.current, marks, hscroll: 0 }
    }
}

/// Case folded one character at a time, never `str::to_lowercase`: a few
/// characters lowercase into two, which would shift every column after them
/// and highlight the wrong text.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

#[derive(Clone, Debug, Default)]
pub struct DocLine {
    pub segs: Vec<Seg>,
    /// 0-based body line this rendered line came from, best effort.
    pub src_line: usize,
}

impl DocLine {
    pub fn plain_text(&self) -> String {
        self.segs.iter().map(|s| s.text.as_str()).collect()
    }
}

#[derive(Clone, Debug)]
pub struct DocLink {
    pub target: String,
    pub kind: LinkKind,
    /// Rendered line the link starts on.
    pub line: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub lines: Vec<DocLine>,
    pub links: Vec<DocLink>,
    /// `(rendered line, level, text)` for the outline popup.
    pub headings: Vec<(usize, u8, String)>,
    /// Footnote labels in order of first citation, so the body can show a
    /// short `[1]` where the source writes `[^atarodi_2024_…]` and the
    /// inspector can number the same sources identically.
    pub citations: Vec<String>,
    /// Display column each table column ends at, for a doc that is one table
    /// (a CSV). Empty for prose: a page with several tables has no single
    /// column geometry to report.
    pub col_stops: Vec<usize>,
    /// Widest rendered line, in display columns. Kept as the lines are pushed:
    /// the reader asks for it on every frame, and a long CSV would otherwise
    /// re-measure thousands of lines to answer.
    pub width: usize,
}

impl Doc {
    /// 1-based citation number for a footnote label, if the body cites it.
    pub fn citation_number(&self, label: &str) -> Option<usize> {
        self.citations.iter().position(|l| l == label).map(|i| i + 1)
    }

    /// Index of the first body link for a footnote label — the `[n]` the
    /// reader can highlight when its source row is picked in the inspector.
    pub fn citation_link(&self, label: &str) -> Option<usize> {
        self.links
            .iter()
            .position(|l| l.kind == LinkKind::Footnote && l.target == label)
    }
}

impl Doc {
    pub fn height(&self) -> usize {
        self.lines.len()
    }

    /// Table columns wholly left of `hscroll`, and those starting beyond
    /// `hscroll + view` — what a pan is hiding on either side.
    pub fn hidden_columns(&self, hscroll: usize, view: usize) -> (usize, usize) {
        if self.col_stops.is_empty() {
            return (0, 0);
        }
        let left = self.col_stops.iter().filter(|stop| **stop <= hscroll).count();
        let right = self.col_stops.iter().filter(|stop| **stop > hscroll + view).count();
        (left, right)
    }

    /// Extract selected text as a string, preserving line breaks.
    ///
    /// Rendered, not raw-source, text — links, emphasis markers and heading
    /// hashes are already stripped by rendering. The reader's copy commands
    /// use `Open::selected_source_text` instead, which returns the untouched
    /// markdown; this stays as the primitive the rendering tests exercise.
    #[allow(dead_code)]
    pub fn selected_text(&self, sel: Selection) -> String {
        let (start, end) = sel.range();
        if start == end {
            return String::new();
        }
        let mut result = Vec::new();
        for line_idx in start.line..=end.line {
            let Some(line) = self.lines.get(line_idx) else {
                break;
            };
            let plain = line.plain_text();
            let chars: Vec<char> = plain.chars().collect();
            let col_start = if line_idx == start.line { start.col.min(chars.len()) } else { 0 };
            let col_end = if line_idx == end.line { end.col.min(chars.len()) } else { chars.len() };
            if col_start < col_end {
                result.push(chars[col_start..col_end].iter().collect::<String>());
            } else if (line_idx != start.line && line_idx != end.line)
                || (line_idx == start.line && col_start == 0 && chars.is_empty())
            {
                // A line wholly inside the selection, or an empty first line,
                // still contributes a newline to the copied text.
                result.push(String::new());
            }
        }
        result.join("\n")
    }

    #[allow(dead_code)]
    pub fn plain_text(&self) -> String {
        self.lines.iter().map(|l| l.plain_text()).collect::<Vec<_>>().join("\n")
    }

    /// Rendered line for a body line — used when switching from the editor back
    /// to the reader without losing your place.
    pub fn line_for_source(&self, src: usize) -> usize {
        self.lines
            .iter()
            .position(|l| l.src_line >= src)
            .unwrap_or(self.lines.len().saturating_sub(1))
    }

    pub fn source_for_line(&self, line: usize) -> usize {
        self.lines.get(line).map(|l| l.src_line).unwrap_or(0)
    }

    /// Every case-insensitive occurrence of `query` in the rendered text, in
    /// reading order.
    ///
    /// A hit never spans two rendered lines: the reader rewraps prose, so a
    /// phrase broken across a wrap has no single highlightable span, and
    /// pretending otherwise would tint half a line for no visible reason.
    pub fn find(&self, query: &str) -> Vec<Match> {
        let needle: Vec<char> = query.chars().map(fold).collect();
        if needle.is_empty() {
            return Vec::new();
        }
        let mut hits = Vec::new();
        for (line, text) in self.lines.iter().enumerate() {
            let hay: Vec<char> = text.plain_text().chars().map(fold).collect();
            let mut col = 0;
            while col + needle.len() <= hay.len() {
                if hay[col..col + needle.len()] == needle[..] {
                    hits.push(Match { line, start: col, end: col + needle.len() });
                    col += needle.len();
                } else {
                    col += 1;
                }
            }
        }
        hits
    }

    /// The next link at or after `from`, wrapping around.
    pub fn link_after(&self, from: Option<usize>, forward: bool) -> Option<usize> {
        if self.links.is_empty() {
            return None;
        }
        let n = self.links.len();
        Some(match (from, forward) {
            (None, true) => 0,
            (None, false) => n - 1,
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
        })
    }

    /// Convert to ratatui lines, highlighting `active` link, `selection`, any
    /// `finds` from the in-page search, and any agent `marks` that fall in view.
    pub fn to_lines(
        &self,
        theme: &Theme,
        over: &Overlays,
        from: usize,
        height: usize,
    ) -> Vec<Line<'static>> {
        let sel_range = over.selection.filter(|s| !s.is_empty()).map(|s| s.range());
        let sel_style = Style::default().fg(theme.bg).bg(theme.accent);
        // The hit you are on gets the solid background; the rest are merely
        // underlined, so "there are more of these" never competes with "you
        // are here".
        let find_style = Style::default().fg(theme.bg).bg(theme.warn).add_modifier(Modifier::BOLD);
        let other_find_style =
            Style::default().fg(theme.warn).add_modifier(Modifier::UNDERLINED | Modifier::BOLD);
        let mark_bg = theme.surface;

        self.lines
            .iter()
            .enumerate()
            .skip(from)
            .take(height)
            .map(|(line_idx, line)| {
                let line_sel = sel_range.and_then(|(start, end)| {
                    if line_idx >= start.line && line_idx <= end.line {
                        let total_chars = line.plain_text().chars().count();
                        let col_start = if line_idx == start.line { start.col } else { 0 };
                        let col_end = if line_idx == end.line { end.col } else { total_chars };
                        if col_start < col_end {
                            Some((col_start, col_end))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });

                // Every column range on this line that overrides the segment
                // style, sorted and de-overlapped so the split below can walk
                // them in one pass. The selection is pushed first, so where it
                // collides with a find hit it is the one that survives.
                let mut highlights: Vec<(usize, usize, Style)> = Vec::new();
                if let Some((start, end)) = line_sel {
                    highlights.push((start, end, sel_style));
                }
                for (i, hit) in over.finds.iter().enumerate().filter(|(_, h)| h.line == line_idx) {
                    let style = if i == over.current_find { find_style } else { other_find_style };
                    highlights.push((hit.start, hit.end, style));
                }
                highlights.sort_by_key(|(start, _, _)| *start);
                highlights.dedup_by(|cur, prev| prev.1 > cur.0);

                let mut spans: Vec<Span<'static>> = Vec::new();
                let mut cur_char_offset = 0usize;

                for seg in &line.segs {
                    let base_style = match (seg.link, over.link) {
                        (Some(i), Some(a)) if i == a => {
                            Style::default().fg(theme.bg).bg(theme.link).add_modifier(Modifier::BOLD)
                        }
                        _ => seg.style,
                    };

                    let seg_char_count = seg.text.chars().count();
                    let seg_start = cur_char_offset;
                    let seg_end = cur_char_offset + seg_char_count;
                    cur_char_offset = seg_end;

                    let chars: Vec<char> = seg.text.chars().collect();
                    if chars.is_empty() {
                        spans.push(Span::styled(seg.text.clone(), base_style));
                        continue;
                    }
                    // Walk the segment, emitting one span per run that shares a
                    // style: either inside a highlight, or up to where the next
                    // one begins.
                    let mut col = seg_start;
                    while col < seg_end {
                        let (until, style) =
                            match highlights.iter().find(|(start, end, _)| col >= *start && col < *end) {
                                Some((_, end, style)) => ((*end).min(seg_end), *style),
                                None => (
                                    highlights
                                        .iter()
                                        .map(|(start, _, _)| *start)
                                        .find(|start| *start > col)
                                        .unwrap_or(seg_end)
                                        .min(seg_end),
                                    base_style,
                                ),
                            };
                        let text: String = chars[col - seg_start..until - seg_start].iter().collect();
                        spans.push(Span::styled(text, style));
                        col = until;
                    }
                }

                // A mark tints the whole rendered line rather than a column
                // span: the reader rewraps, so there is no honest mapping from
                // a source column to a rendered one.
                if let Some(mark) = over.marks.iter().find(|m| m.covers(line.src_line)) {
                    for span in &mut spans {
                        if span.style.bg.is_none() {
                            span.style = span.style.bg(mark_bg);
                        }
                    }
                    let opens = line_idx == 0
                        || !self.lines.get(line_idx - 1).is_some_and(|prev| mark.covers(prev.src_line));
                    if let (true, Some(label)) = (opens, mark.label.as_deref()) {
                        spans.push(Span::styled(
                            format!("  ◂ {label}"),
                            Style::default()
                                .fg(theme.accent)
                                .bg(mark_bg)
                                .add_modifier(Modifier::ITALIC),
                        ));
                    }
                }
                Line::from(shift(spans, over.hscroll))
            })
            .collect()
    }
}

/// Drop `offset` display columns off the front of a rendered line.
///
/// Applied after styling, so selections, find hits and marks — all addressed in
/// whole-line character columns — stay on the text they were computed for. A
/// double-width glyph straddling the cut is dropped and replaced by a space, or
/// every column right of it would sit one cell late.
fn shift(spans: Vec<Span<'static>>, offset: usize) -> Vec<Span<'static>> {
    if offset == 0 {
        return spans;
    }
    let mut out: Vec<Span<'static>> = Vec::with_capacity(spans.len());
    let mut used = 0usize;
    for span in spans {
        if used >= offset {
            out.push(span);
            continue;
        }
        let w = span.content.width();
        if used + w <= offset {
            used += w;
            continue;
        }
        let mut rest = String::new();
        for ch in span.content.chars() {
            if used >= offset {
                rest.push(ch);
            } else {
                used += ch.to_string().width();
            }
        }
        if used > offset {
            out.push(Span::styled(" ".repeat(used - offset), span.style));
        }
        out.push(Span::styled(rest, span.style));
        used = offset.max(used);
    }
    out
}

pub fn render(body: &str, width: u16, theme: &Theme) -> Doc {
    let width = width.max(20) as usize;
    let body = &neutralize_footnote_definitions(body);
    let mut w = Writer::new(width, theme);
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    // Alerts, so `> [!WARNING]` arrives as a typed blockquote rather than as
    // literal prose inside the quote.
    opts.insert(Options::ENABLE_GFM);
    // `$…$`, `$$…$$`, `\(…\)` and `\[…\]` become math events rather than prose.
    // A lone `$` in front of a price is safe: `$` only opens a span when the
    // next character is not whitespace and a whitespace-preceded `$` can never
    // close one, so `$40 to $50` stays text.
    opts.insert(Options::ENABLE_MATH);

    let parser = Parser::new_ext(body, opts).into_offset_iter();
    for (event, range) in parser {
        w.src_line = body[..range.start].matches('\n').count();
        w.event(event);
    }
    w.flush();
    w.push_blank();
    w.doc
}

/// Blank out `[^id]: …` lines, keeping the line count.
///
/// CommonMark reads such a line as a *link reference definition* with the label
/// `^id`, which turns every `[^id]` in the prose into a shortcut reference link
/// — rendering it as bare `^id` with the brackets eaten, and swallowing the
/// definition block whole. OKF footnotes are not CommonMark links, so the
/// definitions are removed before parsing and shown in the inspector instead.
/// Lines are blanked rather than deleted so source-line mapping still holds.
fn neutralize_footnote_definitions(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    for (i, line) in body.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if !is_footnote_definition(line) {
            out.push_str(line);
        }
    }
    if body.ends_with('\n') {
        out.push('\n');
    }
    out
}

pub fn is_footnote_definition(line: &str) -> bool {
    line.trim_start()
        .strip_prefix("[^")
        .and_then(|rest| rest.find("]:"))
        .map(|end| end > 0)
        .unwrap_or(false)
}

/// Nesting context, so a close tag knows what it is closing.
#[derive(Clone, Debug)]
enum Block {
    /// `indent` is how far the item's text sits from the block's left edge —
    /// the width of its own marker, so a nested list lines up under the text
    /// of its parent rather than under the parent's bullet. `items` counts the
    /// items opened so far, which is what tells a second item from a first,
    /// and `item_line` is where the open item started, which is what tells a
    /// one-line item from one that wrapped.
    List { ordered: Option<u64>, index: u64, indent: usize, items: usize, item_line: usize },
    Quote { tone: Tone },
    Code,
    Table { rows: Vec<Row>, head: bool, aligns: Vec<Alignment> },
}

/// One table row: cells, each a styled run that may contain links.
type Row = Vec<Vec<Seg>>;

/// What a blockquote is *for*. GFM spells it `> [!WARNING]`; the wiki spells
/// it with a leading emoji (`> ⚠️ Moderate evidence`), and both mean the same
/// thing to a reader, so both colour the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Plain,
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl Tone {
    fn of(kind: Option<BlockQuoteKind>) -> Self {
        match kind {
            Some(BlockQuoteKind::Note) => Self::Note,
            Some(BlockQuoteKind::Tip) => Self::Tip,
            Some(BlockQuoteKind::Important) => Self::Important,
            Some(BlockQuoteKind::Warning) => Self::Warning,
            Some(BlockQuoteKind::Caution) => Self::Caution,
            None => Self::Plain,
        }
    }

    /// The tone a leading emoji implies, for quotes that predate GFM alerts.
    fn of_emoji(text: &str) -> Option<Self> {
        let lead = text.trim_start().chars().next()?;
        Some(match lead {
            '⚠' | '❗' | '‼' => Self::Warning,
            '🚫' | '❌' | '⛔' => Self::Caution,
            '✅' | '✔' | '💡' => Self::Tip,
            'ℹ' | '📝' | '📌' => Self::Note,
            _ => return None,
        })
    }

    fn title(self) -> Option<&'static str> {
        Some(match self {
            Self::Note => "Note",
            Self::Tip => "Tip",
            Self::Important => "Important",
            Self::Warning => "Warning",
            Self::Caution => "Caution",
            Self::Plain => return None,
        })
    }
}

struct Writer<'a> {
    doc: Doc,
    theme: &'a Theme,
    width: usize,
    /// Pending inline segments for the current paragraph.
    pending: Vec<Seg>,
    style: Style,
    blocks: Vec<Block>,
    /// Prefix applied to the first line of the current block (a bullet).
    marker: Option<(String, Style)>,
    link: Option<usize>,
    src_line: usize,
    /// Suppress a blank line at the very top of the document.
    started: bool,
    /// The table cell being collected, as a styled run.
    cell: Option<Vec<Seg>>,
    /// Source of a mermaid block being collected.
    mermaid: Option<String>,
    /// `(language, source, first body line)` of a fenced block being collected.
    /// Buffered rather than emitted line by line because the band is drawn to
    /// the full pane width and the highlighter needs the language before the
    /// first line. The line is remembered from the opening fence, since by the
    /// time the block is drawn `src_line` has moved to the closing one.
    code: Option<(String, String, usize)>,
    /// `(destination, alt text so far)` of the image being collected.
    image: Option<(String, String)>,
    /// Whether each link has been placed on a line yet, so a link's recorded
    /// line is the one it is actually drawn on rather than the one its
    /// paragraph started on.
    placed: Vec<bool>,
    /// Level of the heading being collected.
    heading: u8,
    /// Suppress the blank line before the next paragraph — a callout's title
    /// row belongs to the paragraph under it, not above it.
    tight: bool,
    /// The style a strong span started from, so closing it restores exactly
    /// what `**` opened over.
    strong: Option<Style>,
}

impl<'a> Writer<'a> {
    fn new(width: usize, theme: &'a Theme) -> Self {
        // The document leads with one blank row so its first line (normally the
        // title) never sits flush against the pane's border. It is part of the
        // doc — recorded first, so every line index to come already accounts
        // for it — rather than chrome pinned to the top of the pane, so it
        // scrolls away with the content.
        let mut doc = Doc::default();
        doc.lines.push(DocLine { segs: Vec::new(), src_line: 0 });
        Self {
            doc,
            theme,
            width,
            pending: Vec::new(),
            style: Style::default().fg(theme.text),
            blocks: Vec::new(),
            marker: None,
            link: None,
            src_line: 0,
            started: false,
            cell: None,
            mermaid: None,
            code: None,
            image: None,
            placed: Vec::new(),
            heading: 0,
            tight: false,
            strong: None,
        }
    }

    fn indent(&self) -> usize {
        self.blocks
            .iter()
            .map(|b| match b {
                Block::List { indent, .. } => *indent,
                Block::Quote { .. } => 2,
                Block::Code => 2,
                Block::Table { .. } => 0,
            })
            .sum()
    }

    fn quote_depth(&self) -> usize {
        self.blocks.iter().filter(|b| matches!(b, Block::Quote { .. })).count()
    }

    /// The tone of the innermost quote, which colours its bar.
    fn quote_tone(&self) -> Tone {
        self.blocks
            .iter()
            .rev()
            .find_map(|b| match b {
                Block::Quote { tone } => Some(*tone),
                _ => None,
            })
            .unwrap_or(Tone::Plain)
    }

    fn tone_colour(&self, tone: Tone) -> Color {
        match tone {
            Tone::Plain => self.theme.accent,
            Tone::Note => self.theme.link,
            Tone::Tip => self.theme.ok,
            Tone::Important => self.theme.accent,
            Tone::Warning => self.theme.warn,
            Tone::Caution => self.theme.err,
        }
    }

    /// Math shares the literal colour with code, but keeps an italic face and no
    /// band, so it reads as a changed register mid-sentence rather than as code.
    fn math_style(&self) -> Style {
        Style::default().fg(self.theme.literal).add_modifier(Modifier::ITALIC)
    }

    fn in_table(&self) -> bool {
        self.blocks.iter().any(|b| matches!(b, Block::Table { .. }))
    }

    fn push_blank(&mut self) {
        if !self.started {
            return;
        }
        if self.doc.lines.last().map(|l| l.segs.is_empty()).unwrap_or(true) {
            return;
        }
        self.doc.lines.push(DocLine { segs: Vec::new(), src_line: self.src_line });
    }

    fn push_line(&mut self, segs: Vec<Seg>) {
        self.started = true;
        let n = self.doc.lines.len();
        for seg in &segs {
            if let Some(i) = seg.link {
                if !self.placed[i] {
                    self.placed[i] = true;
                    self.doc.links[i].line = n;
                }
            }
        }
        let width = segs.iter().map(|s| s.text.width()).sum();
        self.doc.width = self.doc.width.max(width);
        self.doc.lines.push(DocLine { segs, src_line: self.src_line });
    }

    /// Runs with the same style are coalesced. pulldown-cmark hands `[^id]`
    /// back as four separate text events (`[`, `^id`, `]`, …) because OKF
    /// footnotes have no in-body definition to resolve against, so the citation
    /// only becomes visible once the run is whole again.
    fn text(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        if let Some((_, alt)) = self.image.as_mut() {
            alt.push_str(s);
            return;
        }
        // The first text of an untyped quote decides its tone: the wiki marks
        // confidence with a leading emoji, not with a GFM alert header.
        if self.pending.is_empty() && self.cell.is_none() {
            if let Some(tone) = Tone::of_emoji(s) {
                if let Some(Block::Quote { tone: current }) =
                    self.blocks.iter_mut().rev().find(|b| matches!(b, Block::Quote { .. }))
                {
                    if *current == Tone::Plain {
                        *current = tone;
                    }
                }
            }
        }
        let (style, link) = (self.style, self.link);
        let run = self.cell.as_mut().unwrap_or(&mut self.pending);
        match run.last_mut() {
            Some(last) if last.style == style && last.link == link => last.text.push_str(s),
            _ => run.push(Seg { text: s.to_string(), style, link }),
        }
    }

    /// Restyle `[^id]` citations inside a finished run, rewriting them as
    /// `[id]` — the caret is syntax, not something to read. Code spans are left
    /// alone, since a bracket in code is just a bracket.
    fn expand_footnotes(&mut self, segs: Vec<Seg>) -> Vec<Seg> {
        let citation = Style::default().fg(self.theme.literal).add_modifier(Modifier::DIM);
        let mut out = Vec::with_capacity(segs.len());
        for seg in segs {
            if seg.style.bg.is_some() || !seg.text.contains("[^") {
                out.push(seg);
                continue;
            }
            for (text, label) in split_footnotes(&seg.text) {
                match label {
                    Some(label) => {
                        let n = self.cite(&label);
                        // A fresh `DocLink` per occurrence — same citation
                        // clicked twice in the body is two separate targets,
                        // just like two `<a>` tags to the same URL.
                        self.doc.links.push(DocLink {
                            target: label,
                            kind: LinkKind::Footnote,
                            line: self.doc.lines.len(),
                        });
                        // Kept in lockstep with `doc.links` — `push_line` indexes
                        // both by the same `seg.link`.
                        self.placed.push(false);
                        let link = Some(self.doc.links.len() - 1);
                        out.push(Seg { text: format!("[{n}]"), style: citation, link });
                    }
                    None => out.push(Seg { text, style: seg.style, link: seg.link }),
                }
            }
        }
        out
    }

    /// The citation number for a label, assigning the next one on first sight.
    fn cite(&mut self, label: &str) -> usize {
        match self.doc.citations.iter().position(|l| l == label) {
            Some(i) => i + 1,
            None => {
                self.doc.citations.push(label.to_string());
                self.doc.citations.len()
            }
        }
    }

    /// Flush the pending inline run as wrapped lines.
    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let taken = std::mem::take(&mut self.pending);
        let segs = self.expand_footnotes(taken);
        let indent = self.indent();
        let quote = self.quote_depth();
        let marker = self.marker.take();

        // Columns the marker needs beyond the block indent — a `10. ` or a
        // heading's `### ` is wider than the two spaces a list nests by, and
        // its continuation lines hang under the text, not under the marker.
        let pad = indent.saturating_sub(quote * 2);
        let marker_width = marker.as_ref().map(|(t, _)| t.width()).unwrap_or(0);
        let hang = marker_width.saturating_sub(pad);
        let avail = self.width.saturating_sub(indent + hang).max(8);

        let out = wrap(segs, avail);

        let bar = Seg {
            text: "▌ ".repeat(quote),
            style: Style::default().fg(self.tone_colour(self.quote_tone())),
            link: None,
        };
        for (i, mut line) in out.into_iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let mut full = Vec::new();
            if quote > 0 {
                full.push(bar.clone());
            }
            match (i, &marker) {
                (0, Some((text, style))) => {
                    let lead = pad.saturating_sub(marker_width);
                    if lead > 0 {
                        full.push(Seg { text: " ".repeat(lead), style: Style::default(), link: None });
                    }
                    full.push(Seg { text: text.clone(), style: *style, link: None });
                }
                _ if pad + hang > 0 => full.push(Seg {
                    text: " ".repeat(pad + hang),
                    style: Style::default(),
                    link: None,
                }),
                _ => {}
            }
            full.append(&mut line);
            self.push_line(full);
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => {
                if let Some(code) = self.mermaid.as_mut() {
                    code.push_str(&text);
                } else if let Some((_, code, _)) = self.code.as_mut() {
                    code.push_str(&text);
                } else {
                    self.text(&text);
                }
            }
            Event::Code(code) => {
                let saved = self.style;
                self.style = Style::default().fg(self.theme.literal).bg(self.theme.surface);
                self.text(&code);
                self.style = saved;
            }
            Event::FootnoteReference(label) => {
                let saved = self.style;
                self.style = Style::default().fg(self.theme.literal).add_modifier(Modifier::DIM);
                self.text(&format!("[{label}]"));
                self.style = saved;
            }
            Event::InlineMath(math) => {
                let saved = self.style;
                self.style = self.math_style();
                self.text(&tex_to_unicode(&math));
                self.style = saved;
            }
            Event::DisplayMath(math) => {
                self.flush();
                self.push_blank();
                let style = self.math_style();
                let indent = self.indent();
                let avail = self.width.saturating_sub(indent);
                for line in tex_to_unicode(&math).split('\n') {
                    let body = line.trim();
                    if body.is_empty() {
                        continue;
                    }
                    let (text, fits) = if body.width() <= avail {
                        (body.to_string(), true)
                    } else {
                        // A formula is never wrapped — a broken one reads as a
                        // different expression. What does not fit is cut with an
                        // ellipsis; the editor is one keypress away.
                        let (head, _) = split_at_width(body, avail.saturating_sub(1));
                        (format!("{head}…"), false)
                    };
                    let lead = if fits { (avail - text.width()) / 2 } else { 0 };
                    self.push_line(vec![
                        Seg { text: " ".repeat(indent + lead), style: Style::default(), link: None },
                        Seg { text, style, link: None },
                    ]);
                }
                self.push_blank();
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => {
                self.flush();
            }
            Event::Rule => {
                self.flush();
                self.push_blank();
                let indent = self.indent();
                self.push_line(vec![
                    Seg { text: " ".repeat(indent), style: Style::default(), link: None },
                    Seg {
                        text: "─".repeat(self.width.saturating_sub(indent).max(1)),
                        style: Style::default().fg(self.theme.faint),
                        link: None,
                    },
                ]);
                self.push_blank();
            }
            Event::TaskListMarker(done) => {
                let mark = if done { "[x] " } else { "[ ] " };
                let saved = self.style;
                self.style = Style::default().fg(if done { self.theme.ok } else { self.theme.subtext });
                self.text(mark);
                self.style = saved;
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                let trimmed = html.trim();
                if !trimmed.is_empty() && !trimmed.starts_with("<!--") {
                    let saved = self.style;
                    self.style = Style::default().fg(self.theme.faint);
                    self.text(trimmed);
                    self.style = saved;
                }
            }
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush();
                self.push_blank();
                let n = heading_level(level);
                self.heading = n;
                self.style = self.theme.heading(n);
                // No `#` marker. This is a rendered view, not a source view,
                // and echoing the syntax back is the one thing a renderer is
                // there to avoid.
                //
                // They were kept as a depth ruler, on the grounds that they
                // were the only thing separating an h4 from an h5. That was
                // defending a case the corpus does not contain: across 279
                // pages there are 939 h2s, 460 h3s, 31 h4s in four files, and
                // no h5 or h6 at all. So the ruler was drawn on 1,708 headings
                // to disambiguate a pair that never occurs — and drawn in
                // `faint`, at 2.4:1, which would not have carried the
                // distinction anyway. `Theme::heading` separates every level
                // the corpus uses by colour and weight on its own.
            }
            Tag::Paragraph => {
                self.flush();
                if !self.in_table() && !std::mem::take(&mut self.tight) {
                    self.push_blank();
                }
            }
            Tag::BlockQuote(kind) => {
                self.flush();
                self.push_blank();
                let tone = Tone::of(kind);
                self.blocks.push(Block::Quote { tone });
                if let Some(title) = tone.title() {
                    let colour = self.tone_colour(tone);
                    let quote = self.quote_depth();
                    self.push_line(vec![
                        Seg {
                            text: "▌ ".repeat(quote),
                            style: Style::default().fg(colour),
                            link: None,
                        },
                        Seg {
                            text: format!("{} {title}", tone_glyph(tone)),
                            style: Style::default().fg(colour).add_modifier(Modifier::BOLD),
                            link: None,
                        },
                    ]);
                    self.tight = true;
                }
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                self.push_blank();
                let lang = match &kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                let words: Vec<&str> = lang.split_whitespace().collect();
                let first = words.first().copied().unwrap_or("");
                if first.eq_ignore_ascii_case("mermaid") {
                    // Collected rather than printed: a mermaid block is a
                    // picture, and it can only be drawn once the whole source
                    // is in hand.
                    let mut initial = String::new();
                    if words.len() > 1 {
                        initial.push_str(&words[1..].join(" "));
                        initial.push('\n');
                    }
                    self.mermaid = Some(initial);
                } else if first.eq_ignore_ascii_case("xychart-beta") || first.eq_ignore_ascii_case("xychart") {
                    let mut initial = String::from(first);
                    initial.push('\n');
                    self.mermaid = Some(initial);
                } else {
                    // Also collected: the band is drawn to the pane width and
                    // the highlighter wants the language before the first line.
                    self.code = Some((lang, String::new(), self.src_line + 1));
                }
                self.blocks.push(Block::Code);
            }
            Tag::List(ordered) => {
                self.flush();
                if self.blocks.iter().all(|b| !matches!(b, Block::List { .. })) {
                    self.push_blank();
                }
                let index = ordered.unwrap_or(1);
                let indent = match ordered {
                    Some(start) => format!("{start}. ").width(),
                    None => 2,
                };
                self.blocks.push(Block::List { ordered, index, indent, items: 0, item_line: 0 });
            }
            Tag::Item => {
                self.flush();
                let line = self.doc.lines.len();
                let depth =
                    self.blocks.iter().filter(|b| matches!(b, Block::List { .. })).count();
                // A blank line between top-level items, but only where one of
                // them runs past a single line. A row is the smallest gap a
                // terminal has, so it is too much air between one-line items —
                // a list of short entries reads as a list already — and about
                // right between items that are themselves paragraphs.
                let wrapped = match self.blocks.last_mut() {
                    Some(Block::List { items, item_line, .. }) => {
                        *items += 1;
                        let wrapped = *items > 1 && line - *item_line > 1;
                        *item_line = line;
                        wrapped
                    }
                    _ => false,
                };
                if wrapped && depth == 1 {
                    self.push_blank();
                    if let Some(Block::List { item_line, .. }) = self.blocks.last_mut() {
                        *item_line = self.doc.lines.len();
                    }
                }
                let text = match self.blocks.last_mut() {
                    Some(Block::List { ordered: Some(_), index, .. }) => {
                        let n = *index;
                        *index += 1;
                        format!("{n}. ")
                    }
                    // Nesting is two spaces deep, which on its own is easy to
                    // lose in a wrapped paragraph; the glyph makes the level
                    // readable without counting columns.
                    _ => format!("{} ", ["•", "◦", "▪"][(depth.max(1) - 1) % 3]),
                };
                self.marker = Some((text, Style::default().fg(self.theme.accent)));
            }
            Tag::Emphasis => self.style = self.style.add_modifier(Modifier::ITALIC),
            Tag::Strong => {
                // Bold text jumps to the page's contrast pole (black on light,
                // white on dark) because on many terminals `BOLD` alone renders
                // like body text. A link keeps its own colour so emphasis
                // inside a link doesn't erase the "this is a link" cue.
                self.strong = Some(self.style);
                self.style = if self.link.is_some() {
                    self.style.add_modifier(Modifier::BOLD)
                } else {
                    let bold = self.theme.bold();
                    if self.style.add_modifier.contains(Modifier::ITALIC) {
                        bold.add_modifier(Modifier::ITALIC)
                    } else {
                        bold
                    }
                };
            }
            Tag::Strikethrough => self.style = self.style.add_modifier(Modifier::CROSSED_OUT),
            Tag::Link { dest_url, .. } => {
                let kind = classify(&dest_url);
                self.doc.links.push(DocLink {
                    target: dest_url.to_string(),
                    kind,
                    line: self.doc.lines.len(),
                });
                self.placed.push(false);
                self.link = Some(self.doc.links.len() - 1);
                self.style = self.style.fg(self.theme.link).add_modifier(Modifier::UNDERLINED);
            }
            Tag::Image { dest_url, .. } => {
                self.image = Some((dest_url.to_string(), String::new()));
            }
            Tag::Table(aligns) => {
                self.flush();
                self.push_blank();
                self.blocks.push(Block::Table { rows: Vec::new(), head: false, aligns });
            }
            Tag::TableHead => {
                // pulldown emits the header's cells directly inside TableHead,
                // with no TableRow of their own — so the row has to be opened
                // here or the header is dropped on the floor.
                if let Some(Block::Table { rows, head, .. }) = self.blocks.last_mut() {
                    *head = true;
                    rows.push(Vec::new());
                }
            }
            Tag::TableRow => {
                if let Some(Block::Table { rows, .. }) = self.blocks.last_mut() {
                    rows.push(Vec::new());
                }
            }
            Tag::TableCell => self.cell = Some(Vec::new()),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                let text: String = self.pending.iter().map(|s| s.text.as_str()).collect();
                let line = self.doc.lines.len();
                let level = std::mem::take(&mut self.heading);
                self.flush();
                // A page's h1 is its title, and a rule under it separates the
                // title from the body the way a blank line cannot.
                if level == 1 {
                    let indent = self.indent();
                    self.push_line(vec![
                        Seg { text: " ".repeat(indent), style: Style::default(), link: None },
                        Seg {
                            text: "━".repeat(self.width.saturating_sub(indent).max(1)),
                            style: Style::default().fg(self.theme.accent).add_modifier(Modifier::DIM),
                            link: None,
                        },
                    ]);
                }
                self.doc.headings.push((line, level, text));
                self.style = Style::default().fg(self.theme.text);
            }
            TagEnd::Paragraph => self.flush(),
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.blocks.pop();
                self.push_blank();
            }
            TagEnd::CodeBlock => {
                self.blocks.pop();
                if let Some(code) = self.mermaid.take() {
                    self.draw_mermaid(&code);
                } else if let Some((lang, code, from)) = self.code.take() {
                    self.draw_code(&lang, &code, from);
                }
                self.push_blank();
            }
            TagEnd::List(_) => {
                self.flush();
                self.blocks.pop();
                self.marker = None;
            }
            TagEnd::Item => {
                self.flush();
                self.marker = None;
            }
            TagEnd::Emphasis => self.style = self.style.remove_modifier(Modifier::ITALIC),
            TagEnd::Strong => {
                if let Some(saved) = self.strong.take() {
                    self.style = saved;
                }
            }
            TagEnd::Strikethrough => self.style = self.style.remove_modifier(Modifier::CROSSED_OUT),
            TagEnd::Link => {
                self.link = None;
                self.style = Style::default().fg(self.theme.text);
            }
            TagEnd::Image => {
                if let Some((dest, alt)) = self.image.take() {
                    let label = if alt.trim().is_empty() { dest } else { alt };
                    let saved = self.style;
                    self.style = Style::default().fg(self.theme.subtext).add_modifier(Modifier::ITALIC);
                    self.text(&format!("🖼 {label}"));
                    self.style = saved;
                }
            }
            TagEnd::TableCell => {
                // Through the same path as prose, so a citation in a cell is
                // numbered in document order alongside the rest.
                let raw = self.cell.take().unwrap_or_default();
                let cell = trim_segs(self.expand_footnotes(raw));
                if let Some(Block::Table { rows, .. }) = self.blocks.last_mut() {
                    if let Some(row) = rows.last_mut() {
                        row.push(cell);
                    }
                }
            }
            TagEnd::TableHead => {
                if let Some(Block::Table { head, .. }) = self.blocks.last_mut() {
                    *head = false;
                }
            }
            TagEnd::Table => {
                if let Some(Block::Table { rows, aligns, .. }) = self.blocks.pop() {
                    self.render_table(&rows, &aligns);
                }
            }
            _ => {}
        }
    }

    /// Draw a fenced code block as a banded slab: a tinted background the full
    /// width of the block, a coloured gutter down its left edge, and the
    /// language as a chip on the first row. The band is what separates code
    /// from prose at a glance — tinting only the glyphs leaves a ragged edge
    /// that reads as damage rather than as structure.
    fn draw_code(&mut self, lang: &str, code: &str, from: usize) {
        let indent = self.indent();
        let band = self.width.saturating_sub(indent).max(8);
        let surface = Style::default().bg(self.theme.surface);
        let mut hl = crate::ui::highlight::Highlighter::for_code(lang, code);

        let lines: Vec<&str> = code.trim_end_matches('\n').lines().collect();
        let total_lines = lines.len().max(1);
        let num_width = total_lines.to_string().len().max(2);
        // gutter: "▎" (1) + " " (1) + num (num_width) + " " (1) + "│" (1) + " " (1) = num_width + 5
        let gutter_w = num_width + 5;
        // Gutter plus at least 1 trailing space of breathing room
        let avail = band.saturating_sub(gutter_w + 1).max(1);

        let row = |this: &mut Self, line_num: Option<usize>, body: Vec<Seg>| {
            let mut segs = Vec::new();
            if indent > 0 {
                segs.push(Seg { text: " ".repeat(indent), style: Style::default(), link: None });
            }
            segs.push(Seg {
                text: "▎".to_string(),
                style: surface.fg(this.theme.accent),
                link: None,
            });
            segs.push(Seg { text: " ".to_string(), style: surface, link: None });
            let num_text = match line_num {
                Some(n) => format!("{:>width$}", n, width = num_width),
                None => " ".repeat(num_width),
            };
            segs.push(Seg {
                text: num_text,
                style: surface.fg(this.theme.faint),
                link: None,
            });
            segs.push(Seg { text: " ".to_string(), style: surface, link: None });
            segs.push(Seg {
                text: "│".to_string(),
                style: surface.fg(this.theme.faint),
                link: None,
            });
            segs.push(Seg { text: " ".to_string(), style: surface, link: None });

            let used: usize = body.iter().map(|s| s.text.width()).sum();
            segs.extend(body);
            segs.push(Seg {
                text: " ".repeat(band.saturating_sub(used + gutter_w)),
                style: surface,
                link: None,
            });
            this.push_line(segs);
        };

        let start = self.src_line;
        if lines.is_empty() {
            row(self, Some(1), Vec::new());
        }
        for (n, line) in lines.iter().enumerate() {
            // Source lines map one to one here, unlike prose, so the editor
            // lands on the line you were looking at inside the block.
            self.src_line = from + n;
            let expanded = line.replace('\t', "    ");
            if expanded.is_empty() {
                row(self, Some(n + 1), Vec::new());
                continue;
            }
            let mut used = 0usize;
            let mut body = Vec::new();
            let mut first_row = true;
            for (text, tok) in hl.line(&expanded) {
                let style = surface.patch(self.token_style(tok));
                let mut remaining = text;
                while !remaining.is_empty() {
                    let room = avail.saturating_sub(used);
                    if room == 0 {
                        let l_num = if first_row { Some(n + 1) } else { None };
                        first_row = false;
                        row(self, l_num, std::mem::take(&mut body));
                        used = 0;
                        continue;
                    }
                    let w = remaining.width();
                    if w <= room {
                        used += w;
                        body.push(Seg { text: remaining, style, link: None });
                        break;
                    }
                    let (head, tail) = split_at_width(&remaining, room);
                    if head.is_empty() {
                        if used > 0 {
                            let l_num = if first_row { Some(n + 1) } else { None };
                            first_row = false;
                            row(self, l_num, std::mem::take(&mut body));
                            used = 0;
                            continue;
                        }
                        let mut chars = remaining.chars();
                        let head = chars.next().map(String::from).unwrap_or_default();
                        let tail: String = chars.collect();
                        let head_w = head.width();
                        used += head_w;
                        body.push(Seg { text: head, style, link: None });
                        remaining = tail;
                    } else {
                        let head_w = head.width();
                        used += head_w;
                        body.push(Seg { text: head, style, link: None });
                        remaining = tail;
                    }
                }
            }
            if !body.is_empty() {
                let l_num = if first_row { Some(n + 1) } else { None };
                row(self, l_num, body);
            }
        }
        self.src_line = start;
    }

    fn token_style(&self, tok: crate::ui::highlight::Tok) -> Style {
        crate::ui::highlight::token_style(self.theme, tok)
    }

    /// Draw a mermaid block, falling back to its source when the kind is not
    /// one we can draw — a wrong picture is worse than legible source.
    fn draw_mermaid(&mut self, code: &str) {
        let indent = self.indent();
        let available = self.width.saturating_sub(indent);
        let Some(rows) = super::mermaid::render(code, available) else {
            self.push_line(vec![Seg {
                text: format!("{}mermaid", " ".repeat(indent)),
                style: Style::default().fg(self.theme.faint).add_modifier(Modifier::ITALIC),
                link: None,
            }]);
            for line in code.lines() {
                self.push_line(vec![Seg {
                    text: format!("{}{line}", " ".repeat(indent)),
                    style: Style::default().fg(self.theme.literal).bg(self.theme.surface),
                    link: None,
                }]);
            }
            return;
        };

        for row in rows {
            // A diagram is never wrapped — a broken box is unreadable — so an
            // over-long row (a label in the outline fallback) is trimmed.
            let mut used = indent;
            let mut segs: Vec<Seg> = vec![Seg {
                text: " ".repeat(indent),
                style: Style::default(),
                link: None,
            }];
            for (text, ink) in row {
                let style = self.ink(ink);
                if used + text.width() <= self.width {
                    used += text.width();
                    segs.push(Seg { text, style, link: None });
                    continue;
                }
                let room = self.width.saturating_sub(used + 1);
                if room > 0 {
                    let cut: String = text.chars().take(room).collect();
                    segs.push(Seg { text: format!("{cut}…"), style, link: None });
                }
                break;
            }
            self.push_line(segs);
        }
    }

    fn ink(&self, ink: super::mermaid::Ink) -> Style {
        use super::mermaid::Ink;
        match ink {
            Ink::Frame => Style::default().fg(self.theme.faint),
            Ink::Node => Style::default().fg(self.theme.text).add_modifier(Modifier::BOLD),
            Ink::Edge => Style::default().fg(self.theme.accent),
            Ink::Label => Style::default().fg(self.theme.literal),
            Ink::Title => Style::default().fg(self.theme.subtext).add_modifier(Modifier::ITALIC),
        }
    }

}

fn tone_glyph(tone: Tone) -> &'static str {
    match tone {
        Tone::Note => "ℹ",
        Tone::Tip => "💡",
        Tone::Important => "◆",
        Tone::Warning => "⚠",
        Tone::Caution => "⛔",
        Tone::Plain => "▌",
    }
}


fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn classify(target: &str) -> LinkKind {
    if target.starts_with('#') {
        LinkKind::Anchor
    } else if target.contains("://") || target.starts_with("mailto:") || target.starts_with("gdrive:") {
        LinkKind::External
    } else {
        LinkKind::Relative
    }
}

fn split_footnotes(text: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[^") {
        if let Some(len) = rest[start + 2..].find(']') {
            let label = &rest[start + 2..start + 2 + len];
            if !label.is_empty() && !label.contains('[') {
                if start > 0 {
                    out.push((rest[..start].to_string(), None));
                }
                out.push((label.to_string(), Some(label.to_string())));
                rest = &rest[start + 2 + len + 1..];
                continue;
            }
        }
        out.push((rest[..start + 2].to_string(), None));
        rest = &rest[start + 2..];
    }
    if !rest.is_empty() {
        out.push((rest.to_string(), None));
    }
    out
}

/// Wrap a styled run to `avail` columns, breaking only at whitespace and
/// dropping the space a break falls on.
///
/// Styling boundaries are not break opportunities: `varies[1].` arrives as
/// three segments — prose, citation, full stop — and breaking between them
/// would leave a line starting with `[1].`. Adjacent non-space segments are
/// therefore welded into one token and move together.
///
/// A token wider than the whole line — a bare URL, a long identifier — is cut
/// across lines rather than allowed to run past the pane, because the reader
/// clips at the viewport edge and everything past it would simply be gone.
fn wrap(segs: Vec<Seg>, avail: usize) -> Vec<Vec<Seg>> {
    let mut out: Vec<Vec<Seg>> = vec![Vec::new()];
    let mut used = 0usize;
    let newline = |out: &mut Vec<Vec<Seg>>, used: &mut usize| {
        out.push(Vec::new());
        *used = 0;
    };

    for token in tokenize(segs) {
        let width: usize = token.pieces.iter().map(|p| p.text.width()).sum();
        if token.space {
            if used == 0 {
                continue;
            }
            if used + width > avail {
                newline(&mut out, &mut used);
                continue;
            }
            out.last_mut().unwrap().extend(token.pieces);
            used += width;
            continue;
        }

        if used + width > avail && used > 0 {
            newline(&mut out, &mut used);
        }
        if width <= avail {
            out.last_mut().unwrap().extend(token.pieces);
            used += width;
            continue;
        }
        // Walked by byte index rather than by splitting off a fresh tail per
        // line: the tail was re-collected into a new String on every wrap, so a
        // long unbroken token — a base64 blob, a data URI, everything after a
        // stray quote in a CSV — cost O(n²) and could freeze the reader for
        // tens of seconds.
        for piece in token.pieces {
            let text = piece.text;
            let mut start = 0usize;
            let mut chunk = 0usize;
            let emit = |out: &mut Vec<Vec<Seg>>, range: std::ops::Range<usize>| {
                out.last_mut().unwrap().push(Seg {
                    text: text[range].to_string(),
                    style: piece.style,
                    link: piece.link,
                });
            };
            for (i, ch) in text.char_indices() {
                let w = UnicodeWidthChar::width(ch).unwrap_or(0);
                // `chunk > 0` is what keeps a single character wider than the
                // whole line from spinning: it is taken anyway.
                if chunk > 0 && used + chunk + w > avail {
                    emit(&mut out, start..i);
                    start = i;
                    chunk = 0;
                    newline(&mut out, &mut used);
                }
                chunk += w;
            }
            if start < text.len() {
                emit(&mut out, start..text.len());
                used += chunk;
            }
        }
    }
    out
}

/// A run that moves as a unit when wrapping: either one whitespace run or one
/// word, the word carrying however many styled pieces it was written with.
struct Token {
    pieces: Vec<Seg>,
    space: bool,
}

fn tokenize(segs: Vec<Seg>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    for seg in segs {
        for part in split_keeping_spaces(&seg.text) {
            let space = part.chars().all(char::is_whitespace);
            let piece = Seg { text: part, style: seg.style, link: seg.link };
            match out.last_mut() {
                Some(last) if !space && !last.space => last.pieces.push(piece),
                _ => out.push(Token { pieces: vec![piece], space }),
            }
        }
    }
    out
}

/// Split a string at a display width, never inside a character.
fn split_at_width(text: &str, width: usize) -> (String, String) {
    let mut head = String::new();
    let mut used = 0usize;
    let mut chars = text.chars().peekable();
    while let Some(&ch) = chars.peek() {
        let w = ch.to_string().width();
        if used + w > width {
            break;
        }
        used += w;
        head.push(ch);
        chars.next();
    }
    (head, chars.collect())
}

/// Split into words and the whitespace runs between them, so wrapping can drop
/// a space at a line break without gluing words together.
fn split_keeping_spaces(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_space = false;
    for ch in text.chars() {
        let is_space = ch.is_whitespace();
        if !current.is_empty() && is_space != in_space {
            out.push(std::mem::take(&mut current));
        }
        in_space = is_space;
        current.push(ch);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

