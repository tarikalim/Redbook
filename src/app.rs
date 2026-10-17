// Root view: sidebar + top bar + page pane, plus page operations shared with the sidebar.
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, Theme, ThemeMode, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::database::{DatabaseView, RowProperties};
use crate::db;
use crate::editor::PageEditor;
use crate::model::{Block, BlockType, PageNode, RichText};
use crate::search::{self, Mode};
use crate::sidebar::Sidebar;
use crate::store::Store;

gpui_kit::actions!(
    workspace,
    [NewPage, OpenSearch, ToggleSidebar, GoBack, GoForward]
);

/// Key context of the main view; bindings scoped to it fire only while focus is inside.
pub const WORKSPACE_CONTEXT: &str = "Workspace";

const MAX_RECENT: usize = 20;

/// Last session state, kept next to the database file.
#[derive(Default, Serialize, Deserialize)]
struct Settings {
    workspace: Option<String>,
    sidebar_width: Option<f32>,
    #[serde(default)]
    sidebar_hidden: bool,
    #[serde(default)]
    recent: Vec<String>,
}

impl Settings {
    fn path() -> PathBuf {
        Store::default_path().with_extension("settings.json")
    }

    fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Keeps keys owned by others (`sync_dir`).
    fn save(&self) {
        let mut all: Value = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| Value::Object(Default::default()));
        if let (Some(all), Ok(Value::Object(mine))) =
            (all.as_object_mut(), serde_json::to_value(self))
        {
            all.extend(mine);
        }
        if let Ok(s) = serde_json::to_string_pretty(&all) {
            let _ = std::fs::write(Self::path(), s);
        }
    }
}

/// Saved theme choice: "light", "dark" or "system" (default). Kept outside `Settings` so its saves keep it.
fn saved_theme() -> String {
    std::fs::read_to_string(Settings::path())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["theme"].as_str().map(str::to_string))
        .unwrap_or_else(|| "system".into())
}

/// Applies the saved theme choice; "system" follows the OS appearance.
pub fn apply_theme(window: Option<&mut Window>, cx: &mut App) {
    match saved_theme().as_str() {
        "light" => Theme::change(ThemeMode::Light, window, cx),
        "dark" => Theme::change(ThemeMode::Dark, window, cx),
        _ => Theme::sync_system_appearance(window, cx),
    }
}

/// Saves and applies a theme choice.
pub fn set_theme(choice: &str, window: Option<&mut Window>, cx: &mut App) {
    let mut all: Value = std::fs::read_to_string(Settings::path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| Value::Object(Default::default()));
    all["theme"] = choice.into();
    if let Ok(s) = serde_json::to_string_pretty(&all) {
        let _ = std::fs::write(Settings::path(), s);
    }
    apply_theme(window, cx);
}

/// Open page data, reloaded on navigation and on every db revision.
struct PageView {
    block: Block,
    crumbs: Vec<PageNode>,
    is_row: bool,
}

/// Identity of the body view; a change recreates it.
#[derive(PartialEq)]
struct BodyKey {
    id: String,
    database: bool,
    row: bool,
}

#[derive(Clone)]
struct ResizeSidebar;

impl Render for ResizeSidebar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

pub struct AppView {
    focus_handle: FocusHandle,
    sidebar: Entity<Sidebar>,
    sidebar_width: Pixels,
    sidebar_hidden: bool,
    page: Option<PageView>,
    body: Option<AnyView>,
    body_key: Option<BodyKey>,
    current: Option<String>,
    back: Vec<String>,
    forward: Vec<String>,
    from_history: bool,
    settings: Settings,
    /// The blank launch page while it is still untouched.
    draft: Option<String>,
    /// Focus taken away while the window is inactive, so the cursor stops blinking.
    parked_focus: Option<FocusHandle>,
    _db_watch: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl AppView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut settings = Settings::load();
        let store = db::store(cx);
        let ws = ensure_workspace(&store, settings.workspace.as_deref()).expect("create workspace");
        // Every launch starts on a blank page, ready to type into.
        let draft = new_draft(&store, &ws).ok();
        let page = draft.clone().or_else(|| first_page(&store, &ws));
        cx.set_global(search::Recent(settings.recent.clone()));
        settings.workspace = Some(ws.clone());
        settings.save();
        let nav = db::nav(cx);
        nav.update(cx, |n, _| {
            n.workspace = Some(ws);
            n.page = page.clone();
        });

        let subscriptions = vec![
            cx.observe_in(&nav, window, |this, _, window, cx| this.on_nav(window, cx)),
            cx.observe_in(&db::revision(cx), window, |this, _, window, cx| {
                this.reload(window, cx)
            }),
            cx.observe_window_appearance(window, |_, window, cx| apply_theme(Some(window), cx)),
            cx.observe_window_activation(window, |this, window, cx| this.on_activation(window, cx)),
            cx.observe(&db::revision(cx), |_, _, cx| crate::sync::changed(cx)),
            cx.on_app_quit(|this, cx| {
                this.leave_draft(cx);
                crate::sync::quit(cx);
                async {}
            }),
        ];

        // The MCP server writes the same file; reload all views when another connection commits.
        let mut db_watch: Vec<_> = db::watch(&Store::default_path(), cx).into_iter().collect();
        let watched = !db_watch.is_empty();
        // Safety net for missed events (or no watcher): slow, and idle while inactive.
        db_watch.push(cx.spawn_in(window, async move |_, cx| {
            loop {
                let active = cx.update(|w, _| w.is_window_active()).unwrap_or(false);
                let secs = match (watched, active) {
                    (false, true) => 1,
                    (false, false) => 5,
                    (true, _) => 30,
                };
                cx.background_executor()
                    .timer(Duration::from_secs(secs))
                    .await;
                let alive = cx.update(|w, cx| {
                    if !watched || w.is_window_active() {
                        db::sync_external(cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        }));

        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let mut this = Self {
            focus_handle,
            sidebar: cx.new(|cx| Sidebar::new(window, cx)),
            sidebar_width: px(settings.sidebar_width.unwrap_or(248.)),
            sidebar_hidden: settings.sidebar_hidden,
            page: None,
            body: None,
            body_key: None,
            current: page,
            back: vec![],
            forward: vec![],
            from_history: false,
            settings,
            draft,
            parked_focus: None,
            _db_watch: db_watch,
            _subscriptions: subscriptions,
        };
        this.reload(window, cx);
        if this.draft.is_some()
            && let Some(editor) = this.editor()
        {
            editor.update(cx, |e, cx| e.focus_title(window, cx));
        }
        this
    }

    fn editor(&self) -> Option<Entity<PageEditor>> {
        self.body.clone()?.downcast::<PageEditor>().ok()
    }

    /// Deletes the launch page if it was left untouched; true when it was.
    fn leave_draft(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.draft.take() else {
            return false;
        };
        // Typing still in the save debounce counts as an edit.
        if let Some(editor) = self.editor() {
            editor.update(cx, |e, cx| e.flush(cx));
        }
        let store = db::store(cx);
        if !is_blank(&store, &id) {
            let _ = store.set_prop(&id, DRAFT, Value::Null);
            return false;
        }
        if store.delete_forever(&id).is_err() {
            return false;
        }
        db::changed(cx);
        true
    }

    /// A focused input blinks (and repaints) even in an inactive window; park focus meanwhile.
    fn on_activation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.is_window_active() {
            self.parked_focus = window.focused(cx);
            if self.parked_focus.is_some() {
                window.blur(cx);
            }
        } else {
            crate::sync::activated(cx);
            if let Some(handle) = self.parked_focus.take()
                && window.focused(cx).is_none()
            {
                handle.focus(window, cx);
            }
        }
    }

    fn on_nav(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (ws, page) = {
            let n = db::nav(cx).read(cx);
            (n.workspace.clone(), n.page.clone())
        };
        if page != self.current {
            let deleted = self.leave_draft(cx);
            if !self.from_history {
                self.back.extend(self.current.take().filter(|_| !deleted));
                self.forward.clear();
            }
            self.from_history = false;
            self.current = page.clone();
            if let Some(p) = &page {
                let recent = &mut self.settings.recent;
                recent.retain(|r| r != p);
                recent.insert(0, p.clone());
                recent.truncate(MAX_RECENT);
                cx.set_global(search::Recent(recent.clone()));
            }
        }
        self.settings.workspace = ws;
        self.settings.save();
        self.reload(window, cx);
    }

    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let store = db::store(cx);
        if let Some(id) = &self.draft
            && !is_blank(&store, id)
        {
            let _ = store.set_prop(id, DRAFT, Value::Null);
            self.draft = None;
        }
        self.page = self.current.as_ref().and_then(|id| {
            let block = store.block(id).ok()?;
            let is_row = block
                .parent_id
                .as_ref()
                .and_then(|p| store.block(p).ok())
                .is_some_and(|p| p.kind.is_collection());
            Some(PageView {
                crumbs: store.ancestors(id).unwrap_or_default(),
                is_row,
                block,
            })
        });
        self.sync_body(window, cx);
        // Deleted forever (here or by the MCP server): fall back to the first page.
        if self.page.is_none() && self.current.is_some() {
            let nav = db::nav(cx);
            let first = nav
                .read(cx)
                .workspace
                .as_ref()
                .and_then(|ws| first_page(&store, ws));
            nav.update(cx, |n, cx| {
                n.page = first;
                cx.notify();
            });
        }
        cx.notify();
    }

    /// Recreates the editor / database view when the page or its kind changes.
    fn sync_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.page.as_ref().map(|p| BodyKey {
            id: p.block.id.clone(),
            database: p.block.kind == BlockType::CollectionViewPage,
            row: p.is_row,
        });
        if key == self.body_key {
            return;
        }
        self.body = key.as_ref().map(|k| {
            let id = k.id.clone();
            if k.database {
                return cx.new(|cx| DatabaseView::new(id, window, cx)).into();
            }
            let editor = cx.new(|cx| PageEditor::new(id.clone(), window, cx));
            if k.row {
                let props = cx.new(|cx| RowProperties::new(id, window, cx));
                editor.update(cx, |e, cx| e.set_header(props.into(), cx));
            }
            editor.into()
        });
        self.body_key = key;
    }

    fn go(&mut self, back: bool, cx: &mut Context<Self>) {
        let store = db::store(cx);
        loop {
            let next = if back {
                self.back.pop()
            } else {
                self.forward.pop()
            };
            let Some(id) = next else { return };
            let Ok(b) = store.block(&id) else { continue };
            if let Some(cur) = self.current.clone() {
                if back {
                    &mut self.forward
                } else {
                    &mut self.back
                }
                .push(cur);
            }
            self.from_history = true;
            db::nav(cx).update(cx, |n, cx| {
                n.workspace = Some(b.workspace_id);
                n.page = Some(id);
                cx.notify();
            });
            return;
        }
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_hidden = !self.sidebar_hidden;
        self.settings.sidebar_hidden = self.sidebar_hidden;
        self.settings.save();
        cx.notify();
    }

    fn render_top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let crumbs = self
            .page
            .as_ref()
            .map(|p| p.crumbs.clone())
            .unwrap_or_default();
        let last = crumbs.len().saturating_sub(1);
        let crumbs = crumbs.into_iter().enumerate().flat_map(|(ix, n)| {
            let sep = (ix > 0).then(|| {
                div()
                    .text_color(theme.muted_foreground)
                    .child("/")
                    .into_any_element()
            });
            let id = n.id.clone();
            let item = h_flex()
                .id(SharedString::from(format!("crumb-{}", n.id)))
                .min_w_0()
                .max_w(px(180.))
                .px_1()
                .gap_1()
                .rounded(px(4.))
                .cursor_pointer()
                .hover(|s| s.bg(theme.secondary))
                .when(ix == last, |e| e.flex_shrink_0())
                .children(n.icon.clone())
                .child(div().truncate().child(title_or_untitled(&n.title)))
                .on_click(move |_, _, cx| db::open_page(id.clone(), cx))
                .into_any_element();
            sep.into_iter().chain([item])
        });

        h_flex()
            .h(px(44.))
            .flex_shrink_0()
            .px_2()
            .gap_1()
            .text_sm()
            .child(
                Button::new("toggle-sidebar")
                    .ghost()
                    .small()
                    .icon(if self.sidebar_hidden {
                        IconName::PanelLeftOpen
                    } else {
                        IconName::PanelLeftClose
                    })
                    .tooltip("Toggle sidebar  ⌘\\")
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
            )
            .child(
                Button::new("go-back")
                    .ghost()
                    .small()
                    .icon(IconName::ArrowLeft)
                    .disabled(self.back.is_empty())
                    .tooltip("Back  ⌘[")
                    .on_click(cx.listener(|this, _, _, cx| this.go(true, cx))),
            )
            .child(
                Button::new("go-forward")
                    .ghost()
                    .small()
                    .icon(IconName::ArrowRight)
                    .disabled(self.forward.is_empty())
                    .tooltip("Forward  ⌘]")
                    .on_click(cx.listener(|this, _, _, cx| this.go(false, cx))),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .gap_1()
                    .children(crumbs),
            )
            .when_some(self.page.as_ref(), |el, p| {
                let b = &p.block;
                let (id, fav, full, small, locked) = (
                    b.id.clone(),
                    b.favorite,
                    flag(b, "full_width"),
                    flag(b, "small_text"),
                    flag(b, "locked"),
                );
                let star_id = id.clone();
                el.when(locked, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("🔒 Locked"),
                    )
                })
                .child(
                    div()
                        .px_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!("Edited {}", ago(b.edited))),
                )
                .child(
                    Button::new("favorite")
                        .ghost()
                        .small()
                        .icon(if fav {
                            IconName::StarFill
                        } else {
                            IconName::Star
                        })
                        .tooltip(if fav {
                            "Remove from Favorites"
                        } else {
                            "Add to Favorites"
                        })
                        .on_click(move |_, _, cx| toggle_favorite(&star_id, cx)),
                )
                .child(
                    Button::new("page-menu")
                        .ghost()
                        .small()
                        .icon(IconName::Ellipsis)
                        .dropdown_menu(move |menu, _, _| {
                            let (a, b, c) = (id.clone(), id.clone(), id.clone());
                            let menu =
                                menu.min_w(px(240.))
                                    .item(PopupMenuItem::new("Small text").checked(small).on_click(
                                        move |_, _, cx| toggle_flag(&a, "small_text", cx),
                                    ))
                                    .item(PopupMenuItem::new("Full width").checked(full).on_click(
                                        move |_, _, cx| toggle_flag(&b, "full_width", cx),
                                    ))
                                    .item(
                                        PopupMenuItem::new("Lock page").checked(locked).on_click(
                                            move |_, _, cx| toggle_flag(&c, "locked", cx),
                                        ),
                                    )
                                    .separator();
                            page_menu(menu, id.clone(), fav)
                        }),
                )
            })
    }

    /// Main pane content: the page editor, or the database view of a full-page database.
    fn page_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let (Some(p), Some(view)) = (self.page.as_ref(), self.body.clone()) else {
            return Empty.into_any_element();
        };
        let theme = cx.theme();
        let b = &p.block;
        let page_id = b.id.clone();
        let restore_id = b.id.clone();
        v_flex()
            .flex_1()
            .min_h_0()
            .when(b.trashed.is_some(), |el| {
                el.child(
                    h_flex()
                        .justify_center()
                        .gap_3()
                        .py_2()
                        .bg(theme.danger)
                        .text_color(theme.danger_foreground)
                        .text_sm()
                        .child("This page is in Trash.")
                        .child(
                            Button::new("restore-page")
                                .small()
                                .label("Restore page")
                                .on_click(move |_, window, cx| {
                                    let r = db::store(cx).restore(&restore_id);
                                    done(r, window, cx);
                                }),
                        )
                        .child(
                            Button::new("delete-page")
                                .small()
                                .label("Delete from Trash")
                                .on_click(move |_, window, cx| {
                                    delete_forever(page_id.clone(), window, cx)
                                }),
                        ),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .when(b.kind == BlockType::CollectionViewPage, |d| {
                        d.flex().flex_col().px(px(64.)).pt(px(40.)).pb_4()
                    })
                    .child(view),
            )
            .into_any_element()
    }
}

impl Render for AppView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, border) = (theme.background, theme.foreground, theme.border);
        let body = match self.page {
            Some(_) => self.page_body(cx),
            None => v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_3()
                .text_color(cx.theme().muted_foreground)
                .child("No page open")
                .child(
                    Button::new("empty-new-page")
                        .label("New page")
                        .icon(IconName::Plus)
                        .on_click(|_, window, cx| new_page(None, window, cx)),
                )
                .into_any_element(),
        };
        h_flex()
            .key_context(WORKSPACE_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|_, _: &NewPage, window, cx| new_page(None, window, cx)))
            .on_action(
                cx.listener(|_, _: &OpenSearch, window, cx| search::open(Mode::Search, window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
            .on_action(cx.listener(|this, _: &GoBack, _, cx| this.go(true, cx)))
            .on_action(cx.listener(|this, _: &GoForward, _, cx| this.go(false, cx)))
            // Keep key bindings live after clicks on non-focusable chrome.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if !this.focus_handle.contains_focused(window, cx) {
                        this.focus_handle.focus(window, cx);
                    }
                }),
            )
            .on_drag_move(
                cx.listener(|this, ev: &DragMoveEvent<ResizeSidebar>, _, cx| {
                    this.sidebar_width = px(f32::from(ev.event.position.x).clamp(180., 480.));
                    cx.notify();
                }),
            )
            .on_drop(cx.listener(|this, _: &ResizeSidebar, _, _| {
                this.settings.sidebar_width = Some(f32::from(this.sidebar_width));
                this.settings.save();
            }))
            .size_full()
            .bg(bg)
            .text_color(fg)
            .when(!self.sidebar_hidden, |el| {
                el.child(
                    div()
                        .w(self.sidebar_width)
                        .h_full()
                        .flex_shrink_0()
                        // Cached: a blink or edit in the page must not re-render the sidebar.
                        .child(
                            self.sidebar
                                .clone()
                                .cached(StyleRefinement::default().size_full()),
                        ),
                )
                .child(
                    div()
                        .id("sidebar-resize")
                        .w(px(4.))
                        .h_full()
                        .flex_shrink_0()
                        .cursor_col_resize()
                        .hover(|s| s.bg(border))
                        .on_drag(ResizeSidebar, |d, _, _, cx| cx.new(|_| d.clone())),
                )
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.render_top_bar(cx))
                    .child(body),
            )
    }
}

// ---- workspace + page operations (also used by the sidebar and search palette)

/// Returns `preferred` if it exists, else the first workspace; creates an empty one on first run.
pub fn ensure_workspace(store: &Store, preferred: Option<&str>) -> Result<String> {
    let all = store.workspaces()?;
    if all.is_empty() {
        return Ok(store.create_workspace("Personal")?.id);
    }
    Ok(preferred
        .and_then(|p| all.iter().find(|w| w.id == p))
        .unwrap_or(&all[0])
        .id
        .clone())
}

/// Prop on the blank page opened at launch; removed on its first edit.
const DRAFT: &str = "draft";

/// No title and no content besides empty text blocks.
fn is_blank(store: &Store, id: &str) -> bool {
    let empty = |b: &Block| b.title.plain_text().trim().is_empty();
    store.block(id).is_ok_and(|b| empty(&b))
        && store.children(id).is_ok_and(|kids| {
            kids.iter().all(|k| {
                k.kind == BlockType::Text
                    && empty(k)
                    && store.children(&k.id).is_ok_and(|c| c.is_empty())
            })
        })
}

/// A launch page the user never touched.
fn is_empty_draft(store: &Store, id: &str) -> bool {
    store.block(id).is_ok_and(|b| flag(&b, DRAFT)) && is_blank(store, id)
}

/// Creates the launch page after deleting untouched ones left by a crash.
fn new_draft(store: &Store, ws: &str) -> Result<String> {
    for w in store.workspaces()? {
        for n in store.page_tree(&w.id)? {
            if n.parent_id.is_none() && is_empty_draft(store, &n.id) {
                store.delete_forever(&n.id)?;
            }
        }
    }
    let page = store.create_block(ws, None, None, BlockType::Page)?;
    store.set_prop(&page.id, DRAFT, true.into())?;
    Ok(page.id)
}

fn first_page(store: &Store, ws: &str) -> Option<String> {
    store
        .page_tree(ws)
        .ok()?
        .into_iter()
        .find(|n| n.parent_id.is_none())
        .map(|n| n.id)
}

pub fn open_workspace(ws: String, cx: &mut App) {
    let page = first_page(&db::store(cx), &ws);
    db::nav(cx).update(cx, |n, cx| {
        n.workspace = Some(ws);
        n.page = page;
        cx.notify();
    });
}

/// Refreshes views on success, shows the error otherwise.
pub fn done<T>(r: Result<T>, window: &mut Window, cx: &mut App) -> Option<T> {
    match r {
        Ok(v) => {
            db::changed(cx);
            Some(v)
        }
        Err(e) => {
            window.push_notification(format!("Error: {e}"), cx);
            None
        }
    }
}

pub fn new_page(parent: Option<String>, window: &mut Window, cx: &mut App) {
    let Some(ws) = db::nav(cx).read(cx).workspace.clone() else {
        return;
    };
    let r = db::store(cx).create_block(&ws, parent.as_deref(), None, BlockType::Page);
    if let Some(b) = done(r, window, cx) {
        db::open_page(b.id, cx);
    }
}

pub fn toggle_favorite(id: &str, cx: &mut App) {
    if db::store(cx).toggle_favorite(id).is_ok() {
        db::changed(cx);
    }
}

fn flag(b: &Block, key: &str) -> bool {
    b.props.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn toggle_flag(id: &str, key: &str, cx: &mut App) {
    let store = db::store(cx);
    let Ok(on) = store.block(id).map(|b| flag(&b, key)) else {
        return;
    };
    let value = if on { Value::Null } else { true.into() };
    if store.set_prop(id, key, value).is_ok() {
        db::changed(cx);
    }
}

pub fn duplicate_page(id: &str, window: &mut Window, cx: &mut App) {
    let r = db::store(cx).duplicate_page(id);
    if let Some(copy) = done(r, window, cx) {
        db::open_page(copy, cx);
    }
}

pub fn trash_page(id: &str, window: &mut Window, cx: &mut App) {
    let r = db::store(cx).trash(id);
    done(r, window, cx);
}

pub fn delete_forever(id: String, window: &mut Window, cx: &mut App) {
    confirm(
        "Delete permanently?",
        "This page and everything inside it will be gone for good.".into(),
        "Delete",
        window,
        cx,
        move |window, cx| {
            let r = db::store(cx).delete_forever(&id);
            done(r, window, cx);
        },
    );
}

pub fn rename_page(id: String, window: &mut Window, cx: &mut App) {
    let title = db::store(cx)
        .block(&id)
        .map(|b| b.title.plain_text())
        .unwrap_or_default();
    prompt_text("Rename page", title, window, cx, move |text, window, cx| {
        let r = db::store(cx).update_title(&id, &RichText::from_plain(&text));
        done(r, window, cx);
    });
}

/// Writes the page as markdown to a path from the save dialog (~/Downloads if it fails).
pub fn export_markdown(id: &str, window: &mut Window, cx: &mut App) {
    let store = db::store(cx);
    let Some(md) = done(store.page_to_markdown(id), window, cx) else {
        return;
    };
    let title = store
        .block(id)
        .map(|b| b.title.plain_text())
        .unwrap_or_default();
    let name: String = title_or_untitled(&title)
        .chars()
        .map(|c| if "/\\:".contains(c) { '-' } else { c })
        .collect::<String>()
        + ".md";
    let downloads = dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_default();
    let rx = cx.prompt_for_new_path(&downloads, Some(&name));
    let handle = window.window_handle();
    cx.spawn(async move |cx: &mut AsyncApp| {
        let path = match rx.await {
            Ok(Ok(Some(p))) => p,
            Ok(Ok(None)) => return,
            _ => downloads.join(&name),
        };
        let msg = match std::fs::write(&path, md) {
            Ok(()) => format!("Exported to {}", path.display()),
            Err(e) => format!("Export failed: {e}"),
        };
        let _ = handle.update(cx, |_, window, cx| window.push_notification(msg, cx));
    })
    .detach();
}

/// The `…` actions of a page row and of the top bar.
pub fn page_menu(menu: PopupMenu, id: String, favorite: bool) -> PopupMenu {
    let ids: [String; 6] = std::array::from_fn(|_| id.clone());
    let [fav, dup, rename, mv, export, trash] = ids;
    menu.item(
        PopupMenuItem::new(if favorite {
            "Remove from Favorites"
        } else {
            "Add to Favorites"
        })
        .icon(if favorite {
            IconName::StarOff
        } else {
            IconName::Star
        })
        .on_click(move |_, _, cx| toggle_favorite(&fav, cx)),
    )
    .item(
        PopupMenuItem::new("Duplicate")
            .icon(IconName::Copy)
            .on_click(move |_, window, cx| duplicate_page(&dup, window, cx)),
    )
    .item(
        PopupMenuItem::new("Rename")
            .icon(IconName::Replace)
            .on_click(move |_, window, cx| rename_page(rename.clone(), window, cx)),
    )
    .item(
        PopupMenuItem::new("Move to")
            .icon(IconName::ArrowRight)
            .on_click(move |_, window, cx| search::open(Mode::MoveTo(mv.clone()), window, cx)),
    )
    .item(
        PopupMenuItem::new("Export Markdown")
            .icon(IconName::ExternalLink)
            .on_click(move |_, window, cx| export_markdown(&export, window, cx)),
    )
    .separator()
    .item(
        PopupMenuItem::new("Move to Trash")
            .icon(IconName::Delete)
            .on_click(move |_, window, cx| trash_page(&trash, window, cx)),
    )
}

// ---- small dialogs

/// Single-line text dialog; Enter or Save calls `on_ok` with the trimmed text.
pub fn prompt_text(
    title: &'static str,
    value: String,
    window: &mut Window,
    cx: &mut App,
    on_ok: impl Fn(String, &mut Window, &mut App) + 'static,
) {
    let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
    let submit = {
        let input = input.clone();
        Rc::new(move |window: &mut Window, cx: &mut App| {
            let text = input.read(cx).value().trim().to_string();
            window.close_dialog(cx);
            on_ok(text, window, cx);
        })
    };
    let enter = submit.clone();
    let sub = window.subscribe(&input, cx, move |_, ev: &InputEvent, window, cx| {
        if let InputEvent::PressEnter { .. } = ev {
            enter(window, cx);
        }
    });
    let dialog_input = input.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let _keep = &sub;
        let submit = submit.clone();
        dialog
            .title(title)
            .w(px(420.))
            .child(Input::new(&dialog_input))
            .footer(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("prompt-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("prompt-ok")
                            .primary()
                            .label("Save")
                            .on_click(move |_, window, cx| submit(window, cx)),
                    ),
            )
    });
    input.update(cx, |s, cx| {
        s.focus(window, cx);
        s.select_all(window, cx);
    });
}

pub fn confirm(
    title: &'static str,
    body: SharedString,
    ok: &'static str,
    window: &mut Window,
    cx: &mut App,
    on_ok: impl Fn(&mut Window, &mut App) + 'static,
) {
    let on_ok = Rc::new(on_ok);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let on_ok = on_ok.clone();
        alert
            .confirm()
            .title(title)
            .description(body.clone())
            .ok_text(ok)
            .ok_variant(ButtonVariant::Danger)
            .on_ok(move |_, window, cx| {
                on_ok(window, cx);
                true
            })
    });
}

pub fn title_or_untitled(title: &str) -> SharedString {
    if title.trim().is_empty() {
        "Untitled".into()
    } else {
        title.to_string().into()
    }
}

fn ago(ms: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    match (now - ms).max(0) / 1000 {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};

    use super::AppView;
    use crate::database::DatabaseView;
    use crate::db;
    use crate::editor::PageEditor;
    use crate::model::{BlockType, RichText};
    use crate::store::Store;

    #[test]
    fn empty_draft_needs_flag_and_no_content() {
        let store = Store::open_in_memory().unwrap();
        let ws = store.create_workspace("W").unwrap().id;
        let page = store
            .create_block(&ws, None, None, BlockType::Page)
            .unwrap()
            .id;
        assert!(!super::is_empty_draft(&store, &page), "unflagged page");
        store.set_prop(&page, super::DRAFT, true.into()).unwrap();
        assert!(super::is_empty_draft(&store, &page));
        let text = store
            .create_block(&ws, Some(&page), None, BlockType::Text)
            .unwrap()
            .id;
        assert!(super::is_empty_draft(&store, &page), "empty text block");
        store
            .update_title(&text, &RichText::from_plain("hi"))
            .unwrap();
        assert!(!super::is_empty_draft(&store, &page), "typed text");
        store.update_title(&text, &RichText::default()).unwrap();
        store
            .create_block(&ws, Some(&page), None, BlockType::ToDo)
            .unwrap();
        assert!(!super::is_empty_draft(&store, &page), "other block");
        let titled = store
            .create_block(&ws, None, None, BlockType::Page)
            .unwrap()
            .id;
        store.set_prop(&titled, super::DRAFT, true.into()).unwrap();
        store
            .update_title(&titled, &RichText::from_plain("Notes"))
            .unwrap();
        assert!(!super::is_empty_draft(&store, &titled), "titled");
    }

    #[gpui_kit::test]
    fn opens_pages_rows_and_databases(cx: &mut TestAppContext) {
        let dir = std::env::temp_dir().join(format!("redbook-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("t.db");
        // Settings live next to $REDBOOK_DB; keep them out of the real profile.
        unsafe { std::env::set_var("REDBOOK_DB", &path) };
        let store = Store::open(&path).unwrap();
        let ws = store.create_workspace("Test").unwrap().id;
        let page = store
            .create_block(&ws, None, None, BlockType::Page)
            .unwrap();
        store
            .update_title(&page.id, &RichText::from_plain("Home"))
            .unwrap();
        for kind in [BlockType::Header, BlockType::Text, BlockType::ToDo] {
            let b = store.create_block(&ws, Some(&page.id), None, kind).unwrap();
            store
                .update_title(&b.id, &RichText::from_plain("hello"))
                .unwrap();
        }
        let sub = store
            .create_block(&ws, Some(&page.id), None, BlockType::Page)
            .unwrap();
        let inline = store
            .create_database(&ws, Some(&page.id), None, false)
            .unwrap();
        let row = store.add_row(&inline.id).unwrap();
        store.add_row(&inline.id).unwrap();
        let full = store
            .create_database(&ws, Some(&page.id), None, true)
            .unwrap();

        cx.update(|cx| {
            gpui_kit::init(cx);
            db::init(store, cx);
        });
        let (window, app) = cx.update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| AppView::new(window, cx))
            })
            .unwrap()
        });
        for id in [&page.id, &sub.id, &full.id, &row.id] {
            cx.update(|cx| db::open_page(id.clone(), cx));
            cx.run_until_parked();
            cx.update_window(window, |_, w, cx| w.render_frame(cx))
                .unwrap();
            let body = app.read_with(cx, |a, _| a.body.clone()).expect("body view");
            if *id == full.id {
                assert!(body.downcast::<DatabaseView>().is_ok());
            } else {
                let editor = body.downcast::<PageEditor>().expect("editor");
                let (header, dbs) =
                    editor.read_with(cx, |e, _| (e.header.is_some(), e.databases.len()));
                assert_eq!(header, *id == row.id);
                assert_eq!(dbs, usize::from(*id == page.id));
            }
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
