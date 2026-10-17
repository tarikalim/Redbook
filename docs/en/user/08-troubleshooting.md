# Troubleshooting

Each section names a symptom that you can see, its likely cause and the fix. Commands are written as `redbook`; replace it with the program path for your system, as [Running Redbook commands in a terminal](01-install.md#running-redbook-commands-in-a-terminal) shows.

## Installing and starting

| Symptom | Likely cause | Fix |
|---|---|---|
| macOS says "Redbook" Not Opened | The app isn't notarized by Apple | Allow it once in **System Settings > Privacy & Security** with **Open Anyway**; see [Installing on macOS](01-install.md#installing-on-macos) |
| macOS says the app "is damaged and can't be opened" | macOS quarantined the download | See [The app "is damaged and can't be opened" on macOS](#the-app-is-damaged-and-cant-be-opened-on-macos) |
| Windows shows **Windows protected your PC** | The program isn't signed | Click **More info**, then **Run anyway** |
| The Linux menu entry does nothing | The session can't find the `redbook` command | See [The Linux menu entry does nothing](#the-linux-menu-entry-does-nothing) |
| On Linux, the program stops at once with a graphics or Vulkan error | No usable Vulkan or OpenGL driver is installed | Install the Vulkan loader (`libvulkan1` on Debian and Ubuntu) and your graphics driver's Vulkan support |
| On Linux, the program reports that `GLIBC_2.35` is not found | The distribution is older than the build | Use Ubuntu 22.04 or later, or another distribution with glibc 2.35 or later |
| A new empty page appears each time Redbook starts | This is the launch page | Start typing, or open another page; an untouched launch page is deleted. See [The blank page at launch](02-pages-and-workspaces.md#the-blank-page-at-launch) |

## The app "is damaged and can't be opened" on macOS

macOS sometimes shows this message for apps that a browser downloaded and that Apple didn't notarize. The app isn't damaged. Remove the download mark that macOS put on it.

1. Move **Redbook.app** into **Applications**.
2. Open **Terminal**.
3. Run `xattr -dr com.apple.quarantine /Applications/Redbook.app`.
4. Open Redbook again.

## The Linux menu entry does nothing

The launcher starts the command `redbook`. Some desktop sessions don't include `~/.local/bin` in their search path.

1. Open `~/.local/share/applications/redbook.desktop` in a text editor.
2. Change the line `Exec=redbook` to the full path, for example `Exec=/home/<you>/.local/bin/redbook`.
3. Save the file.

If the menu shows no icon, check that `redbook.png` is in `~/.local/share/icons/hicolor/512x512/apps`. Some desktops show the change only after you log out and in again.

## Writing and pages

| Symptom | Likely cause | Fix |
|---|---|---|
| A page doesn't react to typing | The page is locked; the top bar shows **🔒 Locked** | Open the **…** page menu and click **Lock page** to unlock |
| A page was empty after a restart and is gone | It was the launch page and had no title or text | Nothing was lost. Redbook deletes only untouched launch pages |
| A Markdown shortcut such as `# ` does nothing | The block is a code block, a callout or an equation, or there is text before the cursor | Use the slash menu, or type the shortcut at the start of a text block |
| **Cmd+K** opens the search instead of adding a link | No text was selected | Select the text first, then press **Cmd+K** (**Ctrl+K** on Linux and Windows) |
| A click on a link only puts the cursor in it | Links open with a modifier key | Hold **Cmd** on macOS, or the **Super**/**Windows** key on Linux and Windows, and click |
| An image shows nothing | The image was added before Redbook copied pictures, and the picture file moved, was renamed or was deleted | Put the file back, or add the image again; see [Adding images](03-writing.md#adding-images) |
| A deleted block isn't in the Trash | The Trash holds pages and databases, not other blocks | A deleted block can't be restored in the app. A sub-page or database inside it is gone with it |
| Search finds nothing for a word on a page | The page is in another workspace or in the Trash | Switch to that workspace, or restore the page |
| A database row can't be deleted from the table | Tables have no delete command | Open the row and choose **Move to Trash** in the **…** page menu |

## Importing an export

| Symptom | Likely cause | Fix |
|---|---|---|
| The import prints `usage: redbook import <Export.zip\|folder> [--workspace NAME]` | The path is missing, or there is an extra word | Give one path to the zip file or folder, and put names with spaces in quotes |
| A warning says a database "is not in the export (linked view?), skipped" | The page held a linked view of a database stored elsewhere | Export the page that holds the original database too, and import that export |
| A warning names an image or file "not found in export" | The export didn't include it | Export again with **Include content** set to **Everything** |
| The summary says pages were "already imported (skipped)" | Those pages came in with an earlier import | This is expected. To import a page again, delete its earlier copy permanently, including from the Trash |
| Pages changed in the other app didn't update | The import never changes existing pages | Delete the old pages permanently, then import a new export |
| On Windows, the summary appears after the prompt returns | `redbook.exe` is a window program that writes to the terminal later | Wait a moment, then press **Enter** to get a clean prompt |

## Sync fails or never finishes

The **Sync** line in **Settings** shows the last error after **Last try … failed:**. `redbook sync status` shows it too.

| Error or symptom | Likely cause | Fix |
|---|---|---|
| `cannot run git; is it installed?` | Git isn't installed or isn't on the program's path | Install Git; on macOS, run `xcode-select --install` |
| An error from `git fetch` or `git push` about authentication, "Permission denied" or "could not read Username" | The device can't reach the repository without a prompt | Set up an SSH key or a saved HTTPS credential, then check with `git ls-remote <repository address>` |
| `git fetch timed out` or `git push timed out` | The network is slow or blocked | Check the connection, then click **Sync now** |
| `another sync is running` | A sync from the app or a terminal is still in progress | Wait, then try again. A lock older than 5 minutes is cleared by itself |
| `sync folder … is missing; run redbook sync clone` | Sync was never set up on this device, or the sync folder was deleted or moved outside Redbook | Quit Redbook and run `redbook sync clone <repository address> --force`. **Change folder…** can't reattach a moved or deleted folder |
| `sync is not set up for this database` | The sync folder exists, but this database was never set up, for example because the database file was replaced | Quit Redbook and run `redbook sync clone <repository address> --dir <empty folder> --force`. Without `--dir`, the command refuses because the old sync folder is still there |
| `push rejected twice; try again` | Another device pushed during this sync | Click **Sync now** again |
| "remote has new changes; they are merged at next launch" | Redbook quit while the repository had newer changes | Nothing to do. Your changes are kept and sent at the next launch |
| `sync init` says the folder "is already a sync folder" or "is not empty" | The target folder has content | Choose an empty folder with `--dir`, or empty the folder |
| `sync clone` says "this database already has N pages" | The device has its own pages | Quit Redbook and run `sync clone` again with `--force` to merge them |
| `sync init` says "set up locally, but the push failed" | The repository refused the first push | Make sure the repository is empty and reachable, then run `redbook sync now` |
| **Settings** shows **Off** after `sync init` or `sync clone` | Redbook was running during setup | Quit Redbook and start it again |
| The status line doesn't change | **Settings** reads the status only when it opens | Close and reopen **Settings** |
| A page called "… (conflict …)" appeared | Two devices changed the same block | See [Conflicts and conflict copies](06-sync-between-devices.md#conflicts-and-conflict-copies) |

## AI agents

| Symptom | Likely cause | Fix |
|---|---|---|
| `claude mcp list` shows `redbook` as failed | The program path is wrong or relative | Register it again with the full path; see [Registering Redbook in Claude Code](07-ai-agents-mcp.md#registering-redbook-in-claude-code) |
| The agent's changes don't show in the window | The window missed the update | Switch to another page and back. While the window is active, Redbook also checks every 30 seconds |
| The agent replaced a page's content by mistake | `update_page` deletes the old blocks permanently | Restore from another synced device or from the sync repository's history, if you have them |
| The agent created pages in the wrong workspace | It didn't name a workspace, so it used the first one | Ask the agent to move them with `move_page` and the right workspace name |

## Getting your data out

If Redbook doesn't start at all, your notes are still in the data folder; see [Where Redbook keeps your data](01-install.md#where-redbook-keeps-your-data). Copy the whole folder before you try a fix. If you use sync, the `read/` folder of the sync repository has a Markdown copy of every page.

## What this page doesn't cover

- Problems inside the export itself. Check the zip file in the other app's help first.
- Git hosting problems, such as a full disk quota or a deleted repository. Check them on the hosting site.
- Recovery of permanently deleted pages. Redbook has no way back once a page leaves the Trash, except a backup or the sync repository's history.
