//! Filter and sort bar of a view; edits go to the view query in the store.
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
};
use gpui_kit::*;
use serde_json::{Value, json};

use super::{DatabaseView, calc, type_icon};
use crate::model::*;

fn filter_text(kind: PropType, v: &Value) -> String {
    match (kind, v.as_i64()) {
        (PropType::CreatedTime | PropType::LastEditedTime, Some(ms)) => {
            chrono::DateTime::from_timestamp_millis(ms)
                .map(|d| {
                    d.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d")
                        .to_string()
                })
                .unwrap_or_default()
        }
        _ => crate::store::value_text(v),
    }
}

impl DatabaseView {
    /// One text input per filter, created lazily and never reset by reloads.
    pub(super) fn sync_filter_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let filters = self.view().query.filters.clone();
        self.filter_inputs.truncate(filters.len());
        for f in &filters[self.filter_inputs.len()..] {
            let kind = self
                .schema
                .get(&f.property)
                .map_or(PropType::Text, |p| p.kind);
            let text = filter_text(kind, &f.value);
            let placeholder = if matches!(
                kind,
                PropType::Date | PropType::CreatedTime | PropType::LastEditedTime
            ) {
                "YYYY-MM-DD"
            } else {
                "Value"
            };
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(text)
            });
            let sub = cx.subscribe(&input, |this, state, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    let Some(i) = this.filter_inputs.iter().position(|(e, _)| e == &state) else {
                        return;
                    };
                    let text = state.read(cx).value().to_string();
                    let Some(f) = this.view().query.filters.get(i) else {
                        return;
                    };
                    let kind = this
                        .schema
                        .get(&f.property)
                        .map_or(PropType::Text, |p| p.kind);
                    let value = calc::filter_value(kind, &text);
                    this.save_view(cx, |v| v.query.filters[i].value = value);
                }
            });
            self.filter_inputs.push((input, sub));
        }
    }

    pub(super) fn add_filter(&mut self, prop: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(kind) = self.schema.get(prop).map(|p| p.kind) else {
            return;
        };
        let f = calc::default_filter(prop, kind);
        self.save_view(cx, |v| v.query.filters.push(f));
        self.sync_filter_inputs(window, cx);
        if let Some((input, _)) = self.filter_inputs.last() {
            input.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    fn remove_filter(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.filter_inputs.len() {
            drop(self.filter_inputs.remove(i));
        }
        self.save_view(cx, |v| {
            if i < v.query.filters.len() {
                v.query.filters.remove(i);
            }
        });
    }

    fn set_filter_prop(
        &mut self,
        i: usize,
        prop: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(kind) = self.schema.get(&prop).map(|p| p.kind) else {
            return;
        };
        if let Some((input, _)) = self.filter_inputs.get(i) {
            input.update(cx, |s, cx| s.set_value("", window, cx));
        }
        self.save_view(cx, |v| {
            v.query.filters[i] = calc::default_filter(&prop, kind)
        });
    }

    pub(super) fn add_sort(&mut self, prop: &str, cx: &mut Context<Self>) {
        let prop = prop.to_string();
        self.save_view(cx, |v| {
            v.query.sorts.retain(|s| s.property != prop);
            v.query.sorts.push(Sort {
                property: prop,
                direction: Direction::Ascending,
            });
        });
    }

    fn filter_value_control(
        &self,
        i: usize,
        f: &Filter,
        p: &Property,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if matches!(f.op, FilterOp::IsEmpty | FilterOp::IsNotEmpty) {
            return div().into_any_element();
        }
        let view = cx.entity();
        let choices: Vec<(String, Value)> = match p.kind {
            PropType::Checkbox => vec![
                ("Checked".into(), json!(true)),
                ("Unchecked".into(), json!(false)),
            ],
            k if calc::is_options(k) => p
                .options
                .iter()
                .map(|o| (o.value.clone(), json!(o.value)))
                .collect(),
            _ => {
                return self
                    .filter_inputs
                    .get(i)
                    .map_or(div().into_any_element(), |(input, _)| {
                        div()
                            .w(px(150.))
                            .child(Input::new(input).xsmall())
                            .into_any_element()
                    });
            }
        };
        let label = choices
            .iter()
            .find(|(_, v)| *v == f.value)
            .map_or("Select…".to_string(), |(l, _)| l.clone());
        Button::new(("filter-value", i))
            .outline()
            .xsmall()
            .label(label)
            .dropdown_menu(move |mut menu, window, _| {
                for (label, value) in &choices {
                    let value = value.clone();
                    menu = menu.item(PopupMenuItem::new(label.clone()).on_click(
                        window.listener_for(&view, move |this, _, _, cx| {
                            let value = value.clone();
                            this.save_view(cx, |v| v.query.filters[i].value = value);
                        }),
                    ));
                }
                menu
            })
            .into_any_element()
    }

    pub(super) fn render_query_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let q = self.view().query.clone();
        if q.filters.is_empty() && q.sorts.is_empty() {
            return div().into_any_element();
        }
        let view = cx.entity();
        let props: Vec<(String, Property)> = calc::ordered_props(&self.schema, &self.view().format)
            .into_iter()
            .filter_map(|id| self.schema.get(&id).cloned().map(|p| (id, p)))
            .collect();
        let pill = |cx: &App| {
            h_flex()
                .gap_0p5()
                .px_1()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
        };
        let mut bar = h_flex().flex_wrap().gap_1().py_1();
        for (i, s) in q.sorts.iter().enumerate() {
            let Some(p) = self.schema.get(&s.property) else {
                continue;
            };
            let dir = s.direction;
            let props = props.clone();
            let v1 = view.clone();
            bar = bar.child(
                pill(cx)
                    .child(IconName::SortAscending)
                    .child(
                        Button::new(("sort-prop", i))
                            .ghost()
                            .xsmall()
                            .label(p.name.clone())
                            .dropdown_menu(move |mut menu, window, _| {
                                for (id, p) in &props {
                                    let id = id.clone();
                                    menu = menu.item(PopupMenuItem::new(p.name.clone()).on_click(
                                        window.listener_for(&v1, move |this, _, _, cx| {
                                            let id = id.clone();
                                            this.save_view(cx, |v| v.query.sorts[i].property = id);
                                        }),
                                    ));
                                }
                                menu
                            }),
                    )
                    .child(
                        Button::new(("sort-dir", i))
                            .ghost()
                            .xsmall()
                            .label(if dir == Direction::Ascending {
                                "Ascending"
                            } else {
                                "Descending"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.save_view(cx, |v| {
                                    v.query.sorts[i].direction = match dir {
                                        Direction::Ascending => Direction::Descending,
                                        Direction::Descending => Direction::Ascending,
                                    }
                                })
                            })),
                    )
                    .child(
                        Button::new(("sort-x", i))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.save_view(cx, |v| {
                                    v.query.sorts.remove(i);
                                })
                            })),
                    ),
            );
        }
        for (i, f) in q.filters.iter().enumerate() {
            let Some(p) = self.schema.get(&f.property).cloned() else {
                continue;
            };
            let props = props.clone();
            let ops = calc::filter_ops(p.kind);
            let (v1, v2) = (view.clone(), view.clone());
            bar = bar.child(
                pill(cx)
                    .child(
                        Button::new(("filter-prop", i))
                            .ghost()
                            .xsmall()
                            .icon(type_icon(p.kind))
                            .label(p.name.clone())
                            .dropdown_menu(move |mut menu, window, _| {
                                for (id, p) in &props {
                                    let id = id.clone();
                                    menu = menu.item(PopupMenuItem::new(p.name.clone()).on_click(
                                        window.listener_for(&v1, move |this, _, window, cx| {
                                            this.set_filter_prop(i, id.clone(), window, cx)
                                        }),
                                    ));
                                }
                                menu
                            }),
                    )
                    .child(
                        Button::new(("filter-op", i))
                            .ghost()
                            .xsmall()
                            .label(calc::op_label(p.kind, f.op))
                            .dropdown_menu(move |mut menu, window, _| {
                                for (op, label) in ops {
                                    let op = *op;
                                    menu = menu.item(PopupMenuItem::new(*label).on_click(
                                        window.listener_for(&v2, move |this, _, _, cx| {
                                            this.save_view(cx, |v| v.query.filters[i].op = op)
                                        }),
                                    ));
                                }
                                menu
                            }),
                    )
                    .child(self.filter_value_control(i, f, &p, cx))
                    .child(
                        Button::new(("filter-x", i))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .on_click(cx.listener(move |this, _, _, cx| this.remove_filter(i, cx))),
                    ),
            );
        }
        bar.into_any_element()
    }
}
