# Markdown & CSV import

`redbook import` reads a "Markdown & CSV" export — the zip a notes app
produces, or the folder it unpacks to — and creates the same pages, subpages
and databases in a Redbook workspace. It is
[`src/import.rs`](../../../src/import.rs), a command-line tool that writes
through `Store`; it shares no state with a running app beyond the database
file.

```
redbook import <Export.zip|folder> [--workspace NAME]
```

`--workspace` takes a workspace name (ASCII case-insensitive) or id and creates it
when no workspace matches. Without it the import goes into the first
workspace, or a new "Personal" one in an empty database. The command prints a
summary: pages, databases, rows and files created, pages skipped as already
imported, and warnings.

## What an export looks like

The export names every item `Title <32-hex-id>`:

| File | Meaning |
|---|---|
| `Title <id>.md` | A page: `# Title`, then the body; database rows also start with `Key: value` lines |
| `Title <id>.csv` | A database, rows of its default view only |
| `Title <id>_all.csv` | The same database, every row; preferred when both exist |
| `Title <id>/` or `Title/` | The children of the page or database with that stem; older exports keep the id in the folder name, newer ones drop it |
| Other files in a page folder | Images and attachments the page links to |

Large exports come as a zip that contains further zips. `unzip` extracts the
outer archive, then every `.zip` inside it into the folder that contains it, and deletes
the inner archives. Entry names are read as UTF-8 even when the zip's UTF-8
flag is not set, because exporters write UTF-8 without always setting it, and
`.`/`..` path components are dropped. A UTF-8 byte-order mark at the start of
a file is stripped.

## How the export is walked

`scan(dir)` turns one folder into `Node`s — `.md` nodes in file-name order, then CSV-only nodes, then the nodes of unmatched folders: each `.md`
becomes a node, each `.csv` attaches to the node with the same stem (or
becomes a database-only node), and each folder attaches to the node whose
stem or title matches it. A folder that matches nothing is flattened into its
parent's list. The node's source id `sid` is the export id from the file
name, or the file path when the name has none.

The import runs in two passes inside **one transaction**:

1. **Create.** Every node becomes a block, recursively, and its export id is
   recorded against the new block id. A page gets an empty `page` block now;
   a database gets its block, schema, rows and row pages now.
2. **Fill.** Page bodies are parsed into blocks only after every node exists,
   so a link to a page later in the export resolves to its new id.

One transaction means a failed import leaves no blocks behind (files already
copied stay in `files/`; a database that fails part-way is kept partly, with a
warning), and a running
app sees the whole import appear in one reload.

## How page bodies are mapped

`split_page` takes a leading `# ` line (the first non-blank line) as the title (the file name's title is
the fallback), and for row pages the `Key: value` lines that follow it. The
rest is the body, parsed with `Store::markdown_to_blocks` (coverage in
[02-data-model-and-storage.md](02-data-model-and-storage.md#how-markdown-goes-in-and-out)).

The export writes a subpage as a line holding only a link to the subpage's file.
`Import::fill` treats such a line specially:

| A line that is only a markdown link, whose target id is … | Becomes |
|---|---|
| A child page or database of this page | The child block itself, moved to this position; text before it is flushed first |
| A `.csv` of a database that is not in the export (a linked view) | Dropped, with a warning |
| Anything else | An ordinary text line with a link |

Children that the body never links are appended at the end of the page. This
is what keeps a subpage where the author put it, instead of
collecting all subpages at the bottom.

Inside any line, link targets are rewritten:

| Target | Rewritten to |
|---|---|
| A file or web URL whose id was imported (`Page%20<id>.md`, `…_all.csv`, `https://…/Title-<id>?v=…`) | `redbook://page/<new id>` |
| `http(s)://`, `mailto:`, `#anchor` | Unchanged |
| A local file in the export | A copy in `files/<8 hex>-<sanitised name>` next to the database file (`<data dir>/RedBook/files/` by default), by absolute path |
| A local file that is missing | Unchanged, with a warning |

## How CSV databases are built

`Import::database` combines two sources for each database: the CSV, and the
row pages in the database's folder.

1. The CSV header's first column names the title property; the other columns
   become properties.
2. Each CSV record is matched to the first unused row page with the same
   title. The row page's `Key: value` lines fill columns the CSV lacks.
3. Row pages that matched no CSV record still become rows; their keys become
   extra columns, added in sorted order.
4. Each column's type is inferred from all its values (`infer`), every value
   is converted with `cell_value`, and select options are created as they are
   met, with colours rotating through the option palette.
5. A row with a page gets that page's body, subpages and files like any other
   page.

The CSV and the row pages are merged because neither is complete: the CSV has
every row but no bodies, and a row page has the body but the export omits some
properties from it.

Type inference checks the non-empty values of a column, in this order; the
first rule where every value fits wins:

| Rule | Type |
|---|---|
| No non-empty values | `text` |
| All `Yes` or `No` | `checkbox` |
| All numbers (thousands separators allowed) | `number` |
| All dates: `October 11, 2026`, `2026-10-11`, `2026/10/11`, optional time, optional `(GMT+3)` suffix, ranges with `→` | `date` |
| All `http://` or `https://` without spaces | `url` |
| All e-mail addresses | `email` |
| Any value over 60 characters or multi-line, or more than 30 distinct comma-separated items | `text` |
| Some value contains `, ` and all items are ≤ 30 characters | `multi_select` |
| Values repeat and there are at most 12 distinct ones | `select` |
| Otherwise | `text` |

Every database gets the default Table view that `Store::create_database`
creates; the source app's views are not in the export.

## How re-import stays idempotent

Every imported page and database stores its source id in `props.source_id`.
Before creating a node, `Import::create` calls `Store::find_by_source`; if a
block already carries that id — trashed ones included — the node is counted
as skipped and its id is reused for link rewriting. For a skipped page the
importer still walks its folder, so subpages added since the last
export are created under the existing page. A skipped database is skipped
whole.

Re-importing the same export therefore creates nothing, and importing a newer
export adds new pages and databases; new rows of a database that was already
imported are not added. A page the user trashed in Redbook is not
brought back.

## How the importer is tested

`imports_fixture_export` runs the importer on
[`tests/fixtures/markdown-export`](../../../tests/fixtures/markdown-export) into
an in-memory store and checks: the workspace and counts, the one linked-view
warning, subpages placed at their link lines, a Turkish title, page-link
rewriting, a callout icon, a copied image, inferred types (title, checkbox, number, date,
select, multi-select, e-mail), the CSV merged with row pages, and that a
second import creates nothing. `helpers` covers `target_id`, `prop_line`,
date parsing and type inference.

## Known gaps

- **Only the Markdown & CSV export.** The HTML export, the workspace-level
  JSON export and API access are not supported.
- **Updated content is not merged.** A page that was imported once is never
  updated by a later import, even when its text changed in the source app.
- **Row matching is by title.** Two rows with the same title can swap bodies
  when the CSV order differs from the file order.
- **What the export does not carry is lost.** Page icons and covers, block and
  text colours, comments, created and edited times (imported blocks get the
  import time), database views, and formulas, rollups and relations, which
  arrive as their displayed text and are usually inferred as `text`.
- **Time zones are dropped.** `October 11, 2026 3:45 PM (GMT+3)` becomes the
  local-less `2026-10-11T15:45`.
- **Inference has no override.** A column of numeric codes becomes `number`;
  change the type in the app afterwards.
- **One transaction holds everything.** A very large export is parsed and
  written in a single transaction; memory use during the import grows with
  the export and has not been measured.
