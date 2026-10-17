# Installing Redbook

Redbook is a desktop notes app for macOS, Linux and Windows. It keeps all notes in one file on your computer and needs no account. Each release on the project's GitHub page has a download for each operating system, and two for macOS.

| Operating system | File | Requirement |
|---|---|---|
| macOS | `Redbook-<version>-macos-universal.dmg` or `…-macos-universal.zip` | macOS 12 or later, Apple silicon or Intel (tested only on a recent macOS) |
| Linux | `Redbook-<version>-linux-x86_64.tar.gz` | x86_64, glibc 2.35 or later (Ubuntu 22.04 or later), a Vulkan or OpenGL graphics driver |
| Windows | `Redbook-<version>-windows-x86_64.zip` | 64-bit Windows on x86_64 |

## Installing on macOS

The macOS app has only an ad-hoc signature, not an Apple Developer ID signature. macOS therefore blocks the first launch, and you allow it once.

1. Download the `.dmg` or the `.zip` file.
2. If you downloaded the `.dmg`, open it and drag **Redbook** into **Applications**.
3. If you downloaded the `.zip`, double-click it and move **Redbook.app** into **Applications**.
4. Open **Redbook** from **Applications**.
5. If macOS shows "Redbook" Not Opened, click **Done**.
6. Open **System Settings** and go to **Privacy & Security**.
7. In the **Security** section, click **Open Anyway** next to the message about Redbook.
8. Confirm with **Open Anyway**, and enter your password if macOS asks for it.

Redbook opens. macOS doesn't ask again for this copy of the app. On macOS 14 and earlier, you can also Control-click the app, choose **Open**, and then click **Open** in the dialog.

If macOS says the app "is damaged and can't be opened", see [The app "is damaged and can't be opened" on macOS](08-troubleshooting.md#the-app-is-damaged-and-cant-be-opened-on-macos).

## Installing on Linux

The Linux download is a folder with three files: the `redbook` program, a `redbook.desktop` launcher and a `redbook.png` icon. These steps install them for your user only, with no administrator rights.

1. Open a terminal in the folder that contains the download.
2. Extract the archive: `tar -xzf Redbook-<version>-linux-x86_64.tar.gz`
3. Install the program: `install -Dm755 Redbook-<version>/redbook ~/.local/bin/redbook`
4. Install the launcher: `install -Dm644 Redbook-<version>/redbook.desktop ~/.local/share/applications/redbook.desktop`
5. Install the icon: `install -Dm644 Redbook-<version>/redbook.png ~/.local/share/icons/hicolor/512x512/apps/redbook.png`
6. Open **Redbook** from your desktop's application menu.

The launcher starts the command `redbook`, so `~/.local/bin` must be on your session's `PATH`. If the menu entry does nothing, see [The Linux menu entry does nothing](08-troubleshooting.md#the-linux-menu-entry-does-nothing).

Redbook draws its window with Vulkan or OpenGL, whichever your graphics driver supports. On Debian and Ubuntu, the `libvulkan1` package supplies the Vulkan loader; your graphics driver supplies the rest.

## Installing on Windows

The Windows download is a zip file with one program, `redbook.exe`. There is no installer.

1. Extract the zip file to a folder that you keep, for example `%LOCALAPPDATA%\Programs\Redbook`.
2. Double-click `redbook.exe`.
3. If Windows shows **Windows protected your PC**, click **More info**.
4. Click **Run anyway**.
5. Optional: create a shortcut to `redbook.exe` on the desktop or in the Start menu.

Windows shows the SmartScreen warning because the program isn't signed. After you allow it once, it opens directly.

## What you see at first launch

Redbook opens on a new, empty page with the cursor in the title. You can type at once. If you leave this page without typing anything, Redbook deletes it, so empty launch pages don't collect in the sidebar. [Pages and workspaces](02-pages-and-workspaces.md#the-blank-page-at-launch) explains this behaviour.

On the very first launch, Redbook also creates a workspace called **Personal**. All your pages go into it until you add another workspace.

Redbook saves every change by itself, a fraction of a second after you stop typing. There is no **Save** button.

## Where Redbook keeps your data

Redbook keeps everything in one data folder per user. The notes themselves are one SQLite database file. The **Database** line in **Settings** shows the exact path on your computer.

| Operating system | Data folder |
|---|---|
| macOS | `~/Library/Application Support/RedBook` |
| Linux | `~/.local/share/RedBook`, or `$XDG_DATA_HOME/RedBook` when that variable is set |
| Windows | `%APPDATA%\RedBook` |

| Item in the data folder | What it holds |
|---|---|
| `redbook.db` | All workspaces, pages, blocks and databases |
| `redbook.db-wal`, `redbook.db-shm` | SQLite's working files; keep them with `redbook.db` |
| `redbook.settings.json` | The last workspace, the sidebar width and state, recent pages, the theme, and the sync folder |
| `files` | Pictures you add to pages, and files copied in by the import and by sync |
| `sync` | The default sync folder, present only after you set up sync |

If the environment variable `REDBOOK_DB` is set to a file path, Redbook uses that file as its database. The settings file and the `files` folder then sit next to that file.

To back up your notes, quit Redbook and copy the whole data folder.

## Running Redbook commands in a terminal

The import, sync setup and the AI agent server are commands of the same Redbook program. You run them in a terminal. The program's path depends on the operating system.

| Operating system | Program to run |
|---|---|
| macOS | `/Applications/Redbook.app/Contents/MacOS/redbook` |
| Linux | `redbook` (or `~/.local/bin/redbook`) |
| Windows | the full path to `redbook.exe`, for example `%LOCALAPPDATA%\Programs\Redbook\redbook.exe` |

The other pages write the program as `redbook`. Replace it with the path for your system. For example, on macOS, `redbook sync status` means `/Applications/Redbook.app/Contents/MacOS/redbook sync status`.

| Command | What it does | Page |
|---|---|---|
| `redbook import <export>` | Imports a Markdown & CSV export | [Importing a Markdown & CSV export](05-import.md) |
| `redbook sync …` | Sets up and runs sync between devices | [Sync between devices](06-sync-between-devices.md) |
| `redbook mcp` | Runs the server for AI agents | [AI agents (MCP)](07-ai-agents-mcp.md) |

## Updating Redbook

To update, install the new release over the old one. On macOS, replace **Redbook.app** in **Applications**. On Linux, repeat the install steps with the new archive. On Windows, replace `redbook.exe`. Your data folder stays where it is, and the new version opens the same notes.

## Uninstalling Redbook

Removing the program keeps your notes. Removing the data folder deletes them.

1. Quit Redbook.
2. On macOS, move **Redbook.app** from **Applications** to the Trash.
3. On Linux, delete `~/.local/bin/redbook`, `~/.local/share/applications/redbook.desktop` and `~/.local/share/icons/hicolor/512x512/apps/redbook.png`.
4. On Windows, delete `redbook.exe` and any shortcuts to it.
5. Optional: delete the data folder for your system. This step deletes all your notes on this computer.

If you used sync, the private sync repository keeps a copy of your notes. Delete that repository separately if you want it gone. If you chose a sync folder outside the data folder, delete it separately too.

## What the installation doesn't do

- Redbook doesn't update itself. You download and install each new release.
- The macOS app isn't notarized and the Windows program isn't signed. The warnings in the install steps are expected.
- There is no Linux build for ARM and no Windows build for ARM.
- The Linux download isn't a package. Your package manager doesn't know about it, and it doesn't install dependencies.
- Redbook doesn't encrypt the database file. Anyone with access to your user account can read it.

Next: [Pages and workspaces](02-pages-and-workspaces.md).
