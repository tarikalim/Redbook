# UI and editor

The GUI is built on GPUI through gpui-kit 0.7.1, which re-exports GPUI
(the `gpui-pre` 0.3.8 crates, a crates.io snapshot of zed@279fe07),
`gpui-base` and gpui-component, plus Lucide icons. This page covers how the
views are put together, why the editor is shaped the way it is, and the GPUI
facts you need before changing either. Four of those crates are vendored with
patches; that is [05-vendored-gpui.md](05-vendored-gpui.md).

## How to depend on GPUI

Depend on `gpui-kit` only, pinned exactly (`gpui-kit = "=0.7.1"` in
`Cargo.toml`). Do not add the `gpui` crate from crates.io (0.2.2, stale) or a
git dependency on zed: a second GPUI copy compiles, then fails with type
mismatches between the two `App`, `Window` and `Element` types. The exact pin
matters because `[patch.crates-io]` replaces four crates at exact versions; a
gpui-kit that asks for other versions silently stops using the patches.

| Import | Gives you |
|---|---|
| `use gpui_kit::*;` | All of GPUI: `div`, `px`, `App`, `Window`, `Context`, `Entity`, … |
| `use gpui_kit::prelude::FluentBuilder as _;` | `.when()`, `.map()` |
| `gpui_kit::component::ActiveTheme as _` | `cx.theme()` |
| `gpui_kit::component::Sizable as _` | `.small()`, `.xsmall()`, `.large()` |
| `gpui_kit::component::WindowExt as _` | `window.open_dialog`, `window.push_notification` |
| `button::ButtonVariants as _` | `.primary()`, `.ghost()`, `.outline()` |
| `menu::{DropdownMenu as _, ContextMenuExt as _}` | `.dropdown_menu()`, `.context_menu()` |

Most capabilities are extension traits. A "method not found" on a builder is
usually a missing trait import, not a missing method.

Sources to read when the API is unclear:
`~/.cargo/registry/src/*/gpui-pre-0.3.8`, `gpui-component-0.7.1` and
`gpui-base-0.7.1`, or the vendored copies under `vendor/`. The gpui-kit
repository at tag `v0.7.1` has `crates/story`, a demo of every component.

## How the views are composed

| View | File | Holds | Observes |
|---|---|---|---|
| `AppView` | `app.rs` | Sidebar entity, top bar, body view, history stacks, settings, draft page | `Nav`, revision, window appearance, window activation |
| `Sidebar` | `sidebar.rs` | Workspaces, page tree, favorites, expanded set, hovered row | revision, `Nav` |
| `PageEditor` | `editor/` | One page's blocks, the shared text input, inline database views | revision (except its own) |
| `DatabaseView` | `database/` | Schema, views, `Rc<DbRow>` rows, filter inputs | revision |
| `RowProperties` | `database/row_props.rs` | A row page's property list | revision |
| `Palette` | `search.rs` | Search or "Move to" results | — (a dialog) |

`gpui_kit::open_window` wraps the root view in gpui-kit's `Root`, which
dialogs, notifications, popovers and tooltips need; do not wrap it again.
`AppView::sync_body` recreates the body only when the page id, "is a full-page
database" or "is a database row" changes (`BodyKey`); otherwise the existing
`PageEditor` or `DatabaseView` reloads in place. A full-page database gets a
`DatabaseView`; a row page gets a `PageEditor` with `RowProperties` as its
header; every other page gets a plain `PageEditor`.

The sidebar is rendered through `self.sidebar.clone().cached(…)`. A cached
view re-renders only when it notifies, so a cursor blink or a keystroke in
the page does not rebuild the sidebar (commit `b52f565`). Its rows are a
`uniform_list` with a fixed `ROW_H` of 28 px, so only visible rows exist.
Each row builds its "more" and "add child" buttons only when it is the last
hovered row (`Sidebar::hovered`); the key stays set after the mouse leaves so
an open menu survives. Commit `aa078e7` measured 31.18 → 30.78 MB with 150
pages.

## Why the editor uses one shared text input

`PageEditor` owns exactly one `TextareaState` (`editor`) for the block being
edited, plus one for the page title and one single-line `InputState`
(`cell`) shared by all simple-table cells. Every other block is rendered as
`StyledText` with highlights computed from its `RichText`
(`render::highlights`). Clicking a block calls `start_edit`, which moves the
shared input onto that block: it sets the text, the placeholder and the caret
(found with `TextLayout::index_for_position` on the rendered text), then
focuses it.

The alternative — one input entity per block, as the pre-build plan in the
removed `docs/architecture.md` had it — costs an entity, a focus handle, a
cursor-blink task and input state per block, on every page. One shared input
keeps the per-block cost to a rendered text run, and only one element can
ever blink.

The input edits plain text, but blocks hold rich text. `on_edit` diffs the
old and new plain text (`text::diff`) and replays the change onto the
`RichText` with `text::apply_edit`, where inserted text inherits the
annotations of the character before it. Formatting commands
(`secondary-b`, …) apply `text::toggle` to the selected byte range.

Saving is debounced: an edit marks the block dirty and restarts a 300 ms
timer (`schedule_save`); `flush` writes all dirty blocks in one transaction.
`flush` also runs on blur and before a draft page is judged blank; when the
editor is dropped, `on_release` writes the dirty blocks (`write_dirty`). While a block is dirty, `reload` keeps the in-memory title
instead of the database's, so a reload triggered by another view never
overwrites what is being typed.

## How the editor lays out a page

The page is a GPUI `list` driven by a `ListState`: item 0 is the header
(icon, cover, title, and `RowProperties` for a row page), items 1..=n are the
visible blocks in document order, and the last item is a click-to-add footer.
`list` measures items as they scroll into view, so blocks can have any height
and only visible ones are laid out.

`rebuild_rows` flattens the block tree into `Row`s (id, depth, list number,
type) and then calls `ListState::splice` only on the range that changed,
found by comparing common prefix and suffix with the previous rows. A
keystroke that does not change the structure splices nothing, and the list
keeps its measured heights. Children of a closed toggle, of a `table` and of
nested pages are not flattened.

Inline databases (`collection_view` blocks) render a `DatabaseView` entity
cached per block id in `PageEditor::databases`. The inline table caps its own
height at 14 rows (`ROW_H` 34 px) and scrolls inside, so it fits in one list
item.

## How database views render

`DatabaseView` renders the current view by type:

| View | Rendering | Virtualised |
|---|---|---|
| Table | Header, `v_virtual_list` of rows (fixed `ROW_H` 34 px), "New" row, footer calculations; columns resizable (min 60 px) | Yes |
| List | `v_virtual_list` of rows | Yes |
| Gallery | `v_virtual_list` of rows of 4 cards (`GALLERY_COLS`), 150 px cards | Yes |
| Board | One column per select or status option plus "No <property name>"; cards in a scrolling column | No |
| Calendar | Month grid by the `calendar_by` date property | No |

`reload` re-reads schema and views on every revision but rows only when
`rows_stamp` changed; `recompute` re-runs the query, the row search and the
footer calculations. All cell editing goes through one `PropEditor` entity.
Table rows, list rows, board and gallery cards carry a right-click `row_menu`
whose **Delete** trashes the row page (`Store::trash`).

## How key bindings are declared

Bindings use `secondary-`, which GPUI maps to `cmd` on macOS and `ctrl` on
Linux and Windows (commit `c0ee35b`). Write every new binding that way, never
`cmd-`.

| Keys | Action | Context |
|---|---|---|
| `secondary-q` | Quit | global |
| `secondary-shift-l` | Toggle light/dark | global |
| `secondary-n` | New page | `Workspace` |
| `secondary-p`, `secondary-k` | Search palette | `Workspace` |
| `secondary-\` | Toggle sidebar | `Workspace` |
| `secondary-[`, `secondary-]` | Back, forward | `Workspace` |
| `secondary-b` / `-i` / `-u` / `-e` | Bold, italic, underline, inline code | `PageEditor` |
| `secondary-shift-s`, `secondary-shift-x` | Strikethrough | `PageEditor` |
| `secondary-k` | Add link to the selection | `PageEditor` |
| `secondary-d` | Duplicate block | `PageEditor` |
| `secondary-alt-0` … `secondary-alt-9` | Turn into: text, H1–H3, to-do, bulleted, numbered, toggle, code, page (`TURN_INTO` order) | `PageEditor` |

Global bindings are in `main.rs`, editor bindings in `editor::bind_keys`
(installed once, guarded by the `KeysBound` global). `secondary-k` is bound in
both contexts: GPUI prefers the deepest matching context, so inside the
editor it adds a link and elsewhere it opens search.

A context-scoped binding fires only when the focused element is inside an
element with that `key_context`. `AppView` re-focuses its own handle on a
mouse-down when nothing inside it has focus, so the workspace shortcuts keep
working after a click on the sidebar or the top bar. Enter, Backspace,
Escape, arrows and Tab are intercepted with `capture_action` on gpui-kit's
input actions (`input::Enter`, `input::MoveUp`, …) before the `Textarea`
handles them, which is how Enter splits a block and an arrow at the edge of a
block moves to the next one.

## What the GPUI API expects from you

| Rule | Consequence of breaking it |
|---|---|
| Call `cx.notify()` after changing state that `render` reads | Nothing re-renders |
| Store every `Subscription` in the view (`_subs`, `_subscriptions`) | Dropping it unsubscribes immediately |
| Bind keys before `cx.set_menus` | Menus copy shortcut labels at call time and show none |
| Use `gpui_kit::actions!`, not `gpui::actions!` | The macro's paths do not resolve |
| `FocusHandle::focus(window, cx)` takes both arguments | Older docs show `focus(cx)` |
| Give `on_drag`, scrolling and stateful elements an `.id(…)` | No drag, no scroll, state lost between frames |
| A scrolling flex child needs `.min_h_0()` / `.min_w_0()` | It grows to its content and never scrolls |
| Close handling: `cx.on_window_closed(… cx.quit())` | Closing the window leaves the process running |
| `with_assets(gpui_kit::assets::Assets)` at startup | Icons render blank |

## How gpui-kit's text input behaves

The editor depends on these behaviours of `InputState` / `TextareaState`:

| Behaviour | Why it matters here |
|---|---|
| `set_value` emits no `Change` event and clears undo | The editor can push model text into the input without a feedback loop |
| `replace_all` keeps undo | Not used; `set_value` is used on block switch, so undo does not cross blocks |
| A caret move alone emits no event | Read `cursor()` (byte offset) or `cursor_position()` when needed, not from events |
| `cursor()` and `selected_range()` are byte offsets | Rich-text edits are byte-indexed to match |
| `set_selected_range` positions the caret without focusing; `set_cursor_position` also focuses | `start_edit` sets the range, then focuses explicitly |
| `Position` is `lsp_types::Position` (line, character in chars) | Different unit from `cursor()` |

## How drag and drop is wired

The drag payload's Rust type is the drop target's filter: `on_drop::<T>` only
fires for a drag started with a value of type `T`. Each drag has its own
payload type that also renders the preview: `DraggedPage` (sidebar),
`DraggedBlock` (editor), `DraggedRow` (board cards), `ResizeSidebar` (the
sidebar edge). A block dropped on another block lands right before it; a
sidebar page dropped on a row lands before, inside or after it, depending on
the pointer's vertical zone in the row (`Zone`).

## Known pitfalls

- **Default icon set.** `gpui_kit::assets::Assets` embeds about 106 Lucide
  icons. `component::IconName` lists only those; `GripVertical`, for example,
  does not exist there. The full catalogue (`AllAssets`) adds about 1 MiB to
  the binary and was not taken.
- **Screen capture from an agent shell fails.** `screencapture` reports
  "could not create image from display" without screen-recording permission.
  Check the window with
  `osascript -e 'tell application "System Events" to get {name,size} of every window of (first process whose name is "redbook")'`.
- **`block v0.1.6` future-incompatibility warning.** It comes from a
  transitive macOS dependency and is harmless.

## Known gaps

- **Formatting is invisible while editing.** The focused block is a plain
  `Textarea`; bold, links and colours show again when the block loses focus.
- **Undo stops at the block.** Switching blocks calls `set_value`, which
  clears the input's undo stack; there is no page-level undo, and structural
  changes (split, merge, move, turn into) cannot be undone.
- **No multi-block selection.** Selection, copy and formatting work inside one
  block.
- **Board and calendar are not virtualised.** Every card is built; large
  boards cost memory and frame time in proportion to their size.
- **Shortcut labels are macOS-only.** The sidebar, the top-bar tooltips and the
  settings dialog show `⌘` even on Linux and Windows, where the binding is `ctrl`.
- **Several features are absent**: comments, backlinks, synced blocks,
  column layouts, timeline view, templates, multiple windows.
- **Inline markdown differs between typing and pasting.** Typing `~text~`
  strikes through (`kinds::inline_markdown`); markdown imported or sent by MCP
  needs `~~text~~` (`store::parse_inline`).
