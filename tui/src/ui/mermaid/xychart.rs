//! xychart-beta: bar and line series drawn against numeric axes.

use super::*;

// ── xychart ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SeriesKind {
    Bar,
    Line,
}

#[derive(Clone, Debug)]
struct Series {
    kind: SeriesKind,
    title: Option<String>,
    values: Vec<f64>,
}

#[derive(Clone, Debug, Default)]
struct Axis {
    title: Option<String>,
    categories: Vec<String>,
    min: Option<f64>,
    max: Option<f64>,
}

#[derive(Clone, Debug, Default)]
struct Chart {
    title: Option<String>,
    horizontal: bool,
    x_axis: Axis,
    y_axis: Axis,
    series: Vec<Series>,
}

fn strip_quotes(s: &str) -> String {
    let t = s.trim();
    if (t.starts_with('"') && t.ends_with('"') && t.len() >= 2)
        || (t.starts_with('\'') && t.ends_with('\'') && t.len() >= 2)
    {
        t[1..t.len() - 1].trim().to_string()
    } else {
        t.to_string()
    }
}

fn parse_string_array(s: &str) -> Vec<String> {
    let trimmed = s.trim();
    let inner = match (trimmed.find('['), trimmed.rfind(']')) {
        (Some(start), Some(end)) if end > start => &trimmed[start + 1..end],
        _ => trimmed,
    };
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '"';

    for ch in inner.chars() {
        match ch {
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = ch;
            }
            c if in_quotes && c == quote_char => {
                in_quotes = false;
            }
            ',' if !in_quotes => {
                let item = strip_quotes(&current);
                if !item.is_empty() {
                    items.push(item);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    let item = strip_quotes(&current);
    if !item.is_empty() {
        items.push(item);
    }
    items
}

fn parse_number_array(s: &str) -> Vec<f64> {
    let trimmed = s.trim();
    let inner = match (trimmed.find('['), trimmed.rfind(']')) {
        (Some(start), Some(end)) if end > start => &trimmed[start + 1..end],
        _ => trimmed,
    };
    inner
        .split([',', ';'])
        .flat_map(|part| part.split_whitespace())
        .filter_map(|part| {
            let clean = part.trim_matches(|c: char| !c.is_numeric() && c != '.' && c != '-');
            clean.parse::<f64>().ok()
        })
        .collect()
}

fn parse_axis_clause(text: &str) -> Axis {
    let trimmed = text.trim();
    let mut axis = Axis::default();

    if let Some(bracket_start) = trimmed.find('[') {
        let before = trimmed[..bracket_start].trim();
        if !before.is_empty() {
            axis.title = Some(strip_quotes(before));
        }
        axis.categories = parse_string_array(&trimmed[bracket_start..]);
        return axis;
    }

    if let Some((left, right)) = trimmed.split_once("-->") {
        let left_trimmed = left.trim();
        if let Some(quote_start) = left_trimmed.find(['"', '\'']) {
            let quote_char = left_trimmed[quote_start..].chars().next().unwrap();
            if let Some(quote_end) = left_trimmed[quote_start + 1..].find(quote_char) {
                axis.title = Some(left_trimmed[quote_start + 1..quote_start + 1 + quote_end].to_string());
                let rest_min = left_trimmed[quote_start + 1 + quote_end + 1..].trim();
                axis.min = rest_min.parse::<f64>().ok();
            } else {
                axis.min = left_trimmed.parse::<f64>().ok();
            }
        } else {
            let words: Vec<&str> = left_trimmed.split_whitespace().collect();
            if words.len() > 1 {
                axis.min = words.last().and_then(|w| w.parse::<f64>().ok());
                axis.title = Some(words[..words.len() - 1].join(" "));
            } else if words.len() == 1 {
                axis.min = words[0].parse::<f64>().ok();
            }
        }
        axis.max = right.trim().split_whitespace().next().and_then(|w| w.parse::<f64>().ok());
        return axis;
    }

    if !trimmed.is_empty() {
        axis.title = Some(strip_quotes(trimmed));
    }
    axis
}

fn parse_series_clause(kind: SeriesKind, text: &str) -> Series {
    let trimmed = text.trim();
    let (title, array_part) = if let Some(bracket_start) = trimmed.find('[') {
        let before = trimmed[..bracket_start].trim();
        let title = if !before.is_empty() {
            Some(strip_quotes(before))
        } else {
            None
        };
        (title, &trimmed[bracket_start..])
    } else {
        (None, trimmed)
    };
    let values = parse_number_array(array_part);
    Series { kind, title, values }
}

fn parse_xychart(code: &str) -> Option<Chart> {
    let mut chart = Chart::default();
    let mut found_header = false;

    for raw_line in code.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("xychart-beta") || lower.starts_with("xychart") {
            found_header = true;
            if lower.contains("horizontal") {
                chart.horizontal = true;
            }
            continue;
        }

        if lower.starts_with("title") {
            let rest = line["title".len()..].trim().trim_start_matches(':').trim();
            chart.title = Some(strip_quotes(rest));
        } else if lower.starts_with("x-axis") || lower.starts_with("xaxis") {
            let prefix_len = if lower.starts_with("x-axis") { 6 } else { 5 };
            let rest = line[prefix_len..].trim().trim_start_matches(':').trim();
            chart.x_axis = parse_axis_clause(rest);
        } else if lower.starts_with("y-axis") || lower.starts_with("yaxis") {
            let prefix_len = if lower.starts_with("y-axis") { 6 } else { 5 };
            let rest = line[prefix_len..].trim().trim_start_matches(':').trim();
            chart.y_axis = parse_axis_clause(rest);
        } else if lower.starts_with("bar") {
            let rest = line[3..].trim();
            chart.series.push(parse_series_clause(SeriesKind::Bar, rest));
        } else if lower.starts_with("line") {
            let rest = line[4..].trim();
            chart.series.push(parse_series_clause(SeriesKind::Line, rest));
        } else if lower.contains("horizontal") {
            chart.horizontal = true;
        }
    }

    if !found_header && chart.series.is_empty() && chart.x_axis.categories.is_empty() {
        return None;
    }
    Some(chart)
}

fn fmt_num(val: f64) -> String {
    if (val - val.round()).abs() < 1e-6 {
        format!("{:.0}", val.round())
    } else if (val * 10.0 - (val * 10.0).round()).abs() < 1e-4 {
        format!("{:.1}", val)
    } else {
        format!("{:.2}", val)
    }
}

const VERT_BLOCKS: [char; 9] = [' ', ' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const HORIZ_BLOCKS: [char; 9] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

pub(super) fn xychart(code: &str, width: usize) -> Option<Vec<Row>> {
    let chart = parse_xychart(code)?;

    let (categories, val_axis) = if !chart.y_axis.categories.is_empty() && chart.x_axis.categories.is_empty() {
        (&chart.y_axis.categories, &chart.x_axis)
    } else {
        (&chart.x_axis.categories, &chart.y_axis)
    };

    let all_values: Vec<f64> = chart.series.iter().flat_map(|s| s.values.iter().copied()).collect();
    if all_values.is_empty() && categories.is_empty() {
        return None;
    }

    let data_min = all_values.iter().copied().fold(f64::INFINITY, f64::min);
    let data_max = all_values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (data_min, data_max) = if all_values.is_empty() { (0.0, 1.0) } else { (data_min, data_max) };

    let y_min = val_axis.min.unwrap_or_else(|| if data_min > 0.0 { 0.0 } else { data_min });
    let y_max = val_axis.max.unwrap_or_else(|| if data_max <= y_min { y_min + 1.0 } else { data_max });
    let y_range = if (y_max - y_min).abs() < 1e-9 { 1.0 } else { y_max - y_min };

    let max_series_len = chart.series.iter().map(|s| s.values.len()).max().unwrap_or(0);
    let n = categories.len().max(max_series_len).max(1);

    let s_max = fmt_num(y_max);
    let s_mid = fmt_num((y_max + y_min) / 2.0);
    let s_min = fmt_num(y_min);
    let y_label_w = s_max.width().max(s_mid.width()).max(s_min.width()).max(1);
    let axis_col = y_label_w + 1;

    let available_plot_w = width.saturating_sub(axis_col + 2);
    let col_step = available_plot_w / n;

    if chart.horizontal || col_step < 3 || width < 28 {
        Some(xychart_horizontal(&chart, categories, y_min, y_max, y_range, n, width))
    } else {
        Some(xychart_vertical(
            &chart,
            categories,
            y_min,
            y_max,
            y_range,
            n,
            width,
            axis_col,
            col_step,
            &s_max,
            &s_mid,
            &s_min,
        ))
    }
}

fn xychart_vertical(
    chart: &Chart,
    categories: &[String],
    y_min: f64,
    y_max: f64,
    y_range: f64,
    n: usize,
    width: usize,
    axis_col: usize,
    col_step: usize,
    s_max: &str,
    s_mid: &str,
    s_min: &str,
) -> Vec<Row> {
    let mut canvas = Canvas::new(width);
    let mut current_row = 0;

    let mut title_text = String::from("xychart");
    if let Some(t) = &chart.title {
        title_text.push_str(" · ");
        title_text.push_str(t);
    }
    if let Some(yt) = chart.y_axis.title.as_deref().or(chart.x_axis.title.as_deref()) {
        title_text.push_str(" (");
        title_text.push_str(yt);
        title_text.push(')');
    }
    canvas.write_str(current_row, 0, &title_text, Ink::Title);
    current_row += 2;

    let plot_top = current_row;
    let plot_height = 8usize;
    let plot_bottom = plot_top + plot_height - 1;
    let baseline_row = plot_bottom + 1;

    let mid_row = plot_top + plot_height / 2;
    for r in plot_top..=plot_bottom {
        if r == plot_top {
            let start = axis_col.saturating_sub(s_max.width() + 1);
            canvas.write_str(r, start, s_max, Ink::Label);
            canvas.put(r, axis_col, '┤', Ink::Frame);
        } else if r == mid_row {
            let start = axis_col.saturating_sub(s_mid.width() + 1);
            canvas.write_str(r, start, s_mid, Ink::Label);
            canvas.put(r, axis_col, '┤', Ink::Frame);
        } else if r == plot_bottom {
            let start = axis_col.saturating_sub(s_min.width() + 1);
            canvas.write_str(r, start, s_min, Ink::Label);
            canvas.put(r, axis_col, '┤', Ink::Frame);
        } else {
            canvas.put(r, axis_col, '│', Ink::Frame);
        }
    }

    canvas.put(baseline_row, axis_col, '└', Ink::Frame);
    let chart_end = (axis_col + 1 + n * col_step).min(width.saturating_sub(1));
    for c in axis_col + 1..chart_end {
        canvas.put(baseline_row, c, '─', Ink::Frame);
    }

    let bar_w = (col_step.saturating_sub(1)).clamp(1, 4);

    for i in 0..n {
        let center_col = axis_col + 1 + i * col_step + col_step / 2;
        if center_col >= width {
            break;
        }
        canvas.put(baseline_row, center_col, '┴', Ink::Frame);

        for s in &chart.series {
            if s.kind == SeriesKind::Bar {
                let val = s.values.get(i).copied().unwrap_or(0.0);
                let val_clamped = val.clamp(y_min, y_max);
                let norm = (val_clamped - y_min) / y_range;
                let total_eighths = (norm * (plot_height * 8) as f64).round() as usize;

                let bar_start = center_col.saturating_sub(bar_w / 2);
                for h in 0..plot_height {
                    let r = plot_bottom - h;
                    let eighths = total_eighths.saturating_sub(h * 8).min(8);
                    if eighths > 0 {
                        let ch = VERT_BLOCKS[eighths];
                        for bw in 0..bar_w {
                            if bar_start + bw < width {
                                canvas.put(r, bar_start + bw, ch, Ink::Node);
                            }
                        }
                    }
                }

                let top_h = (total_eighths + 7) / 8;
                let val_str = fmt_num(val);
                let val_len = val_str.width();
                if val_len <= col_step + 1 {
                    let val_col = center_col.saturating_sub(val_len / 2);
                    let val_row = if top_h < plot_height {
                        plot_bottom.saturating_sub(top_h)
                    } else {
                        plot_top
                    };
                    if val_row < baseline_row {
                        canvas.write_str(val_row, val_col, &val_str, Ink::Label);
                    }
                }
            }
        }
    }

    for s in &chart.series {
        if s.kind == SeriesKind::Line {
            let mut prev_pt: Option<(usize, usize)> = None;
            for i in 0..n {
                let center_col = axis_col + 1 + i * col_step + col_step / 2;
                if center_col >= width {
                    break;
                }
                let val = s.values.get(i).copied().unwrap_or(0.0);
                let val_clamped = val.clamp(y_min, y_max);
                let norm = (val_clamped - y_min) / y_range;
                let level = (norm * (plot_height - 1) as f64).round() as usize;
                let r = plot_bottom - level;

                if let Some((px, py)) = prev_pt {
                    let (min_x, max_x) = (px.min(center_col), px.max(center_col));
                    let mid_x = (min_x + max_x) / 2;
                    for x in min_x + 1..=mid_x {
                        canvas.connect(py, x, '─');
                    }
                    let (min_y, max_y) = (py.min(r), py.max(r));
                    for y in min_y..=max_y {
                        canvas.connect(y, mid_x, '│');
                    }
                    for x in mid_x..max_x {
                        canvas.connect(r, x, '─');
                    }
                }
                canvas.put(r, center_col, '●', Ink::Edge);
                prev_pt = Some((center_col, r));
            }
        }
    }

    let label_row_1 = baseline_row + 1;
    let label_row_2 = baseline_row + 2;
    let mut has_second_line = false;

    for i in 0..n {
        let center_col = axis_col + 1 + i * col_step + col_step / 2;
        if center_col >= width {
            break;
        }
        let cat = categories.get(i).map(|s| s.as_str()).unwrap_or("");
        if cat.width() <= col_step {
            let start = center_col.saturating_sub(cat.width() / 2);
            canvas.write_str(label_row_1, start, cat, Ink::Label);
        } else {
            let (l1, l2) = if let Some(space_idx) = cat.rfind(' ') {
                (&cat[..space_idx], &cat[space_idx + 1..])
            } else if let Some(hyphen_idx) = cat.find('-') {
                (&cat[..=hyphen_idx], &cat[hyphen_idx + 1..])
            } else {
                (cat, "")
            };
            let start1 = center_col.saturating_sub(l1.width().min(col_step) / 2);
            canvas.write_str(label_row_1, start1, l1, Ink::Label);
            if !l2.is_empty() {
                has_second_line = true;
                let start2 = center_col.saturating_sub(l2.width().min(col_step) / 2);
                canvas.write_str(label_row_2, start2, l2, Ink::Label);
            }
        }
    }

    let show_legend = chart.series.len() > 1 || chart.series.iter().any(|s| s.title.is_some());
    if show_legend {
        let leg_row = if has_second_line { label_row_2 + 2 } else { label_row_1 + 2 };
        let mut col = axis_col + 1;
        for s in &chart.series {
            let (sym, ink) = match s.kind {
                SeriesKind::Bar => ("■ ", Ink::Node),
                SeriesKind::Line => ("● ", Ink::Edge),
            };
            let title = s.title.as_deref().unwrap_or(match s.kind {
                SeriesKind::Bar => "Bar",
                SeriesKind::Line => "Line",
            });
            let entry = format!("{sym}{title}");
            if col + entry.width() < width {
                canvas.write_str(leg_row, col, &entry, ink);
                col += entry.width() + 3;
            }
        }
    }

    canvas.rows()
}

fn xychart_horizontal(
    chart: &Chart,
    categories: &[String],
    y_min: f64,
    y_max: f64,
    y_range: f64,
    n: usize,
    width: usize,
) -> Vec<Row> {
    let mut canvas = Canvas::new(width);
    let mut current_row = 0;

    let mut title_text = String::from("xychart");
    if let Some(t) = &chart.title {
        title_text.push_str(" · ");
        title_text.push_str(t);
    }
    if let Some(yt) = chart.y_axis.title.as_deref().or(chart.x_axis.title.as_deref()) {
        title_text.push_str(" (");
        title_text.push_str(yt);
        title_text.push(')');
    }
    canvas.write_str(current_row, 0, &title_text, Ink::Title);
    current_row += 2;

    let cat_col_w = categories
        .iter()
        .map(|c| c.width())
        .max()
        .unwrap_or(0)
        .clamp(2, 18);
    let axis_col = cat_col_w + 1;
    let bar_area_w = width.saturating_sub(axis_col + 10).max(6);

    for i in 0..n {
        let row = current_row + i;
        let cat = categories.get(i).map(|s| s.as_str()).unwrap_or("");
        let cat_trimmed: String = cat.chars().take(cat_col_w).collect();
        let start = axis_col.saturating_sub(cat_trimmed.width() + 1);
        canvas.write_str(row, start, &cat_trimmed, Ink::Label);
        canvas.put(row, axis_col, '│', Ink::Frame);

        if let Some(s) = chart.series.first() {
            let val = s.values.get(i).copied().unwrap_or(0.0);
            let val_clamped = val.clamp(y_min, y_max);
            let norm = (val_clamped - y_min) / y_range;
            let total_eighths = (norm * (bar_area_w * 8) as f64).round() as usize;

            let full_blocks = total_eighths / 8;
            let rem = total_eighths % 8;

            for b in 0..full_blocks {
                if axis_col + 1 + b < width {
                    canvas.put(row, axis_col + 1 + b, '█', Ink::Node);
                }
            }
            if rem > 0 && axis_col + 1 + full_blocks < width {
                canvas.put(row, axis_col + 1 + full_blocks, HORIZ_BLOCKS[rem], Ink::Node);
            }

            let bar_len = full_blocks + if rem > 0 { 1 } else { 0 };
            let val_str = format!(" {}", fmt_num(val));
            if axis_col + 1 + bar_len + val_str.width() < width {
                canvas.write_str(row, axis_col + 1 + bar_len, &val_str, Ink::Label);
            }
        }
    }

    let baseline_row = current_row + n;
    canvas.put(baseline_row, axis_col, '└', Ink::Frame);
    let chart_end = (axis_col + 1 + bar_area_w).min(width.saturating_sub(1));
    for c in axis_col + 1..chart_end {
        canvas.put(baseline_row, c, '─', Ink::Frame);
    }
    if chart_end < width {
        canvas.put(baseline_row, chart_end, '┘', Ink::Frame);
    }

    let ticks_row = baseline_row + 1;
    let s_min = fmt_num(y_min);
    let s_max = fmt_num(y_max);
    canvas.write_str(ticks_row, axis_col, &s_min, Ink::Label);
    if chart_end >= s_max.width() {
        canvas.write_str(ticks_row, chart_end.saturating_sub(s_max.width()), &s_max, Ink::Label);
    }

    canvas.rows()
}
