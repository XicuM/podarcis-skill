//! Word-wrap for pre-styled lines, used by the reader and the inspector.

use super::*;

/// Wrap already-styled lines to `width` columns, one output `Line` per screen
/// row, breaking at whitespace where possible. Mirrors what
/// `Paragraph::wrap` would draw, except the caller gets the row count back
/// instead of it being a rendering-time secret.
pub fn wrap_lines(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    if width == 0 {
        return lines;
    }
    lines.into_iter().flat_map(|line| wrap_line(line, width)).collect()
}

pub(super) fn wrap_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    use unicode_width::UnicodeWidthChar;

    let chars: Vec<(char, Style)> = line
        .spans
        .iter()
        .flat_map(|span| span.content.chars().map(move |c| (c, span.style)))
        .collect();
    if chars.is_empty() {
        return vec![Line::default()];
    }

    let mut rows: Vec<&[(char, Style)]> = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let mut col = 0usize;
        let mut end = start;
        let mut last_space = None;
        while end < chars.len() {
            let w = chars[end].0.width().unwrap_or(1);
            if col + w > width {
                break;
            }
            if chars[end].0 == ' ' {
                last_space = Some(end);
            }
            col += w;
            end += 1;
        }
        if end == chars.len() {
            rows.push(&chars[start..end]);
            break;
        }
        // Break at the last space in this row when there is one, so a break
        // never falls mid-word; otherwise hard-break (a token wider than the
        // whole row, e.g. a long id).
        let break_at = last_space.filter(|&pos| pos >= start).unwrap_or(end.max(start + 1));
        rows.push(&chars[start..break_at]);
        start = if chars.get(break_at).is_some_and(|(c, _)| *c == ' ') { break_at + 1 } else { break_at };
        while start < chars.len() && chars[start].0 == ' ' {
            start += 1;
        }
    }

    rows.into_iter()
        .map(|row| {
            let end = row.iter().rposition(|(c, _)| *c != ' ').map_or(0, |i| i + 1);
            let mut spans: Vec<Span<'static>> = Vec::new();
            for &(c, style) in &row[..end] {
                match spans.last_mut() {
                    Some(last) if last.style == style => last.content.to_mut().push(c),
                    _ => spans.push(Span::styled(c.to_string(), style)),
                }
            }
            Line::from(spans)
        })
        .collect()
}

/// Wrap a bibliography row so its continuation lines hang under the citation
/// text instead of under its `[1]` mark. The row's first span *is* the mark
/// (with its trailing space or as a separate span), so everything after it wraps
/// into `width - mark` columns and every continuation line opens with a spacer that wide.
pub(super) fn wrap_biblio(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    use unicode_width::UnicodeWidthStr;
    let Some(mark) = line.spans.first().cloned() else {
        return wrap_line(line, width);
    };
    let (prefix, hang, remove_count) = if mark.content.ends_with(' ') {
        (vec![mark.clone()], mark.content.as_ref().width(), 1)
    } else if let Some(space) = line.spans.get(1) {
        if space.content.chars().all(|c| c.is_whitespace()) {
            let hang = mark.content.as_ref().width() + space.content.as_ref().width();
            (vec![mark.clone(), space.clone()], hang, 2)
        } else {
            (vec![mark.clone()], mark.content.as_ref().width(), 1)
        }
    } else {
        (vec![mark.clone()], mark.content.as_ref().width(), 1)
    };

    if width <= hang {
        return wrap_line(line, width);
    }
    let mut rest = line;
    for _ in 0..remove_count {
        rest.spans.remove(0);
    }
    let rest_rows = wrap_line(rest, width - hang);
    if rest_rows.len() == 1 && rest_rows[0].spans.is_empty() {
        return vec![Line::from(prefix)];
    }
    let spacer = Span::raw(" ".repeat(hang));
    rest_rows
        .into_iter()
        .enumerate()
        .map(|(i, row)| {
            let mut spans = Vec::with_capacity(row.spans.len() + prefix.len());
            if i == 0 {
                spans.extend(prefix.clone());
            } else {
                spans.push(spacer.clone());
            }
            spans.extend(row.spans);
            Line::from(spans)
        })
        .collect()
}
