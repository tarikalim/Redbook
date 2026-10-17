use std::ops::Range;
use std::path::PathBuf;

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, Textarea},
    menu::{DropdownMenu as _, PopupMenuItem},
    popover::Popover,
    scroll::ScrollableElement as _,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::json;

use super::*;

const MAX_WIDTH: f32 = 900.;
const GUTTER: f32 = 48.;
const INDENT: f32 = 26.;

type MenuAction = Box<dyn Fn(&mut PageEditor, &mut Window, &mut Context<PageEditor>)>;

/// Drag payload and preview for reordering blocks.
#[derive(Clone)]
struct DraggedBlock {
    id: String,
    label: SharedString,
}

impl Render for DraggedBlock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .max_w(px(320.))
            .rounded(cx.theme().radius)
            .bg(cx.theme().secondary)
            .opacity(0.8)
            .text_sm()
            .child(self.label.clone())
    }
}

/// Text size and weight per block type.
fn text_style(kind: &BlockType) -> (Pixels, FontWeight, Pixels) {
    match kind {
        BlockType::Header => (px(30.), FontWeight::BOLD, px(24.)),
        BlockType::SubHeader => (px(24.), FontWeight::BOLD, px(16.)),
        BlockType::SubSubHeader => (px(20.), FontWeight::SEMIBOLD, px(10.)),
        BlockType::Header4 => (px(18.), FontWeight::SEMIBOLD, px(8.)),
        BlockType::Code | BlockType::Equation => (px(14.), FontWeight::NORMAL, px(4.)),
        _ => (px(16.), FontWeight::NORMAL, px(2.)),
    }
}

pub(super) fn line_height(size: Pixels) -> Pixels {
    size * 1.5
}

/// Text color, or background for `*_background` names.
fn block_color(name: &str, cx: &App) -> Option<(bool, Hsla)> {
    let hex = kinds::color_hex(name)?;
    let bg = name.ends_with("_background");
    let c: Hsla = rgb(hex).into();
    Some(if bg {
        (
            true,
            c.opacity(if cx.theme().is_dark() { 0.25 } else { 0.15 }),
        )
    } else {
        (false, c)
    })
}

pub(super) fn highlights(rt: &RichText, cx: &App) -> Vec<(Range<usize>, HighlightStyle)> {
    let theme = cx.theme();
    let mut out = Vec::new();
    let mut pos = 0;
    for s in &rt.0 {
        let range = pos..pos + s.text.len();
        pos = range.end;
        if s.annotations.is_empty() {
            continue;
        }
        let mut h = HighlightStyle::default();
        for a in &s.annotations {
            match a {
                Annotation::Bold => h.font_weight = Some(FontWeight::BOLD),
                Annotation::Italic => h.font_style = Some(FontStyle::Italic),
                Annotation::Strike => {
                    h.strikethrough = Some(StrikethroughStyle {
                        thickness: px(1.),
                        color: None,
                    })
                }
                Annotation::Underline | Annotation::Link(_) => {
                    h.underline = Some(UnderlineStyle {
                        thickness: px(1.),
                        ..Default::default()
                    });
                    if matches!(a, Annotation::Link(_)) {
                        h.color = Some(theme.link);
                    }
                }
                Annotation::Code => {
                    h.background_color = Some(theme.muted);
                    h.color = Some(rgb(0xeb5757).into());
                }
                Annotation::Color(c) => match block_color(c, cx) {
                    Some((true, c)) => h.background_color = Some(c),
                    Some((false, c)) => h.color = Some(c),
                    None => {}
                },
                _ => {}
            }
        }
        out.push((range, h));
    }
    out
}

pub(super) fn link_at(rt: &RichText, ix: usize) -> Option<String> {
    let mut pos = 0;
    for s in &rt.0 {
        let end = pos + s.text.len();
        if (pos..end).contains(&ix) {
            return s.link().map(str::to_string);
        }
        pos = end;
    }
    None
}

/// A `redbook://page/` link opens that page, any other link the browser.
pub(super) fn open_link(url: &str, cx: &mut App) {
    match url.strip_prefix("redbook://page/") {
        Some(page) => db::open_page(page, cx),
        None => cx.open_url(url),
    }
}

pub(super) fn panel(cx: &App) -> Div {
    v_flex()
        .py_1()
        .bg(cx.theme().popover)
        .text_color(cx.theme().popover_foreground)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
        .shadow_lg()
        .text_sm()
}

pub(super) fn menu_label(text: &'static str, cx: &App) -> Div {
    div()
        .px_3()
        .pt_2()
        .pb_1()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

pub(super) fn menu_row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    let accent = cx.theme().accent;
    h_flex()
        .id(id)
        .mx_1()
        .px_2()
        .py_1()
        .gap_2()
        .rounded(cx.theme().radius)
        .cursor_pointer()
        .when(selected, |el| el.bg(accent))
        .hover(move |s| s.bg(accent))
}

impl PageEditor {
    /// Inline database; the view caps its own height, so it fits in a list item.
    fn render_database(
        &mut self,
        block_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let view = self
            .databases
            .entry(block_id.to_string())
            .or_insert_with(|| {
                let id = block_id.to_string();
                cx.new(|cx| DatabaseView::new(id, window, cx))
            })
            .clone();
        div().w_full().my_2().child(view).into_any_element()
    }

    /// Text size, weight and top padding, scaled down for "Small text" pages.
    fn text_style(&self, kind: &BlockType) -> (Pixels, FontWeight, Pixels) {
        let (size, weight, pt) = text_style(kind);
        let k = if self.flag("small_text") { 0.875 } else { 1. };
        (size * k, weight, pt * k)
    }

    fn render_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let inner = if ix == 0 {
            self.render_header(window, cx)
        } else if ix > self.rows.len() {
            div()
                .id("page-footer")
                .w_full()
                .h(px(240.))
                .cursor_text()
                .on_drop(cx.listener(|this, d: &DraggedBlock, window, cx| {
                    this.drop_block(&d.id, None, window, cx)
                }))
                .on_click(cx.listener(|this, _, window, cx| this.edit_end(window, cx)))
                .into_any_element()
        } else {
            self.render_row(ix - 1, window, cx)
        };
        h_flex()
            .w_full()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .when(!self.flag("full_width"), |d| d.max_w(px(MAX_WIDTH)))
                    .px(px(GUTTER))
                    .child(inner),
            )
            .into_any_element()
    }

    fn render_header(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(page) = self.page.clone() else {
            return div()
                .pt(px(80.))
                .text_color(cx.theme().muted_foreground)
                .child("This page does not exist or is in the trash.")
                .into_any_element();
        };
        let icon = page.icon().map(str::to_string);
        let cover = page.str_prop("cover").map(str::to_string);
        let has_cover = cover.is_some();
        let locked = self.locked();
        let add_buttons = h_flex()
            .gap_1()
            .opacity(0.)
            .group_hover("page-header", |s| s.opacity(1.))
            .when(icon.is_none() && !locked, |el| {
                el.child(
                    self.emoji_popover(
                        "add-icon",
                        Button::new("add-icon-btn")
                            .ghost()
                            .small()
                            .icon(IconName::Plus)
                            .label("Add icon"),
                        cx,
                    ),
                )
            })
            .when(!has_cover && !locked, |el| {
                el.child(
                    Button::new("add-cover")
                        .ghost()
                        .small()
                        .icon(IconName::Plus)
                        .label("Add cover")
                        .on_click(cx.listener(|this, _, window, cx| {
                            let n = rand_index(kinds::GRADIENTS.len());
                            let page = this.page_id.clone();
                            this.set_block_prop(
                                &page,
                                "cover",
                                json!(format!("gradient_{n}")),
                                window,
                                cx,
                            );
                        })),
                )
            });
        let title = Textarea::new(&self.title)
            .appearance(false)
            .readonly(locked)
            .xsmall()
            .text_size(px(40.))
            .line_height(px(52.))
            .font_weight(FontWeight::BOLD);
        v_flex()
            .w_full()
            .group("page-header")
            .when_some(cover, |el, c| el.child(self.render_cover(&c, locked, cx)))
            .when(!has_cover, |el| el.pt(px(72.)))
            .when_some(icon, |el, icon| {
                let big = div().text_size(px(64.)).line_height(px(80.)).child(icon);
                let icon: AnyElement = if locked {
                    big.into_any_element()
                } else {
                    let btn = Button::new("page-icon-btn").ghost().h(px(80.)).child(big);
                    self.emoji_popover("page-icon", btn, cx).into_any_element()
                };
                el.child(div().when(has_cover, |d| d.mt(px(-40.))).child(icon))
            })
            .child(add_buttons.mt_2())
            .child(div().pb_2().child(title))
            .children(self.header.clone().map(|h| div().pb_3().child(h)))
            .into_any_element()
    }

    fn render_cover(&mut self, cover: &str, locked: bool, cx: &mut Context<Self>) -> AnyElement {
        let base = div()
            .w_full()
            .h(px(200.))
            .rounded(cx.theme().radius)
            .overflow_hidden();
        let base = match cover
            .strip_prefix("gradient_")
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| kinds::GRADIENTS.get(n))
        {
            Some(&(a, b)) => base.bg(linear_gradient(
                135.,
                linear_color_stop(rgb(a), 0.),
                linear_color_stop(rgb(b), 1.),
            )),
            None => base.child(
                img(PathBuf::from(cover))
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            ),
        };
        let view = cx.entity();
        let page_id = self.page_id.clone();
        let picker = Popover::new("cover-picker")
            .anchor(Anchor::TopRight)
            .trigger(Button::new("change-cover").small().label("Change cover"))
            .content(move |_, _, cx| {
                let pop = cx.entity();
                let set = {
                    let (view, pop, page_id) = (view.clone(), pop.clone(), page_id.clone());
                    move |value: serde_json::Value, window: &mut Window, cx: &mut App| {
                        view.update(cx, |this, cx| {
                            this.set_block_prop(&page_id, "cover", value, window, cx)
                        });
                        pop.update(cx, |p, cx| p.dismiss(window, cx));
                    }
                };
                let set = std::rc::Rc::new(set);
                v_flex()
                    .w(px(280.))
                    .gap_2()
                    .child(div().flex().flex_wrap().gap_2().children(
                        kinds::GRADIENTS.iter().enumerate().map(|(i, &(a, b))| {
                            let set = set.clone();
                            div()
                                .id(("gradient", i))
                                .w(px(56.))
                                .h(px(32.))
                                .rounded_sm()
                                .cursor_pointer()
                                .bg(linear_gradient(
                                    135.,
                                    linear_color_stop(rgb(a), 0.),
                                    linear_color_stop(rgb(b), 1.),
                                ))
                                .on_click(move |_, window, cx| {
                                    set(json!(format!("gradient_{i}")), window, cx)
                                })
                        }),
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("cover-image")
                                    .small()
                                    .label("Choose image…")
                                    .on_click({
                                        let (view, pop, page_id) =
                                            (view.clone(), pop.clone(), page_id.clone());
                                        move |_, window, cx| {
                                            pop.update(cx, |p, cx| p.dismiss(window, cx));
                                            let page_id = page_id.clone();
                                            view.update(cx, |this, cx| {
                                                this.pick_file(
                                                    window,
                                                    cx,
                                                    move |this, path, window, cx| {
                                                        this.set_block_prop(
                                                            &page_id,
                                                            "cover",
                                                            json!(path),
                                                            window,
                                                            cx,
                                                        )
                                                    },
                                                )
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("cover-remove")
                                    .small()
                                    .ghost()
                                    .label("Remove")
                                    .on_click({
                                        let set = set.clone();
                                        move |_, window, cx| {
                                            set(serde_json::Value::Null, window, cx)
                                        }
                                    }),
                            ),
                    )
            });
        div()
            .relative()
            .w_full()
            .group("cover")
            .child(base)
            .child(
                div()
                    .absolute()
                    .right_2()
                    .bottom_2()
                    .opacity(0.)
                    .group_hover("cover", |s| s.opacity(1.))
                    .when(!locked, |d| d.child(picker)),
            )
            .into_any_element()
    }

    fn emoji_popover(&self, id: &'static str, trigger: Button, cx: &mut Context<Self>) -> Popover {
        let view = cx.entity();
        let page_id = self.page_id.clone();
        Popover::new(id).trigger(trigger).content(move |_, _, cx| {
            let pop = cx.entity();
            let set = {
                let (view, pop, page_id) = (view.clone(), pop.clone(), page_id.clone());
                std::rc::Rc::new(
                    move |value: serde_json::Value, window: &mut Window, cx: &mut App| {
                        view.update(cx, |this, cx| {
                            this.set_block_prop(&page_id, "icon", value, window, cx)
                        });
                        pop.update(cx, |p, cx| p.dismiss(window, cx));
                    },
                )
            };
            v_flex()
                .w(px(330.))
                .gap_2()
                .child(
                    h_flex().justify_between().child("Emoji").child(
                        Button::new("icon-remove")
                            .ghost()
                            .xsmall()
                            .label("Remove")
                            .on_click({
                                let set = set.clone();
                                move |_, window, cx| set(serde_json::Value::Null, window, cx)
                            }),
                    ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .children(kinds::EMOJIS.iter().enumerate().map(|(i, e)| {
                            let set = set.clone();
                            div()
                                .id(("emoji", i))
                                .size(px(36.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_sm()
                                .text_size(px(22.))
                                .cursor_pointer()
                                .hover(|s| s.bg(cx.theme().accent))
                                .child(*e)
                                .on_click(move |_, window, cx| set(json!(e), window, cx))
                        })),
                )
        })
    }

    fn render_row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let row = self.rows[ix].clone();
        let Some(b) = self.blocks.get(&row.id).cloned() else {
            return div().into_any_element();
        };
        let id = row.id.clone();
        let (size, _, pt) = self.text_style(&b.kind);
        let lh = line_height(size);
        let (theme, radius) = (cx.theme().colors, cx.theme().radius);

        let content = match &b.kind {
            BlockType::Divider => div()
                .w_full()
                .h(lh)
                .flex()
                .items_center()
                .child(div().w_full().h(px(1.)).bg(theme.border))
                .into_any_element(),
            BlockType::Image => self.render_image(&b, cx),
            BlockType::Page | BlockType::CollectionViewPage => {
                let fallback = if b.kind == BlockType::Page {
                    "📄"
                } else {
                    "🗂"
                };
                let icon = b.icon().unwrap_or(fallback).to_string();
                let title = Some(b.title.plain_text()).filter(|t| !t.is_empty());
                let target = id.clone();
                h_flex()
                    .id(SharedString::from(format!("page-link-{id}")))
                    .gap_2()
                    .px_1()
                    .h(lh)
                    .rounded(radius)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.accent))
                    .child(icon)
                    .child(
                        div()
                            .underline()
                            .font_weight(FontWeight::MEDIUM)
                            .child(title.unwrap_or_else(|| "Untitled".into())),
                    )
                    .on_click(move |_, _, cx| db::open_page(target.clone(), cx))
                    .into_any_element()
            }
            BlockType::CollectionView => self.render_database(&id, window, cx),
            BlockType::Table => self.render_table(&b, window, cx),
            k if kinds::editable(k) => self.render_text(&b, window, cx),
            k => div()
                .px_1()
                .text_color(theme.muted_foreground)
                .child(format!("[{}] {}", k.as_str(), b.title.plain_text()))
                .into_any_element(),
        };

        let content = match &b.kind {
            BlockType::Quote => div()
                .border_l(px(3.))
                .border_color(theme.foreground)
                .pl_3()
                .child(content)
                .into_any_element(),
            BlockType::Callout => h_flex()
                .items_start()
                .gap_2()
                .p_3()
                .rounded(radius)
                .when(b.str_prop("color").is_none(), |el| el.bg(theme.muted))
                .child(
                    div()
                        .h(lh)
                        .flex()
                        .items_center()
                        .child(b.icon().unwrap_or("💡").to_string()),
                )
                .child(div().flex_1().min_w_0().child(content))
                .into_any_element(),
            BlockType::Code => self.render_code(&b, content, cx),
            _ => content,
        };

        let marker = self.render_marker(&row, &b, lh, cx);
        let color = b.str_prop("color").and_then(|c| block_color(c, cx));
        let target = id.clone();
        let accent = theme.drop_target;
        let selected = self.selected_rows().is_some_and(|r| r.contains(&ix));
        let hovered = id.clone();

        h_flex()
            .id(SharedString::from(format!("row-{id}")))
            .group("block")
            .w_full()
            .items_start()
            .pt(pt)
            .pb(px(1.))
            .ml(px(-GUTTER))
            .drag_over::<DraggedBlock>(move |s, _, _, _| s.border_t_2().border_color(accent))
            .on_drop(cx.listener(move |this, d: &DraggedBlock, window, cx| {
                this.drop_block(&d.id, Some(&target), window, cx)
            }))
            .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, window, cx| {
                this.drag_select(&hovered, ev, window, cx)
            }))
            .map(|el| {
                if self.locked() {
                    el.child(div().flex_none().w(px(GUTTER)))
                } else {
                    el.child(self.render_gutter(&b, lh, cx))
                }
            })
            .child(div().flex_none().w(px(INDENT * row.depth as f32)))
            .children(marker)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .rounded(radius)
                    .when(selected, |el| el.bg(theme.selection))
                    .map(|el| match color {
                        Some((true, c)) => el.bg(c),
                        Some((false, c)) => el.text_color(c),
                        None => el,
                    })
                    .child(content),
            )
            .into_any_element()
    }

    fn render_gutter(&self, b: &Block, lh: Pixels, cx: &mut Context<Self>) -> impl IntoElement {
        let id = b.id.clone();
        let label: SharedString = {
            let t = b.title.plain_text();
            if t.is_empty() {
                b.kind.as_str().to_string()
            } else {
                t.chars().take(60).collect()
            }
        }
        .into();
        let drag = DraggedBlock {
            id: id.clone(),
            label,
        };
        let add_id = id.clone();
        let muted = cx.theme().muted_foreground;
        let hover = cx.theme().accent;
        h_flex()
            .flex_none()
            .w(px(GUTTER))
            .h(lh)
            .pr_1()
            .justify_end()
            .text_color(muted)
            .opacity(0.)
            .group_hover("block", |s| s.opacity(1.))
            .child(
                div()
                    .id(SharedString::from(format!("add-{id}")))
                    .rounded_sm()
                    .hover(move |s| s.bg(hover))
                    .cursor_pointer()
                    .child(Icon::new(IconName::Plus).small())
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.add_below(&add_id, window, cx)),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("grip-{id}")))
                    .px(px(2.))
                    .rounded_sm()
                    .hover(move |s| s.bg(hover))
                    .cursor_grab()
                    .text_sm()
                    .child("⋮⋮")
                    .on_drag(drag, |d, _, _, cx| cx.new(|_| d.clone()))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseUpEvent, _, cx| {
                            this.menu = Some(BlockMenu {
                                id: id.clone(),
                                at: ev.position,
                            });
                            cx.notify();
                        }),
                    ),
            )
    }

    fn render_marker(
        &self,
        row: &Row,
        b: &Block,
        lh: Pixels,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let theme = cx.theme();
        let boxed = || {
            div()
                .flex_none()
                .min_w(px(24.))
                .h(lh)
                .flex()
                .items_center()
                .justify_center()
        };
        let id = b.id.clone();
        Some(match b.kind {
            BlockType::BulletedList => boxed().child(kinds::bullet(row.depth)).into_any_element(),
            BlockType::NumberedList => boxed()
                .pr_1()
                .child(kinds::list_marker(row.number, row.depth))
                .into_any_element(),
            BlockType::ToDo => {
                let checked = b.checked();
                boxed()
                    .child(
                        div()
                            .id(SharedString::from(format!("check-{id}")))
                            .size(px(16.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .border_1()
                            .border_color(theme.foreground)
                            .cursor_pointer()
                            .text_xs()
                            .when(checked, |el| {
                                el.bg(theme.primary)
                                    .border_color(theme.primary)
                                    .text_color(theme.primary_foreground)
                                    .child("✓")
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.toggle_checked(&id, window, cx)
                            })),
                    )
                    .into_any_element()
            }
            BlockType::Toggle => {
                let open = self.open_toggles.contains(&id);
                boxed()
                    .child(
                        div()
                            .id(SharedString::from(format!("toggle-{id}")))
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.accent))
                            .child(
                                Icon::new(if open {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .small(),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.toggle_open(&id, cx))),
                    )
                    .into_any_element()
            }
            _ => return None,
        })
    }

    fn render_text(
        &mut self,
        b: &Block,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (size, weight, _) = self.text_style(&b.kind);
        let lh = line_height(size);
        let mono = kinds::is_monospace(&b.kind);
        let done = b.kind == BlockType::ToDo && b.checked();
        let muted = cx.theme().muted_foreground;
        let editing = self.editing.as_deref() == Some(b.id.as_str())
            && (self.editor_focused(window, cx) || self.slash.is_some());
        if editing {
            return Textarea::new(&self.editor)
                .appearance(false)
                .xsmall()
                .text_size(size)
                .line_height(lh)
                .font_weight(weight)
                .when(mono, |el| el.font_family("Menlo"))
                .when(done, |el| el.line_through().text_color(muted))
                .into_any_element();
        }
        let text = b.title.plain_text();
        let id = b.id.clone();
        let el = div()
            .id(SharedString::from(format!("text-{id}")))
            .w_full()
            .px(px(4.))
            .min_h(lh)
            .text_size(size)
            .line_height(lh)
            .font_weight(weight)
            .cursor_text()
            .when(mono, |el| el.font_family("Menlo"))
            .when(done, |el| el.line_through().text_color(muted));
        if text.is_empty() {
            let placeholder = kinds::placeholder(&b.kind);
            let heading = matches!(
                b.kind,
                BlockType::Header
                    | BlockType::SubHeader
                    | BlockType::SubSubHeader
                    | BlockType::Header4
            );
            return el
                .when(heading, |el| el.text_color(muted).child(placeholder))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.start_edit(&id, 0, window, cx)
                    }),
                )
                .into_any_element();
        }
        let styled = StyledText::new(text).with_highlights(highlights(&b.title, cx));
        let layout = styled.layout().clone();
        let title = b.title.clone();
        el.on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                let ix = match layout.index_for_position(ev.position) {
                    Ok(i) | Err(i) => i,
                };
                if ev.modifiers.shift && this.shift_click(&id, window, cx) {
                    return;
                }
                if ev.modifiers.platform
                    && let Some(url) = link_at(&title, ix)
                {
                    open_link(&url, cx);
                    return;
                }
                this.start_edit(&id, ix, window, cx)
            }),
        )
        .child(styled)
        .into_any_element()
    }

    fn render_code(
        &mut self,
        b: &Block,
        content: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let lang = b.str_prop("language").unwrap_or("Plain Text").to_string();
        let view = cx.entity();
        let id = b.id.clone();
        let current = lang.clone();
        let picker = Button::new(SharedString::from(format!("lang-{id}")))
            .ghost()
            .xsmall()
            .label(lang)
            .dropdown_menu(move |menu, window, _| {
                let mut menu = menu.scrollable(true).max_h(px(320.));
                for l in kinds::LANGUAGES {
                    let id = id.clone();
                    menu = menu.item(PopupMenuItem::new(*l).checked(*l == current).on_click(
                        window.listener_for(&view, move |this, _, window, cx| {
                            this.set_block_prop(&id, "language", json!(l), window, cx)
                        }),
                    ));
                }
                menu
            });
        v_flex()
            .w_full()
            .p_2()
            .gap_1()
            .rounded(cx.theme().radius)
            .bg(cx.theme().secondary)
            .child(h_flex().child(picker))
            .child(content)
            .into_any_element()
    }

    fn render_image(&mut self, b: &Block, cx: &mut Context<Self>) -> AnyElement {
        let id = b.id.clone();
        match b.str_prop("source") {
            Some(src) => div()
                .w_full()
                .py_1()
                .child(
                    img(PathBuf::from(src))
                        .max_w_full()
                        .max_h(px(600.))
                        .object_fit(ObjectFit::Contain)
                        .rounded(cx.theme().radius),
                )
                .into_any_element(),
            None => {
                Button::new(SharedString::from(format!("img-{id}")))
                    .ghost()
                    .icon(IconName::Plus)
                    .label("Add an image")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.choose_image(id.clone(), window, cx)
                    }))
                    .into_any_element()
            }
        }
    }

    fn render_slash(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let s = self.slash.as_ref()?;
        if !self.editor_focused(window, cx) {
            return None;
        }
        let q = self.slash_query(cx)?;
        let (caret, _) = self.editor.read(cx).cursor_layout()?;
        let matches = kinds::slash_matches(&q);
        let body = panel(cx)
            .w(px(260.))
            .id("slash-menu")
            .max_h(px(340.))
            .overflow_y_scroll();
        let body = if matches.is_empty() {
            body.child(menu_label("No results", cx))
        } else {
            body.child(menu_label("Basic blocks", cx))
                .children(matches.iter().enumerate().map(|(i, &item)| {
                    menu_row(("slash", i), i == s.selected, cx)
                        .child(kinds::SLASH_ITEMS[item].0)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.choose_slash(Some(item), window, cx)
                            }),
                        )
                }))
        };
        Some(
            deferred(
                anchored()
                    .position(point(caret.left(), caret.bottom() + px(4.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(body),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    fn render_block_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let m = self.menu.as_ref()?;
        let b = self.blocks.get(&m.id)?;
        let id = b.id.clone();
        let item = |key: String, label: SharedString, f: MenuAction| {
            menu_row(SharedString::from(key), false, cx)
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.menu = None;
                    f(this, window, cx);
                    cx.notify();
                }))
        };
        let mut body = panel(cx)
            .w(px(240.))
            .id("block-menu")
            .max_h(px(440.))
            .overflow_y_scroll()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.menu = None;
                cx.notify();
            }));
        let i = id.clone();
        body = body.child(item(
            "bm-delete".into(),
            "Delete".into(),
            Box::new(move |this, w, cx| this.delete_block(&i, w, cx)),
        ));
        let i = id.clone();
        body = body.child(item(
            "bm-dup".into(),
            "Duplicate".into(),
            Box::new(move |this, w, cx| this.duplicate_block(&i, w, cx)),
        ));
        if kinds::editable(&b.kind) {
            body = body.child(menu_label("Turn into", cx));
            for (label, kind) in kinds::TURN_INTO {
                let i = id.clone();
                body = body.child(item(
                    format!("bm-turn-{kind}"),
                    (*label).into(),
                    Box::new(move |this, w, cx| this.turn_into(&i, BlockType::parse(kind), w, cx)),
                ));
            }
        }
        for (section, suffix) in [("Color", ""), ("Background", "_background")] {
            body = body.child(menu_label(section, cx));
            for (name, label, _) in kinds::COLORS {
                let i = id.clone();
                let value = if *name == "default" {
                    serde_json::Value::Null
                } else {
                    json!(format!("{name}{suffix}"))
                };
                body = body.child(item(
                    format!("bm-color-{name}{suffix}"),
                    label.to_string().into(),
                    Box::new(move |this, w, cx| {
                        this.set_block_prop(&i, "color", value.clone(), w, cx)
                    }),
                ));
            }
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

    fn render_link_prompt(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = self.link.as_ref()?;
        let row = self.row_index(&p.id)?;
        let bounds = self.list.bounds_for_item(row + 1)?;
        Some(
            deferred(
                anchored()
                    .position(point(bounds.left() + px(GUTTER * 2.), bounds.bottom()))
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        panel(cx)
                            .p_2()
                            .w(px(320.))
                            .child(Input::new(&p.input).small()),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}

/// Cheap pseudo-random index (no rand dependency).
fn rand_index(n: usize) -> usize {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    t % n.max(1)
}

impl Render for PageEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let slash = self.render_slash(window, cx);
        let menu = self.render_block_menu(cx);
        let link = self.render_link_prompt(cx);
        let table_menu = self.render_table_menu(cx);
        div()
            .id("page-editor")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .size_full()
            .relative()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .capture_action(cx.listener(Self::on_enter))
            .capture_action(cx.listener(Self::on_backspace))
            .capture_action(cx.listener(Self::on_escape))
            .capture_action(cx.listener(Self::on_text_select_all))
            .on_action(cx.listener(Self::on_copy_blocks))
            .on_action(cx.listener(Self::on_cut_blocks))
            .on_action(cx.listener(Self::on_delete_blocks))
            .on_action(cx.listener(Self::on_clear_selection))
            .on_action(cx.listener(Self::on_select_all_blocks))
            .on_action(
                cx.listener(|this, _: &SelectUp, _, cx| this.move_selection(true, false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectDown, _, cx| this.move_selection(false, false, cx)),
            )
            .on_action(cx.listener(|this, _: &ExtendUp, _, cx| this.move_selection(true, true, cx)))
            .on_action(
                cx.listener(|this, _: &ExtendDown, _, cx| this.move_selection(false, true, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &input::MoveUp, w, cx| this.on_vertical(true, w, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &input::MoveDown, w, cx| this.on_vertical(false, w, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &input::MoveLeft, w, cx| this.on_horizontal(true, w, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &input::MoveRight, w, cx| this.on_horizontal(false, w, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &input::IndentInline, w, cx| this.on_indent(false, w, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &input::OutdentInline, w, cx| this.on_indent(true, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Bold, w, cx| {
                    this.toggle_annotation(Annotation::Bold, w, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &Italic, w, cx| {
                this.toggle_annotation(Annotation::Italic, w, cx)
            }))
            .on_action(cx.listener(|this, _: &UnderlineText, w, cx| {
                this.toggle_annotation(Annotation::Underline, w, cx)
            }))
            .on_action(cx.listener(|this, _: &Strike, w, cx| {
                this.toggle_annotation(Annotation::Strike, w, cx)
            }))
            .on_action(cx.listener(|this, _: &InlineCode, w, cx| {
                this.toggle_annotation(Annotation::Code, w, cx)
            }))
            .on_action(cx.listener(|this, _: &AddLink, w, cx| this.open_link_prompt(w, cx)))
            .on_action(cx.listener(
                |this, _: &DuplicateBlock, w, cx| match this.focused_block(w, cx) {
                    Some(id) => this.duplicate_block(&id, w, cx),
                    None => cx.propagate(),
                },
            ))
            .on_action(cx.listener(|this, _: &TurnText, w, cx| this.turn_focused(0, w, cx)))
            .on_action(cx.listener(|this, _: &TurnH1, w, cx| this.turn_focused(1, w, cx)))
            .on_action(cx.listener(|this, _: &TurnH2, w, cx| this.turn_focused(2, w, cx)))
            .on_action(cx.listener(|this, _: &TurnH3, w, cx| this.turn_focused(3, w, cx)))
            .on_action(cx.listener(|this, _: &TurnTodo, w, cx| this.turn_focused(4, w, cx)))
            .on_action(cx.listener(|this, _: &TurnBulleted, w, cx| this.turn_focused(5, w, cx)))
            .on_action(cx.listener(|this, _: &TurnNumbered, w, cx| this.turn_focused(6, w, cx)))
            .on_action(cx.listener(|this, _: &TurnToggle, w, cx| this.turn_focused(7, w, cx)))
            .on_action(cx.listener(|this, _: &TurnCode, w, cx| this.turn_focused(8, w, cx)))
            .on_action(cx.listener(|this, _: &TurnPage, w, cx| this.turn_focused(9, w, cx)))
            .child(
                list(
                    self.list.clone(),
                    cx.processor(|this, ix: usize, window, cx| this.render_item(ix, window, cx)),
                )
                .size_full(),
            )
            .children(slash)
            .children(menu)
            .children(link)
            .children(table_menu)
            .vertical_scrollbar(&self.list)
    }
}
