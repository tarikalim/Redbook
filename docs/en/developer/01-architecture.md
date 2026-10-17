# Architecture

Redbook is a local-first, single-user, block-based notes app —
nested pages, blocks, databases with views — written in Rust on GPUI. One
goal outranks the rest: an open, idle window must cost almost nothing. The
idle footprint went from 121 MB to about 31 MB, an idle window draws no
frames and does no polling work beyond a 30 s safety-net check while the
window is active, and most of the structure on this page
exists to keep it that way. The measurements and the method are in
[04-memory-and-idle.md](04-memory-and-idle.md).

## What runs: one binary, four entry points

Everything ships as one executable, `redbook`. `main()` in
[`src/main.rs`](../../../src/main.rs) looks at the first argument and either
runs a command-line tool and exits, or starts the GUI.

| Invocation | What it does | Code | Exits |
|---|---|---|---|
| `redbook` | The GUI: one window, sidebar, editor, database views | `app.rs`, `sidebar.rs`, `editor/`, `database/` | On quit or when the last window closes |
| `redbook mcp` | MCP server on stdin/stdout, newline-delimited JSON-RPC 2.0 | `mcp.rs` | When stdin closes |
| `redbook import <Export.zip\|folder> [--workspace NAME]` | Imports a "Markdown & CSV" export | `import.rs` | After the import |
| `redbook sync init\|clone\|now\|status\|dir …` | Sets up and runs git-based sync | `sync.rs` | After the command |

The reason for one binary is one store: every entry point opens the same
SQLite file through the same `Store` type in
[`src/store.rs`](../../../src/store.rs), so a schema change or a migration
reaches the MCP server and the importer in the same build. It also means the
MCP client is registered against the installed app's binary, for example
`/Applications/Redbook.app/Contents/MacOS/redbook mcp`.

On Windows the release binary is a GUI-subsystem program with no console.
`sync` and `import` call `AttachConsole` on the parent terminal so their output
is visible; `mcp` does not need it, because its stdio is a pipe.

## Why the process re-executes itself at startup

The first call in `main()` is `mem::ensure_space_efficient_malloc()`. On macOS
it checks for the `MallocSpaceEfficient` environment variable and, when it is
missing, `exec`s the same binary with the same arguments and
`MallocSpaceEfficient=1` set. `exec` replaces the process image, so the PID
stays the same and no second process exists.

The variable has to be in the environment before libmalloc initialises, which
is before any Rust code runs; setting it from inside the process has no
effect. With it, freed small-allocation pages leave the process footprint
instead of staying dirty. Commit `ddfb002` measured `MALLOC_SMALL` 17.8 →
8.5 MB at idle.

The app bundle sets the variable through `LSEnvironment` in `Info.plist`
(written by [`scripts/bundle.sh`](../../../scripts/bundle.sh)), so a launch
from Finder or the Dock skips the `exec`. A launch from a terminal, and every
`redbook mcp` started by an MCP client, pays one extra `exec`. If `exec`
fails, the process continues with default malloc settings. On Linux and
Windows the function is empty.

## Which files the processes share

All four entry points resolve the database with `Store::default_path()`.

| Item | Location | Notes |
|---|---|---|
| Database | `$REDBOOK_DB`, else `<data dir>/RedBook/redbook.db` | Data dir from `dirs::data_dir()`: `~/Library/Application Support` (macOS), `$XDG_DATA_HOME` or `~/.local/share` (Linux), `%APPDATA%` (Windows) |
| WAL and shared memory | `redbook.db-wal`, `redbook.db-shm` next to it | SQLite WAL mode |
| Settings | `redbook.settings.json` next to the database | `workspace`, `sidebar_width`, `sidebar_hidden`, `recent` (written by the GUI), `theme` (`light`, `dark` or `system`, written by `app::set_theme`) and `sync_dir` (written by sync) |
| Attachments | `files/` next to the database | Files copied by the importer and by a sync import; a file picked in the editor is not copied, and `props.source` holds its original absolute path |
| Sync working tree | `sync/` next to the database, or `sync_dir` | Only when sync is set up; see [06-sync.md](06-sync.md) |

The data folder is named `RedBook` and the binary `redbook` although the
display name is `Redbook`: commit `10520b3` renamed the display name only, so
existing data and the `redbook://page/<id>` link scheme kept working.
`REDBOOK_DB` exists for tests and for running a second, throwaway profile; the
headless GUI test and `tests/mcp_stdio.rs` both set it.

The settings file is shared by three writers. `Settings::save()` in
[`src/app.rs`](../../../src/app.rs) reads the file, merges its own keys and
writes it back, so it keeps `sync_dir`, which only `sync.rs` writes, and
`theme`, which only `set_theme` writes. `apply_theme` reads `theme` at launch
and on every system appearance change; only `system` follows the OS.

## Module map

| Module | Role | Depends on (besides `model`) |
|---|---|---|
| `main.rs` | Argument dispatch, global key bindings, menus, window | everything |
| `mem.rs` | Process-level memory settings: malloc relaunch, glyph cache cap | — |
| `model.rs` | `Block`, `BlockType`, `RichText`, `Schema`, `View`, `Query`, `DbRow` | serde |
| `store.rs` | SQLite schema, migrations, all reads and writes, search, markdown in and out, query evaluation | `model` |
| `db.rs` | GPUI globals: the shared `Store`, the revision signal, the file watcher, navigation | `store` |
| `app.rs` | Root view `AppView`: sidebar, top bar, page pane, history, drafts, settings file | `db`, `store`, `editor`, `database`, `sidebar`, `search`, `sync` |
| `sidebar.rs` | Workspace switcher, favorites, page tree, trash dialog, settings dialog | `app`, `db`, `store`, `search`, `sync` |
| `search.rs` | `secondary-p` palette: full-text search, recent pages, "Move to" | `app`, `db` |
| `editor/` | `PageEditor`: blocks, slash menu, shortcuts, simple tables | `db`, `database` |
| `database/` | `DatabaseView` (table, board, list, gallery, calendar), `RowProperties`, property editors, calculations | `db`, `store` (`Store`, `apply_query`, `value_text`) |
| `import.rs` | Markdown & CSV importer | `store` |
| `mcp.rs` | MCP server | `store`, `import` (value parsing, workspace lookup) |
| `sync.rs` | Git sync: export, merge, import, background runs | `store`, `db` |

`store.rs` has no GPUI dependency. That boundary is what lets `mcp`, `import`
and `sync` run without a window, and what lets most tests run without one.

## How views share the store: `db.rs`

[`src/db.rs`](../../../src/db.rs) installs one GPUI global, `Db`, holding:

| Field | Type | Purpose |
|---|---|---|
| `store` | `Rc<Store>` | The GUI's single SQLite connection |
| `revision` | `Entity<Revision>` | A counter bumped after every write |
| `seen` | `Cell<Option<i64>>` | `PRAGMA data_version` at the last check |

Views read with `db::store(cx)` and, after any write, call `db::changed(cx)`,
which increments the counter and notifies observers. Every long-lived view
observes the revision entity and reloads: `AppView`, `Sidebar`, `PageEditor`,
each `DatabaseView` and `RowProperties`.

The connection is an `Rc`, not an `Arc` behind a lock: every GUI read and write
happens on the main thread. The sync background job opens its own connection
instead of borrowing this one.

The revision is one counter for the whole database, not a per-page event.
That keeps the write side trivial — a writer cannot forget which views to tell
— and moves the cost to the readers, which re-query on every bump. Two readers
cut that cost: `PageEditor` remembers the revision it caused itself
(`own_rev`) and skips its own reload, and `DatabaseView` re-reads rows only
when `Store::rows_stamp()` (row count, max `edited`, sum of `sort`) changed.

`db.rs` also holds navigation: a `Nav` entity with the open `page` and
`workspace`. Any view calls `db::open_page(id, cx)`; `AppView` and `Sidebar`
observe `Nav` and follow. Links inside text use the same call for
`redbook://page/<id>` targets.

## How the GUI notices writes from other processes

The MCP server, the importer and a sync run write the same file through their
own connections. `db::watch()` starts a `notify` watcher on the database's
folder (non-recursive) and reacts only to events on `redbook.db` and
`redbook.db-wal`. A burst of events is coalesced for 150 ms, then
`db::sync_external()` reads `PRAGMA data_version`. That value changes only
when *another* connection committed, so the GUI's own writes never trigger a
second reload. When it changed, `db::changed()` refreshes every view.

| Mechanism | Interval | Runs when |
|---|---|---|
| File watcher, coalesced | 150 ms after the first event | Always, while the watcher works |
| Safety-net check | 30 s | Timer always armed; the check runs only while the window is active |
| Fallback poll, no watcher | 1 s active, 5 s inactive | Only if `notify` could not start |

The watcher replaced a 1 s `data_version` poll in commit `b52f565`. The poll
woke the process every second for a check that almost never found anything;
the watcher costs nothing until the file changes, and the commit measured an
MCP write reaching the UI in about 160–190 ms instead of up to 1 s.

## What happens on one keystroke

An edit in the focused text block travels this path, in this order:

1. The shared `TextareaState` emits `InputEvent::Change`.
   `PageEditor::on_edit` diffs the old and new plain text (`text::diff`) and
   applies the edit to the block's `RichText` with `text::apply_edit`, so
   formatting around the edit survives.
2. Markdown shortcuts (`# `, `[ ] `, `1. `, …), inline markdown
   (`**bold**`, `` `code` ``) and the slash menu are checked on the same event.
3. `set_title` marks the block dirty and `schedule_save` restarts a 300 ms
   timer. Typing continuously writes nothing.
4. When the timer fires, `flush` → `write_dirty` writes every dirty block in
   one transaction (`Store::tx`) with `Store::update_title`.
5. SQLite triggers update the FTS index (`blocks_au`) and, when sync is set
   up, the change log `sync_dirty` (`sync_bu`).
6. `notify_db` calls `db::changed` and records the new revision as `own_rev`.
7. Observers run: `PageEditor` skips its own revision; `AppView` re-reads the
   page block and breadcrumb; `Sidebar` re-reads the page tree; every
   `DatabaseView` compares its rows stamp.
8. `AppView`'s second revision observer calls `sync::changed`, which restarts
   a 10 s debounce. When it fires, a sync run exports, commits and pushes on a
   background thread with its own connection.

Leaving a block (`Blur`) calls `flush`; quitting calls it only while the
launch page is still a draft (`leave_draft`); dropping the editor
(`on_release`) calls `write_dirty` and `db::changed`. Either way the 300 ms
window does not lose text.

## What happens at startup and at quit

At startup, after argument dispatch, the GUI:

1. Initialises gpui-kit, then caps CoreGraphics' glyph cache to 0 bytes
   (`mem::cap_glyph_cache(0)`; see
   [04-memory-and-idle.md](04-memory-and-idle.md)).
2. Opens the database and installs `Db` (`db::init`).
3. Starts sync if this database is set up for it, with one full pull
   (`sync::start`).
4. Binds global keys, *then* sets the menus — `cx.set_menus` copies shortcut
   labels at call time.
5. Opens one 1200×800 window with `AppView`, which picks the remembered
   workspace (creating "Personal" on first run), deletes untouched launch pages
   left by a crash, creates a new blank page, and focuses its title.

At quit, `AppView`'s `on_app_quit` handler deletes the launch page if it is
still blank and calls `sync::quit`, which exports and pushes with a 5 s git
deadline while the app waits.

## Why every launch opens a blank page

Commit `1bbc5c0` dropped the welcome page and the last-page restore. Each
launch creates a top-level page with `props.draft = true` and focuses its
title, so the app opens ready to type. The flag is removed on the first edit.
A page still blank when the user navigates away or quits is deleted for good,
not trashed, and leftovers from a crash are removed at the next launch. Sync
skips draft pages, so an untouched launch page never reaches the remote.

## Why focus is parked while the window is inactive

A focused text input blinks its cursor, and each blink is a frame. GPUI keeps
blinking in an inactive window. `AppView::on_activation` blurs the focused
element when the window deactivates and restores it on activation, so a
background window draws nothing. Commit `b52f565`, which added the parking, also made the sidebar a cached
view, so a blink or an edit in the page does not re-render it. The blink cap
in the vendored `gpui-base` covers the focused, front window; see
[05-vendored-gpui.md](05-vendored-gpui.md).

## Known gaps

- **One window.** Closing the last window quits the app. There is no second
  window, no tabs, and no per-window state.
- **All GUI SQL runs on the main thread.** A slow query blocks input. The
  editor re-reads the whole open page (block by block, recursively) on every
  revision it did not cause, including writes to unrelated pages.
- **The revision is global.** Any write anywhere reloads the sidebar's full
  page tree and every open database's schema and views. Cheap at hundreds of
  pages; not measured at tens of thousands.
- **Concurrent writers do not coordinate.** The GUI, the MCP server and a sync
  run each hold their own connection; SQLite serialises commits
  (`busy_timeout` 5000 ms) but nothing merges them. A block the MCP server
  deletes while the editor still holds an unsaved edit for it is updated in
  zero rows, and the edit is lost without a message.
- **The safety-net timer is always armed.** It skips its check while the
  window is inactive but still wakes every 30 s. In a background window
  measured with `top -c d` on macOS 26 it did not show up as idle wakeups over
  two minutes; that is App Nap's timer coalescing, not a guarantee.
- **A sync run echoes.** The run's own commit changes the WAL, the watcher
  reports it, `db::changed` fires and schedules one more push run 10 s later,
  which finds nothing to export and spawns no git process.
- **The watcher is local-filesystem only.** On a network share `notify` may
  miss events; the 30 s check then covers an active window only.
