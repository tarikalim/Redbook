# AI agents (MCP)

Redbook includes a local MCP server, so an AI agent such as Claude Code can read and write your notes. The agent starts the server itself, on your computer. Nothing goes over the network, and the server has no password.

The server reads and writes the same database file as the app. A change that an agent makes appears in the open Redbook window within a moment.

## What an agent can do

An agent can find, read, create and change pages and databases. The server offers these tools.

| Tool | What the agent can do with it |
|---|---|
| `list_workspaces` | List the workspaces and their ids |
| `search` | Find pages by words in their titles, text, table cells and database values |
| `list_pages` | List the page tree of a workspace, or of one page |
| `get_page` | Read a page as Markdown, with its sub-pages; for a database, its properties, views and rows |
| `create_page` | Create a page with Markdown content, or add a row to a database |
| `update_page` | Change a page's title or icon, or replace its content |
| `append_blocks` | Add Markdown content at the end of a page |
| `move_page` | Move a page under another page, or to the top level |
| `trash_page` | Move a page to the Trash |
| `restore_page` | Bring a page back from the Trash |
| `create_database` | Create a full-page database with properties |
| `query_database` | Read database rows, with filters and sorts |
| `add_db_row` | Add a row with property values |
| `update_db_row` | Change property values of a row |

Agents write content as Markdown. The server understands headings, nested lists, to-dos, quotes, code blocks, tables, images, toggles and callouts. Links that an agent writes to other pages use the `redbook://page/` form, and they open those pages in Redbook.

## Registering Redbook in Claude Code

Prerequisites:

- Redbook is installed, and you know the program path for your system; see [Running Redbook commands in a terminal](01-install.md#running-redbook-commands-in-a-terminal).
- Claude Code is installed.

Procedure:

1. Open a terminal.
2. Run `claude mcp add --scope user redbook -- <program path> mcp`.
3. Run `claude mcp list` and check that `redbook` is in the list and connected.

On macOS, step 2 is `claude mcp add --scope user redbook -- /Applications/Redbook.app/Contents/MacOS/redbook mcp`. Use the full path to the program on every system.

`--scope user` makes Redbook available in all your projects. Leave it out to add Redbook to the current project only.

In a Claude Code session, `/mcp` shows the server and its tools. Ask the agent, for example, to "search my Redbook notes for the meeting with Ana" or to "add a page called Reading list under Personal".

## Registering Redbook in other agents

Any MCP client that starts a local server over standard input and output can use Redbook. Configure it with two values.

| Setting | Value |
|---|---|
| Command | The full Redbook program path |
| Arguments | `mcp` |

The client starts the program when it needs it and stops it at the end of the session. The Redbook app doesn't have to run.

## Seeing an agent's changes

Changes by an agent are written to the database at once. If Redbook is open, the sidebar and the open page reload by themselves within a moment. If sync is on and Redbook runs, the changes are synced like your own; see [When Redbook syncs](06-sync-between-devices.md#when-redbook-syncs).

An agent's changes have no special marker. Check the **Edited** time in the top bar, or the Trash, to see what changed.

## Things to know before you let an agent write

- `update_page` with new content replaces all the page's blocks. The old blocks go to the Trash as one page named "<page title> (replaced content)". Restore that page to get them back. Sub-pages and databases stay in the page, also the ones inside a toggle or another block. They move below the new content and keep their order.
- An agent can ask `update_page` not to keep the old blocks (`trash_replaced` set to false). Then the old blocks are deleted and do not go to the Trash. Tools that rewrite a page often, such as a daily note, use this.
- `trash_page` moves a page to the Trash. You can restore it in the app.
- No tool deletes a page permanently. No tool empties the Trash. Only the old blocks of `update_page` with `trash_replaced` set to false are deleted.
- An agent can't change a page that has **Lock page** on. `update_page`, `append_blocks`, `update_db_row` and `trash_page` stop with an error. Turn off **Lock page** to let the agent change the page.
- Search and page lists cover every workspace. A new top-level page goes into the first workspace unless the agent names another one.
- The agent sees all your notes. Register Redbook only with agents that you trust with them.

## What the MCP server doesn't do

- It works only for agents on the same computer. It doesn't listen on a network port.
- It has no access control. Every tool can read or change every page.
- It can't change database views, delete properties or change a property's type. It can create new select options while it sets values.
- It doesn't change page settings such as **Full width**, **Small text**, **Lock page** or Favorites.
- It doesn't run sync. Redbook sends the agent's changes when the app runs.

Next: [Troubleshooting](08-troubleshooting.md).
