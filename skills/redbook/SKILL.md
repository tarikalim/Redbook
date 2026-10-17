---
name: redbook
description: Read, search and write the local Redbook notes (pages, blocks and databases in one SQLite file). Use it to find a note, read a page, write or update pages as markdown, or keep a database (a task list, a log, a reading list) current.
---

# Redbook

Redbook keeps notes on this computer: **workspaces** hold a tree of **pages**; a page holds
**blocks** (text, headings, lists, to-dos, toggles, callouts, code, tables, images) and can hold
child pages and **databases**. A database is a page with a schema; each **row** is a page too, with
property values and its own content. Everything lives in one SQLite file; the app and this server
open the same file, so a change shows in an open window at once.

## Reading

1. Find the page: `search` (full-text prefix search over titles, text, table cells and database
   values; one hit per page) or `list_pages` (the page tree of a workspace or of one subtree).
   `list_workspaces` names the workspaces; tools take a workspace by name or id and default to the
   first one.
2. `get_page` reads it as markdown with a header: id, title, type, parent, breadcrumb, child pages.
   A row comes with its property values; a database with its schema, views and rows.
3. `query_database` returns rows as a markdown table with their ids, filtered and sorted.

Links to other pages look like `redbook://page/<id>`; follow them with `get_page`.

## Writing

- `create_page` under a page, at a workspace root, or under a database to add a row page.
  Content is markdown: [references/markdown.md](references/markdown.md).
- `append_blocks` adds markdown to the end of a page.
- `update_page` changes the title or icon and can replace the whole content. Child pages and
  databases are kept and placed after the new content; the replaced blocks go to the trash as one
  restorable page unless `trash_replaced` is false. Locked pages are refused.
- `move_page`, `trash_page` (restorable) and `restore_page` reorganise the tree.
- `create_database` makes a full-page database with a Table view; `add_db_row` and `update_db_row`
  set values by property name. Value formats: [references/markdown.md](references/markdown.md#database-values).

## Rules of thumb

- Read before you write: `get_page` first, then `append_blocks` for additions, and `update_page`
  with markdown only when the whole page should change.
- Keep page titles short and put the detail in the body; a title is what the sidebar and search show.
- Use a database for anything with repeated fields (tasks, logs, lists) and one row per item, rather
  than a long page of bullets.
- Prefer `trash_page` to anything that cannot be undone; the user can restore from the Trash.
- When a tool fails, its message says what to change; fix that and call again.
