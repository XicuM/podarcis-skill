//! Pointer handling: clicks, wheel, and dragging dividers.
//!
//! Split out of `app.rs` verbatim; inherent `impl App` blocks merge
//! across the crate, so this is the same type seen from another file.

use super::*;

impl App {
pub(super) fn scroll_from_track_y(y: u16, track_y: u16, track_len: usize, span: usize, max_scroll: usize) -> usize {
    if track_len <= span || max_scroll == 0 {
        return 0;
    }
    let click_offset = (y.saturating_sub(track_y) as usize).min(track_len.saturating_sub(1));
    let target_top = click_offset.saturating_sub(span / 2);
    let travel = track_len.saturating_sub(span);
    // Rounded rather than truncated, so dragging the thumb to the bottom of the
    // track really does reach the bottom of the document.
    max_scroll
        .checked_mul(target_top)
        .and_then(|scaled| scaled.checked_add(travel / 2))
        .and_then(|scaled| scaled.checked_div(travel))
        .map_or(0, |scroll| scroll.min(max_scroll))
}

fn is_on_scrollbar(area: Rect, x: u16, y: u16) -> bool {
    if area.height < 4 || area.width < 2 {
        return false;
    }
    let right_border = area.x + area.width.saturating_sub(1);
    let reach = if area.width >= 10 { 2 } else { 1 };
    let left_edge = right_border.saturating_sub(reach);
    x >= left_edge
        && x <= right_border
        && y > area.y
        && y < area.y + area.height.saturating_sub(1)
}

    /// Click to focus and select, wheel to scroll, drag a divider to resize.
    /// The pane under the pointer acts, whether or not it has keyboard focus —
    /// that is what a pointer is for.
    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        // Number every Down, so a double-click pair must be two *adjacent*
        // clicks on the same row — any click elsewhere in between disqualifies.
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.mouse_down += 1;
        }

        if self.overlay.is_some() {
            self.overlay_mouse(mouse);
            return;
        }
        let (x, y) = (mouse.column, mouse.row);

        // A drag in progress owns the pointer until the button comes up, even
        // if it strays outside the divider's own column.
        if let Some(divider) = self.dragging {
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) => return self.resize_to(divider, x, y),
                MouseEventKind::Up(_) => {
                    self.dragging = None;
                    self.persist_widths();
                    return;
                }
                _ => {}
            }
        }

        if let Some(drag) = self.dragging_scrollbar {
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    match drag {
                        ScrollbarDrag::Doc => self.scroll_doc_to_y(y),
                        ScrollbarDrag::Inspector => self.scroll_inspector_to_y(y),
                    }
                    return;
                }
                MouseEventKind::Up(_) => {
                    self.dragging_scrollbar = None;
                    return;
                }
                _ => {}
            }
        }

        if self.selecting_text {
            match mouse.kind {
                MouseEventKind::Drag(btn) if (btn == MouseButton::Right) == self.selecting_right => {
                    return self.extend_selection(x, y);
                }
                MouseEventKind::Up(btn) if (btn == MouseButton::Right) == self.selecting_right => {
                    if self.selecting_right {
                        return self.finish_selection_right(x, y);
                    }
                    return self.finish_selection(x, y);
                }
                _ => {}
            }
        }

        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            if let Some((focus, _, _)) =
                self.areas.tab_bar.iter().find(|(_, _, r)| x >= r.x && x < r.x + r.width && y == r.y)
            {
                self.select_tab(*focus);
                return;
            }
            if let Some(i) = self
                .areas
                .tree_panes
                .iter()
                .position(|p| p.fold.is_some_and(|r| r.x == x && r.y == y))
            {
                self.fold_collection(i);
                return;
            }
            if self.areas.tree_toggle.is_some_and(|r| r.x == x && r.y == y) {
                self.run(Cmd::ToggleTree);
                return;
            }
            if self.areas.project_selector.is_some_and(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height) {
                self.run(Cmd::Projects);
                return;
            }
            if self.areas.sidebar_toggle.is_some_and(|r| r.x == x && r.y == y) {
                self.run(Cmd::ToggleSidebar);
                return;
            }
            if self.areas.inspector_toggle.is_some_and(|r| r.x == x && r.y == y) {
                self.run(Cmd::ToggleInspector);
                return;
            }
            if self.areas.nav_back.is_some_and(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height) {
                self.set_focus(Focus::Doc);
                self.run(Cmd::Back);
                return;
            }
            if self.areas.nav_forward.is_some_and(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height) {
                self.set_focus(Focus::Doc);
                self.run(Cmd::Forward);
                return;
            }
            if self.areas.doc_edit.is_some_and(|r| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height) {
                self.set_focus(Focus::Doc);
                self.run(Cmd::Edit);
                return;
            }

            let inspect_area = self.areas.inspector;
            let inspect_total = self.inspector_total_lines();
            let inspect_h = inspect_area.height.saturating_sub(2) as usize;
            if !inspect_area.is_empty() && inspect_total > inspect_h && Self::is_on_scrollbar(inspect_area, x, y) {
                self.dragging_scrollbar = Some(ScrollbarDrag::Inspector);
                self.scroll_inspector_to_y(y);
                return;
            }

            let doc_area = if !self.areas.doc_main.is_empty() {
                self.areas.doc_main
            } else {
                self.areas.doc
            };
            let doc_total = self.open.as_ref().map(|o| o.doc.height()).unwrap_or(0);
            let doc_h = self.areas.doc_body.height as usize;
            if !doc_area.is_empty() && doc_total > doc_h && doc_h > 0 && Self::is_on_scrollbar(doc_area, x, y) {
                self.dragging_scrollbar = Some(ScrollbarDrag::Doc);
                self.scroll_doc_to_y(y);
                return;
            }

            if let Some(divider) = self.divider_at(x, y) {
                self.dragging = Some(divider);
                self.zoom = false;
                return;
            }
        }

        let inside = |rect: Rect| {
            !rect.is_empty()
                && x >= rect.x
                && x < rect.x + rect.width
                && y >= rect.y
                && y < rect.y + rect.height
        };

        if inside(self.areas.sidebar) {
            let area = self.areas.sidebar;
            let cols = area.width.saturating_sub(2);
            let rows = area.height.saturating_sub(2);

            if matches!(mouse.kind, MouseEventKind::Down(_)) {
                self.set_focus(Focus::Sidebar);
            }

            if cols > 0 && rows > 0 {
                let inside_inner = x > area.x
                    && x <= area.x + cols
                    && y > area.y
                    && y <= area.y + rows;

                if inside_inner || !matches!(mouse.kind, MouseEventKind::Down(_)) {
                    let col = (x.saturating_sub(area.x)).clamp(1, cols);
                    let row = (y.saturating_sub(area.y)).clamp(1, rows);
                    let pixel = self.sidebar.as_ref().and_then(|pane| {
                        pane.pixel_mouse().then_some(herdr::keys::PTY_CELL_PX)
                    });
                    let bytes = herdr::keys::encode_mouse(&mouse, col, row, pixel);
                    if let Some(pane) = self.sidebar.as_mut() {
                        pane.send(&bytes);
                    }
                }
            }
            return;
        }

        if inside(self.areas.tree) {
            match mouse.kind {
                MouseEventKind::ScrollDown => self.tree.move_by(1),
                MouseEventKind::ScrollUp => self.tree.move_by(-1),
                MouseEventKind::Down(MouseButton::Left) => {
                    self.set_focus(Focus::Tree);
                    if let Some(name) = self.hit_tree_title(x, y) {
                        self.open_repo_config(&name);
                    } else if let Some(i) = self.hit_tree_row(x, y) {
                        self.tree.move_to(i);
                        let now = Instant::now();
                        let double = match self.last_tree_click.replace((now, i, self.mouse_down)) {
                            Some((at, row, gen)) => {
                                row == i
                                    && gen + 1 == self.mouse_down
                                    && now.duration_since(at) <= DOUBLE_CLICK
                            }
                            None => false,
                        };
                        if double {
                            // The first press already toggled, so a folder is
                            // made sure to stay expanded before its index opens.
                            if let Some(path) = self.tree.open_double(i) {
                                self.open_path(&path, true);
                            }
                        } else {
                            // A single click selects a page, and toggles a
                            // folder — opening is saved for the double click.
                            self.tree.toggle();
                        }
                    }
                }
                MouseEventKind::Down(MouseButton::Right) => {
                    self.set_focus(Focus::Tree);
                    if let Some(i) = self.hit_tree_row(x, y) {
                        self.tree.move_to(i);
                        self.open_tree_menu(i, x, y);
                    }
                }
                _ => {}
            }
            return;
        }

        if inside(self.areas.inspector) {
            match mouse.kind {
                MouseEventKind::ScrollDown => self.scroll_inspector(3),
                MouseEventKind::ScrollUp => self.scroll_inspector(-3),
                MouseEventKind::Down(MouseButton::Left) => {
                    self.set_focus(Focus::Doc);
                    let body = self.areas.inspector_body;
                    if body.is_empty()
                        || y < body.y
                        || y >= body.y + body.height
                        || x < body.x
                        || x >= body.x + body.width
                    {
                        return;
                    }
                    let row = (y - body.y) as usize;
                    let col = (x - body.x) as u16;

                    if let Some(link) = self
                        .areas
                        .inspector_backlinks
                        .iter()
                        .find(|b| b.row == row && col >= b.col_start && col < b.col_end)
                    {
                        let path = link.path.clone();
                        self.open_path(&path, true);
                        return;
                    }

                    if let Some(Some(id)) = self.areas.inspector_rows.get(row).cloned() {
                        let now = Instant::now();
                        let double = match self.last_inspector_click.replace((now, row, self.mouse_down)) {
                            Some((at, r, gen)) => {
                                r == row
                                    && gen + 1 == self.mouse_down
                                    && now.duration_since(at) <= DOUBLE_CLICK
                            }
                            None => false,
                        };
                        if double {
                            self.open_source(&id);
                        } else {
                            // A single click selects the source — its `[n]` is
                            // highlighted in the body. Opening is a double click.
                            self.select_source(&id);
                        }
                    }
                }
                _ => {}
            }
            return;
        }

        if inside(self.areas.doc) {
            // The editor resolves the pointer itself, against the text area it
            // recorded at render time: click to place the cursor, drag to
            // select, wheel to scroll. Without this the pointer does nothing
            // at all in the one pane where typing happens.
            if self.open.as_ref().is_some_and(Open::editing) {
                if matches!(mouse.kind, MouseEventKind::Down(_)) {
                    self.set_focus(Focus::Doc);
                }
                if let Some(editor) = self.open.as_mut().and_then(|o| o.editor.as_mut()) {
                    editor.mouse(mouse);
                }
                return;
            }
            match mouse.kind {
                // Shift+wheel is how a terminal reports a sideways scroll on a
                // mouse that has no tilt; a trackpad sends it directly.
                MouseEventKind::ScrollDown if mouse.modifiers.contains(KeyModifiers::SHIFT) => {
                    self.pan(Self::PAN_STEP)
                }
                MouseEventKind::ScrollUp if mouse.modifiers.contains(KeyModifiers::SHIFT) => {
                    self.pan(-Self::PAN_STEP)
                }
                MouseEventKind::ScrollRight => self.pan(Self::PAN_STEP),
                MouseEventKind::ScrollLeft => self.pan(-Self::PAN_STEP),
                MouseEventKind::ScrollDown => self.scroll(3),
                MouseEventKind::ScrollUp => self.scroll(-3),
                MouseEventKind::Down(MouseButton::Left) => {
                    self.set_focus(Focus::Doc);
                    if self.open.as_ref().is_some_and(|o| !o.editing()) {
                        self.start_selection(x, y);
                    }
                }
                MouseEventKind::Down(MouseButton::Right) => {
                    // A right-drag is the same selection gesture, paid to the
                    // agent on release instead of the clipboard.
                    self.set_focus(Focus::Doc);
                    if self.open.as_ref().is_some_and(|o| !o.editing()) {
                        self.selecting_right = true;
                        self.start_selection(x, y);
                    }
                }
                _ => {}
            }
        }
    }

    /// Whole terminal in cells, inferred from the panes being drawn.
    fn viewport(&self) -> (u16, u16) {
        let rects = [self.areas.tree, self.areas.doc, self.areas.sidebar, self.areas.inspector];
        let width = rects
            .iter()
            .filter(|r| !r.is_empty())
            .map(|r| r.x.saturating_add(r.width))
            .max()
            .unwrap_or(80);
        let height = rects
            .iter()
            .filter(|r| !r.is_empty())
            .map(|r| r.y.saturating_add(r.height))
            .max()
            .unwrap_or(24);
        (width, height)
    }

    /// The right-click menu is the only overlay that takes the pointer. A left
    /// press highlights the item under it and a release on the same row runs
    /// it; a press anywhere else dismisses the menu.
    fn overlay_mouse(&mut self, mouse: MouseEvent) {
        let area = match self.overlay.as_ref() {
            Some(Overlay::Menu(menu)) => menu.area,
            _ => return,
        };
        if area.is_empty() {
            return;
        }
        let inside = mouse.column >= area.x
            && mouse.column < area.x + area.width
            && mouse.row >= area.y
            && mouse.row < area.y + area.height;
        let row = mouse.row.saturating_sub(area.y + 1) as usize;

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if inside => {
                if let Some(Overlay::Menu(menu)) = self.overlay.as_mut() {
                    menu.selected = row.min(menu.items.len().saturating_sub(1));
                    menu.pressed = (row < menu.items.len()).then_some(row);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => self.overlay = None,
            MouseEventKind::Up(MouseButton::Left) if inside => {
                let fire = self.overlay.as_ref().is_some_and(|o| match o {
                    Overlay::Menu(menu) => menu.pressed == Some(row),
                    _ => false,
                });
                if fire {
                    self.activate_menu();
                }
            }
            _ => {}
        }
    }

    /// Open the right-click context menu for tree row `i`, positioned at the
    /// pointer but clamped to the layout so it never leaves the screen.
    fn open_tree_menu(&mut self, i: usize, x: u16, y: u16) {
        let Some(row) = self.tree.rows.get(i).cloned() else { return };
        let items: Vec<(MenuAction, &'static str)> = if row.is_collection {
            // A collection root has no rename or delete: those names belong to
            // `repositories:` in the config, not to the row in the tree.
            vec![
                (MenuAction::Open, "open"),
                (MenuAction::NewPage, "new page…"),
                (MenuAction::CopyRelative, "copy relative path"),
                (MenuAction::CopyAbsolute, "copy absolute path"),
            ]
        } else if row.is_dir {
            vec![
                (MenuAction::Open, "open"),
                (MenuAction::NewPage, "new page…"),
                (MenuAction::Rename, "rename…"),
                (MenuAction::Delete, "delete…"),
                (MenuAction::CopyRelative, "copy relative path"),
                (MenuAction::CopyAbsolute, "copy absolute path"),
            ]
        } else {
            vec![
                (MenuAction::Open, "open"),
                (MenuAction::Rename, "rename…"),
                (MenuAction::Delete, "delete…"),
                (MenuAction::CopyRelative, "copy relative path"),
                (MenuAction::CopyAbsolute, "copy absolute path"),
            ]
        };
        let width = items.iter().map(|(_, label)| label.len()).max().unwrap_or(12) as u16 + 6;
        let height = items.len() as u16 + 3;
        let (screen_w, screen_h) = self.viewport();
        let area = Rect {
            x: x.saturating_add(1).min(screen_w.saturating_sub(width)),
            y: y.saturating_sub(height / 2).min(screen_h.saturating_sub(height)),
            width,
            height,
        };
        self.overlay = Some(Overlay::Menu(Menu {
            path: row.path,
            items,
            selected: 0,
            area,
            pressed: None,
        }));
    }

    pub(super) fn divider_at(&self, x: u16, y: u16) -> Option<Divider> {
        let body = self.areas.doc;
        if body.is_empty() {
            return None;
        }
        let near = |pos: Option<u16>, coord: u16| {
            pos.is_some_and(|p| coord.abs_diff(p) <= crate::ui::GRAB)
        };
        let tree = self.areas.tree;
        if !tree.is_empty() && x >= tree.x && x < tree.x + tree.width {
            // Checked before the vertical dividers: inside the tree column a
            // horizontal boundary is the likelier target, and the two only
            // overlap on the corner cell.
            if let Some((i, _)) = self
                .areas
                .collection_dividers
                .iter()
                .find(|(_, edge)| y.abs_diff(*edge) <= crate::ui::GRAB)
            {
                return Some(Divider::Collection(*i));
            }
        }
        if y >= body.y && y < body.y + body.height {
            if near(self.areas.tree_divider, x) {
                return Some(Divider::Tree);
            }
            // Grab the shared border and the document side of it, not the first
            // inner column of the herdr pane — that column is a real click target.
            if self.areas.sidebar_divider.is_some_and(|p| x == p || x + 1 == p) {
                return Some(Divider::Sidebar);
            }
        }
        if x >= body.x && x < body.x + body.width
            && near(self.areas.inspector_divider, y) {
                return Some(Divider::Inspector);
            }
        None
    }

    /// Move a divider to column `x` or row `y`. Widths are clamped so neither the document
    /// nor the pane being dragged can be squeezed out of existence.
    pub(super) fn resize_to(&mut self, divider: Divider, x: u16, y: u16) {
        let total = self.areas.tree.width + self.areas.doc.width + self.areas.sidebar.width;
        let origin = self.areas.tree.x.min(self.areas.doc.x);
        let min_doc = 30u16;
        match divider {
            Divider::Tree => {
                if total == 0 {
                    return;
                }
                let max = total.saturating_sub(min_doc + self.areas.sidebar.width);
                let width = x.saturating_sub(origin).saturating_add(1);
                self.cfg.tree_width = width.clamp(MIN_PANE, max.max(MIN_PANE));
            }
            Divider::Sidebar => {
                if total == 0 {
                    return;
                }
                let max = total.saturating_sub(min_doc + self.areas.tree.width);
                let width = (origin + total).saturating_sub(x);
                self.cfg.sidebar_width = width.clamp(MIN_PANE, max.max(MIN_PANE));
            }
            Divider::Collection(i) => {
                let Some(pane) = self.areas.tree_panes.get(i) else { return };
                // A folded box is one row by definition; there is nothing to
                // drag, and writing a height here would silently resize it the
                // moment it was unfolded.
                if pane.folded {
                    return;
                }
                let names = self.collection_names();
                let Some(name) = names.get(i).cloned() else { return };
                // The divider sits on the first row of the box below, so the
                // box above keeps every row up to it.
                let rows = y.saturating_sub(pane.area.y);
                let max = self
                    .areas
                    .tree
                    .height
                    .saturating_sub(crate::ui::panes::MIN_COLLECTION_ROWS);
                let rows = rows.clamp(
                    crate::ui::panes::MIN_COLLECTION_ROWS,
                    max.max(crate::ui::panes::MIN_COLLECTION_ROWS),
                );
                self.cfg.collection_heights.insert(name, rows);
            }
            Divider::Inspector => {
                let doc_bottom = self.areas.doc.y + self.areas.doc.height;
                let min_main = 8u16;
                let min_inspect = 4u16;
                let max_inspect = self.areas.doc.height.saturating_sub(min_main);
                let new_h = doc_bottom.saturating_sub(y);
                self.cfg.inspector_height = new_h.clamp(min_inspect, max_inspect.max(min_inspect));
            }
        }
    }

    pub fn persist_widths(&self) {
        let _ = self.cfg.save_tui();
    }

    pub fn adjust_inspector_height(&mut self, delta: i16) {
        let min_main = 8u16;
        let min_inspect = 4u16;
        let max_inspect = self.areas.doc.height.saturating_sub(min_main);
        let current = self.cfg.inspector_height as i16;
        let new_h = (current + delta).clamp(min_inspect as i16, max_inspect.max(min_inspect) as i16) as u16;
        if new_h != self.cfg.inspector_height {
            self.cfg.inspector_height = new_h;
            self.persist_widths();
        }
    }

    pub fn adjust_tree_width(&mut self, delta: i16) {
        let total = self.areas.tree.width + self.areas.doc.width + self.areas.sidebar.width;
        let min_doc = 30u16;
        let sidebar_w = if self.show_sidebar { self.areas.sidebar.width } else { 0 };
        let max = if total > 0 {
            total.saturating_sub(min_doc + sidebar_w)
        } else {
            120
        };
        let current = self.cfg.tree_width as i16;
        let new_w = (current + delta).clamp(MIN_PANE as i16, max.max(MIN_PANE) as i16) as u16;
        if new_w != self.cfg.tree_width {
            self.cfg.tree_width = new_w;
            self.persist_widths();
        }
    }

    pub fn adjust_sidebar_width(&mut self, delta: i16) {
        let total = self.areas.tree.width + self.areas.doc.width + self.areas.sidebar.width;
        let min_doc = 30u16;
        let tree_w = if self.show_tree { self.areas.tree.width } else { 0 };
        let max = if total > 0 {
            total.saturating_sub(min_doc + tree_w)
        } else {
            120
        };
        let current = self.cfg.sidebar_width as i16;
        let new_w = (current + delta).clamp(MIN_PANE as i16, max.max(MIN_PANE) as i16) as u16;
        if new_w != self.cfg.sidebar_width {
            self.cfg.sidebar_width = new_w;
            self.persist_widths();
        }
    }

    pub fn shrink_active_pane(&mut self) {
        match self.focus {
            Focus::Tree => self.adjust_tree_width(-4),
            Focus::Sidebar => self.adjust_sidebar_width(-4),
            Focus::Doc => {
                if self.show_sidebar {
                    self.adjust_sidebar_width(4);
                } else if self.show_tree {
                    self.adjust_tree_width(-4);
                }
            }
        }
    }

    pub fn widen_active_pane(&mut self) {
        match self.focus {
            Focus::Tree => self.adjust_tree_width(4),
            Focus::Sidebar => self.adjust_sidebar_width(4),
            Focus::Doc => {
                if self.show_sidebar {
                    self.adjust_sidebar_width(-4);
                } else if self.show_tree {
                    self.adjust_tree_width(4);
                }
            }
        }
    }
}
