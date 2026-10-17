# MCP server

`redbook mcp` is a Model Context Protocol server over stdio that lets an AI
client read and write the same notes the app shows. It is the same binary as
the app, opens the same database, and is implemented in
[`src/mcp.rs`](../../../src/mcp.rs). It exists so AI tooling can work on local notes
without a cloud service, which is why its tools mirror what a person does in the
app — find a page, read it, write markdown, fill a database row — rather than
exposing the block model.

## How the protocol is spoken

| Aspect | Behaviour |
|---|---|
| Transport | stdin/stdout, one JSON-RPC 2.0 message per line; blank lines ignored; each response flushed |
| Protocol version | `PROTOCOL` = `2025-06-18`; echoes the client's version if it is in `SUPPORTED` (`2025-06-18`, `2025-03-26`, `2024-11-05`), else answers with its own |
| Capabilities | `tools` only, `listChanged: false`; no resources, prompts or logging |
| Methods | `initialize`, `ping`, `tools/list`, `tools/call` |
| Notifications | Accepted and ignored (no response) |
| Authentication | None |
| Lifetime | Until stdin closes |

`handle(store, line)` turns one line into at most one response, which is
what the unit test `protocol_and_tools` and the end-to-end test
[`tests/mcp_stdio.rs`](../../../tests/mcp_stdio.rs) exercise. The stdio test
starts the real binary with `REDBOOK_DB` pointing at a temporary file.

Errors come in two kinds, as MCP specifies:

| Situation | Response |
|---|---|
| Unparseable line | JSON-RPC error `-32700` |
| A message with an `id` but no `method`, `result` or `error` | `-32600` |
| Unknown method | `-32601` |
| `tools/call` for an unknown tool | `-32602` |
| A tool that runs and fails (missing argument, unknown id, invalid value) | A normal result with `isError: true` and the error chain as text |

Tool failures are results, not protocol errors, so the model reads the
message — for example `no property "Stauts" (have: Name, Status, Due)` — and
can correct its next call.

## How to register it with a client

Point the client at the installed binary with the `mcp` argument. For Claude
Code:

```sh
claude mcp add --scope user redbook -- /Applications/Redbook.app/Contents/MacOS/redbook mcp
```

On macOS the server re-executes itself once at startup to set
`MallocSpaceEfficient` (see
[01-architecture.md](01-architecture.md#why-the-process-re-executes-itself-at-startup));
the client does not notice, since the PID and stdio stay the same. Set
`REDBOOK_DB` in the client's server environment to serve a different
database.

## Which tools exist

| Tool | Required | Optional | Does |
|---|---|---|---|
| `list_workspaces` | — | — | Names and ids |
| `search` | `query` | `workspace`, `limit` (20) | Prefix full-text search, one hit per page, with snippet |
| `get_page` | `page_id` | — | A front-matter header (id, title, type, workspace, parent, breadcrumb, icon, trashed) and the page as markdown; databases list properties, views and up to 100 rows; rows list their property values; child pages listed at the end |
| `list_pages` | — | `workspace`, `parent_id` | The page tree (pages and databases, not rows) as an indented list |
| `create_page` | `title` | `parent_id`, `workspace`, `markdown`, `icon` | A page under a page, a row in a database (when `parent_id` is a database), or a root page |
| `update_page` | `page_id` | `title`, `icon` (empty removes), `markdown`, `trash_replaced` (true) | Retitle; with `markdown`, replace the content; `trash_replaced: false` deletes the old blocks instead of trashing them |
| `append_blocks` | `page_id`, `markdown` | — | Append markdown at the end |
| `move_page` | `page_id` | `new_parent_id`, `workspace`, `index` | Move under a page or to a workspace root; `index` defaults to last |
| `trash_page` | `page_id` | — | Trash the page and its subtree |
| `restore_page` | `page_id` | — | Restore; to the root if the parent is still trashed |
| `create_database` | `title` | `parent_id`, `workspace`, `properties` | Full-page database with a title property and a Table view |
| `query_database` | `database_id` | `filter`, `sorts`, `limit` (100) | Rows as a markdown table with row ids |
| `add_db_row` | `database_id` | `values` | New row |
| `update_db_row` | `row_id`, `values` | — | Set only the given properties |

`update_page`, `append_blocks`, `update_db_row` and `trash_page` first check
the target's `locked` prop (the app's **Lock page**) and return `isError` when
it is set (`unlocked`).

Output is markdown text, not JSON. The consumer is a language model, and ids
appear inline next to titles (`- Ideas (id: 9f…)`) so the next call can use
them without a second lookup.

The markdown accepted by `create_page`, `update_page` and `append_blocks` is
`Store::markdown_to_blocks`; its coverage is in
[02-data-model-and-storage.md](02-data-model-and-storage.md#how-markdown-goes-in-and-out).

## How values are named and parsed

Every argument that names something takes the name a person would see:

| Argument | Accepts |
|---|---|
| `workspace` | Workspace id or name, ASCII case-insensitive; default the first workspace |
| A property key in `values`, `filter`, `sorts` | Property id or name, ASCII case-insensitive (`prop_id`) |
| `properties[].type` in `create_database` | `PropType` names; also `rich_text` and `multi-select` as aliases |

Property ids are 8 random hex characters that appear nowhere a model would
look, while names appear in every `get_page` and `query_database` output.
Accepting names removes a lookup round-trip per write and a class of
wrong-id errors.

Values in `values` are coerced per property type (`coerce`, which reuses the
importer's `cell_value`):

| Property type | Accepted values |
|---|---|
| `checkbox` | `true`/`false`, or strings `yes`, `true`, `1`, `x`, `checked` (anything else is false) |
| `number` | Number, or a numeric string |
| `date` | `{"start": …}` object, or `2026-10-11`, `2026-10-11T15:30`, `October 11, 2026`; a range as `A → B` |
| `select`, `status` | Option name; a missing option is created with the next colour |
| `multi_select` | Array, or comma-separated string; missing options are created |
| `relation`, `file` | Array of strings; a single string is stored as is |
| `created_time`, `last_edited_time`, `formula`, `rollup` | Refused as read-only |
| any | `null` clears the value; an empty string does too, except on the read-only types, which refuse it |

A value for the title property sets the row's title. New select options are
saved to the schema in the same transaction as the row.

`query_database` takes `filter` as an array of conditions (all must match) or
`{"operator": "and"|"or", "conditions": [...]}`; a condition is
`{property, op, value}` with `op` from `FilterOp` (`is`, `is_not`, `contains`,
`does_not_contain`, `starts_with`, `ends_with`, `is_empty`, `is_not_empty`,
`greater_than`, `less_than`, `greater_than_or_equal_to`,
`less_than_or_equal_to`). Without `filter` and `sorts`, the first view's
query applies, so the model sees what the user sees.

## How the GUI picks up MCP writes

The server holds its own SQLite connection; WAL mode lets it commit while the
app reads. Writes that touch several rows — `create_page`, `update_page`,
`create_database`, `add_db_row`, `update_db_row` — run in one transaction, so
the app never shows half a page.

The app's file watcher sees the WAL change, waits 150 ms for the burst to
settle, finds `PRAGMA data_version` changed and reloads every view; commit
`b52f565` measured 160–190 ms from MCP write to visible change. If the app is
not running, nothing else is needed: the next launch reads the file. With sync
set up, the triggers log MCP writes like any other, and the app exports them
on its next sync run.

## Known gaps

- **No authentication.** Any process that can start the binary can read and
  write every note; the protection is the same as for the database file.
- **No block-level editing.** A tool can replace a page's whole content or
  append to it, not change one paragraph. `update_page` with `markdown`
  moves page-like blocks nested in the old blocks up to the page, puts the old
  non-page blocks under one new page and trashes it (`clear_content`), so the
  user restores them from the Trash. With `trash_replaced: false` the old
  blocks are deleted for good instead, so a client that rewrites a page often
  (a generated daily note) does not fill the Trash. Either way the kept
  sub-pages are moved after the new content in their old order. The new
  content loses
  what markdown cannot carry: underline, block and text colours, page and
  date mentions.
- **`create_page` and `create_database` can create a workspace by
  accident.** A `workspace` name that matches nothing creates a workspace with
  that name; `move_page` and
  `search` refuse unknown names instead.
- **No delete-forever of pages, no workspace management, no view editing.** Trash,
  views and workspaces are managed in the app.
- **Lists are capped.** `get_page` shows at most 100 database rows and
  `query_database` 100 unless `limit` is raised; both say how many were left
  out.
- **No change notifications.** A client cannot subscribe to edits made in the
  app; it re-reads.
- **Writes race with unsaved typing.** The app saves 300 ms after the last
  keystroke; an MCP write to the same block inside that window is
  overwritten, and a block the MCP server trashes while it has unsaved text
  in the app loses that text.
