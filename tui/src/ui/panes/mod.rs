//! The three panes and the status line.

use edtui::{EditorStatusLine, EditorTheme, EditorView, LineNumbers};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use tui_term::widget::PseudoTerminal;

use crate::app::{App, DocStop, Focus, InspectorBacklink, Open, TreePane};
use crate::theme::Theme;
use crate::ui::markdown::{Finds, Overlays};
use crate::vault::git::GitStatus;
use crate::vault::index::Severity;
use unicode_width::UnicodeWidthStr;

mod chrome;
mod document;
mod inspector;
mod tree;
mod wrap;
#[cfg(test)]
mod tests;

pub use chrome::{sidebar, status, tab_bar, toggles};
pub use document::document;
pub use inspector::inspector_lines;
pub use tree::{collection_rows, tree, FOLDED_ROWS, MIN_COLLECTION_ROWS};
pub use wrap::wrap_lines;

/// A block in the app's chrome: rounded, titled, dim unless focused.
fn pane(theme: &Theme, title: &str, focused: bool) -> Block<'static> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme.border(focused))
        .style(Style::default().bg(theme.bg));
    if title.is_empty() {
        block
    } else {
        block.title(Span::styled(format!(" {title} "), theme.title(focused)))
    }
}

fn scroll_offset(selected: usize, total: usize, height: usize) -> usize {
    if total <= height {
        return 0;
    }
    // Keep the cursor a third of the way down where possible: context above,
    // room to read below.
    selected.saturating_sub(height / 3).min(total - height)
}

/// Clip to `width` cells, marking the cut with an ellipsis.
fn truncate(text: &str, width: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if text.width() <= width {
        return text.to_string();
    }
    // One cell is reserved for the ellipsis, so the result is never wider than
    // asked for — the caller has already spent the rest of the row.
    let room = width.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if used + w > room {
            break;
        }
        used += w;
        out.push(ch);
    }
    out.push('…');
    out
}

fn scrollbar(frame: &mut Frame, app: &App, area: Rect, scroll: usize, total: usize, height: usize) {
    if total <= height || area.height < 4 || area.width < 2 {
        return;
    }
    let track = area.height.saturating_sub(2) as usize;
    let span = ((height * track) / total).max(1);
    let top = ((scroll * track) / total).min(track.saturating_sub(span));
    let x = area.x + area.width - 1;
    // The scrollbar lives in the right border column. The thumb renders as
    // a left half-block `▌` in the accent color, giving a sleek midpoint
    // between a hairline `┃` and a massive full block without intruding on
    // the inner document padding.
    for i in 0..track {
        let filled = i >= top && i < top + span;
        let y = area.y + 1 + i as u16;
        let (cell, style) = if filled {
            ("▌", Style::default().fg(app.theme.accent))
        } else {
            ("│", app.theme.border(false))
        };
        frame.render_widget(
            Paragraph::new(Span::styled(cell, style)),
            Rect::new(x, y, 1, 1),
        );
    }
}
