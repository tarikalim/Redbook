//! Board, list, gallery and calendar views.
use std::collections::HashMap;
use std::rc::Rc;

use chrono::{Datelike as _, Duration, Local, Months, NaiveDate};
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::ContextMenuExt as _,
    scroll::{ScrollableElement as _, ScrollbarAxis},
    v_flex, v_virtual_list,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Value, json};

use super::editor::{chip, value_view};
use super::{DatabaseView, DraggedRow, ROW_H, calc, row_menu, write};
use crate::db;
use crate::model::*;

const GALLERY_COLS: usize = 4;
const CARD_H: Pixels = px(150.);

fn title_of(row: &DbRow) -> SharedString {
    let t = row.title.plain_text();
    if t.is_empty() {
        "Untitled".into()
    } else {
        t.into()
    }
}

impl DatabaseView {
    /// Visible non-title properties shown on cards and list rows.
    fn card_props(&self, skip: Option<&str>) -> Vec<(String, Property)> {
        self.visible_props()
            .into_iter()
            .filter(|(id, _)| id != "title" && Some(id.as_str()) != skip)
            .collect()
    }

    fn card(
        &self,
        row: &DbRow,
        props: &[(String, Property)],
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let open = row.id.clone();
        v_flex()
            .id(SharedString::from(format!("card-{}", row.id)))
            .gap_1()
            .p_2()
            .rounded(cx.theme().radius)
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .shadow_xs()
            .hover(|s| s.bg(cx.theme().secondary))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(title_of(row)),
            )
            .children(props.iter().filter_map(|(id, p)| {
                let v = row.row_value(id, p);
                (!calc::is_empty_value(&v)).then(|| value_view(&row.id, id, p, &v, cx))
            }))
            .on_click(move |_, _, cx| db::open_page(open.clone(), cx))
    }

    fn empty_note(&self, text: &str, cx: &App) -> AnyElement {
        div()
            .p_4()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(text.to_string())
            .into_any_element()
    }

    pub(super) fn render_board(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let group = self.view().query.group_by.clone();
        let Some((gid, gp)) = group
            .and_then(|g| self.schema.get(&g).cloned().map(|p| (g, p)))
            .filter(|(_, p)| matches!(p.kind, PropType::Select | PropType::Status))
        else {
            return self.empty_note("Choose a Select or Status property under Group.", cx);
        };
        let mut groups: Vec<(Option<SelectOption>, Vec<usize>)> = std::iter::once(None)
            .chain(gp.options.iter().cloned().map(Some))
            .map(|o| (o, vec![]))
            .collect();
        for (ix, r) in self.shown.iter().enumerate() {
            let slot = r
                .row_value(&gid, &gp)
                .as_str()
                .and_then(|s| gp.option_index(s))
                .map_or(0, |i| i + 1);
            groups[slot].1.push(ix);
        }
        let props = self.card_props(Some(&gid));
        let columns: Vec<AnyElement> = groups
            .into_iter()
            .map(|(opt, ixs)| {
                let value = opt.as_ref().map_or(Value::Null, |o| json!(o.value));
                let key = opt.as_ref().map_or("none".to_string(), |o| o.id.clone());
                let head = match &opt {
                    Some(o) => chip(o.value.clone(), &o.color, cx),
                    None => chip(format!("No {}", gp.name), "default", cx),
                };
                let cards: Vec<AnyElement> = ixs
                    .iter()
                    .map(|&ix| {
                        let row = &self.shown[ix];
                        let drag = DraggedRow {
                            id: row.id.clone(),
                            title: title_of(row),
                        };
                        self.card(row, &props, cx)
                            .on_drag(drag, |d, _, _, cx| cx.new(|_| d.clone()))
                            .context_menu(row_menu(&row.id))
                            .into_any_element()
                    })
                    .collect();
                let (drop_value, drop_gid, new_value, new_gid) =
                    (value.clone(), gid.clone(), value, gid.clone());
                v_flex()
                    .id(SharedString::from(format!("col-{key}")))
                    .w(px(260.))
                    .flex_none()
                    .gap_2()
                    .p_2()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().secondary.opacity(0.5))
                    .child(
                        h_flex().gap_2().child(head).child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(ixs.len().to_string()),
                        ),
                    )
                    .child(
                        v_flex()
                            .id(SharedString::from(format!("cards-{key}")))
                            .gap_2()
                            .max_h(px(if self.full_page { 2000. } else { 520. }))
                            .overflow_y_scroll()
                            .children(cards),
                    )
                    .child(
                        Button::new(SharedString::from(format!("new-{key}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .label("New")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let v = if new_value.is_null() {
                                    vec![]
                                } else {
                                    vec![(new_gid.clone(), new_value.clone())]
                                };
                                this.new_row(v, window, cx)
                            })),
                    )
                    .drag_over::<DraggedRow>(|s, _, _, cx| s.bg(cx.theme().accent))
                    .on_drop(cx.listener(move |_, d: &DraggedRow, _, cx| {
                        let (row, gid, v) = (d.id.clone(), drop_gid.clone(), drop_value.clone());
                        write(cx, move |s| s.set_row_value(&row, &gid, v));
                    }))
                    .into_any_element()
            })
            .collect();
        h_flex()
            .id("board")
            .items_start()
            .gap_3()
            .py_2()
            .w_full()
            .overflow_x_scroll()
            .when(self.full_page, |d| d.flex_1().min_h_0())
            .children(columns)
            .into_any_element()
    }

    pub(super) fn render_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let rows_h = ROW_H * (self.shown.len().clamp(1, 14) as f32);
        div()
            .id("list")
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
                    "list-rows",
                    self.row_sizes.clone(),
                    |this, range, _, cx| {
                        let props = this.card_props(None);
                        range
                            .map(|ix| {
                                let row = &this.shown[ix];
                                let open = row.id.clone();
                                h_flex()
                                    .id(SharedString::from(row.id.clone()))
                                    .h(ROW_H)
                                    .px_2()
                                    .gap_2()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .hover(|s| s.bg(cx.theme().secondary))
                                    .child(IconName::FileText)
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_sm()
                                            .child(title_of(row)),
                                    )
                                    .children(props.iter().take(4).filter_map(|(id, p)| {
                                        let v = row.row_value(id, p);
                                        (!calc::is_empty_value(&v)).then(|| {
                                            div()
                                                .max_w(px(180.))
                                                .overflow_hidden()
                                                .child(value_view(&row.id, id, p, &v, cx))
                                        })
                                    }))
                                    .on_click(move |_, _, cx| db::open_page(open.clone(), cx))
                                    .context_menu(row_menu(&row.id))
                            })
                            .collect::<Vec<_>>()
                    },
                )
                .track_scroll(&self.scroll),
            )
            .scrollbar(&self.scroll, ScrollbarAxis::Vertical)
            .into_any_element()
    }

    pub(super) fn render_gallery(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let chunks = self.shown.len().div_ceil(GALLERY_COLS);
        let sizes = Rc::new(vec![size(px(0.), CARD_H + px(12.)); chunks]);
        let gallery_h = (CARD_H + px(12.)) * (chunks.clamp(1, 3) as f32);
        div()
            .id("gallery")
            .relative()
            .map(|d| {
                if self.full_page {
                    d.flex_1().min_h_0()
                } else {
                    d.h(gallery_h)
                }
            })
            .child(
                v_virtual_list(cx.entity(), "gallery-rows", sizes, |this, range, _, cx| {
                    let props = this.card_props(None);
                    range
                        .map(|chunk| {
                            let start = chunk * GALLERY_COLS;
                            let end = (start + GALLERY_COLS).min(this.shown.len());
                            h_flex()
                                .h(CARD_H)
                                .gap_3()
                                .children((start..end).map(|ix| {
                                    let row = &this.shown[ix];
                                    div().flex_1().min_w_0().h_full().child(
                                        this.card(row, &props, cx)
                                            .h_full()
                                            .overflow_hidden()
                                            .child(div().flex_1())
                                            .context_menu(row_menu(&row.id)),
                                    )
                                }))
                                .children((end..start + GALLERY_COLS).map(|_| div().flex_1()))
                        })
                        .collect::<Vec<_>>()
                })
                .track_scroll(&self.scroll),
            )
            .into_any_element()
    }

    pub(super) fn render_calendar(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let by = self
            .view()
            .format
            .get("calendar_by")
            .and_then(Value::as_str)
            .map(str::to_string);
        let Some((did, dp)) = by
            .and_then(|id| self.schema.get(&id).cloned().map(|p| (id, p)))
            .filter(|(_, p)| p.kind == PropType::Date)
        else {
            return self.empty_note("Choose a Date property under By.", cx);
        };
        let mut by_day: HashMap<NaiveDate, Vec<usize>> = HashMap::new();
        for (ix, r) in self.shown.iter().enumerate() {
            if let Some(d) = calc::date_start(&r.row_value(&did, &dp)) {
                by_day.entry(d).or_default().push(ix);
            }
        }
        let month = self.month;
        let today = Local::now().date_naive();
        let first = month - Duration::days(month.weekday().num_days_from_sunday() as i64);
        let nav = h_flex()
            .gap_1()
            .child(
                div()
                    .flex_1()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(month.format("%B %Y").to_string()),
            )
            .child(
                Button::new("prev-month")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChevronLeft)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.month = this.month - Months::new(1);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("today-month")
                    .ghost()
                    .xsmall()
                    .label("Today")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.month = Local::now().date_naive().with_day(1).unwrap_or(this.month);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("next-month")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChevronRight)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.month = this.month + Months::new(1);
                        cx.notify();
                    })),
            );
        let weekdays =
            h_flex().children(["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"].map(|d| {
                div()
                    .flex_1()
                    .px_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(d)
            }));
        let weeks = (0..6).map(|w| {
            h_flex().children((0..7).map(|d| {
                let day = first + Duration::days(w * 7 + d);
                let items = by_day.get(&day).cloned().unwrap_or_default();
                let (did, value) = (did.clone(), calc::date_value(day));
                v_flex()
                    .id(SharedString::from(format!("day-{day}")))
                    .flex_1()
                    .min_w_0()
                    .h(px(110.))
                    .p_1()
                    .gap_0p5()
                    .border_1()
                    .border_color(cx.theme().border)
                    .when(day.month() != month.month(), |c| {
                        c.bg(cx.theme().secondary.opacity(0.5))
                    })
                    .hover(|s| s.bg(cx.theme().secondary))
                    .child(
                        div()
                            .text_xs()
                            .when(day == today, |t| {
                                t.text_color(cx.theme().red).font_weight(FontWeight::BOLD)
                            })
                            .child(day.day().to_string()),
                    )
                    .children(items.iter().take(3).map(|&ix| {
                        let row = &self.shown[ix];
                        let open = row.id.clone();
                        div()
                            .id(SharedString::from(format!("ev-{}", row.id)))
                            .px_1()
                            .rounded(px(3.))
                            .bg(cx.theme().background)
                            .border_1()
                            .border_color(cx.theme().border)
                            .text_xs()
                            .truncate()
                            .child(title_of(row))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                db::open_page(open.clone(), cx);
                            })
                    }))
                    .when(items.len() > 3, |c| {
                        c.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("+{} more", items.len() - 3)),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.new_row(vec![(did.clone(), value.clone())], window, cx)
                    }))
            }))
        });
        v_flex()
            .id("calendar")
            .gap_1()
            .when(self.full_page, |d| d.flex_1().min_h_0().overflow_y_scroll())
            .child(nav)
            .child(weekdays)
            .children(weeks)
            .into_any_element()
    }
}
