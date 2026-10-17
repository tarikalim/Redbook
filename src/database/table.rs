//! Table view: resizable columns, header menus, inline cell editing, footer calculations.
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem},
    scroll::{ScrollableElement as _, ScrollbarAxis},
    v_flex, v_virtual_list,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Value, json};

use super::editor::{editable, value_view};
use super::{DatabaseView, ROW_H, Rename, calc, prop_menu, row_menu, set_format, type_icon};
use crate::db;
use crate::model::*;

const ADD_COL_W: Pixels = px(120.);
const MIN_COL_W: f32 = 60.;

/// Drag payload of a column resize handle.
#[derive(Clone)]
struct ResizeColumn(String);

struct Nothing;

impl Render for Nothing {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl DatabaseView {
    fn columns(&self) -> Vec<(String, Property, Pixels)> {
        let widths = self.view().format.get("widths");
        self.visible_props()
            .into_iter()
            .map(|(id, p)| {
                let default = if id == "title" { 280. } else { 180. };
                let w = widths
                    .and_then(|w| w.get(&id))
                    .and_then(Value::as_f64)
                    .unwrap_or(default);
                (id, p, px(w as f32))
            })
            .collect()
    }

    /// Width changes while dragging are saved without a revision bump (other views pick them up later).
    fn set_width(&mut self, prop: &str, w: Pixels, cx: &mut Context<Self>) {
        let w = f32::from(w).max(MIN_COL_W).round();
        let Some(v) = self.views.iter_mut().find(|v| v.id == self.view_id) else {
            return;
        };
        let mut widths = v.format.get("widths").cloned().unwrap_or_else(|| json!({}));
        widths[prop] = json!(w);
        set_format(v, "widths", widths);
        if let Err(e) = db::store(cx).update_view(v) {
            eprintln!("save width failed: {e:#}");
        }
        cx.notify();
    }

    fn render_th(&self, id: &str, p: &Property, w: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity();
        let (pid, prop, coll) = (id.to_string(), p.clone(), self.coll.clone());
        let renaming = self.renaming == Some(Rename::Prop(id.to_string()));
        let label: AnyElement = if renaming {
            div()
                .flex_1()
                .child(Input::new(&self.rename).xsmall())
                .into_any_element()
        } else {
            Button::new(SharedString::from(format!("th-{id}")))
                .ghost()
                .xsmall()
                .icon(type_icon(p.kind))
                .label(p.name.clone())
                .dropdown_menu(move |menu, window, cx| {
                    let pid2 = pid.clone();
                    let (a, b, c) = (pid.clone(), pid.clone(), pid.clone());
                    let menu = menu
                        .item(PopupMenuItem::new("Rename").on_click(window.listener_for(
                            &view,
                            move |this, _, window, cx| {
                                this.start_rename(Rename::Prop(pid2.clone()), window, cx)
                            },
                        )))
                        .item(
                            PopupMenuItem::new("Sort ascending")
                                .icon(IconName::SortAscending)
                                .on_click(window.listener_for(&view, move |this, _, _, cx| {
                                    let a = a.clone();
                                    this.save_view(cx, |v| {
                                        v.query.sorts = vec![Sort {
                                            property: a,
                                            direction: Direction::Ascending,
                                        }]
                                    });
                                })),
                        )
                        .item(
                            PopupMenuItem::new("Sort descending")
                                .icon(IconName::SortDescending)
                                .on_click(window.listener_for(&view, move |this, _, _, cx| {
                                    let b = b.clone();
                                    this.save_view(cx, |v| {
                                        v.query.sorts = vec![Sort {
                                            property: b,
                                            direction: Direction::Descending,
                                        }]
                                    });
                                })),
                        )
                        .when(pid != "title", |m| {
                            m.item(PopupMenuItem::new("Hide").icon(IconName::EyeOff).on_click(
                                window.listener_for(&view, move |this, _, _, cx| {
                                    this.toggle_hidden(&c, cx)
                                }),
                            ))
                        })
                        .separator();
                    prop_menu(menu, &coll, &pid, &prop, window, cx)
                })
                .into_any_element()
        };
        let drag_id = id.to_string();
        let move_id = id.to_string();
        h_flex()
            .id(SharedString::from(format!("thc-{id}")))
            .relative()
            .w(w)
            .h_full()
            .flex_none()
            .px_1()
            .overflow_hidden()
            .border_r_1()
            .border_color(cx.theme().border)
            .text_color(cx.theme().muted_foreground)
            .child(label)
            .child(
                div()
                    .id(SharedString::from(format!("resize-{id}")))
                    .absolute()
                    .right_0()
                    .top_0()
                    .h_full()
                    .w(px(5.))
                    .cursor_col_resize()
                    .hover(|s| s.bg(cx.theme().ring))
                    .on_drag(ResizeColumn(drag_id), |_, _, _, cx| cx.new(|_| Nothing)),
            )
            .on_drag_move(
                cx.listener(move |this, ev: &DragMoveEvent<ResizeColumn>, _, cx| {
                    if ev.drag(cx).0 == move_id {
                        this.set_width(&move_id, ev.event.position.x - ev.bounds.left(), cx);
                    }
                }),
            )
            .into_any_element()
    }

    fn render_row(
        &self,
        ix: usize,
        cols: &[(String, Property, Pixels)],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row = &self.shown[ix];
        let editor = self.editor.read(cx);
        let cells: Vec<AnyElement> = cols
            .iter()
            .map(|(pid, p, w)| {
                let editing = editor.editing(&row.id, pid);
                let cell = h_flex()
                    .id(SharedString::from(format!("{}-{pid}", row.id)))
                    .w(*w)
                    .h_full()
                    .flex_none()
                    .px_2()
                    .gap_1()
                    .overflow_hidden()
                    .border_r_1()
                    .border_color(cx.theme().border);
                if editing {
                    return cell
                        .bg(cx.theme().secondary)
                        .child(self.editor.clone())
                        .into_any_element();
                }
                let (r, id) = (row.id.clone(), pid.clone());
                let start = cx.listener(move |this, _, window, cx| {
                    this.editor.update(cx, |e, cx| e.start(&r, &id, window, cx));
                });
                let value = value_view(&row.id, pid, p, &row.row_value(pid, p), cx);
                if pid == "title" {
                    let open = row.id.clone();
                    return cell
                        .child(
                            div()
                                .id("title")
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .flex()
                                .items_center()
                                .child(value)
                                .on_click(start),
                        )
                        .child(
                            Button::new("open")
                                .ghost()
                                .xsmall()
                                .label("Open")
                                .on_click(move |_, _, cx| db::open_page(open.clone(), cx)),
                        )
                        .into_any_element();
                }
                cell.child(value)
                    .when(editable(p.kind), |c| c.on_click(start))
                    .into_any_element()
            })
            .collect();
        h_flex()
            .id(SharedString::from(row.id.clone()))
            .h(ROW_H)
            .border_b_1()
            .border_color(cx.theme().border)
            .hover(|s| s.bg(cx.theme().secondary.opacity(0.4)))
            .children(cells)
            .context_menu(row_menu(&row.id))
            .into_any_element()
    }

    fn render_footer(
        &self,
        cols: &[(String, Property, Pixels)],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let view = cx.entity();
        h_flex().h(px(30.)).children(cols.iter().map(|(id, p, w)| {
            let label = self.calcs.get(id).cloned();
            let (view, id, kind) = (view.clone(), id.clone(), p.kind);
            h_flex().w(*w).flex_none().justify_end().px_1().child(
                Button::new(SharedString::from(format!("calc-{id}")))
                    .ghost()
                    .xsmall()
                    .label(label.unwrap_or_else(|| "Calculate".into()))
                    .text_color(cx.theme().muted_foreground)
                    .dropdown_menu(move |mut menu, window, _| {
                        let none = id.clone();
                        menu = menu.item(PopupMenuItem::new("None").on_click(
                            window.listener_for(&view, move |this, _, _, cx| {
                                this.set_calc(&none, None, cx)
                            }),
                        ));
                        for c in calc::Calc::for_kind(kind) {
                            let id = id.clone();
                            menu = menu.item(PopupMenuItem::new(c.label()).on_click(
                                window.listener_for(&view, move |this, _, _, cx| {
                                    this.set_calc(&id, Some(c), cx)
                                }),
                            ));
                        }
                        menu
                    }),
            )
        }))
    }

    fn set_calc(&mut self, prop: &str, c: Option<calc::Calc>, cx: &mut Context<Self>) {
        let prop = prop.to_string();
        self.save_view(cx, move |v| {
            let mut calcs = v.format.get("calcs").cloned().unwrap_or_else(|| json!({}));
            match c {
                Some(c) => calcs[&prop] = json!(c.id()),
                None => {
                    calcs.as_object_mut().map(|o| o.remove(&prop));
                }
            }
            set_format(v, "calcs", calcs);
        });
    }

    pub(super) fn render_table(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let cols = self.columns();
        let total = cols.iter().fold(ADD_COL_W, |a, c| a + c.2);
        let view = cx.entity();
        let header = h_flex()
            .h(ROW_H)
            .border_b_1()
            .border_color(cx.theme().border)
            .text_sm()
            .children(
                cols.iter()
                    .map(|(id, p, w)| self.render_th(id, p, *w, cx))
                    .collect::<Vec<_>>(),
            )
            .child(
                Button::new("add-prop")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .dropdown_menu(move |mut menu, window, _| {
                        menu = menu.label("Property type");
                        for (kind, label) in calc::PROP_TYPES {
                            let kind = *kind;
                            menu = menu.item(
                                PopupMenuItem::new(*label).icon(type_icon(kind)).on_click(
                                    window.listener_for(&view, move |this, _, window, cx| {
                                        this.add_property_here(kind, window, cx)
                                    }),
                                ),
                            );
                        }
                        menu
                    }),
            );
        let rows_h = ROW_H * (self.shown.len().clamp(1, 14) as f32);
        let list = div()
            .id("table-rows")
            .relative()
            .map(|d| {
                if self.full_page {
                    d.flex_1().min_h_0()
                } else {
                    d.h(rows_h)
                }
            })
            .child(
                v_virtual_list(
                    cx.entity(),
                    "rows",
                    self.row_sizes.clone(),
                    |this, range, _, cx| {
                        let cols = this.columns();
                        range
                            .map(|ix| this.render_row(ix, &cols, cx))
                            .collect::<Vec<_>>()
                    },
                )
                .track_scroll(&self.scroll),
            )
            .scrollbar(&self.scroll, ScrollbarAxis::Vertical);
        let new_row = h_flex()
            .id("table-new")
            .h(ROW_H)
            .px_2()
            .gap_1()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .hover(|s| s.bg(cx.theme().secondary))
            .child(IconName::Plus)
            .child("New")
            .on_click(cx.listener(|this, _, window, cx| this.new_row(vec![], window, cx)));
        div()
            .id("table-x")
            .w_full()
            .overflow_x_scroll()
            .when(self.full_page, |d| d.flex_1().min_h_0())
            .child(
                v_flex()
                    .w(total)
                    .when(self.full_page, |d| d.h_full())
                    .child(header)
                    .child(list)
                    .child(new_row)
                    .child(self.render_footer(&cols, cx)),
            )
            .into_any_element()
    }
}
