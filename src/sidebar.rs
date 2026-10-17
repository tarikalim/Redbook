// Sidebar: workspace switcher, favorites, private page tree (drag to reorder/nest), trash.
use std::collections::{HashMap, HashSet};
use std::ops::Range;

use anyhow::Result;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::{self, title_or_untitled};
use crate::db;
use crate::model::{Block, PageNode, Workspace};
use crate::search::{self, Mode};
use crate::store::Store;
use crate::sync;

const ROW_H: Pixels = px(28.);

const EMOJIS: &[&str] = &[
    "📝", "📚", "🏠", "💼", "🚀", "⭐", "🔥", "💡", "🎯", "📌", "🗂️", "📦", "🧠", "🛠️", "🎨", "🌱",
    "🌍", "🍀", "🐱", "🐶", "🦊", "🐼", "🎵", "🎮", "⚽", "☕", "🍕", "❤️", "✅", "📅", "🔒", "🧪",
];

#[derive(Clone)]
enum Row {
    Header(&'static str),
    Favorite(PageNode),
    Page { node: PageNode, depth: usize },
    NoPages(usize),
    AddPage,
}

#[derive(Clone, Copy, PartialEq)]
enum Zone {
    Before,
    Inside,
    After,
}

/// Drag payload of a page row; also renders the drag preview.
#[derive(Clone)]
struct DraggedPage {
    id: String,
    title: SharedString,
}

impl Render for DraggedPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_3()
            .py_1()
            .rounded(px(6.))
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .text_sm()
            .opacity(0.9)
            .child(self.title.clone())
    }
}

pub struct Sidebar {
    workspaces: Vec<Workspace>,
    ws: Option<String>,
    /// Children by parent page id ("" = workspace root), from one `page_tree` read.
    tree: HashMap<String, Vec<PageNode>>,
    parents: HashMap<String, String>,
    favorites: Vec<PageNode>,
    expanded: HashSet<String>,
    rows: Vec<Row>,
    active: Option<String>,
    drop: Option<(String, Zone)>,
    /// Row key that last had the mouse; only it builds the action buttons.
    hovered: Option<String>,
    scroll: UniformListScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl Sidebar {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let nav = db::nav(cx);
        let subscriptions = vec![
            cx.observe(&db::revision(cx), |this, _, cx| this.reload(cx)),
            cx.observe(&nav, |this, nav, cx| {
                let (ws, page) = {
                    let n = nav.read(cx);
                    (n.workspace.clone(), n.page.clone())
                };
                this.active = page;
                if ws != this.ws {
                    this.reload(cx);
                }
                this.reveal_active();
                this.rebuild();
                cx.notify();
            }),
        ];
        let mut this = Self {
            workspaces: vec![],
            ws: None,
            tree: HashMap::new(),
            parents: HashMap::new(),
            favorites: vec![],
            expanded: HashSet::new(),
            rows: vec![],
            active: nav.read(cx).page.clone(),
            drop: None,
            hovered: None,
            scroll: UniformListScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        this.reload(cx);
        this.reveal_active();
        this.rebuild();
        this
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let store = db::store(cx);
        self.ws = db::nav(cx).read(cx).workspace.clone();
        self.workspaces = store.workspaces().unwrap_or_default();
        self.tree.clear();
        self.parents.clear();
        self.favorites.clear();
        if let Some(ws) = &self.ws {
            for n in store.page_tree(ws).unwrap_or_default() {
                if let Some(p) = &n.parent_id {
                    self.parents.insert(n.id.clone(), p.clone());
                }
                self.tree
                    .entry(n.parent_id.clone().unwrap_or_default())
                    .or_default()
                    .push(n);
            }
            self.favorites = store.favorites(ws).unwrap_or_default();
        }
        self.rebuild();
        cx.notify();
    }

    fn reveal_active(&mut self) {
        let mut id = self.active.clone();
        while let Some(p) = id.and_then(|i| self.parents.get(&i).cloned()) {
            self.expanded.insert(p.clone());
            id = Some(p);
        }
    }

    fn rebuild(&mut self) {
        let mut rows = vec![];
        if !self.favorites.is_empty() {
            rows.push(Row::Header("Favorites"));
            rows.extend(self.favorites.iter().cloned().map(Row::Favorite));
        }
        rows.push(Row::Header("Private"));
        let start = rows.len();
        self.push_children("", 0, &mut rows);
        if rows.len() == start {
            rows.push(Row::AddPage);
        }
        self.rows = rows;
    }

    fn push_children(&self, parent: &str, depth: usize, rows: &mut Vec<Row>) {
        for n in self.tree.get(parent).into_iter().flatten() {
            rows.push(Row::Page {
                node: n.clone(),
                depth,
            });
            if self.expanded.contains(&n.id) {
                if self.tree.contains_key(&n.id) {
                    self.push_children(&n.id, depth + 1, rows);
                } else if !n.kind.is_collection() {
                    rows.push(Row::NoPages(depth + 1));
                }
            }
        }
    }

    fn toggle_expand(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.to_string());
        }
        self.rebuild();
        cx.notify();
    }

    fn drop_page(&mut self, dragged: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((target, zone)) = self.drop.take() else {
            return;
        };
        if target == dragged {
            return cx.notify();
        }
        let store = db::store(cx);
        let roots: Vec<String> = self
            .tree
            .get("")
            .into_iter()
            .flatten()
            .map(|n| n.id.clone())
            .collect();
        let nest =
            zone == Zone::Inside && store.block(&target).is_ok_and(|t| !t.kind.is_collection());
        let r = if nest {
            store.move_block(dragged, Some(&target), usize::MAX)
        } else {
            move_next_to(&store, dragged, &target, zone == Zone::Before, roots)
        };
        if app::done(r, window, cx).is_some() && nest {
            self.expanded.insert(target);
        }
    }

    fn render_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        range
            .filter_map(|ix| self.rows.get(ix).cloned())
            .map(|row| match row {
                Row::Header(label) => self.render_header(label, cx),
                Row::Favorite(node) => self.render_page(&node, 0, true, cx),
                Row::Page { node, depth } => self.render_page(&node, depth, false, cx),
                Row::NoPages(depth) => div()
                    .h(ROW_H)
                    .flex()
                    .items_center()
                    .pl(indent(depth) + px(24.))
                    .text_color(cx.theme().muted_foreground)
                    .child("No pages inside")
                    .into_any_element(),
                Row::AddPage => h_flex()
                    .id("sb-add-page")
                    .h(ROW_H)
                    .mx_1()
                    .px_2()
                    .gap_2()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_color(cx.theme().muted_foreground)
                    .hover(|s| s.bg(cx.theme().sidebar_accent))
                    .child(Icon::new(IconName::Plus).small())
                    .child("Add a page")
                    .on_click(|_, window, cx| app::new_page(None, window, cx))
                    .into_any_element(),
            })
            .collect()
    }

    fn render_header(&self, label: &'static str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let private = label == "Private";
        h_flex()
            .id(label)
            .group("sb-header")
            .h(ROW_H)
            .mx_1()
            .px_2()
            .pt_1()
            .rounded(px(6.))
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.muted_foreground)
            .child(div().flex_1().child(label))
            .when(private, |el| {
                el.child(
                    div()
                        .invisible()
                        .group_hover("sb-header", |s| s.visible())
                        .child(
                            Button::new("sb-add-root")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Plus)
                                .tooltip("Add a page")
                                .on_click(|_, window, cx| app::new_page(None, window, cx)),
                        ),
                )
                .drag_over::<DraggedPage>(|s, _, _, cx| s.bg(cx.theme().drop_target))
                .on_drop(cx.listener(|_, d: &DraggedPage, window, cx| {
                    let r = db::store(cx).move_block(&d.id, None, usize::MAX);
                    app::done(r, window, cx);
                }))
            })
            .into_any_element()
    }

    fn render_page(
        &self,
        node: &PageNode,
        depth: usize,
        fav: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let id = node.id.clone();
        let key = format!("{}-{}", if fav { "fav" } else { "page" }, id);
        let active = self.active.as_deref() == Some(id.as_str());
        let expanded = self.expanded.contains(&id);
        let is_fav = fav || self.favorites.iter().any(|f| f.id == id);
        let zone = self
            .drop
            .as_ref()
            .filter(|(t, _)| !fav && *t == id && cx.has_active_drag())
            .map(|d| d.1);
        let title = title_or_untitled(&node.title);
        let icon = match &node.icon {
            Some(e) => div().w(px(18.)).child(e.clone()).into_any_element(),
            None => Icon::new(if node.kind.is_collection() {
                IconName::LayoutDashboard
            } else {
                IconName::FileText
            })
            .small()
            .text_color(theme.muted_foreground)
            .into_any_element(),
        };

        let toggle = {
            let id = id.clone();
            div()
                .id(SharedString::from(format!("tog-{key}")))
                .size(px(18.))
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .hover(|s| s.bg(theme.border))
                .child(
                    Icon::new(if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .xsmall(),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_expand(&id, cx)))
        };
        // Sticky: stays on the last hovered row so an open "more" menu survives leaving it.
        let show_actions = self.hovered.as_deref() == Some(key.as_str());
        let actions = show_actions.then(|| {
            let (menu_id, add_id) = (id.clone(), id.clone());
            h_flex()
                .flex_shrink_0()
                .invisible()
                .group_hover("sb-row", |s| s.visible())
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    Button::new(SharedString::from(format!("more-{key}")))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Ellipsis)
                        .dropdown_menu(move |menu, _, _| {
                            app::page_menu(menu.min_w(px(220.)), menu_id.clone(), is_fav)
                        }),
                )
                .when(!fav, |el| {
                    el.child(
                        Button::new(SharedString::from(format!("add-{key}")))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("Add a page inside")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.expanded.insert(add_id.clone());
                                app::new_page(Some(add_id.clone()), window, cx);
                            })),
                    )
                })
        });
        let hover_key = key.clone();

        let open_id = id.clone();
        let row = h_flex()
            .id(SharedString::from(key))
            .group("sb-row")
            .h(ROW_H)
            .mx_1()
            .pl(indent(depth))
            .pr_1()
            .gap_1()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|s| s.bg(theme.sidebar_accent))
            .when(active, |el| {
                el.bg(theme.sidebar_accent)
                    .text_color(theme.sidebar_accent_foreground)
                    .font_weight(FontWeight::MEDIUM)
            })
            .when(!fav, |el| el.child(toggle))
            .child(icon)
            .child(div().flex_1().min_w_0().truncate().child(title.clone()))
            .children(actions)
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered && this.hovered.as_deref() != Some(hover_key.as_str()) {
                    this.hovered = Some(hover_key.clone());
                    cx.notify();
                }
            }))
            .on_click(move |_, _, cx| db::open_page(open_id.clone(), cx))
            .when_some(zone, |el, z| match z {
                Zone::Before => el.border_t_2().border_color(theme.primary),
                Zone::After => el.border_b_2().border_color(theme.primary),
                Zone::Inside => el.bg(theme.drop_target),
            });
        if fav {
            return row.into_any_element();
        }
        row.on_drag(
            DraggedPage {
                id: id.clone(),
                title,
            },
            |d, _, _, cx| cx.new(|_| d.clone()),
        )
        .on_drag_move(
            cx.listener(move |this, ev: &DragMoveEvent<DraggedPage>, _, cx| {
                if !ev.bounds.contains(&ev.event.position) {
                    return;
                }
                let f = (ev.event.position.y - ev.bounds.origin.y) / ev.bounds.size.height;
                let zone = if f < 0.25 {
                    Zone::Before
                } else if f > 0.75 {
                    Zone::After
                } else {
                    Zone::Inside
                };
                let target = Some((id.clone(), zone));
                if this.drop != target {
                    this.drop = target;
                    cx.notify();
                }
            }),
        )
        .on_drop(cx.listener(|this, d: &DraggedPage, window, cx| this.drop_page(&d.id, window, cx)))
        .into_any_element()
    }

    fn render_switcher(&self) -> impl IntoElement {
        let current = self
            .workspaces
            .iter()
            .find(|w| Some(&w.id) == self.ws.as_ref());
        let label = current.map_or("No workspace".to_string(), |w| {
            format!("{} {}", w.icon.as_deref().unwrap_or("🏠"), w.name)
        });
        let list: Vec<(String, String, bool)> = self
            .workspaces
            .iter()
            .map(|w| {
                let name = format!("{} {}", w.icon.as_deref().unwrap_or("🏠"), w.name);
                (w.id.clone(), name, Some(&w.id) == self.ws.as_ref())
            })
            .collect();
        let ws = current.cloned();
        h_flex().h(px(44.)).px_2().child(
            Button::new("ws-switcher")
                .ghost()
                .w_full()
                .label(label)
                .dropdown_menu(move |mut menu, _, _| {
                    menu = menu.min_w(px(240.)).label("Workspaces");
                    for (id, name, active) in &list {
                        let id = id.clone();
                        menu = menu.item(
                            PopupMenuItem::new(name.clone())
                                .checked(*active)
                                .on_click(move |_, _, cx| app::open_workspace(id.clone(), cx)),
                        );
                    }
                    menu = menu.separator().item(
                        PopupMenuItem::new("New workspace")
                            .icon(IconName::Plus)
                            .on_click(|_, window, cx| new_workspace(window, cx)),
                    );
                    let Some(ws) = &ws else { return menu };
                    let (a, b, c) = (ws.clone(), ws.clone(), ws.clone());
                    menu.item(
                        PopupMenuItem::new("Rename workspace")
                            .icon(IconName::Replace)
                            .on_click(move |_, window, cx| rename_workspace(&a, window, cx)),
                    )
                    .item(
                        PopupMenuItem::new("Change icon")
                            .icon(IconName::Palette)
                            .on_click(move |_, window, cx| {
                                workspace_icon(b.id.clone(), window, cx)
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Delete workspace")
                            .icon(IconName::Delete)
                            .on_click(move |_, window, cx| delete_workspace(&c, window, cx)),
                    )
                }),
        )
    }
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let switcher = self.render_switcher();
        let theme = cx.theme();
        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .text_sm()
            .child(switcher)
            .child(
                v_flex()
                    .pb_2()
                    .child(nav_item(
                        "sb-search",
                        IconName::Search,
                        "Search",
                        "⌘P",
                        cx,
                        |_, window, cx| search::open(Mode::Search, window, cx),
                    ))
                    .child(nav_item(
                        "sb-settings",
                        IconName::Settings,
                        "Settings",
                        "",
                        cx,
                        |_, window, cx| open_settings(window, cx),
                    ))
                    .child(nav_item(
                        "sb-new",
                        IconName::Plus,
                        "New page",
                        "⌘N",
                        cx,
                        |_, window, cx| app::new_page(None, window, cx),
                    )),
            )
            .child(
                div().flex_1().min_h_0().child(
                    uniform_list(
                        "sidebar-rows",
                        self.rows.len(),
                        cx.processor(|this, range, _, cx| this.render_rows(range, cx)),
                    )
                    .size_full()
                    .track_scroll(&self.scroll),
                ),
            )
            .child(
                v_flex()
                    .py_1()
                    .border_t_1()
                    .border_color(theme.sidebar_border)
                    .child(nav_item(
                        "sb-trash",
                        IconName::Delete,
                        "Trash",
                        "",
                        cx,
                        |_, window, cx| open_trash(window, cx),
                    )),
            )
    }
}

fn indent(depth: usize) -> Pixels {
    px(4. + depth as f32 * 14.)
}

fn nav_item(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    hint: &'static str,
    cx: &App,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .id(id)
        .h(ROW_H)
        .mx_1()
        .px_2()
        .gap_2()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(theme.sidebar_accent))
        .child(Icon::new(icon).small().text_color(theme.muted_foreground))
        .child(div().flex_1().child(label))
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(hint),
        )
        .on_click(on_click)
}

/// Moves `id` right before or after `target` among the target's real siblings.
fn move_next_to(
    store: &Store,
    id: &str,
    target: &str,
    before: bool,
    roots: Vec<String>,
) -> Result<()> {
    let parent = store.block(target)?.parent_id;
    let sibs: Vec<String> = match &parent {
        Some(p) => store.children(p)?.into_iter().map(|b| b.id).collect(),
        None => roots,
    };
    let sibs: Vec<&String> = sibs.iter().filter(|s| *s != id).collect();
    let ix = sibs.iter().position(|s| *s == target).unwrap_or(sibs.len()) + usize::from(!before);
    store.move_block(id, parent.as_deref(), ix)
}

// ---- workspace dialogs

fn new_workspace(window: &mut Window, cx: &mut App) {
    app::prompt_text(
        "New workspace",
        String::new(),
        window,
        cx,
        |name, window, cx| {
            let name = if name.is_empty() {
                "Untitled workspace".to_string()
            } else {
                name
            };
            if let Some(ws) = app::done(db::store(cx).create_workspace(&name), window, cx) {
                app::open_workspace(ws.id, cx);
            }
        },
    );
}

fn rename_workspace(ws: &Workspace, window: &mut Window, cx: &mut App) {
    let id = ws.id.clone();
    app::prompt_text(
        "Rename workspace",
        ws.name.clone(),
        window,
        cx,
        move |name, window, cx| {
            if !name.is_empty() {
                let r = db::store(cx).rename_workspace(&id, &name);
                app::done(r, window, cx);
            }
        },
    );
}

fn workspace_icon(id: String, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, _| {
        let set = |icon: Option<&'static str>, id: String| {
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                let r = db::store(cx).set_workspace_icon(&id, icon);
                app::done(r, window, cx);
                window.close_dialog(cx);
            }
        };
        dialog
            .title("Workspace icon")
            .w(px(360.))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_1()
                    .children(EMOJIS.iter().enumerate().map(|(ix, e)| {
                        div()
                            .id(("emoji", ix))
                            .size(px(36.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .text_xl()
                            .cursor_pointer()
                            .hover(|s| s.bg(gpui_kit::black().opacity(0.08)))
                            .child(*e)
                            .on_click(set(Some(e), id.clone()))
                    })),
            )
            .footer(
                h_flex().justify_end().child(
                    Button::new("icon-remove")
                        .label("Remove icon")
                        .on_click(set(None, id.clone())),
                ),
            )
    });
}

fn delete_workspace(ws: &Workspace, window: &mut Window, cx: &mut App) {
    let id = ws.id.clone();
    app::confirm(
        "Delete workspace?",
        format!(
            "“{}” and all of its pages will be deleted permanently.",
            ws.name
        )
        .into(),
        "Delete",
        window,
        cx,
        move |window, cx| {
            let store = db::store(cx);
            let r = store
                .delete_workspace(&id)
                .and_then(|_| app::ensure_workspace(&store, None));
            if let Some(next) = app::done(r, window, cx) {
                app::open_workspace(next, cx);
            }
        },
    );
}

/// One-line sync state for Settings, read once when the dialog opens.
fn sync_summary(cx: &App) -> String {
    let db = Store::default_path();
    if !sync::enabled(cx) {
        return "Off. Set up with `redbook sync init <private repo url>`.".into();
    }
    match sync::status(&db) {
        None => "On; not synced yet".into(),
        Some(st) => {
            let when = chrono::DateTime::from_timestamp_millis(st.time)
                .map(|t| {
                    t.with_timezone(&chrono::Local)
                        .format("%b %-d, %H:%M")
                        .to_string()
                })
                .unwrap_or_default();
            match st.error {
                Some(e) => format!("Last try {when} failed: {e}"),
                None => format!("Last sync {when}: {}", st.summary),
            }
        }
    }
}

fn change_sync_dir(window: &mut Window, cx: &mut App) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Use as sync folder".into()),
    });
    let handle = window.window_handle();
    cx.spawn(async move |cx: &mut AsyncApp| {
        let Ok(Ok(Some(paths))) = rx.await else {
            return;
        };
        let Some(path) = paths.into_iter().next() else {
            return;
        };
        let _ = handle.update(cx, |_, window, cx| {
            let msg = if sync::busy(cx) {
                "A sync is running; try again in a moment".to_string()
            } else {
                let db = Store::default_path();
                sync::paused(cx, || sync::switch_dir(&db, &path))
                    .unwrap_or_else(|e| format!("Sync folder not changed: {e:#}"))
            };
            window.push_notification(msg, cx);
        });
    })
    .detach();
}

fn open_settings(window: &mut Window, cx: &mut App) {
    let db_path = Store::default_path().display().to_string();
    let sync_dir = sync::dir(&Store::default_path()).display().to_string();
    let sync_line = sync_summary(cx);
    let sync_on = sync::enabled(cx);
    window.open_dialog(cx, move |dialog, _, cx| {
        let muted = cx.theme().muted_foreground;
        dialog
            .title("Settings")
            .w(px(460.))
            .child(
                v_flex()
                    .gap_3()
                    .text_sm()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().child("Appearance"))
                            .child(Button::new("theme-system").small().label("System").on_click(|_, window, cx| {
                                crate::app::set_theme("system", Some(window), cx)
                            }))
                            .child(Button::new("theme-light").small().label("Light").on_click(|_, window, cx| {
                                crate::app::set_theme("light", Some(window), cx)
                            }))
                            .child(Button::new("theme-dark").small().label("Dark").on_click(|_, window, cx| {
                                crate::app::set_theme("dark", Some(window), cx)
                            })),
                    )
                    .child(v_flex().child("Database").child(div().text_xs().text_color(muted).child(db_path.clone())))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(div().flex_1().child("Sync"))
                                    .when(sync_on, |el| {
                                        el.child(Button::new("sync-now").small().label("Sync now").on_click(|_, window, cx| {
                                            sync::launch(sync::Mode::Full, cx);
                                            window.push_notification("Syncing…", cx);
                                        }))
                                    })
                                    .child(Button::new("sync-dir").small().label("Change folder…").on_click(|_, window, cx| {
                                        change_sync_dir(window, cx)
                                    })),
                            )
                            .child(div().text_xs().text_color(muted).child(sync_dir.clone()))
                            .child(div().text_xs().text_color(muted).child(sync_line.clone())),
                    )
                    .child(
                        v_flex().child("Shortcuts").child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child("⌘N new page · ⌘P / ⌘K search · ⌘\\ sidebar · ⌘[ ⌘] back/forward · ⌘⇧L dark mode"),
                        ),
                    ),
            )
    });
}

// ---- trash

fn open_trash(window: &mut Window, cx: &mut App) {
    let list = cx.new(TrashList::new);
    window.open_dialog(cx, move |dialog, _, _| {
        dialog.title("Trash").w(px(480.)).child(list.clone())
    });
}

struct TrashList {
    items: Vec<Block>,
    _sub: Subscription,
}

impl TrashList {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            items: vec![],
            _sub: cx.observe(&db::revision(cx), |this, _, cx| this.reload(cx)),
        };
        this.reload(cx);
        this
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let ws = db::nav(cx).read(cx).workspace.clone();
        self.items = ws
            .and_then(|ws| db::store(cx).list_trash(&ws).ok())
            .unwrap_or_default();
        cx.notify();
    }
}

impl Render for TrashList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let rows = self.items.iter().map(|b| {
            let (open, restore, delete) = (b.id.clone(), b.id.clone(), b.id.clone());
            let title = title_or_untitled(&b.title.plain_text());
            h_flex()
                .id(SharedString::from(format!("trash-{}", b.id)))
                .h(ROW_H)
                .px_2()
                .gap_2()
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|s| s.bg(theme.secondary))
                .child(div().w(px(18.)).child(b.icon().unwrap_or("📄").to_string()))
                .child(div().flex_1().min_w_0().truncate().child(title))
                .child(
                    h_flex()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(
                            Button::new(SharedString::from(format!("restore-{}", b.id)))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Undo2)
                                .tooltip("Restore")
                                .on_click(move |_, window, cx| {
                                    let r = db::store(cx).restore(&restore);
                                    app::done(r, window, cx);
                                }),
                        )
                        .child(
                            Button::new(SharedString::from(format!("purge-{}", b.id)))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Delete)
                                .tooltip("Delete permanently")
                                .on_click(move |_, window, cx| {
                                    app::delete_forever(delete.clone(), window, cx)
                                }),
                        ),
                )
                .on_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    db::open_page(open.clone(), cx);
                })
        });
        v_flex()
            .gap_1()
            .min_h(px(120.))
            .text_sm()
            .child(
                h_flex()
                    .pb_1()
                    .child(
                        div()
                            .flex_1()
                            .text_color(theme.muted_foreground)
                            .child(format!("{} pages", self.items.len())),
                    )
                    .child(
                        Button::new("empty-trash")
                            .small()
                            .danger()
                            .label("Empty trash")
                            .disabled(self.items.is_empty())
                            .on_click(|_, window, cx| {
                                app::confirm(
                                    "Empty trash?",
                                    "All pages in Trash will be deleted permanently.".into(),
                                    "Empty",
                                    window,
                                    cx,
                                    |window, cx| {
                                        let ws = db::nav(cx).read(cx).workspace.clone();
                                        if let Some(ws) = ws {
                                            let r = db::store(cx).empty_trash(&ws);
                                            app::done(r, window, cx);
                                        }
                                    },
                                )
                            }),
                    ),
            )
            .when(self.items.is_empty(), |el| {
                el.child(
                    div()
                        .py_6()
                        .text_center()
                        .text_color(theme.muted_foreground)
                        .child("Trash is empty"),
                )
            })
            .children(rows)
    }
}
