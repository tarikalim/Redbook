# Redbook

A local-first, block-based notes app — pages, blocks and databases — written in Rust with [GPUI](https://www.gpui.rs/). Your notes live in one SQLite file on your computer. There is no account and no cloud service.

The name is Tolkien's *Red Book of Westmarch*: the book in which Bilbo and Frodo wrote down their story, kept at home and handed on.

The project's goal is to keep pages, blocks and databases at a very low memory cost. An idle Redbook window uses about **30 MB** (macOS `phys_footprint`, 1200×800 window on a Retina display). In a one-off check on the same Mac on 2026-10-11, Apple Notes idled at about 117 MB (`footprint`). [Memory and idle cost](docs/en/developer/04-memory-and-idle.md) shows how that was measured and what made it possible.

## What it shows

- **Pages**: nested pages in workspaces, with favorites, Trash and full-text search.
- **Editor**: blocks with the slash menu and Markdown shortcuts: to-dos, toggles, callouts, code, images, equations and simple tables.
- **Databases**: table, board, list, gallery and calendar views, with filters, sorts and calculations.

The window has a light and a dark theme; ⌘⇧L (Ctrl+Shift+L on Linux and Windows) switches them.

## Getting data in

- **Import**: `redbook import` reads a **Markdown & CSV** export from another notes app: pages, sub-pages, databases, images and files. [Importing a Markdown & CSV export](docs/en/user/05-import.md).
- **Sync**: your own devices share notes through a private git repository, in a line-per-block text format that git can merge. [Sync between devices](docs/en/user/06-sync-between-devices.md).

Redbook keeps no copy of the sync repository's address. Each sync runs `git` in the sync folder, so the folder's `origin` remote decides where the notes go. To move your notes to another repository:

1. Create an empty private repository.
2. Run `git -C <sync folder> remote set-url origin <new repository address>`.
3. Run `redbook sync now`. The full note history goes to the new repository.

Run the same `remote set-url` command on each of your other devices. Until you do, they keep syncing with the old repository.

### MCP

`redbook mcp` lets AI agents such as Claude Code work on your notes. It opens the same database as the app, so a change shows in an open window at once. Tool results are text.

| Tool | For |
|---|---|
| `list_workspaces` | The workspaces and their ids |
| `search` | Full-text search over titles, text, table cells and database values; one hit per page |
| `list_pages` | The page tree of a workspace or of one page |
| `get_page` | A page as markdown, with its header; a row with its values, a database with its schema, views and rows |
| `create_page`, `append_blocks`, `update_page` | Write pages as markdown; `update_page` can replace the content, keeping child pages |
| `move_page`, `trash_page`, `restore_page` | Reorganise the tree; the Trash keeps what is removed |
| `create_database`, `query_database`, `add_db_row`, `update_db_row` | Databases: schema, filtered and sorted rows, values by property name |

The `redbook` skill ([skills/redbook](skills/redbook/SKILL.md)) explains reading and writing pages and databases, and the markdown and values the tools take. It is served over the MCP Skills extension (`skills/list`, `skills/get`, `resources/read`).

## Install and run

### From a release

1. Download the build for your system from [Releases](../../releases): a universal macOS app (`.dmg` or `.zip`, Apple Silicon and Intel), a Linux x86_64 `.tar.gz`, or a Windows x86_64 `.zip`.
2. On macOS, open the `.dmg` and drag **Redbook** to **Applications**.
3. The macOS app is signed ad hoc, not notarized, and the Windows program is not signed, so macOS Gatekeeper and Windows SmartScreen warn on first launch. On macOS, either open **System Settings → Privacy & Security** and choose **Open Anyway** next to the Redbook message, or remove the download's quarantine flag once:

   ```sh
   xattr -dr com.apple.quarantine /Applications/Redbook.app
   ```

4. Optional, the command line and the MCP server for Claude Code (macOS):

   ```sh
   ln -sf /Applications/Redbook.app/Contents/MacOS/redbook ~/.local/bin/redbook
   claude mcp add --scope user redbook -- /Applications/Redbook.app/Contents/MacOS/redbook mcp
   ```

[Installing Redbook](docs/en/user/01-install.md) explains Linux and Windows step by step.

### From source

Rust 1.92 or newer, because the locked dependencies require it. On Linux, also install the system packages listed in [Build and release](docs/en/developer/09-build-and-release.md).

```sh
cargo run --release
scripts/bundle.sh          # macOS: builds release and installs /Applications/Redbook.app in place
```

Then step 4 above.

### Command line

The app, the CLI and the MCP server are one binary; `redbook` with no arguments opens the window.

| Command | What it does |
|---|---|
| `redbook` | Opens the window |
| `redbook import <export.zip>` | Imports a Markdown & CSV export |
| `redbook sync init <repo url>` | Sets up sync on the first device |
| `redbook sync clone <repo url>` | Sets up sync on another device |
| `redbook sync now` / `status` | Syncs immediately / shows sync state |
| `redbook mcp` | Runs the MCP server over stdio |

The database is in the app's data folder (`$REDBOOK_DB` overrides it); [Installing Redbook](docs/en/user/01-install.md) lists where on each system.

### Documentation

All documentation is in English, in [docs/en](docs/en/README.md):

- [User guide](docs/en/README.md#all-pages): install, pages, writing, databases, import, sync, MCP, troubleshooting.
- [Developer guide](docs/en/README.md#all-pages): architecture, storage, editor, memory work, vendored GPUI, sync, MCP, import, releases.

## Releases

Pushing a `v*` tag runs `.github/workflows/release.yml`: a universal macOS build (`.dmg` and `.zip`), a Linux `.tar.gz` and a Windows `.zip`, and a release with all of them.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Changes are listed in [CHANGELOG.md](CHANGELOG.md).

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT),
at your option. Unless you state otherwise, any contribution you submit for inclusion in Redbook
is licensed the same way, without any additional terms or conditions.

The vendored GPUI crates under `vendor/` keep their own Apache-2.0 licence (`vendor/*/LICENSE-APACHE`).
