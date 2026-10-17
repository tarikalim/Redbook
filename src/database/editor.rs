//! Cell value display and the single in-place property editor shared by table cells and row pages.
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    calendar::{Calendar, CalendarEvent, CalendarState, Date},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Value, json};

use super::calc::{self, COLORS};
use super::write;
use crate::db;
use crate::model::*;

/// Background and text color of an option chip.
pub fn option_colors(color: &str, dark: bool) -> (Hsla, Hsla) {
    let (h, s) = match color {
        "gray" => (40., 0.06),
        "brown" => (25., 0.3),
        "orange" => (28., 0.75),
        "yellow" => (45., 0.75),
        "green" | "teal" => (140., 0.35),
        "blue" => (210., 0.6),
        "purple" => (270., 0.45),
        "pink" => (330., 0.6),
        "red" => (5., 0.65),
        _ => (0., 0.),
    };
    let h = h / 360.;
    if dark {
        (hsla(h, s * 0.5, 0.27, 1.), hsla(h, s.min(0.5), 0.85, 1.))
    } else {
        (hsla(h, s, 0.9, 1.), hsla(h, s.min(0.5), 0.25, 1.))
    }
}

pub fn chip(text: impl Into<SharedString>, color: &str, cx: &App) -> Div {
    let (bg, fg) = option_colors(color, cx.theme().is_dark());
    div()
        .flex_none()
        .px_1p5()
        .rounded(px(3.))
        .bg(bg)
        .text_color(fg)
        .text_sm()
        .whitespace_nowrap()
        .child(text.into())
}

fn option_color<'a>(p: &'a Property, value: &str) -> &'a str {
    p.options
        .iter()
        .find(|o| o.value == value)
        .map_or("default", |o| o.color.as_str())
}

/// Read-only rendering of a value; checkboxes toggle in place.
pub fn value_view(row: &str, prop_id: &str, p: &Property, v: &Value, cx: &App) -> AnyElement {
    match p.kind {
        k if calc::is_options(k) => h_flex()
            .gap_1()
            .overflow_hidden()
            .children(calc::option_values(v).into_iter().map(|s| {
                let color = option_color(p, &s);
                chip(s, color, cx)
            }))
            .into_any_element(),
        PropType::Checkbox => {
            let (row, prop_id) = (row.to_string(), prop_id.to_string());
            Checkbox::new(SharedString::from(format!("cb-{row}-{prop_id}")))
                .checked(v.as_bool().unwrap_or(false))
                .on_click(move |checked, _, cx| {
                    let (row, prop_id, checked) = (row.clone(), prop_id.clone(), *checked);
                    write(cx, move |s| s.set_row_value(&row, &prop_id, json!(checked)));
                })
                .into_any_element()
        }
        PropType::Url | PropType::Email => div()
            .text_sm()
            .truncate()
            .underline()
            .child(calc::display_text(p, v))
            .into_any_element(),
        _ => div()
            .text_sm()
            .truncate()
            .when(p.kind == PropType::Number, |d| d.w_full().text_right())
            .when(p.kind == PropType::Title, |d| {
                d.font_weight(FontWeight::MEDIUM)
            })
            .child(calc::display_text(p, v))
            .into_any_element(),
    }
}

/// Whether clicking the cell opens the editor.
pub fn editable(kind: PropType) -> bool {
    !calc::is_timestamp(kind) && !matches!(kind, PropType::Checkbox | PropType::Unknown)
}

struct Target {
    row: String,
    coll: String,
    prop_id: String,
    prop: Property,
    value: Value,
}

/// Edits one (row, property) at a time; the host renders it in place of the cell.
pub struct PropEditor {
    target: Option<Target>,
    input: Entity<InputState>,
    calendar: Entity<CalendarState>,
    _subs: Vec<Subscription>,
}

impl PropEditor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx));
        let calendar = cx.new(|cx| CalendarState::new(window, cx));
        let subs = vec![
            cx.subscribe_in(
                &input,
                window,
                |this, _, ev: &InputEvent, window, cx| match ev {
                    InputEvent::PressEnter { .. } => this.enter(window, cx),
                    InputEvent::Blur if this.kind().is_some_and(is_text_kind) => {
                        this.commit_text(cx)
                    }
                    InputEvent::Change => cx.notify(),
                    _ => {}
                },
            ),
            cx.subscribe(&calendar, |this, _, ev: &CalendarEvent, cx| {
                let CalendarEvent::Selected(date) = ev;
                if let Some(d) = date.start() {
                    this.set(calc::date_value(d), cx);
                    this.close(cx);
                }
            }),
        ];
        Self {
            target: None,
            input,
            calendar,
            _subs: subs,
        }
    }

    pub fn editing(&self, row: &str, prop_id: &str) -> bool {
        self.target
            .as_ref()
            .is_some_and(|t| t.row == row && t.prop_id == prop_id)
    }

    fn kind(&self) -> Option<PropType> {
        self.target.as_ref().map(|t| t.prop.kind)
    }

    pub fn start(&mut self, row: &str, prop_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing(row, prop_id) {
            return;
        }
        if self.kind().is_some_and(is_text_kind) {
            self.commit_text(cx);
        }
        let store = db::store(cx);
        let Ok(block) = store.block(row) else { return };
        let Some(coll) = block.parent_id.clone() else {
            return;
        };
        let Some(prop) = store
            .schema(&coll)
            .ok()
            .and_then(|s| s.get(prop_id).cloned())
        else {
            return;
        };
        if !editable(prop.kind) {
            return;
        }
        let value = block.row_value(prop_id, &prop);
        let text = match prop.kind {
            k if is_text_kind(k) => crate::store::value_text(&value),
            _ => String::new(),
        };
        if prop.kind == PropType::Date {
            let date = calc::date_start(&value);
            self.calendar
                .update(cx, |c, cx| c.set_date(Date::Single(date), window, cx));
        }
        let placeholder = if calc::is_options(prop.kind) {
            "Search or create an option"
        } else {
            "Empty"
        };
        self.input.update(cx, |s, cx| {
            s.set_placeholder(placeholder, window, cx);
            s.set_value(text, window, cx);
            s.focus(window, cx);
        });
        self.target = Some(Target {
            row: row.into(),
            coll,
            prop_id: prop_id.into(),
            prop,
            value,
        });
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.target.take().is_some() {
            cx.notify();
        }
    }

    fn set(&mut self, value: Value, cx: &mut Context<Self>) {
        let Some(t) = &mut self.target else { return };
        if t.value == value {
            return;
        }
        t.value = value.clone();
        let (row, prop) = (t.row.clone(), t.prop_id.clone());
        write(cx, move |s| s.set_row_value(&row, &prop, value));
    }

    fn commit_text(&mut self, cx: &mut Context<Self>) {
        let Some(kind) = self.kind() else { return };
        let text = self.input.read(cx).value().to_string();
        let value = match kind {
            PropType::Number => calc::parse_number(&text),
            PropType::Title => json!(text.trim()),
            _ if text.trim().is_empty() => Value::Null,
            _ => json!(text),
        };
        self.set(value, cx);
        self.close(cx);
    }

    fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.kind() {
            Some(k) if is_text_kind(k) => self.commit_text(cx),
            Some(k) if calc::is_options(k) => {
                let query = self.input.read(cx).value().trim().to_string();
                if !query.is_empty() {
                    let existing = self.matching_options(cx).into_iter().next();
                    let value = existing.map_or(query, |o| o.value);
                    self.pick(value, window, cx);
                }
            }
            _ => {}
        }
    }

    fn matching_options(&self, cx: &App) -> Vec<SelectOption> {
        let q = self.input.read(cx).value().to_lowercase();
        self.target.as_ref().map_or(vec![], |t| {
            t.prop
                .options
                .iter()
                .filter(|o| o.value.to_lowercase().contains(q.trim()))
                .cloned()
                .collect()
        })
    }

    /// Picks an option (creating it when new); multi-select toggles and stays open.
    fn pick(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(t) = &mut self.target else { return };
        if t.prop.option_index(&value).is_none() {
            let color = COLORS[1 + t.prop.options.len() % (COLORS.len() - 1)];
            t.prop.options.push(SelectOption {
                id: uuid::Uuid::new_v4().simple().to_string()[..8].to_string(),
                value: value.clone(),
                color: color.into(),
            });
            let (coll, id, prop) = (t.coll.clone(), t.prop_id.clone(), t.prop.clone());
            write(cx, move |s| s.update_property(&coll, &id, prop));
        }
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        if t.prop.kind == PropType::MultiSelect {
            let mut vals = calc::option_values(&t.value);
            match vals.iter().position(|v| *v == value) {
                Some(i) => {
                    vals.remove(i);
                }
                None => vals.push(value),
            }
            let v = if vals.is_empty() {
                Value::Null
            } else {
                json!(vals)
            };
            self.set(v, cx);
            cx.notify();
        } else {
            let current = calc::option_values(&t.value);
            let v = if current == [value.clone()] {
                Value::Null
            } else {
                json!(value)
            };
            self.set(v, cx);
            self.close(cx);
        }
    }

    fn panel(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .occlude()
            .w(px(260.))
            .p_1()
            .gap_1()
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close(cx)))
    }

    fn render_options(&self, t: &Target, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = calc::option_values(&t.value);
        let query = self.input.read(cx).value().trim().to_string();
        let options = self.matching_options(cx);
        let create = !query.is_empty() && t.prop.option_index(&query).is_none();
        let list =
            options
                .into_iter()
                .map(|o| {
                    let value = o.value.clone();
                    h_flex()
                        .id(SharedString::from(format!("opt-{}", o.id)))
                        .px_2()
                        .py_0p5()
                        .gap_2()
                        .rounded(cx.theme().radius)
                        .hover(|s| s.bg(cx.theme().accent))
                        .child(chip(o.value.clone(), &o.color, cx))
                        .child(div().flex_1())
                        .when(selected.contains(&o.value), |d| d.child(IconName::Check))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.pick(value.clone(), window, cx)
                        }))
                })
                .collect::<Vec<_>>();
        self.panel(cx)
            .child(Input::new(&self.input).small())
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .px_2()
                    .child("Select an option or create one"),
            )
            .child(
                v_flex()
                    .id("options")
                    .max_h(px(240.))
                    .overflow_y_scroll()
                    .children(list),
            )
            .when(create, |d| {
                d.child(
                    h_flex()
                        .id("create-option")
                        .px_2()
                        .py_0p5()
                        .gap_1()
                        .rounded(cx.theme().radius)
                        .hover(|s| s.bg(cx.theme().accent))
                        .text_sm()
                        .child("Create")
                        .child(chip(
                            query.clone(),
                            COLORS[1 + t.prop.options.len() % (COLORS.len() - 1)],
                            cx,
                        ))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.pick(query.clone(), window, cx)
                        })),
                )
            })
    }

    fn render_date(&self, cx: &mut Context<Self>) -> impl IntoElement {
        self.panel(cx)
            .child(Calendar::new(&self.calendar).small())
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("today")
                            .ghost()
                            .xsmall()
                            .label("Today")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set(calc::date_value(chrono::Local::now().date_naive()), cx);
                                this.close(cx);
                            })),
                    )
                    .child(
                        Button::new("clear-date")
                            .ghost()
                            .xsmall()
                            .label("Clear")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set(Value::Null, cx);
                                this.close(cx);
                            })),
                    ),
            )
    }
}

fn is_text_kind(k: PropType) -> bool {
    matches!(
        k,
        PropType::Title
            | PropType::Text
            | PropType::Number
            | PropType::Url
            | PropType::Email
            | PropType::PhoneNumber
    )
}

impl Render for PropEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(t) = &self.target else {
            return div().into_any_element();
        };
        if is_text_kind(t.prop.kind) {
            return div()
                .size_full()
                .child(Input::new(&self.input).appearance(false).small())
                .into_any_element();
        }
        let current = value_view(&t.row, &t.prop_id, &t.prop, &t.value, cx);
        let panel = if t.prop.kind == PropType::Date {
            self.render_date(cx).into_any_element()
        } else {
            self.render_options(t, cx).into_any_element()
        };
        h_flex()
            .size_full()
            .relative()
            .child(current)
            .child(
                div().absolute().left_0().bottom_0().child(
                    deferred(anchored().snap_to_window_with_margin(px(8.)).child(panel))
                        .with_priority(1),
                ),
            )
            .into_any_element()
    }
}
