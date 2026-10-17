//! Block selection: Esc, Shift+click, drag across blocks or Cmd+A twice; copy, cut and delete it.
use std::collections::HashSet;
use std::ops::RangeInclusive;

use super::*;

impl PageEditor {
    /// Selected rows as row indices, while both ends still exist.
    pub(super) fn selected_rows(&self) -> Option<RangeInclusive<usize>> {
        let (anchor, head) = self.select.as_ref()?;
        let (a, h) = (self.row_index(anchor)?, self.row_index(head)?);
        Some(a.min(h)..=a.max(h))
    }

    pub(super) fn select_blocks(
        &mut self,
        anchor: String,
        head: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flush(cx);
        self.editing = None;
        self.slash = None;
        self.menu = None;
        self.select = Some((anchor, head));
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    /// Selected blocks without those whose ancestor is selected too (it brings them along).
    fn selected_ids(&self) -> Vec<String> {
        let Some(range) = self.selected_rows() else {
            return Vec::new();
        };
        let rows = &self.rows[range];
        let set: HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        let inside = |id: &str| {
            let mut p = self.blocks.get(id).and_then(|b| b.parent_id.as_deref());
            while let Some(id) = p {
                if set.contains(id) {
                    return true;
                }
                p = self.blocks.get(id).and_then(|b| b.parent_id.as_deref());
            }
            false
        };
        rows.iter()
            .filter(|r| !inside(&r.id))
            .map(|r| r.id.clone())
            .collect()
    }

    /// Writes the selection to the clipboard as markdown; false without a selection.
    fn copy_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let ids = self.selected_ids();
        if ids.is_empty() {
            return false;
        }
        self.flush(cx);
        if let Some(md) = ok(db::store(cx).blocks_to_markdown(&ids)) {
            cx.write_to_clipboard(ClipboardItem::new_string(md));
        }
        true
    }

    pub(super) fn on_copy_blocks(
        &mut self,
        _: &CopyBlocks,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.copy_selection(cx) {
            cx.propagate();
        }
    }

    pub(super) fn on_cut_blocks(&mut self, _: &CutBlocks, w: &mut Window, cx: &mut Context<Self>) {
        if !self.copy_selection(cx) {
            cx.propagate();
        } else if !self.locked() {
            self.trash_selection(w, cx);
        }
    }

    pub(super) fn on_delete_blocks(
        &mut self,
        _: &DeleteBlocks,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_rows().is_none() {
            cx.propagate();
        } else if !self.locked() {
            self.trash_selection(w, cx);
        }
    }

    fn trash_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.selected_ids();
        self.select = None;
        self.flush(cx);
        let store = db::store(cx);
        ok(store.tx(|| ids.iter().try_for_each(|id| store.trash(id))));
        self.after_write(window, cx);
        self.focus_handle.focus(window, cx);
    }

    pub(super) fn on_clear_selection(
        &mut self,
        _: &ClearSelection,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.select.take().is_none() {
            cx.propagate();
        }
        cx.notify();
    }

    /// Up/Down move a one-block selection; with Shift they grow or shrink it.
    pub(super) fn move_selection(&mut self, up: bool, extend: bool, cx: &mut Context<Self>) {
        let Some((anchor, head)) = self.select.clone() else {
            cx.propagate();
            return;
        };
        let Some(ix) = self.row_index(&head) else {
            return;
        };
        let next = if up { ix.checked_sub(1) } else { Some(ix + 1) };
        let Some(next) = next.and_then(|i| self.rows.get(i)).map(|r| r.id.clone()) else {
            return;
        };
        let anchor = if extend { anchor } else { next.clone() };
        self.select = Some((anchor, next.clone()));
        if let Some(ix) = self.row_index(&next) {
            self.list.scroll_to_reveal_item(ix + 1);
        }
        cx.notify();
    }

    fn select_all_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ends = self.rows.first().zip(self.rows.last());
        if let Some((first, last)) = ends.map(|(f, l)| (f.id.clone(), l.id.clone())) {
            self.select_blocks(first, last, window, cx);
        }
    }

    pub(super) fn on_select_all_blocks(
        &mut self,
        _: &SelectAllBlocks,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.select.is_none() {
            cx.propagate();
            return;
        }
        self.select_all_rows(w, cx);
    }

    /// Cmd+A in a block whose text is all selected already selects every block.
    pub(super) fn on_text_select_all(
        &mut self,
        _: &input::SelectAll,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editor_focused(w, cx) {
            return;
        }
        let s = self.editor.read(cx);
        if s.selected_range() == (0..s.value().len()) {
            cx.stop_propagation();
            self.select_all_rows(w, cx);
        }
    }

    /// Shift+click on a block grows the selection, or starts one at the block being edited.
    pub(super) fn shift_click(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let anchor = match &self.select {
            Some((a, _)) => Some(a.clone()),
            None => self.focused_block(window, cx),
        };
        let Some(anchor) = anchor else { return false };
        self.select_blocks(anchor, id.to_string(), window, cx);
        true
    }

    /// A text drag that leaves its block turns into a block selection.
    pub(super) fn drag_select(
        &mut self,
        id: &str,
        ev: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ev.pressed_button != Some(MouseButton::Left) || cx.has_active_drag() {
            return;
        }
        let anchor = match &self.select {
            Some((_, head)) if head == id => return,
            Some((a, _)) => a.clone(),
            None => match self.focused_block(window, cx) {
                Some(a) if a != id => a,
                _ => return,
            },
        };
        self.select_blocks(anchor, id.to_string(), window, cx);
    }
}
