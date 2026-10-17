# Redbook documentation

Redbook is a local-first, block-based notes app: pages, blocks and databases, stored in one SQLite file on your computer. It idles at about 30 MB of memory. It can import a Markdown & CSV export, sync between your own devices through a private git repository, and give AI agents access through a local MCP server.

This documentation has two stacks. Both describe the app as it runs today.

- The **user stack** explains how to install and use Redbook. Its pages follow the intent of ASD-STE100 Simplified Technical English (short sentences, one instruction per sentence), not its word list.
- The **developer stack** explains how Redbook is built and why it is built that way. Every page ends with its known gaps.

## Start here, by role

| You are | Read first |
|---|---|
| Someone who wants to take notes | [Installing Redbook](user/01-install.md), then [Pages and workspaces](user/02-pages-and-workspaces.md) |
| Moving from another notes app | [Importing a Markdown & CSV export](user/05-import.md) |
| Using Redbook on more than one computer | [Sync between devices](user/06-sync-between-devices.md) |
| Connecting Claude or another AI agent | [AI agents (MCP)](user/07-ai-agents-mcp.md) |
| A developer changing the code | [Architecture](developer/01-architecture.md), then [Memory and idle cost](developer/04-memory-and-idle.md) |
| A developer upgrading GPUI | [Vendored GPUI crates](developer/05-vendored-gpui.md) |
| Cutting a release | [Build and release](developer/09-build-and-release.md) |

## By task

| Task | Page |
|---|---|
| Install on macOS, Linux or Windows | [Installing Redbook](user/01-install.md) |
| Create, nest, move or delete pages | [Pages and workspaces](user/02-pages-and-workspaces.md) |
| Find a page | [Searching pages](user/02-pages-and-workspaces.md#searching-pages) |
| Export a page as Markdown | [Exporting a page as Markdown](user/02-pages-and-workspaces.md#exporting-a-page-as-markdown) |
| Add headings, lists, to-dos, tables, code | [Writing on a page](user/03-writing.md) |
| Learn the keyboard shortcuts | [Keyboard shortcuts for writing](user/03-writing.md#keyboard-shortcuts-for-writing), [for the window](user/02-pages-and-workspaces.md#keyboard-shortcuts-for-the-window) |
| Build a table, board or calendar | [Databases](user/04-databases.md) |
| Bring notes over from another app | [Importing a Markdown & CSV export](user/05-import.md) |
| Keep two computers in step | [Sync between devices](user/06-sync-between-devices.md) |
| Let an agent read and write notes | [AI agents (MCP)](user/07-ai-agents-mcp.md) |
| Measure memory or wakeups | [How to measure footprint on macOS](developer/04-memory-and-idle.md#how-to-measure-footprint-on-macos) |

## By screen

| Screen | Page |
|---|---|
| Sidebar, Trash, workspace switcher | [Pages and workspaces](user/02-pages-and-workspaces.md) |
| Page body, slash menu, block handle | [Writing on a page](user/03-writing.md) |
| Database views and row pages | [Databases](user/04-databases.md) |
| Settings → Sync | [Sync between devices](user/06-sync-between-devices.md) |

## By symptom

| Symptom | Page |
|---|---|
| macOS says the app is damaged | [Troubleshooting](user/08-troubleshooting.md#the-app-is-damaged-and-cant-be-opened-on-macos) |
| Sync fails or never finishes | [Troubleshooting](user/08-troubleshooting.md#sync-fails-or-never-finishes) |
| An import brought less than expected | [Troubleshooting](user/08-troubleshooting.md#importing-an-export) |
| An agent cannot see the notes | [Troubleshooting](user/08-troubleshooting.md#ai-agents) |
| Memory use went up after a change | [How to keep it from regressing](developer/04-memory-and-idle.md#how-to-keep-it-from-regressing) |

## All pages

User stack:

1. [Installing Redbook](user/01-install.md)
2. [Pages and workspaces](user/02-pages-and-workspaces.md)
3. [Writing on a page](user/03-writing.md)
4. [Databases](user/04-databases.md)
5. [Importing a Markdown & CSV export](user/05-import.md)
6. [Sync between devices](user/06-sync-between-devices.md)
7. [AI agents (MCP)](user/07-ai-agents-mcp.md)
8. [Troubleshooting](user/08-troubleshooting.md)

Developer stack:

1. [Architecture](developer/01-architecture.md)
2. [Data model and storage](developer/02-data-model-and-storage.md)
3. [UI and editor](developer/03-ui-and-editor.md)
4. [Memory and idle cost](developer/04-memory-and-idle.md)
5. [Vendored GPUI crates](developer/05-vendored-gpui.md)
6. [Sync](developer/06-sync.md)
7. [MCP server](developer/07-mcp-server.md)
8. [Markdown & CSV import](developer/08-import.md)
9. [Build and release](developer/09-build-and-release.md)

The documentation is in English only.
