//! Simple tables: a `table` block whose `table_row` children hold `props.cells`.
//! Cells render as text; only the focused cell gets the shared `cell` Input.
use std::collections::HashMap;

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, input::Input, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Value, json};

use super::render::{highlights, line_height, link_at, menu_label, menu_row, open_link, panel};
use super::*;

pub(super) struct CellEdit {
    row: String,
    col: usize,
    rich: RichText,
}

pub(super) struct TableMenu {
    table: String,
    row: String,
    col: usize,
    at: Point<Pixels>,
}

#[derive(Clone, Copy)]
enum Op {
    RowAbove,
    RowBelow,
    DeleteRow,
    ColLeft,
    ColRight,
    DeleteCol,
    Header,
}

const OPS: &[(Op, &str)] = &[
    (Op::RowAbove, "Insert row above"),
    (Op::RowBelow, "Insert row below"),
    (Op::DeleteRow, "Delete row"),
    (Op::ColLeft, "Insert column left"),
    (Op::ColRight, "Insert column right"),
    (Op::DeleteCol, "Delete column"),
    (Op::Header, "Header row"),
];

/// Markdown tables always have a header, so a missing flag means on.
fn header_on(b: &Block) -> bool {
    b.props
        .get("header")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

fn set_cell(b: &mut Block, col: usize, rich: RichText) {
    let mut cells: Vec<Value> = b.cells().into_iter().map(Value::from).collect();
    if cells.len() <= col {
        cells.resize(col + 1, json!([]));
    }
    cells[col] = Value::from(rich);
    match b.props.as_object_mut() {
        Some(o) => {
            o.insert("cells".into(), json!(cells));
        }
        None => b.props = json!({ "cells": cells }),
    }
}

impl PageEditor {
    pub(super) fn cell_focused(&self, window: &Window, cx: &App) -> bool {
        self.cell.read(cx).focus_handle(cx).is_focused(window)
    }

    fn table_rows(&self, table: &str) -> Vec<String> {
        self.children.get(table).cloned().unwrap_or_default()
    }

    fn table_width(&self, table: &str) -> usize {
        let w = self
            .blocks
            .get(table)
            .and_then(|b| b.props.get("width"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        self.table_rows(table)
            .iter()
            .filter_map(|r| self.blocks.get(r))
            .map(|r| r.cells().len())
            .fold(w, usize::max)
            .max(1)
    }

    /// Binds the cell input to (`row`, `col`) with the caret at byte `cursor`.
    pub(super) fn start_cell_edit(
        &mut self,
        row: &str,
        col: usize,
        cursor: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(b) = self.blocks.get(row) else {
            return;
        };
        if self.locked() {
            return;
        }
        let rich = b.cells().into_iter().nth(col).unwrap_or_default();
        let text = rich.plain_text();
        let cursor = cursor.min(text.len());
        self.slash = None;
        self.menu = None;
        self.table_menu = None;
        self.cell_edit = Some(CellEdit {
            row: row.to_string(),
            col,
            rich,
        });
        self.cell.update(cx, |s, cx| {
            s.set_value(text, window, cx);
            s.set_selected_range(cursor..cursor, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn on_cell_change(&mut self, cx: &mut Context<Self>) {
        let Some(ce) = self.cell_edit.as_mut() else {
            return;
        };
        let value = self.cell.read(cx).value().to_string();
        let old = ce.rich.plain_text();
        if value == old {
            return;
        }
        let (range, ins) = text::diff(&old, &value);
        ce.rich = text::apply_edit(&ce.rich, range, ins);
        let (row, col, rich) = (ce.row.clone(), ce.col, ce.rich.clone());
        if let Some(b) = self.blocks.get_mut(&row) {
            set_cell(b, col, rich);
            self.dirty.insert(row);
            self.schedule_save(cx);
        }
        cx.notify();
    }

    /// After a reload: drop a vanished cell, pick up outside edits of a clean one.
    pub(super) fn sync_cell(
        &mut self,
        blocks: &HashMap<String, Block>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ce) = self.cell_edit.as_mut() else {
            return;
        };
        let Some(b) = blocks.get(&ce.row) else {
            self.cell_edit = None;
            return;
        };
        let stored = b.cells().into_iter().nth(ce.col).unwrap_or_default();
        if self.dirty.contains(&ce.row) || stored == ce.rich {
            return;
        }
        let text = stored.plain_text();
        ce.rich = stored;
        self.cell.update(cx, |s, cx| {
            let cursor = s.cursor().min(text.len());
            s.set_value(text, window, cx);
            s.set_selected_range(cursor..cursor, cx);
        });
    }

    /// Moves to another cell: `vertical` by rows, else row-major. `grow` adds a row past the end.
    pub(super) fn move_cell(
        &mut self,
        delta: isize,
        vertical: bool,
        grow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ce) = &self.cell_edit else { return };
        let Some(table) = self.blocks.get(&ce.row).and_then(|b| b.parent_id.clone()) else {
            return;
        };
        let rows = self.table_rows(&table);
        let w = self.table_width(&table) as isize;
        let Some(r) = rows.iter().position(|x| *x == ce.row) else {
            return;
        };
        let (r, c) = if vertical {
            (r as isize + delta, ce.col as isize)
        } else {
            let i = r as isize * w + ce.col as isize + delta;
            (i.div_euclid(w), i.rem_euclid(w))
        };
        if r < 0 {
            return;
        }
        let (r, c) = (r as usize, c as usize);
        if let Some(id) = rows.get(r).cloned() {
            self.start_cell_edit(&id, c, usize::MAX, window, cx);
        } else if grow && !self.locked() {
            self.flush(cx);
            let b = ok(db::store(cx).add_table_row(&table, usize::MAX));
            self.after_write(window, cx);
            if let Some(b) = b {
                self.start_cell_edit(&b.id, c, 0, window, cx);
            }
        }
    }

    fn table_op(&mut self, op: Op, window: &mut Window, cx: &mut Context<Self>) {
        let Some(m) = self.table_menu.take() else {
            return;
        };
        let rows = self.table_rows(&m.table);
        let r = rows.iter().position(|x| *x == m.row).unwrap_or(0);
        let header = self.blocks.get(&m.table).is_some_and(header_on);
        self.flush(cx);
        if self.cell_focused(window, cx) {
            self.focus_handle.focus(window, cx);
        }
        self.cell_edit = None;
        let store = db::store(cx);
        let t = m.table.as_str();
        let res = match op {
            Op::RowAbove => store.add_table_row(t, r).map(drop),
            Op::RowBelow => store.add_table_row(t, r + 1).map(drop),
            Op::DeleteRow if rows.len() <= 1 => store.trash(t),
            Op::DeleteRow => store.trash(&m.row),
            Op::ColLeft => store.add_table_column(t, m.col),
            Op::ColRight => store.add_table_column(t, m.col + 1),
            Op::DeleteCol => store.delete_table_column(t, m.col),
            Op::Header => store.set_prop(t, "header", json!(!header)),
        };
        ok(res);
        self.after_write(window, cx);
    }

    fn add_row_end(&mut self, table: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.flush(cx);
        ok(db::store(cx).add_table_row(table, usize::MAX));
        self.after_write(window, cx);
    }

    fn add_col_end(&mut self, table: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.flush(cx);
        ok(db::store(cx).add_table_column(table, usize::MAX));
        self.after_write(window, cx);
    }

    pub(super) fn render_table(
        &mut self,
        b: &Block,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let table = b.id.clone();
        let rows = self.table_rows(&table);
        let width = self.table_width(&table);
        let header = header_on(b);
        let locked = self.locked();
        let editing = self
            .cell_edit
            .as_ref()
            .filter(|_| self.cell_focused(window, cx))
            .map(|c| (c.row.clone(), c.col));
        let size = px(14.) * if self.flag("small_text") { 0.875 } else { 1. };
        let lh = line_height(size);
        let theme = cx.theme().colors;

        let mut grid = v_flex().w_full().border_1().border_color(theme.border);
        for (ri, rid) in rows.iter().enumerate() {
            let cells = self.blocks.get(rid).map(Block::cells).unwrap_or_default();
            let mut row = h_flex()
                .w_full()
                .items_stretch()
                .when(ri > 0, |r| r.border_t_1().border_color(theme.border))
                .when(header && ri == 0, |r| {
                    r.bg(theme.muted).font_weight(FontWeight::SEMIBOLD)
                });
            for ci in 0..width {
                let cell = div()
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .py_1()
                    .min_h(lh + px(8.))
                    .text_size(size)
                    .line_height(lh)
                    .when(ci > 0, |d| d.border_l_1().border_color(theme.border));
                let cell = if editing.as_ref() == Some(&(rid.clone(), ci)) {
                    cell.child(Input::new(&self.cell).appearance(false).xsmall())
                } else {
                    let rich = cells.get(ci).cloned().unwrap_or_default();
                    self.cell_text(cell, &table, rid, ci, rich, cx)
                };
                row = row.child(cell);
            }
            grid = grid.child(row);
        }
        if locked {
            return div().w_full().my_1().child(grid).into_any_element();
        }

        let bar = |id: String| {
            div()
                .id(SharedString::from(id))
                .flex()
                .items_center()
                .justify_center()
                .rounded_sm()
                .cursor_pointer()
                .text_color(theme.muted_foreground)
                .opacity(0.)
                .group_hover("table", |s| s.opacity(1.))
                .hover(|s| s.bg(theme.accent))
                .child(Icon::new(IconName::Plus).xsmall())
        };
        let (t1, t2) = (table.clone(), table.clone());
        let add_row = bar(format!("table-add-row-{table}"))
            .w_full()
            .h(px(18.))
            .on_click(cx.listener(move |this, _, window, cx| this.add_row_end(&t1, window, cx)));
        let add_col = bar(format!("table-add-col-{table}"))
            .w(px(18.))
            .mb(px(18.))
            .on_click(cx.listener(move |this, _, window, cx| this.add_col_end(&t2, window, cx)));
        h_flex()
            .group("table")
            .w_full()
            .my_1()
            .items_stretch()
            .child(v_flex().flex_1().min_w_0().child(grid).child(add_row))
            .child(add_col)
            .into_any_element()
    }

    /// A cell as styled text: click edits at the clicked char, platform-click opens a link,
    /// right click opens the table menu.
    fn cell_text(
        &self,
        cell: Div,
        table: &str,
        row: &str,
        col: usize,
        rich: RichText,
        cx: &mut Context<Self>,
    ) -> Div {
        let (t, r) = (table.to_string(), row.to_string());
        let cell = cell.on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                if !this.locked() {
                    this.table_menu = Some(TableMenu {
                        table: t.clone(),
                        row: r.clone(),
                        col,
                        at: ev.position,
                    });
                    cx.notify();
                }
            }),
        );
        let text = rich.plain_text();
        let row = row.to_string();
        if text.is_empty() {
            return cell.cursor_text().on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.start_cell_edit(&row, col, 0, window, cx)
                }),
            );
        }
        let styled = StyledText::new(text).with_highlights(highlights(&rich, cx));
        let layout = styled.layout().clone();
        cell.cursor_text()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    let ix = match layout.index_for_position(ev.position) {
                        Ok(i) | Err(i) => i,
                    };
                    if ev.modifiers.platform
                        && let Some(url) = link_at(&rich, ix)
                    {
                        open_link(&url, cx);
                        return;
                    }
                    this.start_cell_edit(&row, col, ix, window, cx)
                }),
            )
            .child(styled)
    }

    pub(super) fn render_table_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let m = self.table_menu.as_ref()?;
        let header = header_on(self.blocks.get(&m.table)?);
        let mut body = panel(cx)
            .w(px(220.))
            .id("table-menu")
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.table_menu = None;
                cx.notify();
            }))
            .child(menu_label("Table", cx));
        for (i, &(op, label)) in OPS.iter().enumerate() {
            let label = match op {
                Op::Header if header => "Header row: on",
                Op::Header => "Header row: off",
                _ => label,
            };
            body = body.child(menu_row(("table-op", i), false, cx).child(label).on_click(
                cx.listener(move |this, _, window, cx| {
                    this.table_op(op, window, cx);
                    cx.notify();
                }),
            ));
        }
        Some(
            deferred(
                anchored()
                    .position(m.at)
                    .snap_to_window_with_margin(px(8.))
                    .child(body),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}
