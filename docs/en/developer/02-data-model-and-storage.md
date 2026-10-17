# Data model and storage

Redbook stores everything in one SQLite file through `Store` in
[`src/store.rs`](../../../src/store.rs), with the types in
[`src/model.rs`](../../../src/model.rs). In the model a page is
a block, everything inside a page is a block, a database is a block with a
schema, and a database row is a page. Type ids are short names (`text`,
`header`, `to_do`, …).

## Where the database file lives

`Store::default_path()` returns `$REDBOOK_DB` when set, else
`<data dir>/RedBook/redbook.db`:

| OS | Data dir (`dirs::data_dir()`) |
|---|---|
| macOS | `~/Library/Application Support` |
| Linux | `$XDG_DATA_HOME`, else `~/.local/share` |
| Windows | `%APPDATA%` |

`Store::open` creates the folder when missing. `Store::open_in_memory` is for
tests.

## What the schema looks like

The schema is created by migration 1 in the `MIGRATIONS` array. The SQL there
is the source of truth; this table summarises it.

| Table | Columns | Notes |
|---|---|---|
| `workspaces` | `id` TEXT PK, `name`, `icon`, `sort` REAL, `created` INTEGER | Top-level containers; a workspace's root pages have `parent_id` NULL |
| `blocks` | `rid` INTEGER PK, `id` TEXT UNIQUE, `workspace_id` → workspaces ON DELETE CASCADE, `parent_id` → blocks ON DELETE CASCADE, `type`, `sort` REAL, `title` TEXT (rich-text JSON, default `'[]'`), `props` TEXT (JSON, default `'{}'`), `created`, `edited` (ms since epoch), `trashed` INTEGER NULL, `favorite` INTEGER | Every page, block and database row |
| `collections` | `id` TEXT PK → blocks ON DELETE CASCADE, `schema` TEXT (JSON) | One per database block; the id is the database block's id |
| `views` | `id` TEXT PK, `collection_id` → collections ON DELETE CASCADE, `type`, `name`, `sort` REAL, `query` TEXT (JSON), `format` TEXT (JSON) | Table, board, list, gallery, calendar views |
| `blocks_fts` | FTS5 `text`, `tokenize = 'unicode61 remove_diacritics 2'` | Plain text of `blocks.title`, `rowid` = `blocks.rid` |
| `sync_state` | `key` TEXT PK, `value` | Migration 2; one row (`host`) once sync is set up |
| `sync_dirty` | `seq` INTEGER PK AUTOINCREMENT, `id` TEXT UNIQUE | Migration 2; change log for sync |

Indexes: `blocks_parent (parent_id, sort)`, `blocks_root (workspace_id,
parent_id, sort)`, `views_collection (collection_id, sort)`.

`blocks` has both an integer `rid` and a text `id`. FTS5 keys its rows by an
integer `rowid`; `rid` is a stable integer for that, while the UUID `id` is
what every other table and every file format uses.

## How migrations run

`Store::migrate` reads `PRAGMA user_version` and runs every `MIGRATIONS`
entry from that index on. Entry `i` migrates to version `i + 1`, and each one
runs in its own transaction together with the `user_version` update, so a
failed migration leaves the version where it was. The current version is 3.

Two rules follow, and nothing enforces them but review:

- **Append, never edit.** A database at version 2 never runs entries 0 or 1
  again, so a change to a shipped entry reaches new databases only.
- **No down-migrations.** An older build opening a newer file runs nothing
  and may misread new columns.

## What the connection PRAGMAs are and why

`Store::init` sets these on every connection — GUI, MCP, import and sync:

| Setting | Value | Why |
|---|---|---|
| `journal_mode` | `WAL` | Readers do not block the writer, so the GUI keeps reading while the MCP server or a sync run commits; also gives the watcher a `-wal` file to observe |
| `foreign_keys` | `ON` | Cascades delete children with their parent. The sync connection turns it `OFF`; see [06-sync.md](06-sync.md) |
| `synchronous` | `NORMAL` | The usual WAL pairing: durable against app crashes, may lose the last commits on power loss |
| `busy_timeout` | `5000` ms | Another process's write waits instead of failing |
| `cache_size` | `-64` (64 KiB) | Commits `c1517e0` and `1e054f3`: the 2 MB default page cache held the whole test database in malloc |
| `mmap_size` | `268435456` (256 MiB) | Reads map the file; mapped clean pages do not count toward the process footprint |
| Prepared-statement cache | 128 | `prepare_cached` everywhere; the hot queries are parsed once |

The page cache and mmap settings work together. With `mmap_size` set, read
access maps file pages instead of copying them into the malloc'd page cache,
so the cache only serves writes and drops to 64 KiB. Commit `1e054f3`
measured `MALLOC_SMALL` 7.34 → 7.03 MB on a content page.

The cost is error reporting: an I/O error on a mapped page arrives as a
`SIGBUS` signal and kills the process, instead of returning `SQLITE_IOERR`.
That was accepted because the database lives on the local data disk. Do not
point `REDBOOK_DB` at a network share, removable media or a cloud-synced
folder.

## How blocks form pages

Every row of `blocks` is a block of one `type`. Blocks form a tree through
`parent_id`; a block with `parent_id` NULL is a root page of its workspace.
Order among siblings is the `sort` column.

Three types are **page-like** (`BlockType::is_page_like`): `page`,
`collection_view` (inline database) and `collection_view_page` (full-page
database). Page-like blocks are what the sidebar tree, breadcrumbs, search
results and sync files are made of. Every other block belongs to the nearest
page-like ancestor.

| Kind | How it is stored |
|---|---|
| Text blocks | `text`, `header`, `sub_header`, `sub_sub_header`, `header_4`, `to_do`, `toggle`, `bulleted_list`, `numbered_list`, `quote`, `callout`, `code`, `equation`; content in `title` |
| Nested content | Children of the block (`parent_id`), e.g. under a `toggle` or a list item |
| Database | A `collection_view` or `collection_view_page` block plus a `collections` row with the same id and at least one `views` row |
| Database row | A `page` block whose parent is a database block; values in `props.properties` |
| Simple table | A `table` block (`props.width`, `props.header`) with `table_row` children; cells in `props.cells`, one rich-text array per cell |
| Unknown type | `BlockType::Unknown(String)` keeps the original type id, so a type Redbook does not render survives import, sync and editing |

The full list of type ids is `BLOCK_TYPE_IDS` in `model.rs`.

## How rich text is encoded

`blocks.title` holds a rich-text array: a list of segments, each a
string with an optional list of annotations.

```json
[["plain "],["bold",[["b"]]],["link",[["a","https://example.com"]]]]
```

| Code | `Annotation` | Argument |
|---|---|---|
| `b` | `Bold` | — |
| `i` | `Italic` | — |
| `s` | `Strike` | — |
| `_` | `Underline` | — |
| `c` | `Code` | — |
| `a` | `Link` | URL; `redbook://page/<id>` for page links |
| `h` | `Color` | Colour name, e.g. `red` or `red_background` |
| `e` | `Equation` | LaTeX; segment text is `⁍` |
| `p` | `PageMention` | Page id; segment text is `‣` |
| `d` | `Date` | Date object; segment text is `‣` |
| anything else | `Other` | Kept verbatim, written back unchanged |

`RichText::normalized()` merges adjacent segments with equal annotations and
drops empty ones. A malformed `title` parses as empty text rather than an
error, so one bad row cannot stop a page from opening.

## What `props` holds

`props` is a free-form JSON object. The keys the code reads:

| Key | Block types | Meaning |
|---|---|---|
| `checked` | `to_do` | bool |
| `language` | `code` | Code language label |
| `icon` | pages, `callout` | Emoji |
| `cover` | pages | `gradient_<i>`, a URL, or an absolute path (a picked file is copied into `files/` first) |
| `color` | any block | Colour name for the block |
| `source` | `image`, `file`, `video`, `audio`, `pdf`, `embed` | An absolute path or a URL; files picked in the app, imported or synced are copied into `files/` (`import::copy_to_files`) |
| `link` | `bookmark` | URL |
| `list_start_index` | `numbered_list` | First number of a list |
| `width`, `header` | `table` | Column count, header row on |
| `cells` | `table_row` | Array of rich-text arrays |
| `properties` | database rows | Property id → value |
| `small_text`, `full_width`, `locked` | pages | Page flags from the page menu |
| `draft` | pages | The untouched launch page; see [01-architecture.md](01-architecture.md#why-every-launch-opens-a-blank-page) |
| `source_id` | imported pages and databases | Export id, for idempotent re-import |

`Store::set_prop` sets or removes one key in a transaction;
`Store::update_props` replaces the object.

## How databases, properties and views are stored

A database is a block plus its `collections.schema`: a JSON object from
property id to `Property` (`name`, `type`, `options`, `config`). The title
property always has the id `title` and is the only one of type `title`;
`add_property`, `update_property` and `remove_property` refuse anything else.
New property ids are 8 hex characters.

| `PropType` | Stored value in `props.properties` |
|---|---|
| `text`, `url`, `email`, `phone_number` | string |
| `number` | number; `config.number_format` picks the display |
| `select`, `status` | option value string; `options` holds `{id, value, color}` |
| `multi_select` | array of option values |
| `date` | `{ "start": "2026-10-11" }`, optional `end` |
| `checkbox` | bool |
| `relation`, `file` | array of strings |
| `created_time`, `last_edited_time` | not stored; read from `created` / `edited` |
| `title` | not stored; the row's `title` |
| `formula`, `rollup` | type exists; no value is computed |

A view's `query` is a `Query`: `filters` (property, `FilterOp`, value),
`operator` (`and` / `or`), `sorts` and `group_by`. Its `format` holds layout
only: `order`, `hidden`, `widths`, `calcs`, `calendar_by`. Filtering and
sorting happen in Rust (`store::apply_query`), not in SQL, because values live
in JSON with type-dependent comparison rules. Select and status sort by option
order; empty values sort last in both directions.

## Why views hold rows as raw JSON

`DatabaseView` does not load rows as `Block`s. `Store::db_rows` returns
`DbRow`: id, title, timestamps and `props.properties` as a `Box<str>` of JSON
text. `DbRow::row_value` parses only the requested key with a streaming serde
visitor (`model::cell`). Rows are shared as `Rc<DbRow>` between the full list
and the filtered one, and `apply_query` reads each sort key once per row.

Commit `1efb00a` measured the reason: 100 rows × 8 properties held as parsed
`serde_json::Value` took 343 KB of heap for 25 KB of JSON, and the filtered
list was a second full clone. Footprint on that page went 33.21 → 32.21 MB.
The MCP server and markdown export still use `Block` rows; they are
short-lived.

## How sibling order works: fractional sort

`sort` is a REAL. Inserting between two siblings takes the midpoint
(`sort_between`); appending takes the last value + 1. When the gap between
neighbours falls below `1e-9`, `Store::place` renumbers all siblings to
1, 2, 3… in the same transaction and retries. A move therefore writes one row
in the common case and N rows rarely.

## How full-text search works

Triggers keep `blocks_fts` in step with `blocks`: `blocks_ai` on insert,
`blocks_au` on `UPDATE OF title, props`, `blocks_ad` on delete. Each indexes
the concatenated segment strings of `title`, the simple-table cells
(`props.cells`) and the text and number leaves of database values
(`props.properties`, via `json_tree`), computed in SQL by `fts_text!`.
Migration 3 rebuilt the triggers and backfilled `blocks_fts` from `blocks`.

`Store::search` turns each whitespace-separated word into a quoted prefix term
(`"word"*`), matches within one workspace, skips trashed blocks, orders by
FTS5 `rank`, reads at most 500 hits, then walks each hit up to its page-like
ancestor and keeps one hit per page until `limit`. `remove_diacritics 2` lets
`é` and `e` match each other.

## How trash works

`Store::trash` stamps the block and its whole subtree with one `trashed`
value, unique across the database (`max(max(trashed) + 1, now)`).
`Store::restore` clears exactly the rows carrying that stamp, so restoring a
page does not resurrect a child that was trashed separately before. If the
restored page's parent is still trashed, the page moves to the workspace
root. `list_trash` shows only the top of each trashed subtree.
`delete_forever` deletes the subtree children first.

Trashed rows stay in `blocks`, so they are still in `blocks_fts`; queries
filter them with `trashed IS NULL`.

## Which triggers exist

| Trigger | On | Does |
|---|---|---|
| `blocks_ai`, `blocks_au`, `blocks_ad` | `blocks` insert, title update, delete | Maintain `blocks_fts` |
| `sync_bi`, `sync_bu`, `sync_bd` | `blocks` insert, update, delete | Log the block id (and the old parent on a move or delete) in `sync_dirty` |
| `sync_ci`, `sync_cu` | `collections` insert, update | Log the database id |
| `sync_vi`, `sync_vu`, `sync_vd` | `views` insert, update, delete | Log the view's database id |

The sync triggers fire only `WHEN EXISTS (SELECT 1 FROM sync_state)`, so a
database without sync set up pays one cheap check per write and logs nothing.
Change tracking lives in triggers rather than in `Store` methods because the
MCP server, the importer and raw SQL in `sync.rs` all write; a trigger cannot
be bypassed by a writer that forgets. Each trigger runs `DELETE` then
`INSERT` instead of `INSERT OR REPLACE`: an outer `INSERT OR REPLACE`
statement would override the conflict clause inside the trigger. See
[06-sync.md](06-sync.md) for how the log is consumed.

## How markdown goes in and out

`Store::page_to_markdown` and `children_to_markdown` render a page for MCP
`get_page`, the page menu's "Export Markdown" and sync's generated
`read/<id>.md`. Child pages become markdown link lines pointing to `redbook://page/<id>`,
databases become a markdown table of the first view's rows, and simple tables
become markdown tables.

`Store::markdown_to_blocks` parses markdown into blocks under a parent, for
MCP `create_page` / `update_page` / `append_blocks` and the importer. It
handles headings `#`–`####`, bulleted, numbered and to-do lists nested by
indent, `>` quotes, fenced code with a language, `|` tables, `---` dividers,
markdown images, `$$ … $$` equations, `<details><summary>` toggles and
`<aside>` callouts with a leading emoji icon. Inline: `**bold**`, `*italic*`,
`` `code` ``, `~~strike~~` and markdown links.

Markdown is a view of the data, not a storage format. A `redbook://page/<id>`
line parses back as a text block with a link, not as the child page;
underline, colours, mentions, covers and database schemas have no markdown
form. Sync uses JSONL for that
reason.

## Known gaps

- **Formula and rollup are types without behaviour.** They can be declared
  (through MCP only; the importer never infers them) and are read-only to MCP writes; nothing evaluates them.
  Relations store id strings with no link to the target database.
- **No schema validation on `props`.** Any writer can store any JSON; readers
  fall back to defaults on a wrong shape.
- **Migrations are append-only by convention.** There is no test that fails
  when a shipped `MIGRATIONS` entry changes.
- **`mmap` trades error handling for footprint.** A disk error on the
  database is a crash, not an error message.
- **Trashed blocks are never purged automatically.** They stay in the file and
  in the FTS index until the user empties the trash.
- **`blocks.edited` is not bumped by every write.** `move_block_in` and
  `update_*` set it; `toggle_favorite` and `trash` do not.
