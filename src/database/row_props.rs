//! Property list at the top of a database row page.
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;

use super::editor::{PropEditor, editable, value_view};
use super::{add_property, calc, prop_menu, type_icon, write};
use crate::db;
use crate::model::*;

pub struct RowProperties {
    row: String,
    coll: Option<String>,
    schema: Schema,
    block: Option<Block>,
    editor: Entity<PropEditor>,
    rename: Entity<InputState>,
    renaming: Option<String>,
    _subs: Vec<Subscription>,
}

impl RowProperties {
    pub fn new(row_page_id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| PropEditor::new(window, cx));
        let rename = cx.new(|cx| InputState::new(window, cx));
        let subs = vec![
            cx.observe(&db::revision(cx), |this, _, cx| this.reload(cx)),
            cx.observe(&editor, |_, _, cx| cx.notify()),
            cx.subscribe(&rename, |this, state, ev: &InputEvent, cx| {
                if !matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    return;
                }
                let (Some(id), Some(coll)) = (this.renaming.take(), this.coll.clone()) else {
                    return;
                };
                let name = state.read(cx).value().trim().to_string();
                if let Some(mut p) = this.schema.get(&id).cloned().filter(|_| !name.is_empty()) {
                    p.name = name;
                    write(cx, move |s| s.update_property(&coll, &id, p));
                }
                cx.notify();
            }),
        ];
        let mut this = Self {
            row: row_page_id,
            coll: None,
            schema: Schema::new(),
            block: None,
            editor,
            rename,
            renaming: None,
            _subs: subs,
        };
        this.reload(cx);
        this
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let store = db::store(cx);
        self.block = store.block(&self.row).ok();
        self.coll = self.block.as_ref().and_then(|b| b.parent_id.clone());
        self.schema = self
            .coll
            .as_ref()
            .and_then(|c| store.schema(c).ok())
            .unwrap_or_default();
        cx.notify();
    }

    fn start_rename(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .schema
            .get(&id)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        self.rename.update(cx, |s, cx| {
            s.set_value(name, window, cx);
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        self.renaming = Some(id);
        cx.notify();
    }
}

impl Render for RowProperties {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (Some(block), Some(coll)) = (&self.block, &self.coll) else {
            return div().into_any_element();
        };
        if self.schema.is_empty() {
            return div().into_any_element();
        }
        let view = cx.entity();
        let editor = self.editor.read(cx);
        let rows = calc::ordered_props(&self.schema, &Value::Null)
            .into_iter()
            .filter(|id| id != "title")
            .filter_map(|id| self.schema.get(&id).cloned().map(|p| (id, p)))
            .map(|(id, p)| {
                let name: AnyElement = if self.renaming.as_deref() == Some(&id) {
                    div()
                        .w(px(160.))
                        .child(Input::new(&self.rename).xsmall())
                        .into_any_element()
                } else {
                    let (view, coll, pid, prop) =
                        (view.clone(), coll.clone(), id.clone(), p.clone());
                    div()
                        .w(px(160.))
                        .flex_none()
                        .overflow_hidden()
                        .child(
                            Button::new(SharedString::from(format!("prop-{id}")))
                                .ghost()
                                .xsmall()
                                .icon(type_icon(p.kind))
                                .label(p.name.clone())
                                .text_color(cx.theme().muted_foreground)
                                .dropdown_menu(move |menu, window, cx| {
                                    let rid = pid.clone();
                                    let menu = menu
                                        .item(PopupMenuItem::new("Rename").on_click(
                                            window.listener_for(
                                                &view,
                                                move |this, _, window, cx| {
                                                    this.start_rename(rid.clone(), window, cx)
                                                },
                                            ),
                                        ))
                                        .separator();
                                    prop_menu(menu, &coll, &pid, &prop, window, cx)
                                }),
                        )
                        .into_any_element()
                };
                let editing = editor.editing(&self.row, &id);
                let v = block.row_value(&id, &p);
                let value: AnyElement = if editing {
                    self.editor.clone().into_any_element()
                } else if calc::is_empty_value(&v) && p.kind != PropType::Checkbox {
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Empty")
                        .into_any_element()
                } else {
                    value_view(&self.row, &id, &p, &v, cx)
                };
                let (row, pid) = (self.row.clone(), id.clone());
                h_flex()
                    .id(SharedString::from(format!("prow-{id}")))
                    .min_h(px(34.))
                    .gap_2()
                    .child(name)
                    .child(
                        h_flex()
                            .id(SharedString::from(format!("pval-{id}")))
                            .flex_1()
                            .min_w_0()
                            .h(px(32.))
                            .px_2()
                            .rounded(cx.theme().radius)
                            .hover(|s| s.bg(cx.theme().secondary))
                            .child(value)
                            .when(editable(p.kind) && !editing, |d| {
                                d.on_click(cx.listener(move |this, _, window, cx| {
                                    this.editor
                                        .update(cx, |e, cx| e.start(&row, &pid, window, cx))
                                }))
                            }),
                    )
            })
            .collect::<Vec<_>>();
        let coll = coll.clone();
        v_flex()
            .w_full()
            .gap_0p5()
            .children(rows)
            .child(
                Button::new("add-row-prop")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .label("Add a property")
                    .text_color(cx.theme().muted_foreground)
                    .dropdown_menu(move |mut menu, window, _| {
                        for (kind, label) in calc::PROP_TYPES {
                            let (kind, coll) = (*kind, coll.clone());
                            menu = menu.item(
                                PopupMenuItem::new(*label).icon(type_icon(kind)).on_click(
                                    window.listener_for(&view, move |this, _, window, cx| {
                                        if let Some(id) = add_property(&coll, kind, cx) {
                                            this.reload(cx);
                                            this.start_rename(id, window, cx);
                                        }
                                    }),
                                ),
                            );
                        }
                        menu
                    }),
            )
            .into_any_element()
    }
}
