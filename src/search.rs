// ⌘P palette: FTS page search (recent pages when empty), also the "Move to" page picker.
use std::collections::HashSet;

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::{self, title_or_untitled};
use crate::db;

/// Recently opened page ids, newest first; kept by the app shell.
pub struct Recent(pub Vec<String>);
impl Global for Recent {}

#[derive(Clone)]
pub enum Mode {
    Search,
    /// Pick a new parent for this page.
    MoveTo(String),
}

struct Item {
    /// None = workspace root (move mode only).
    id: Option<String>,
    title: SharedString,
    detail: SharedString,
    icon: Option<String>,
}

pub struct Palette {
    mode: Mode,
    ws: String,
    input: Entity<InputState>,
    items: Vec<Item>,
    selected: usize,
    scroll: ScrollHandle,
    _sub: Subscription,
}

pub fn open(mode: Mode, window: &mut Window, cx: &mut App) {
    let Some(ws) = db::nav(cx).read(cx).workspace.clone() else {
        return;
    };
    let palette = cx.new(|cx| Palette::new(mode, ws, window, cx));
    let input = palette.read(cx).input.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .w(px(600.))
            .margin_top(px(80.))
            .close_button(false)
            .child(palette.clone())
    });
    input.update(cx, |s, cx| s.focus(window, cx));
}

impl Palette {
    fn new(mode: Mode, ws: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = match mode {
            Mode::Search => "Search pages…",
            Mode::MoveTo(_) => "Move page to…",
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::Change => this.query(cx),
                InputEvent::PressEnter { .. } => this.confirm(window, cx),
                _ => {}
            },
        );
        let mut this = Self {
            mode,
            ws,
            input,
            items: vec![],
            selected: 0,
            scroll: ScrollHandle::new(),
            _sub: sub,
        };
        this.query(cx);
        this
    }

    fn query(&mut self, cx: &mut Context<Self>) {
        let q = self.input.read(cx).value().trim().to_string();
        let store = db::store(cx);
        self.items = match &self.mode {
            Mode::Search if q.is_empty() => {
                let recent = cx
                    .try_global::<Recent>()
                    .map(|r| r.0.clone())
                    .unwrap_or_default();
                recent
                    .iter()
                    .filter_map(|id| store.block(id).ok())
                    .filter(|b| b.workspace_id == self.ws && b.trashed.is_none())
                    .map(|b| Item {
                        id: Some(b.id.clone()),
                        title: title_or_untitled(&b.title.plain_text()),
                        detail: "Recent".into(),
                        icon: b.icon().map(str::to_string),
                    })
                    .collect()
            }
            Mode::Search => store
                .search(&self.ws, &q, 50)
                .unwrap_or_default()
                .into_iter()
                .map(|h| Item {
                    icon: store
                        .block(&h.page_id)
                        .ok()
                        .and_then(|b| b.icon().map(str::to_string)),
                    id: Some(h.page_id),
                    title: title_or_untitled(&h.title),
                    detail: h.snippet.into(),
                })
                .collect(),
            Mode::MoveTo(moving) => {
                let tree = store.page_tree(&self.ws).unwrap_or_default();
                // The page and its descendants can't be the new parent.
                let mut excluded: HashSet<&str> = HashSet::from([moving.as_str()]);
                loop {
                    let before = excluded.len();
                    for n in &tree {
                        if n.parent_id.as_deref().is_some_and(|p| excluded.contains(p)) {
                            excluded.insert(&n.id);
                        }
                    }
                    if excluded.len() == before {
                        break;
                    }
                }
                let ql = q.to_lowercase();
                let root = Item {
                    id: None,
                    title: "Private".into(),
                    detail: "Top level of the workspace".into(),
                    icon: None,
                };
                "private"
                    .contains(&ql)
                    .then_some(root)
                    .into_iter()
                    .chain(
                        tree.iter()
                            .filter(|n| {
                                !excluded.contains(n.id.as_str()) && !n.kind.is_collection()
                            })
                            .filter(|n| n.title.to_lowercase().contains(&ql))
                            .map(|n| Item {
                                id: Some(n.id.clone()),
                                title: title_or_untitled(&n.title),
                                detail: "".into(),
                                icon: n.icon.clone(),
                            }),
                    )
                    .collect()
            }
        };
        self.selected = 0;
        cx.notify();
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.items.get(self.selected) else {
            return;
        };
        let target = item.id.clone();
        window.close_dialog(cx);
        match &self.mode {
            Mode::Search => {
                if let Some(id) = target {
                    db::open_page(id, cx);
                }
            }
            Mode::MoveTo(moving) => {
                let r = db::store(cx).move_block(moving, target.as_deref(), usize::MAX);
                app::done(r, window, cx);
            }
        }
    }

    fn select(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.items.is_empty() {
            return;
        }
        let n = self.items.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(n) as usize;
        self.scroll.scroll_to_item(self.selected);
        cx.notify();
    }
}

impl Render for Palette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let rows = self.items.iter().enumerate().map(|(ix, item)| {
            let icon = match &item.icon {
                Some(e) => div().w(px(20.)).child(e.clone()).into_any_element(),
                None => Icon::new(if item.id.is_some() {
                    IconName::FileText
                } else {
                    IconName::Folder
                })
                .small()
                .text_color(theme.muted_foreground)
                .into_any_element(),
            };
            h_flex()
                .id(("palette-item", ix))
                .px_2()
                .py_1()
                .gap_2()
                .rounded(px(6.))
                .cursor_pointer()
                .when(ix == self.selected, |el| el.bg(theme.accent))
                .hover(|s| s.bg(theme.list_hover))
                .child(icon)
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().child(item.title.clone()))
                        .when(!item.detail.is_empty(), |el| {
                            el.child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(item.detail.clone()),
                            )
                        }),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.selected = ix;
                    this.confirm(window, cx);
                }))
        });
        v_flex()
            .gap_2()
            .text_sm()
            .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                match ev.keystroke.key.as_str() {
                    "up" => this.select(-1, cx),
                    "down" => this.select(1, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(Input::new(&self.input).prefix(Icon::new(IconName::Search).small()))
            .child(
                div()
                    .id("palette-items")
                    .max_h(px(400.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .when(self.items.is_empty(), |el| {
                        el.child(
                            div()
                                .py_4()
                                .text_center()
                                .text_color(theme.muted_foreground)
                                .child("No results"),
                        )
                    })
                    .children(rows),
            )
    }
}
