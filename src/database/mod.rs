//! Database views (table, board, list, gallery, calendar) and row property editing.
mod board;
mod calc;
mod editor;
mod query;
mod row_props;
mod table;

use std::collections::HashMap;
use std::rc::Rc;

use chrono::{Datelike as _, Local, NaiveDate};
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _, VirtualListScrollHandle,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Value, json};

use crate::db;
use crate::model::View;
use crate::model::*;
use crate::store::{Store, apply_query};
use calc::PROP_TYPES;
use editor::PropEditor;
pub use row_props::RowProperties;

const ROW_H: Pixels = px(34.);

/// Runs a store write and signals other views; errors are logged.
pub(crate) fn write(cx: &mut App, f: impl FnOnce(&Store) -> anyhow::Result<()>) {
    let store = db::store(cx);
    match f(&store) {
        Ok(()) => db::changed(cx),
        Err(e) => eprintln!("database write failed: {e:#}"),
    }
}

fn status_options() -> Vec<SelectOption> {
    [
        ("Not started", "gray"),
        ("In progress", "blue"),
        ("Done", "green"),
    ]
    .into_iter()
    .map(|(v, c)| SelectOption {
        id: uuid::Uuid::new_v4().simple().to_string()[..8].into(),
        value: v.into(),
        color: c.into(),
    })
    .collect()
}

/// Adds a property named after its type; returns its id.
fn add_property(coll: &str, kind: PropType, cx: &mut App) -> Option<String> {
    let store = db::store(cx);
    let added = (|| {
        let id = store.add_property(coll, calc::type_label(kind), kind)?;
        if kind == PropType::Status {
            let mut p = store.schema(coll)?[&id].clone();
            p.options = status_options();
            store.update_property(coll, &id, p)?;
        }
        anyhow::Ok(id)
    })();
    db::changed(cx);
    added
        .map_err(|e| eprintln!("add property failed: {e:#}"))
        .ok()
}

fn set_prop_type(coll: &str, id: &str, p: &Property, kind: PropType, cx: &mut App) {
    let mut p = p.clone();
    p.kind = kind;
    if kind == PropType::Status && p.options.is_empty() {
        p.options = status_options();
    }
    let (coll, id) = (coll.to_string(), id.to_string());
    write(cx, move |s| s.update_property(&coll, &id, p));
}

pub(crate) fn type_icon(kind: PropType) -> IconName {
    match kind {
        PropType::Title => IconName::FileText,
        PropType::Text => IconName::ALargeSmall,
        PropType::Number => IconName::Asterisk,
        PropType::Select => IconName::ChevronDown,
        PropType::MultiSelect => IconName::Menu,
        PropType::Status => IconName::LoaderCircle,
        PropType::Date => IconName::Calendar,
        PropType::Checkbox => IconName::CircleCheck,
        PropType::Url => IconName::Globe,
        PropType::Email => IconName::Inbox,
        PropType::PhoneNumber => IconName::User,
        PropType::CreatedTime | PropType::LastEditedTime => IconName::RefreshCw,
        _ => IconName::Info,
    }
}

/// Shared "type / number format / delete" items of a property menu.
fn prop_menu(
    menu: PopupMenu,
    coll: &str,
    id: &str,
    p: &Property,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    if id == "title" {
        return menu;
    }
    let (c, i, prop) = (coll.to_string(), id.to_string(), p.clone());
    let mut menu = menu.submenu("Type", window, cx, move |mut m, _, _| {
        for (kind, label) in PROP_TYPES {
            let (c, i, prop, kind) = (c.clone(), i.clone(), prop.clone(), *kind);
            m = m.item(
                PopupMenuItem::new(*label)
                    .checked(prop.kind == kind)
                    .on_click(move |_, _, cx| set_prop_type(&c, &i, &prop, kind, cx)),
            );
        }
        m
    });
    if p.kind == PropType::Number {
        let (c, i, prop) = (coll.to_string(), id.to_string(), p.clone());
        menu = menu.submenu("Number format", window, cx, move |mut m, _, _| {
            for (fmt, label) in calc::NUMBER_FORMATS {
                let (c, i, mut prop) = (c.clone(), i.clone(), prop.clone());
                let checked = calc::number_format(&prop) == *fmt;
                prop.config = json!({ "number_format": fmt });
                m = m.item(PopupMenuItem::new(*label).checked(checked).on_click(
                    move |_, _, cx| {
                        let (c, i, prop) = (c.clone(), i.clone(), prop.clone());
                        write(cx, move |s| s.update_property(&c, &i, prop));
                    },
                ));
            }
            m
        });
    }
    let (c, i) = (coll.to_string(), id.to_string());
    menu.separator().item(
        PopupMenuItem::new("Delete property")
            .icon(IconName::Delete)
            .on_click(move |_, _, cx| {
                let (c, i) = (c.clone(), i.clone());
                write(cx, move |s| s.remove_property(&c, &i));
            }),
    )
}

fn set_format(v: &mut View, key: &str, value: Value) {
    if !v.format.is_object() {
        v.format = json!({});
    }
    v.format[key] = value;
}

#[derive(Clone, PartialEq)]
enum Rename {
    View(String),
    Prop(String),
}

/// Payload while a row (board card) is dragged; renders the drag preview.
#[derive(Clone)]
struct DraggedRow {
    id: String,
    title: SharedString,
}

impl Render for DraggedRow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(240.))
            .p_2()
            .rounded(cx.theme().radius)
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .shadow_md()
            .text_sm()
            .child(self.title.clone())
    }
}

pub struct DatabaseView {
    coll: String,
    full_page: bool,
    title: Entity<InputState>,
    schema: Schema,
    views: Vec<View>,
    view_id: String,
    rows: Vec<Rc<DbRow>>,
    stamp: Option<(i64, i64, f64)>,
    /// Shares the blocks of `rows`.
    shown: Vec<Rc<DbRow>>,
    row_sizes: Rc<Vec<Size<Pixels>>>,
    calcs: HashMap<String, String>,
    scroll: VirtualListScrollHandle,
    editor: Entity<PropEditor>,
    search: Entity<InputState>,
    search_open: bool,
    rename: Entity<InputState>,
    renaming: Option<Rename>,
    filter_inputs: Vec<(Entity<InputState>, Subscription)>,
    month: NaiveDate,
    _subs: Vec<Subscription>,
}

impl DatabaseView {
    pub fn new(collection_block_id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Untitled"));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search rows…"));
        let rename = cx.new(|cx| InputState::new(window, cx));
        let editor = cx.new(|cx| PropEditor::new(window, cx));
        let subs = vec![
            cx.observe_in(&db::revision(cx), window, |this, _, window, cx| {
                this.reload(window, cx)
            }),
            cx.observe(&editor, |_, _, cx| cx.notify()),
            cx.subscribe(&title, |this, state, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    let text = state.read(cx).value().to_string();
                    let coll = this.coll.clone();
                    let changed = db::store(cx)
                        .block(&coll)
                        .is_ok_and(|b| b.title.plain_text() != text);
                    if changed {
                        write(cx, move |s| {
                            s.update_title(&coll, &RichText::from_plain(&text))
                        });
                    }
                }
            }),
            cx.subscribe(&search, |this, _, ev: &InputEvent, cx| {
                if let InputEvent::Change = ev {
                    this.recompute(cx);
                    cx.notify();
                }
            }),
            cx.subscribe(&rename, |this, _, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    this.commit_rename(cx);
                }
            }),
        ];
        let mut this = Self {
            coll: collection_block_id,
            full_page: false,
            title,
            schema: Schema::new(),
            views: vec![],
            view_id: String::new(),
            rows: vec![],
            stamp: None,
            shown: vec![],
            row_sizes: Rc::new(vec![]),
            calcs: HashMap::new(),
            scroll: VirtualListScrollHandle::new(),
            editor,
            search,
            search_open: false,
            rename,
            renaming: None,
            filter_inputs: vec![],
            month: Local::now().date_naive().with_day(1).unwrap_or_default(),
            _subs: subs,
        };
        this.reload(window, cx);
        this
    }

    /// Re-reads schema and views; rows only when their stamp changed.
    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let store = db::store(cx);
        let Ok(block) = store.block(&self.coll) else {
            return;
        };
        self.full_page = block.kind == BlockType::CollectionViewPage;
        let title = block.title.plain_text();
        if !self.title.read(cx).focus_handle(cx).is_focused(window)
            && self.title.read(cx).value() != title
        {
            self.title
                .update(cx, |s, cx| s.set_value(title, window, cx));
        }
        let schema = store.schema(&self.coll).unwrap_or_default();
        let views = store.views(&self.coll).unwrap_or_default();
        // Revisions from unrelated writes leave this database untouched.
        let mut dirty = schema != self.schema || views != self.views || views.is_empty();
        self.schema = schema;
        self.views = views;
        if self.views.is_empty() {
            match store.create_view(&self.coll, ViewType::Table, "Table") {
                Ok(v) => self.views.push(v),
                Err(e) => eprintln!("create view failed: {e:#}"),
            }
        }
        if !self.views.iter().any(|v| v.id == self.view_id) {
            self.view_id = self.views.first().map(|v| v.id.clone()).unwrap_or_default();
        }
        let stamp = store.rows_stamp(&self.coll).ok();
        if stamp != self.stamp || stamp.is_none() {
            self.stamp = stamp;
            self.rows = store
                .db_rows(&self.coll)
                .unwrap_or_default()
                .into_iter()
                .map(Rc::new)
                .collect();
            dirty = true;
        }
        if !dirty {
            return;
        }
        self.sync_filter_inputs(window, cx);
        self.recompute(cx);
        cx.notify();
    }

    fn recompute(&mut self, cx: &App) {
        let Some(view) = self.views.iter().find(|v| v.id == self.view_id) else {
            return;
        };
        let needle = self.search.read(cx).value().trim().to_lowercase();
        let mut shown = apply_query(
            self.rows.clone(),
            &calc::effective_query(&view.query),
            &self.schema,
        );
        shown.retain(|r| calc::search_match(r, &self.schema, &needle));
        self.calcs.clear();
        if let Some(calcs) = view.format.get("calcs").and_then(Value::as_object) {
            for (id, c) in calcs {
                let (Some(p), Some(c)) =
                    (self.schema.get(id), c.as_str().and_then(calc::Calc::parse))
                else {
                    continue;
                };
                let values: Vec<Value> = shown.iter().map(|r| r.row_value(id, p)).collect();
                self.calcs.insert(
                    id.clone(),
                    format!("{} {}", c.label(), calc::calculate(c, p, &values)),
                );
            }
        }
        self.row_sizes = Rc::new(vec![size(px(0.), ROW_H); shown.len()]);
        self.shown = shown;
    }

    fn view(&self) -> &View {
        self.views
            .iter()
            .find(|v| v.id == self.view_id)
            .unwrap_or(&self.views[0])
    }

    fn save_view(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut View)) {
        let Some(v) = self.views.iter_mut().find(|v| v.id == self.view_id) else {
            return;
        };
        f(v);
        let v = v.clone();
        write(cx, move |s| s.update_view(&v));
    }

    fn select_view(&mut self, id: String, cx: &mut Context<Self>) {
        self.view_id = id;
        self.editor.update(cx, |e, cx| e.close(cx));
        self.recompute(cx);
        cx.notify();
    }

    fn add_view(&mut self, kind: ViewType, cx: &mut Context<Self>) {
        let store = db::store(cx);
        let label = match kind {
            ViewType::Board => "Board",
            ViewType::List => "List",
            ViewType::Gallery => "Gallery",
            ViewType::Calendar => "Calendar",
            _ => "Table",
        };
        let Ok(mut v) = store.create_view(&self.coll, kind, label) else {
            return;
        };
        let find = |k: &[PropType]| {
            self.schema
                .iter()
                .find(|(_, p)| k.contains(&p.kind))
                .map(|(id, _)| id.clone())
        };
        if kind == ViewType::Board {
            v.query.group_by = find(&[PropType::Status, PropType::Select])
                .or_else(|| add_property(&self.coll, PropType::Status, cx));
        }
        if kind == ViewType::Calendar {
            let by =
                find(&[PropType::Date]).or_else(|| add_property(&self.coll, PropType::Date, cx));
            set_format(&mut v, "calendar_by", json!(by));
        }
        self.view_id = v.id.clone();
        write(cx, move |s| s.update_view(&v));
    }

    fn duplicate_view(&mut self, cx: &mut Context<Self>) {
        let src = self.view().clone();
        let Ok(mut v) =
            db::store(cx).create_view(&self.coll, src.kind, &format!("{} copy", src.name))
        else {
            return;
        };
        v.query = src.query;
        v.format = src.format;
        self.view_id = v.id.clone();
        write(cx, move |s| s.update_view(&v));
    }

    fn delete_view(&mut self, cx: &mut Context<Self>) {
        if self.views.len() > 1 {
            let id = self.view_id.clone();
            write(cx, move |s| s.delete_view(&id));
        }
    }

    fn start_rename(&mut self, target: Rename, window: &mut Window, cx: &mut Context<Self>) {
        let name = match &target {
            Rename::View(id) => self
                .views
                .iter()
                .find(|v| v.id == *id)
                .map(|v| v.name.clone()),
            Rename::Prop(id) => self.schema.get(id).map(|p| p.name.clone()),
        };
        self.rename.update(cx, |s, cx| {
            s.set_value(name.unwrap_or_default(), window, cx);
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        self.renaming = Some(target);
        cx.notify();
    }

    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.renaming.take() else {
            return;
        };
        let name = self.rename.read(cx).value().trim().to_string();
        cx.notify();
        if name.is_empty() {
            return;
        }
        match target {
            Rename::View(id) => {
                if let Some(mut v) = self.views.iter().find(|v| v.id == id).cloned() {
                    v.name = name;
                    write(cx, move |s| s.update_view(&v));
                }
            }
            Rename::Prop(id) => {
                if let Some(mut p) = self.schema.get(&id).cloned() {
                    p.name = name;
                    let coll = self.coll.clone();
                    write(cx, move |s| s.update_property(&coll, &id, p));
                }
            }
        }
    }

    /// New row with initial values; table rows start title editing, other views open the page.
    fn new_row(
        &mut self,
        values: Vec<(String, Value)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = db::store(cx);
        let created = (|| {
            let row = store.add_row(&self.coll)?;
            for (prop, v) in values {
                store.set_row_value(&row.id, &prop, v)?;
            }
            anyhow::Ok(row.id)
        })();
        db::changed(cx);
        let Ok(id) = created.map_err(|e| eprintln!("new row failed: {e:#}")) else {
            return;
        };
        if self.view().kind == ViewType::Table {
            self.editor
                .update(cx, |e, cx| e.start(&id, "title", window, cx));
            if let Some(ix) = self.shown.iter().position(|r| r.id == id) {
                self.scroll.scroll_to_item(ix, ScrollStrategy::Center);
            }
        } else {
            db::open_page(id, cx);
        }
    }

    fn add_property_here(&mut self, kind: PropType, window: &mut Window, cx: &mut Context<Self>) {
        let order = calc::ordered_props(&self.schema, &self.view().format);
        let Some(id) = add_property(&self.coll, kind, cx) else {
            return;
        };
        let mut order: Vec<Value> = order.into_iter().map(Value::String).collect();
        order.push(json!(id));
        self.save_view(cx, |v| set_format(v, "order", Value::Array(order)));
        self.start_rename(Rename::Prop(id), window, cx);
    }

    fn toggle_hidden(&mut self, prop: &str, cx: &mut Context<Self>) {
        let prop = prop.to_string();
        self.save_view(cx, move |v| {
            let mut hidden: Vec<Value> = v
                .format
                .get("hidden")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            match hidden.iter().position(|h| h.as_str() == Some(&prop)) {
                Some(i) => {
                    hidden.remove(i);
                }
                None => hidden.push(json!(prop)),
            }
            set_format(v, "hidden", Value::Array(hidden));
        });
    }

    /// Visible properties in view order.
    fn visible_props(&self) -> Vec<(String, Property)> {
        let format = &self.view().format;
        calc::ordered_props(&self.schema, format)
            .into_iter()
            .filter(|id| id == "title" || !calc::is_hidden(format, id))
            .filter_map(|id| self.schema.get(&id).cloned().map(|p| (id, p)))
            .collect()
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let tabs = self.views.iter().map(|v| {
            let active = v.id == self.view_id;
            let id = v.id.clone();
            let renaming = self.renaming == Some(Rename::View(v.id.clone()));
            let icon = match v.kind {
                ViewType::Board => IconName::LayoutDashboard,
                ViewType::List => IconName::Menu,
                ViewType::Gallery => IconName::GalleryVerticalEnd,
                ViewType::Calendar => IconName::Calendar,
                _ => IconName::FileText,
            };
            let entity = entity.clone();
            h_flex()
                .id(SharedString::from(format!("tab-{}", v.id)))
                .gap_1()
                .px_2()
                .py_1()
                .text_sm()
                .rounded(cx.theme().radius)
                .when(active, |d| {
                    d.font_weight(FontWeight::SEMIBOLD)
                        .border_b_2()
                        .border_color(cx.theme().foreground)
                })
                .when(!active, |d| d.text_color(cx.theme().muted_foreground))
                .hover(|s| s.bg(cx.theme().secondary))
                .child(icon)
                .map(|d| {
                    if renaming {
                        d.child(div().w(px(140.)).child(Input::new(&self.rename).xsmall()))
                    } else {
                        d.child(v.name.clone())
                    }
                })
                .on_click(cx.listener(move |this, _, _, cx| this.select_view(id.clone(), cx)))
                .context_menu(move |menu, window, _| view_menu(menu, &entity, window))
        });
        let view = cx.entity();
        h_flex()
            .gap_1()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .children(tabs)
            .child(
                Button::new("add-view")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .dropdown_menu(move |mut menu, window, _| {
                        for (kind, label) in [
                            (ViewType::Table, "Table"),
                            (ViewType::Board, "Board"),
                            (ViewType::List, "List"),
                            (ViewType::Gallery, "Gallery"),
                            (ViewType::Calendar, "Calendar"),
                        ] {
                            menu = menu.item(PopupMenuItem::new(label).on_click(
                                window.listener_for(&view, move |this, _, _, cx| {
                                    this.add_view(kind, cx)
                                }),
                            ));
                        }
                        menu
                    }),
            )
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let v = self.view();
        let props: Vec<(String, Property)> = calc::ordered_props(&self.schema, &v.format)
            .into_iter()
            .filter_map(|id| self.schema.get(&id).cloned().map(|p| (id, p)))
            .collect();
        let filter_count = v.query.filters.len();
        let sort_count = v.query.sorts.len();
        let (p1, p2, p3, p4) = (props.clone(), props.clone(), props.clone(), props);
        let (v1, v2, v3) = (view.clone(), view.clone(), view.clone());
        let group = v.query.group_by.clone();
        let format = v.format.clone();
        h_flex()
            .gap_1()
            .child(
                Button::new("filter")
                    .ghost()
                    .xsmall()
                    .label(if filter_count > 0 {
                        format!("Filter {filter_count}")
                    } else {
                        "Filter".into()
                    })
                    .dropdown_menu(move |mut menu, window, _| {
                        for (id, p) in &p1 {
                            let id = id.clone();
                            menu = menu.item(
                                PopupMenuItem::new(p.name.clone())
                                    .icon(type_icon(p.kind))
                                    .on_click(
                                        window.listener_for(&v1, move |this, _, window, cx| {
                                            this.add_filter(&id, window, cx)
                                        }),
                                    ),
                            );
                        }
                        menu
                    }),
            )
            .child(
                Button::new("sort")
                    .ghost()
                    .xsmall()
                    .label(if sort_count > 0 {
                        format!("Sort {sort_count}")
                    } else {
                        "Sort".into()
                    })
                    .dropdown_menu(move |mut menu, window, _| {
                        for (id, p) in &p2 {
                            let id = id.clone();
                            menu = menu.item(
                                PopupMenuItem::new(p.name.clone())
                                    .icon(type_icon(p.kind))
                                    .on_click(window.listener_for(&v2, move |this, _, _, cx| {
                                        this.add_sort(&id, cx)
                                    })),
                            );
                        }
                        menu
                    }),
            )
            .when(v.kind == ViewType::Board, |d| {
                let name = group
                    .as_ref()
                    .and_then(|g| self.schema.get(g))
                    .map_or("None".to_string(), |p| p.name.clone());
                d.child(
                    Button::new("group")
                        .ghost()
                        .xsmall()
                        .label(format!("Group: {name}"))
                        .dropdown_menu(move |mut menu, window, _| {
                            for (id, p) in p3.iter().filter(|(_, p)| {
                                matches!(p.kind, PropType::Select | PropType::Status)
                            }) {
                                let id = id.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(p.name.clone())
                                        .checked(group.as_ref() == Some(&id))
                                        .on_click(window.listener_for(
                                            &v3,
                                            move |this, _, _, cx| {
                                                let id = id.clone();
                                                this.save_view(cx, |v| v.query.group_by = Some(id));
                                            },
                                        )),
                                );
                            }
                            menu
                        }),
                )
            })
            .when(v.kind == ViewType::Calendar, |d| {
                let by = format
                    .get("calendar_by")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let name = by
                    .as_ref()
                    .and_then(|g| self.schema.get(g))
                    .map_or("None".to_string(), |p| p.name.clone());
                let view = view.clone();
                d.child(
                    Button::new("calendar-by")
                        .ghost()
                        .xsmall()
                        .label(format!("By: {name}"))
                        .dropdown_menu(move |mut menu, window, _| {
                            for (id, p) in p4.iter().filter(|(_, p)| p.kind == PropType::Date) {
                                let id = id.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(p.name.clone())
                                        .checked(by.as_ref() == Some(&id))
                                        .on_click(window.listener_for(
                                            &view,
                                            move |this, _, _, cx| {
                                                let id = id.clone();
                                                this.save_view(cx, |v| {
                                                    set_format(v, "calendar_by", json!(id))
                                                });
                                            },
                                        )),
                                );
                            }
                            menu
                        }),
                )
            })
            .map(|d| {
                if self.search_open {
                    d.child(
                        div()
                            .w(px(180.))
                            .child(Input::new(&self.search).xsmall().cleanable(true)),
                    )
                } else {
                    d
                }
            })
            .child(
                Button::new("search")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Search)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.search_open = !this.search_open;
                        if this.search_open {
                            this.search.update(cx, |s, cx| s.focus(window, cx));
                        } else {
                            this.search.update(cx, |s, cx| s.set_value("", window, cx));
                            this.recompute(cx);
                        }
                        cx.notify();
                    })),
            )
            .child(
                Button::new("view-menu")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Ellipsis)
                    .dropdown_menu({
                        let view = view.clone();
                        move |menu, window, cx| {
                            let menu = view_menu(menu, &view, window);
                            let this = view.read(cx);
                            let format = this.view().format.clone();
                            let props: Vec<(String, String)> =
                                calc::ordered_props(&this.schema, &format)
                                    .into_iter()
                                    .filter(|id| id != "title")
                                    .filter_map(|id| {
                                        this.schema.get(&id).map(|p| (id, p.name.clone()))
                                    })
                                    .collect();
                            let view = view.clone();
                            menu.separator().submenu(
                                "Properties",
                                window,
                                cx,
                                move |mut m, window, _| {
                                    for (id, name) in &props {
                                        let id = id.clone();
                                        m = m.item(
                                            PopupMenuItem::new(name.clone())
                                                .checked(!calc::is_hidden(&format, &id))
                                                .on_click(window.listener_for(
                                                    &view,
                                                    move |this, _, _, cx| {
                                                        this.toggle_hidden(&id, cx)
                                                    },
                                                )),
                                        );
                                    }
                                    m
                                },
                            )
                        }
                    }),
            )
            .child(
                Button::new("new-row")
                    .primary()
                    .xsmall()
                    .label("New")
                    .on_click(cx.listener(|this, _, window, cx| this.new_row(vec![], window, cx))),
            )
    }
}

/// Right-click menu of a row; Delete moves the row page to the Trash.
pub(super) fn row_menu(
    row: &str,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    let row = row.to_string();
    move |menu, _, _| {
        let row = row.clone();
        menu.item(
            PopupMenuItem::new("Delete")
                .icon(IconName::Delete)
                .on_click(move |_, _, cx| {
                    let row = row.clone();
                    write(cx, move |s| s.trash(&row));
                }),
        )
    }
}

/// Rename / duplicate / delete items for the current view.
fn view_menu(menu: PopupMenu, view: &Entity<DatabaseView>, window: &mut Window) -> PopupMenu {
    menu.item(
        PopupMenuItem::new("Rename view").on_click(window.listener_for(
            view,
            |this, _, window, cx| {
                let id = this.view_id.clone();
                this.start_rename(Rename::View(id), window, cx);
            },
        )),
    )
    .item(
        PopupMenuItem::new("Duplicate view")
            .icon(IconName::Copy)
            .on_click(window.listener_for(view, |this, _, _, cx| this.duplicate_view(cx))),
    )
    .item(
        PopupMenuItem::new("Delete view")
            .icon(IconName::Delete)
            .on_click(window.listener_for(view, |this, _, _, cx| this.delete_view(cx))),
    )
}

impl Render for DatabaseView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.views.is_empty() {
            return div().into_any_element();
        }
        let body = match self.view().kind {
            ViewType::Board => self.render_board(cx),
            ViewType::List => self.render_list(cx),
            ViewType::Gallery => self.render_gallery(cx),
            ViewType::Calendar => self.render_calendar(window, cx),
            _ => self.render_table(cx),
        };
        v_flex()
            .w_full()
            .min_w_0()
            .gap_1()
            .when(self.full_page, |d| d.size_full().min_h_0())
            .child(
                div()
                    .text_size(if self.full_page { px(28.) } else { px(18.) })
                    .font_weight(FontWeight::BOLD)
                    .child(Input::new(&self.title).appearance(false)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .pb_1()
                    .child(self.render_tabs(cx))
                    .child(self.render_toolbar(cx)),
            )
            .child(self.render_query_bar(cx))
            .child(body)
            .into_any_element()
    }
}
