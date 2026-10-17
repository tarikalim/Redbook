# Sync between devices

Sync keeps the same notes on several computers through a private Git repository, for example on GitHub. Each device keeps its full copy and works offline. When a device is online, Redbook sends its changes to the repository and takes in the changes of the other devices.

Sync is off until you set it up. Setup uses terminal commands; after that, Redbook syncs by itself while it runs.

## How sync works

Each device has a sync folder, which is a Git working copy of the repository. Redbook writes your changed pages into that folder and commits them. Then it pulls the commits of other devices and pushes its own commit to the repository. Redbook runs the `git` program for this, only while a sync is in progress.

All workspaces sync, with their pages, databases, Favorites and Trash. Image blocks and cover pictures travel with their pages. The sidebar width, recent pages and theme stay on each device.

## What you need before you start

- Git installed on every device. On macOS, `xcode-select --install` installs it. On Windows, use Git for Windows. On Linux, install the `git` package of your distribution.
- An empty private repository, for example a new private GitHub repository without a README.
- Access to that repository from every device without a password prompt. Use an SSH key, or HTTPS with a saved credential. Background syncs can't ask for a password.
- The Redbook program path for your system; see [Running Redbook commands in a terminal](01-install.md#running-redbook-commands-in-a-terminal).

To check access, run `git ls-remote <repository address>` in a terminal. It must finish without asking for anything.

## Setting up the first device

Do this on the device that has your notes now.

1. Optional: to use a sync folder other than the default, add `--dir <folder>` to the next command. The folder must be empty or not exist yet.
2. Run `redbook sync init <repository address>`, for example `redbook sync init git@github.com:you/notes.git`.
3. Wait for the message "Sync set up in" followed by the folder and the number of pages pushed.
4. Start Redbook. If Redbook already runs, it notices the setup by itself within a moment, or at the latest when you switch back to its window. You don't have to restart it.

In **Settings**, the **Sync** section now shows the sync folder and a status line. The first status is **On; not synced yet** until the first sync ends.

If the push fails, the command says "set up locally, but the push failed". Fix the access problem, then run `redbook sync now`.

## Setting up another device

Do this on each additional device, after the first device is set up.

1. Install Redbook. See [Installing Redbook](01-install.md).
2. Quit Redbook if it runs.
3. Run `redbook sync clone <repository address>`.
4. Wait for the message "Cloned", followed by the repository, the folder, and the number of pages imported.
5. Start Redbook. If Redbook already runs, it notices the setup by itself within a moment, or at the latest when you switch back to its window. You don't have to restart it.

The device now shows the same workspaces and pages as the first one.

`sync clone` refuses a device that already has pages. The message is: this database already has N pages; quit Redbook and use --force to merge them in. To keep those pages and add them to the shared notes, quit Redbook and run the command again with `--force`. Without `--force`, the only things replaced are an empty launch page and empty workspaces.

## When Redbook syncs

After setup, the app syncs in the background. A sync both sends and receives, unless the table says otherwise.

| Moment | What happens |
|---|---|
| Redbook starts | Full sync |
| About 10 seconds after your last change | Sends; receives only when the repository has newer changes |
| You come back to the Redbook window, at most once a minute | Full sync |
| You click **Sync now** in **Settings** | Full sync; Redbook shows **Syncing…** |
| You quit Redbook | Sends, for up to about 10 seconds |
| You run `redbook sync now` in a terminal | Full sync, and prints the counts |

Changes from other devices appear in the open window without a restart. When Redbook quits and the repository already has newer changes, it keeps your changes in the sync folder. It sends them, merged with the newer ones, at the next launch.

Changes that an AI agent or the import makes are synced the same way while Redbook runs.

## Conflicts and conflict copies

Redbook merges changes block by block. Two devices can edit different blocks of the same page, and both edits are kept.

A conflict happens when two devices change the same block in different ways between syncs. Redbook then keeps this device's version in the page. It adds the other device's version of the whole page as a new page next to it. The new page has the same title followed by **(conflict** device name and date**)**, for example "Meeting notes (conflict laptop 2026-10-11)".

To resolve a conflict:

1. Open the conflict page and the original page.
2. Copy what you need from the conflict page into the original.
3. Move the conflict page to the Trash.

A conflict copy of a database is a plain page with the database's title. An edit wins over a deletion: if one device deletes a page and another edits it, the page comes back. The device name is the computer's host name.

## Checking the sync status

The **Sync** section of **Settings** shows one status line. Redbook reads it when the dialog opens, so close and reopen **Settings** to see a newer state.

| Status line | Meaning |
|---|---|
| **Off. Set up with `redbook sync init <private repo url>`.** | Sync isn't set up on this device |
| **On; not synced yet** | Sync is set up, and no sync has finished yet |
| **Last sync** date and time, then counts | The last sync worked. The counts are pages exported, pages imported and conflicts |
| **Last try** date and time **failed:** error | The last sync failed; the error says why |

In a terminal, `redbook sync status` shows more detail. It lists the folder, the repository address, the device name and the changed blocks still to send. It also shows the last result, with "(not pushed)" when the commits haven't reached the repository yet.

[Troubleshooting](08-troubleshooting.md#sync-fails-or-never-finishes) lists the common sync errors.

## Changing the sync folder

The default sync folder is `sync` inside the Redbook data folder. You can move it.

1. Open **Settings**.
2. Click **Change folder…**.
3. Choose a folder and confirm with **Use as sync folder**.

Redbook shows a message with the result.

| Folder you choose | Result |
|---|---|
| An empty folder | Redbook moves the sync folder there: "Moved the sync folder to" the folder |
| An empty folder on another drive | Redbook clones the repository there: "Cloned into" the folder, and "the old folder … can be deleted" |
| A sync folder of the same repository | Redbook uses it: "Switched to" the folder "(same remote)" |
| A folder with other content | Refused with "Sync folder not changed": the folder "is not empty" or "holds a different repository" |

If a sync is in progress, the message is **A sync is running; try again in a moment**. The terminal command `redbook sync dir <folder>` does the same as **Change folder…**.

Before sync is set up, **Change folder…** only records the folder, which must be empty or not exist yet. `sync init` and `sync clone` then use it.

## Changing the sync repository

Redbook does not store the repository address. It uses the `origin` remote of the sync folder.

1. Create an empty private repository.
2. Run `git -C <sync folder> remote set-url origin <new repository address>`.
3. Run `redbook sync now`.

The full note history goes to the new repository. Do steps 2 and 3 on each of your other devices. Until then, those devices keep syncing with the old repository.

## What is in the sync repository

The repository holds your notes as text files, so you can read them on GitHub.

| Path in the repository | Content |
|---|---|
| `README.md` | A list of all workspaces and pages, with links |
| `read/` | One readable Markdown file per page |
| `pages/`, `collections/`, `workspaces.jsonl` | The data that Redbook syncs |
| `files/` | Images and files used on pages |

When a sync changes a page, Redbook writes `README.md` and that page's file in `read/` again and overwrites any change to them. Don't edit the repository by hand; make changes in Redbook.

## What sync doesn't do

- Sync isn't real time. Changes reach other devices after the next send and the next receive.
- There is no command to turn sync off or to disconnect a device.
- Sync doesn't encrypt your notes. The repository holds them as plain text, so keep it private.
- Sync can't ask for a password. A repository that needs a password prompt fails in the background.
- Sync works with one repository. You can't sync some workspaces only.
- Settings such as the theme, the sidebar width and recent pages don't sync.
- A permanent deletion on one device deletes the page on every device.
- A background sync stops after about 3 minutes, for example on a very slow network. It tries again at the next trigger.

Next: [AI agents (MCP)](07-ai-agents-mcp.md).
