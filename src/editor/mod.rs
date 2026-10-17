//! Block editor for one page: header, blocks, slash menu, shortcuts, drag to reorder.
//! One Textarea edits the focused block; every other block renders as styled text.
mod kinds;
mod render;
mod select;
mod table;
mod text;

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::time::Duration;

use gpui_kit::component::input::{self, InputEvent, InputState, TextareaState};
use gpui_kit::*;
use serde_json::{Value, json};

use crate::database::DatabaseView;
use crate::db;
use crate::model::{Annotation, Block, BlockType, RichText};

gpui_kit::actions!(
    page_editor,
    [
        Bold,
        Italic,
        UnderlineText,
        Strike,
        InlineCode,
        AddLink,
        DuplicateBlock,
        TurnText,
        TurnH1,
        TurnH2,
        TurnH3,
        TurnTodo,
        TurnBulleted,
        TurnNumbered,
        TurnToggle,
        TurnCode,
        TurnPage,
        CopyBlocks,
        CutBlocks,
        DeleteBlocks,
        ClearSelection,
        SelectUp,
        SelectDown,
        ExtendUp,
        ExtendDown,
        SelectAllBlocks,
    ]
);

const CONTEXT: &str = "PageEditor";

struct KeysBound;
impl Global for KeysBound {}

fn bind_keys(cx: &mut App) {
    if cx.has_global::<KeysBound>() {
        return;
    }
    cx.set_global(KeysBound);
    let c = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("secondary-b", Bold, c),
        KeyBinding::new("secondary-i", Italic, c),
        KeyBinding::new("secondary-u", UnderlineText, c),
        KeyBinding::new("secondary-shift-s", Strike, c),
        KeyBinding::new("secondary-shift-x", Strike, c),
        KeyBinding::new("secondary-e", InlineCode, c),
        KeyBinding::new("secondary-k", AddLink, c),
        KeyBinding::new("secondary-d", DuplicateBlock, c),
        KeyBinding::new("secondary-alt-0", TurnText, c),
        KeyBinding::new("secondary-alt-1", TurnH1, c),
        KeyBinding::new("secondary-alt-2", TurnH2, c),
        KeyBinding::new("secondary-alt-3", TurnH3, c),
        KeyBinding::new("secondary-alt-4", TurnTodo, c),
        KeyBinding::new("secondary-alt-5", TurnBulleted, c),
        KeyBinding::new("secondary-alt-6", TurnNumbered, c),
        KeyBinding::new("secondary-alt-7", TurnToggle, c),
        KeyBinding::new("secondary-alt-8", TurnCode, c),
        KeyBinding::new("secondary-alt-9", TurnPage, c),
        KeyBinding::new("secondary-c", CopyBlocks, c),
        KeyBinding::new("secondary-x", CutBlocks, c),
        KeyBinding::new("backspace", DeleteBlocks, c),
        KeyBinding::new("delete", DeleteBlocks, c),
        KeyBinding::new("escape", ClearSelection, c),
        KeyBinding::new("up", SelectUp, c),
        KeyBinding::new("down", SelectDown, c),
        KeyBinding::new("shift-up", ExtendUp, c),
        KeyBinding::new("shift-down", ExtendDown, c),
        KeyBinding::new("secondary-a", SelectAllBlocks, c),
    ]);
}

fn ok<T>(r: anyhow::Result<T>) -> Option<T> {
    r.map_err(|e| eprintln!("editor: {e:#}")).ok()
}

/// One visible block in document order.
#[derive(Clone, PartialEq)]
struct Row {
    id: String,
    depth: usize,
    number: u64,
    kind: BlockType,
}

struct Slash {
    /// Byte offset of the "/".
    start: usize,
    selected: usize,
}

struct BlockMenu {
    id: String,
    at: Point<Pixels>,
}

struct LinkPrompt {
    id: String,
    range: Range<usize>,
    input: Entity<InputState>,
    _sub: Subscription,
}

pub struct PageEditor {
    page_id: String,
    page: Option<Block>,
    blocks: HashMap<String, Block>,
    children: HashMap<String, Vec<String>>,
    rows: Vec<Row>,
    open_toggles: HashSet<String>,
    /// Item 0 = header, 1..=rows = blocks, last = click-to-add footer.
    list: ListState,
    title: Entity<TextareaState>,
    editor: Entity<TextareaState>,
    editing: Option<String>,
    /// Block selection as (anchor, head) block ids.
    select: Option<(String, String)>,
    rich: RichText,
    dirty: HashSet<String>,
    save_task: Option<Task<()>>,
    own_rev: u64,
    slash: Option<Slash>,
    menu: Option<BlockMenu>,
    link: Option<LinkPrompt>,
    /// One shared single-line input for whichever table cell is being edited.
    cell: Entity<InputState>,
    cell_edit: Option<table::CellEdit>,
    table_menu: Option<table::TableMenu>,
    focus_handle: FocusHandle,
    /// Shown between the title and the blocks (row properties).
    pub(crate) header: Option<AnyView>,
    pub(crate) databases: HashMap<String, Entity<DatabaseView>>,
    _subs: Vec<Subscription>,
}

impl PageEditor {
    pub fn new(page_id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        bind_keys(cx);
        let title = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 20)
                .placeholder("Untitled")
        });
        let editor = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 100_000));
        let cell = cx.new(|cx| InputState::new(window, cx));
        let revision = db::revision(cx);
        let subs = vec![
            cx.subscribe_in(&cell, window, |this, _, ev: &InputEvent, _, cx| match ev {
                InputEvent::Change => this.on_cell_change(cx),
                InputEvent::Blur => {
                    this.flush(cx);
                    cx.notify();
                }
                _ => {}
            }),
            cx.subscribe_in(
                &editor,
                window,
                |this, _, ev: &InputEvent, window, cx| match ev {
                    InputEvent::Change => this.on_edit(window, cx),
                    InputEvent::Blur => {
                        this.flush(cx);
                        cx.notify();
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(&title, window, |this, state, ev: &InputEvent, _, cx| {
                if let (InputEvent::Change, Some(page)) = (ev, this.page.as_mut()) {
                    page.title = RichText::from_plain(&state.read(cx).value());
                    this.dirty.insert(this.page_id.clone());
                    this.schedule_save(cx);
                }
            }),
            cx.observe_in(&revision, window, |this, rev, window, cx| {
                if rev.read(cx).0 != this.own_rev {
                    this.own_rev = rev.read(cx).0;
                    this.reload(window, cx);
                }
            }),
            cx.on_release(|this, cx| {
                if this.write_dirty(cx) {
                    db::changed(cx);
                }
            }),
        ];
        let mut this = Self {
            page_id,
            page: None,
            blocks: HashMap::new(),
            children: HashMap::new(),
            rows: Vec::new(),
            open_toggles: HashSet::new(),
            list: ListState::new(2, ListAlignment::Top, px(600.)),
            title,
            editor,
            editing: None,
            select: None,
            rich: RichText::default(),
            dirty: HashSet::new(),
            save_task: None,
            own_rev: revision.read(cx).0,
            slash: None,
            menu: None,
            link: None,
            cell,
            cell_edit: None,
            table_menu: None,
            focus_handle: cx.focus_handle(),
            header: None,
            databases: HashMap::new(),
            _subs: subs,
        };
        this.reload(window, cx);
        this
    }

    pub fn set_header(&mut self, view: AnyView, cx: &mut Context<Self>) {
        self.header = Some(view);
        cx.notify();
    }

    /// Page flag set from the shell's page menu (`locked`, `small_text`, `full_width`).
    fn flag(&self, key: &str) -> bool {
        self.page
            .as_ref()
            .and_then(|p| p.props.get(key))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    fn locked(&self) -> bool {
        self.flag("locked")
    }

    // ---- loading

    /// Reads the page subtree from the store; unsaved titles win over stored ones.
    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let store = db::store(cx);
        let Some(mut page) = store
            .block(&self.page_id)
            .ok()
            .filter(|p| p.trashed.is_none())
        else {
            self.page = None;
            self.editing = None;
            self.blocks.clear();
            self.children.clear();
            self.rebuild_rows();
            cx.notify();
            return;
        };
        let mut blocks = HashMap::new();
        let mut children = HashMap::new();
        let mut stack = vec![self.page_id.clone()];
        while let Some(parent) = stack.pop() {
            let kids = store.children(&parent).unwrap_or_default();
            if kids.is_empty() {
                continue;
            }
            children.insert(
                parent,
                kids.iter().map(|b| b.id.clone()).collect::<Vec<_>>(),
            );
            for mut b in kids {
                if !b.kind.is_page_like() {
                    stack.push(b.id.clone());
                }
                if self.dirty.contains(&b.id)
                    && let Some(old) = self.blocks.get(&b.id)
                {
                    b.title = old.title.clone();
                    if b.kind == BlockType::TableRow {
                        b.props = old.props.clone();
                    }
                }
                blocks.insert(b.id.clone(), b);
            }
        }
        if self.dirty.contains(&self.page_id) {
            if let Some(old) = &self.page {
                page.title = old.title.clone();
            }
        } else {
            let t = page.title.plain_text();
            if self.title.read(cx).value() != t {
                self.title.update(cx, |s, cx| s.set_value(t, window, cx));
            }
        }
        if let Some(id) = self.editing.clone() {
            match blocks.get(&id) {
                Some(b) if kinds::editable(&b.kind) => {
                    if b.title != self.rich {
                        self.rich = b.title.clone();
                        let text = self.rich.plain_text();
                        self.editor.update(cx, |s, cx| {
                            let cursor = s.cursor().min(text.len());
                            s.set_value(text, window, cx);
                            s.set_selected_range(cursor..cursor, cx);
                        });
                    }
                }
                _ => {
                    self.editing = None;
                    self.slash = None;
                }
            }
        }
        self.sync_cell(&blocks, window, cx);
        self.databases.retain(|id, _| {
            blocks
                .get(id)
                .is_some_and(|b| b.kind == BlockType::CollectionView)
        });
        self.page = Some(page);
        self.blocks = blocks;
        self.children = children;
        self.rebuild_rows();
        cx.notify();
    }

    fn rebuild_rows(&mut self) {
        let mut rows = Vec::with_capacity(self.blocks.len());
        self.flatten(&self.page_id, 0, &mut rows);
        let old = std::mem::replace(&mut self.rows, rows);
        let new = &self.rows;
        let pre = old.iter().zip(new).take_while(|(a, b)| a == b).count();
        if pre == old.len() && pre == new.len() {
            return;
        }
        let max_suf = old.len().min(new.len()) - pre;
        let suf = old
            .iter()
            .rev()
            .zip(new.iter().rev())
            .take(max_suf)
            .take_while(|(a, b)| a == b)
            .count();
        self.list
            .splice(1 + pre..1 + old.len() - suf, new.len() - pre - suf);
    }

    fn flatten(&self, parent: &str, depth: usize, out: &mut Vec<Row>) {
        let Some(ids) = self.children.get(parent) else {
            return;
        };
        let items: Vec<_> = ids
            .iter()
            .map(|id| {
                let b = &self.blocks[id];
                let start = b.props.get("list_start_index").and_then(Value::as_u64);
                (b.kind == BlockType::NumberedList, start)
            })
            .collect();
        for (id, number) in ids.iter().zip(kinds::list_numbers(&items)) {
            let b = &self.blocks[id];
            out.push(Row {
                id: id.clone(),
                depth,
                number,
                kind: b.kind.clone(),
            });
            let closed = (b.kind == BlockType::Toggle && !self.open_toggles.contains(id))
                || b.kind == BlockType::Table;
            if !b.kind.is_page_like() && !closed {
                self.flatten(id, depth + 1, out);
            }
        }
    }

    fn row_index(&self, id: &str) -> Option<usize> {
        self.rows.iter().position(|r| r.id == id)
    }

    fn workspace(&self) -> String {
        self.page
            .as_ref()
            .map(|p| p.workspace_id.clone())
            .unwrap_or_default()
    }

    /// Parent id and index among siblings.
    fn position(&self, id: &str) -> Option<(String, usize)> {
        let parent = self.blocks.get(id)?.parent_id.clone()?;
        let ix = self.children.get(&parent)?.iter().position(|c| c == id)?;
        Some((parent, ix))
    }

    // ---- saving

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        self.save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            this.update(cx, |this, cx| this.flush(cx)).ok();
        }));
    }

    /// Writes pending titles; true when anything was written.
    fn write_dirty(&mut self, cx: &mut App) -> bool {
        self.save_task = None;
        if self.dirty.is_empty() {
            return false;
        }
        let store = db::store(cx);
        let dirty = std::mem::take(&mut self.dirty);
        ok(store.tx(|| {
            for id in &dirty {
                if let Some(b) = self
                    .blocks
                    .get(id)
                    .filter(|b| b.kind == BlockType::TableRow)
                {
                    let cells = b.props.get("cells").cloned().unwrap_or_else(|| json!([]));
                    store.set_prop(id, "cells", cells)?;
                    continue;
                }
                let title = if *id == self.page_id {
                    self.page.as_ref().map(|p| &p.title)
                } else {
                    self.blocks.get(id).map(|b| &b.title)
                };
                if let Some(t) = title {
                    store.update_title(id, t)?;
                }
            }
            Ok(())
        }));
        true
    }

    pub fn flush(&mut self, cx: &mut Context<Self>) {
        if self.write_dirty(cx) {
            self.notify_db(cx);
        }
    }

    pub fn focus_title(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.title.update(cx, |s, cx| s.focus(window, cx));
    }

    fn notify_db(&mut self, cx: &mut App) {
        db::changed(cx);
        self.own_rev = db::revision(cx).read(cx).0;
    }

    /// After a structural write: signal other views and reload our tree.
    fn after_write(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.notify_db(cx);
        self.reload(window, cx);
    }

    fn set_title(&mut self, id: &str, title: RichText, cx: &mut Context<Self>) {
        if let Some(b) = self.blocks.get_mut(id) {
            b.title = title;
            self.dirty.insert(id.to_string());
            self.schedule_save(cx);
        }
    }

    // ---- editing

    fn editor_focused(&self, window: &Window, cx: &App) -> bool {
        self.editor.read(cx).focus_handle(cx).is_focused(window)
    }

    /// Block being edited, only while the editor has focus.
    fn focused_block(&self, window: &Window, cx: &App) -> Option<String> {
        self.editing
            .clone()
            .filter(|_| self.editor_focused(window, cx))
    }

    fn title_focused(&self, window: &Window, cx: &App) -> bool {
        self.title.read(cx).focus_handle(cx).is_focused(window)
    }

    /// Binds the editor to block `id` with the caret at byte `cursor`.
    fn start_edit(&mut self, id: &str, cursor: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = self.blocks.get(id) else { return };
        if !kinds::editable(&b.kind) || self.locked() {
            return;
        }
        self.slash = None;
        self.menu = None;
        self.select = None;
        self.editing = Some(id.to_string());
        self.rich = b.title.clone();
        let text = self.rich.plain_text();
        let cursor = cursor.min(text.len());
        let placeholder = kinds::placeholder(&b.kind);
        self.editor.update(cx, |s, cx| {
            s.set_value(text, window, cx);
            s.set_placeholder(placeholder, window, cx);
            s.set_selected_range(cursor..cursor, cx);
            s.focus(window, cx);
        });
        if let Some(ix) = self.row_index(id) {
            self.list.scroll_to_reveal_item(ix + 1);
        }
        cx.notify();
    }

    fn on_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editing.clone() else {
            return;
        };
        let (value, cursor) = {
            let s = self.editor.read(cx);
            (s.value().to_string(), s.cursor())
        };
        let old = self.rich.plain_text();
        if value == old {
            return;
        }
        let (range, ins) = text::diff(&old, &value);
        let ins = ins.to_string();
        self.rich = text::apply_edit(&self.rich, range, &ins);
        self.set_title(&id, self.rich.clone(), cx);
        let kind = self.blocks[&id].kind.clone();
        let shortcuts = !matches!(
            kind,
            BlockType::Code | BlockType::Callout | BlockType::Equation
        );
        if shortcuts && let Some(sc) = value.get(..cursor).and_then(kinds::detect_shortcut) {
            self.apply_shortcut(&id, sc, cursor, window, cx);
            return;
        }
        if !kinds::is_monospace(&kind)
            && ins.len() == 1
            && let Some((open, dl, ann)) = value.get(..cursor).and_then(kinds::inline_markdown)
        {
            // Drop both delimiters and format what was between them.
            let close = cursor - dl;
            let r = text::apply_edit(&self.rich, close..cursor, "");
            let r = text::apply_edit(&r, open..open + dl, "");
            self.rich = text::toggle(&r, open..close - dl, &ann);
            self.set_title(&id, self.rich.clone(), cx);
            let text = self.rich.plain_text();
            let at = close - dl;
            self.editor.update(cx, |s, cx| {
                s.set_value(text, window, cx);
                s.set_selected_range(at..at, cx);
            });
            cx.notify();
            return;
        }
        if kind != BlockType::Code && ins == "/" {
            self.slash = Some(Slash {
                start: cursor - 1,
                selected: 0,
            });
        } else if self.slash.is_some() {
            let keep = self
                .slash_query(cx)
                .is_some_and(|q| !(q.ends_with(' ') && kinds::slash_matches(&q).is_empty()));
            if !keep {
                self.slash = None;
            } else if let Some(s) = &mut self.slash {
                s.selected = 0;
            }
        }
        cx.notify();
    }

    /// Text typed after the "/" while the slash menu is valid.
    fn slash_query(&self, cx: &App) -> Option<String> {
        let s = self.slash.as_ref()?;
        let ed = self.editor.read(cx);
        let value = ed.value();
        let cursor = ed.cursor();
        if cursor <= s.start || value.get(s.start..s.start + 1) != Some("/") {
            return None;
        }
        let q = value.get(s.start + 1..cursor)?;
        (!q.contains('\n') && q.len() <= 40).then(|| q.to_string())
    }

    fn apply_shortcut(
        &mut self,
        id: &str,
        sc: kinds::Shortcut,
        prefix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rest = text::split_at(&self.rich, prefix).1;
        self.rich = rest.clone();
        self.set_title(id, rest.clone(), cx);
        self.slash = None;
        self.flush(cx);
        let store = db::store(cx);
        if sc.kind == BlockType::Divider {
            let Some((parent, ix)) = self.position(id) else {
                return;
            };
            if rest.is_empty() {
                ok(store.update_type(id, BlockType::Divider));
                let next = self.create_at(&parent, ix + 1, BlockType::Text, cx);
                self.after_write(window, cx);
                if let Some(b) = next {
                    self.start_edit(&b.id, 0, window, cx);
                }
            } else {
                self.create_at(&parent, ix, BlockType::Divider, cx);
                self.after_write(window, cx);
                self.start_edit(id, 0, window, cx);
            }
            return;
        }
        ok(store.update_type(id, sc.kind.clone()));
        if sc.checked {
            ok(store.set_prop(id, "checked", json!(true)));
        }
        if let Some(n) = sc.start {
            ok(store.set_prop(id, "list_start_index", json!(n)));
        }
        if sc.kind == BlockType::Code {
            ok(store.set_prop(id, "language", json!("Plain Text")));
        }
        self.after_write(window, cx);
        self.start_edit(id, 0, window, cx);
    }

    /// New block at `index` under `parent` (writes only; caller reloads).
    fn create_at(&self, parent: &str, index: usize, kind: BlockType, cx: &App) -> Option<Block> {
        let store = db::store(cx);
        let sibs = self.children.get(parent).cloned().unwrap_or_default();
        let after = index.checked_sub(1).and_then(|i| sibs.get(i));
        let b = ok(store.create_block(
            &self.workspace(),
            Some(parent),
            after.map(String::as_str),
            kind,
        ))?;
        if index == 0 && !sibs.is_empty() {
            ok(store.move_block(&b.id, Some(parent), 0));
        }
        Some(b)
    }

    fn turn_into(
        &mut self,
        id: &str,
        kind: BlockType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(b) = self.blocks.get(id) else { return };
        if !kinds::editable(&b.kind) && b.kind != BlockType::Page {
            return;
        }
        let has_lang = b.str_prop("language").is_some();
        self.flush(cx);
        let store = db::store(cx);
        ok(store.update_type(id, kind.clone()));
        if kind == BlockType::Code && !has_lang {
            ok(store.set_prop(id, "language", json!("Plain Text")));
        }
        self.after_write(window, cx);
        if self.editing.as_deref() == Some(id) {
            let p = kinds::placeholder(&kind);
            self.editor
                .update(cx, |s, cx| s.set_placeholder(p, window, cx));
        }
    }

    fn turn_focused(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        match self.focused_block(window, cx) {
            Some(id) => self.turn_into(&id, BlockType::parse(kinds::TURN_INTO[ix].1), window, cx),
            None => cx.propagate(),
        }
    }

    fn set_block_prop(
        &mut self,
        id: &str,
        key: &str,
        value: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flush(cx);
        ok(db::store(cx).set_prop(id, key, value));
        self.after_write(window, cx);
    }

    fn toggle_checked(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let checked = self.blocks.get(id).is_some_and(|b| b.checked());
        self.set_block_prop(id, "checked", json!(!checked), window, cx);
    }

    fn toggle_open(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.open_toggles.remove(id) {
            self.open_toggles.insert(id.to_string());
        }
        self.rebuild_rows();
        cx.notify();
    }

    fn delete_block(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.flush(cx);
        ok(db::store(cx).trash(id));
        self.after_write(window, cx);
    }

    fn duplicate_block(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        self.flush(cx);
        ok(db::store(cx).duplicate_page(id));
        self.after_write(window, cx);
    }

    /// Inserts an empty text block after `id` and opens the slash menu in it.
    fn add_below(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((parent, ix)) = self.position(id) else {
            return;
        };
        self.flush(cx);
        let b = self.create_at(&parent, ix + 1, BlockType::Text, cx);
        self.after_write(window, cx);
        if let Some(b) = b {
            self.start_edit(&b.id, 0, window, cx);
            self.editor.update(cx, |s, cx| s.insert("/", window, cx));
        }
    }

    /// Click below the last block: reuse a trailing empty text block or add one.
    fn edit_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let last = self
            .children
            .get(&self.page_id)
            .and_then(|c| c.last())
            .and_then(|id| self.blocks.get(id));
        if let Some(b) = last.filter(|b| b.kind == BlockType::Text && b.title.is_empty()) {
            let id = b.id.clone();
            self.start_edit(&id, 0, window, cx);
            return;
        }
        let n = self.children.get(&self.page_id).map_or(0, Vec::len);
        let page_id = self.page_id.clone();
        self.flush(cx);
        let b = self.create_at(&page_id, n, BlockType::Text, cx);
        self.after_write(window, cx);
        if let Some(b) = b {
            self.start_edit(&b.id, 0, window, cx);
        }
    }

    fn focus_first_block(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let first = self.rows.iter().find(|r| kinds::editable(&r.kind));
        match first {
            Some(r) => {
                let id = r.id.clone();
                self.start_edit(&id, 0, window, cx);
            }
            None => self.edit_end(window, cx),
        }
    }

    /// Nearest editable row before (`dir` < 0) or after the block being edited.
    fn neighbor(&self, id: &str, back: bool) -> Option<String> {
        let ix = self.row_index(id)?;
        let pick = |r: &Row| kinds::editable(&r.kind).then(|| r.id.clone());
        if back {
            self.rows[..ix].iter().rev().find_map(pick)
        } else {
            self.rows[ix + 1..].iter().find_map(pick)
        }
    }

    fn choose_slash(&mut self, item: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editing.clone() else {
            return;
        };
        let Some(q) = self.slash_query(cx) else {
            self.slash = None;
            return;
        };
        let Some(s) = self.slash.take() else { return };
        let Some(item) = item.or_else(|| kinds::slash_matches(&q).get(s.selected).copied()) else {
            return;
        };
        let end = s.start + 1 + q.len();
        self.rich = text::apply_edit(&self.rich, s.start..end, "");
        self.set_title(&id, self.rich.clone(), cx);
        let text = self.rich.plain_text();
        self.editor.update(cx, |ed, cx| {
            ed.set_value(text, window, cx);
            ed.set_selected_range(s.start..s.start, cx);
        });
        let kind = BlockType::parse(kinds::SLASH_ITEMS[item].1);
        let empty = self.rich.is_empty();
        if empty && kinds::editable(&kind) {
            self.turn_into(&id, kind, window, cx);
            return;
        }
        let Some((parent, ix)) = self.position(&id) else {
            return;
        };
        self.flush(cx);
        let store = db::store(cx);
        let created = if kind.is_collection() {
            let full = kind == BlockType::CollectionViewPage;
            ok(store.create_database(&self.workspace(), Some(&parent), Some(&id), full))
        } else {
            self.create_at(&parent, ix + 1, kind.clone(), cx)
        };
        let Some(b) = created else { return };
        if empty {
            ok(store.trash(&id));
        }
        let after_divider = (kind == BlockType::Divider)
            .then(|| self.create_at(&parent, ix + 2, BlockType::Text, cx))
            .flatten();
        if kind == BlockType::Code {
            ok(store.set_prop(&b.id, "language", json!("Plain Text")));
        }
        if kind == BlockType::Table {
            ok(store.fill_table(&b.id, 2, 3));
        }
        self.after_write(window, cx);
        match kind {
            BlockType::Table => {
                if let Some(r) = self.children.get(&b.id).and_then(|c| c.first()).cloned() {
                    self.start_cell_edit(&r, 0, 0, window, cx);
                }
            }
            BlockType::Page | BlockType::CollectionViewPage => db::open_page(b.id, cx),
            BlockType::Image => self.choose_image(b.id, window, cx),
            BlockType::Divider => {
                if let Some(t) = after_divider {
                    self.start_edit(&t.id, 0, window, cx);
                }
            }
            _ => self.start_edit(&b.id, 0, window, cx),
        }
    }

    fn choose_image(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.pick_file(window, cx, move |this, path, window, cx| {
            this.set_block_prop(&id, "source", json!(path), window, cx)
        });
    }

    fn pick_file(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Self, String, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose image".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            // A copy in the app's files dir survives moving or deleting the original.
            let files = crate::import::files_dir(&crate::store::Store::default_path());
            let path = if path.parent() == Some(files.as_path()) {
                path
            } else {
                crate::import::copy_to_files(&path, &files).unwrap_or(path)
            };
            let path = path.to_string_lossy().to_string();
            this.update_in(cx, |this, window, cx| done(this, path, window, cx))
                .ok();
        })
        .detach();
    }

    fn toggle_annotation(&mut self, ann: Annotation, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.focused_block(window, cx) else {
            cx.propagate();
            return;
        };
        let range = self.editor.read(cx).selected_range();
        self.rich = text::toggle(&self.rich, range, &ann);
        self.set_title(&id, self.rich.clone(), cx);
        cx.notify();
    }

    fn open_link_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let range = self.editor.read(cx).selected_range();
        let Some(id) = self.focused_block(window, cx).filter(|_| !range.is_empty()) else {
            cx.propagate();
            return;
        };
        let current = text::slice(&self.rich, range.clone())
            .0
            .first()
            .and_then(|s| s.link().map(str::to_string))
            .unwrap_or_default();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Paste link, empty to remove")
                .default_value(current)
        });
        let sub = cx.subscribe_in(
            &input,
            window,
            |this, state, ev: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = ev {
                    let url = state.read(cx).value().trim().to_string();
                    this.apply_link(&url, window, cx);
                }
            },
        );
        input.update(cx, |s, cx| s.focus(window, cx));
        self.link = Some(LinkPrompt {
            id,
            range,
            input,
            _sub: sub,
        });
        cx.notify();
    }

    fn apply_link(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.link.take() else { return };
        if let Some(b) = self.blocks.get(&p.id) {
            let title = text::set_link(&b.title, p.range.clone(), url);
            self.set_title(&p.id, title, cx);
        }
        self.start_edit(&p.id, p.range.end, window, cx);
    }

    // ---- key handling (capture phase, before the Textarea sees the key)

    fn on_enter(&mut self, a: &input::Enter, window: &mut Window, cx: &mut Context<Self>) {
        if self.cell_focused(window, cx) {
            cx.stop_propagation();
            self.move_cell(1, true, true, window, cx);
            return;
        }
        if self.title_focused(window, cx) {
            cx.stop_propagation();
            self.focus_first_block(window, cx);
            return;
        }
        if !self.editor_focused(window, cx) {
            return;
        }
        let Some(id) = self.editing.clone() else {
            return;
        };
        if self.slash.is_some() {
            cx.stop_propagation();
            self.choose_slash(None, window, cx);
            return;
        }
        let kind = self.blocks[&id].kind.clone();
        if a.secondary {
            cx.stop_propagation();
            match kind {
                BlockType::ToDo => self.toggle_checked(&id, window, cx),
                BlockType::Toggle => self.toggle_open(&id, cx),
                _ => {}
            }
            return;
        }
        if a.shift || kinds::is_monospace(&kind) {
            return;
        }
        cx.stop_propagation();
        let empty = self.rich.is_empty();
        if kinds::is_list(&kind) && empty {
            self.turn_into(&id, BlockType::Text, window, cx);
            return;
        }
        let cursor = self.editor.read(cx).cursor();
        let new_kind = if kinds::is_list(&kind) {
            kind.clone()
        } else {
            BlockType::Text
        };
        let Some((parent, ix)) = self.position(&id) else {
            return;
        };
        if cursor == 0 && !empty {
            self.flush(cx);
            self.create_at(&parent, ix, new_kind, cx);
            self.after_write(window, cx);
            return;
        }
        let (left, right) = text::split_at(&self.rich, cursor);
        self.rich = left.clone();
        self.set_title(&id, left, cx);
        self.flush(cx);
        let (parent, ix, new_kind) = if kind == BlockType::Toggle && self.open_toggles.contains(&id)
        {
            (id.clone(), 0, BlockType::Text)
        } else {
            (parent, ix + 1, new_kind)
        };
        let Some(b) = self.create_at(&parent, ix, new_kind, cx) else {
            return;
        };
        ok(db::store(cx).update_title(&b.id, &right));
        self.after_write(window, cx);
        self.start_edit(&b.id, 0, window, cx);
    }

    fn on_backspace(&mut self, _: &input::Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editor_focused(window, cx) || self.editor.read(cx).selected_range() != (0..0) {
            return;
        }
        let Some(id) = self.editing.clone() else {
            return;
        };
        let kind = self.blocks[&id].kind.clone();
        let empty = self.rich.is_empty();
        if kind == BlockType::Code && !empty {
            return;
        }
        cx.stop_propagation();
        self.slash = None;
        if kind != BlockType::Text {
            self.turn_into(&id, BlockType::Text, window, cx);
            return;
        }
        let Some(ix) = self.row_index(&id) else {
            return;
        };
        let Some(prev) = ix.checked_sub(1).map(|i| self.rows[i].clone()) else {
            return;
        };
        if !kinds::editable(&prev.kind) {
            if prev.kind == BlockType::Divider {
                self.delete_block(&prev.id, window, cx);
            } else if empty && let Some(target) = self.neighbor(&id, true) {
                self.delete_block(&id, window, cx);
                let end = self
                    .blocks
                    .get(&target)
                    .map_or(0, |b| b.title.plain_text().len());
                self.start_edit(&target, end, window, cx);
            }
            return;
        }
        // Merge into the previous block; our children move up to where we were.
        let prev_title = self.blocks[&prev.id].title.clone();
        let join = prev_title.plain_text().len();
        let merged = text::concat(prev_title, self.rich.clone());
        self.set_title(&prev.id, merged, cx);
        self.dirty.remove(&id);
        self.flush(cx);
        let store = db::store(cx);
        if let Some((parent, pos)) = self.position(&id) {
            for (k, child) in self
                .children
                .get(&id)
                .cloned()
                .unwrap_or_default()
                .iter()
                .enumerate()
            {
                ok(store.move_block(child, Some(&parent), pos + 1 + k));
            }
        }
        ok(store.trash(&id));
        self.after_write(window, cx);
        self.start_edit(&prev.id, join, window, cx);
    }

    fn caret_on_edge(&self, cx: &App) -> (bool, bool) {
        let s = self.editor.read(cx);
        let p = s.cursor_position();
        let lines = s.value().matches('\n').count() + 1;
        let (first, last) = (p.line == 0, p.line as usize + 1 >= lines);
        match (s.cursor_layout(), s.text_bounds()) {
            (Some((c, lh)), Some(t)) => {
                let cy = c.center().y;
                (first && cy < t.top() + lh, last && cy > t.bottom() - lh)
            }
            _ => (first, last),
        }
    }

    /// Byte offset in `text` for char column `col` on its first or last line.
    fn offset_at_column(text: &str, col: usize, last_line: bool) -> usize {
        let start = if last_line {
            text.rfind('\n').map_or(0, |i| i + 1)
        } else {
            0
        };
        let line = &text[start..];
        let line = line.split('\n').next().unwrap_or("");
        start + line.char_indices().nth(col).map_or(line.len(), |(i, _)| i)
    }

    fn on_vertical(&mut self, up: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.cell_focused(window, cx) {
            cx.stop_propagation();
            self.move_cell(if up { -1 } else { 1 }, true, false, window, cx);
            return;
        }
        if self.slash.is_some() && self.editor_focused(window, cx) {
            cx.stop_propagation();
            let n = self
                .slash_query(cx)
                .map_or(0, |q| kinds::slash_matches(&q).len())
                .max(1);
            let Some(s) = self.slash.as_mut() else { return };
            s.selected = if up {
                (s.selected + n - 1) % n
            } else {
                (s.selected + 1) % n
            };
            cx.notify();
            return;
        }
        if self.title_focused(window, cx) {
            if !up {
                cx.stop_propagation();
                self.focus_first_block(window, cx);
            }
            return;
        }
        if !self.editor_focused(window, cx) {
            return;
        }
        let Some(id) = self.editing.clone() else {
            return;
        };
        let (first, last) = self.caret_on_edge(cx);
        if (up && !first) || (!up && !last) {
            return;
        }
        cx.stop_propagation();
        let col = self.editor.read(cx).cursor_position().character as usize;
        match self.neighbor(&id, up) {
            Some(target) => {
                let text = self.blocks[&target].title.plain_text();
                let at = Self::offset_at_column(&text, col, up);
                self.start_edit(&target, at, window, cx);
            }
            None if up => {
                self.editing = None;
                self.title.update(cx, |s, cx| s.focus(window, cx));
                cx.notify();
            }
            None => {}
        }
    }

    fn on_horizontal(&mut self, left: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editor_focused(window, cx) {
            return;
        }
        let Some(id) = self.editing.clone() else {
            return;
        };
        let (sel, len) = {
            let s = self.editor.read(cx);
            (s.selected_range(), s.value().len())
        };
        let edge = if left {
            sel == (0..0)
        } else {
            sel == (len..len)
        };
        if !edge {
            return;
        }
        if let Some(target) = self.neighbor(&id, left) {
            cx.stop_propagation();
            let end = if left {
                self.blocks[&target].title.plain_text().len()
            } else {
                0
            };
            self.start_edit(&target, end, window, cx);
        }
    }

    fn on_indent(&mut self, outdent: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.cell_focused(window, cx) {
            cx.stop_propagation();
            self.move_cell(if outdent { -1 } else { 1 }, false, !outdent, window, cx);
            return;
        }
        if !self.editor_focused(window, cx) {
            return;
        }
        let Some(id) = self.editing.clone() else {
            return;
        };
        if kinds::is_monospace(&self.blocks[&id].kind) {
            return;
        }
        cx.stop_propagation();
        let Some((parent, ix)) = self.position(&id) else {
            return;
        };
        self.flush(cx);
        let store = db::store(cx);
        if outdent {
            if parent == self.page_id {
                return;
            }
            let Some((grand, pix)) = self.position(&parent) else {
                return;
            };
            ok(store.move_block(&id, Some(&grand), pix + 1));
        } else {
            let Some(prev) = ix.checked_sub(1).map(|i| self.children[&parent][i].clone()) else {
                return;
            };
            if !kinds::can_have_children(&self.blocks[&prev].kind) {
                return;
            }
            let n = self.children.get(&prev).map_or(0, Vec::len);
            ok(store.move_block(&id, Some(&prev), n));
            self.open_toggles.insert(prev);
        }
        self.after_write(window, cx);
        self.editor.update(cx, |s, cx| s.focus(window, cx));
    }

    fn on_escape(&mut self, _: &input::Escape, window: &mut Window, cx: &mut Context<Self>) {
        if self.slash.take().is_some()
            || self.menu.take().is_some()
            || self.table_menu.take().is_some()
        {
            cx.stop_propagation();
        } else if self.cell_focused(window, cx) {
            cx.stop_propagation();
            self.flush(cx);
            self.cell_edit = None;
            self.focus_handle.focus(window, cx);
        } else if let Some(p) = self.link.take() {
            cx.stop_propagation();
            self.start_edit(&p.id, p.range.end, window, cx);
        } else if let Some(id) = self.focused_block(window, cx) {
            cx.stop_propagation();
            self.select_blocks(id.clone(), id, window, cx);
        }
        cx.notify();
    }

    /// Drag-and-drop: put `dragged` right before `target`, or at the end of the page.
    fn drop_block(
        &mut self,
        dragged: &str,
        target: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if target == Some(dragged) || self.locked() {
            return;
        }
        let (parent, before) = match target.and_then(|t| self.position(t)) {
            Some((parent, _)) => (parent, target.map(str::to_string)),
            None => (self.page_id.clone(), None),
        };
        let sibs: Vec<_> = self.children.get(&parent).cloned().unwrap_or_default();
        let sibs: Vec<_> = sibs.into_iter().filter(|s| s != dragged).collect();
        let index = before
            .and_then(|b| sibs.iter().position(|s| *s == b))
            .unwrap_or(sibs.len());
        self.flush(cx);
        ok(db::store(cx).move_block(dragged, Some(&parent), index));
        self.after_write(window, cx);
    }
}

impl Focusable for PageEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
