# Pages and workspaces

A workspace is a separate set of pages. A page holds blocks of text and can hold other pages. The sidebar shows the pages of the current workspace as a tree.

Keyboard shortcuts on these pages use **Cmd** on macOS and **Ctrl** on Linux and Windows. The sidebar, tooltips and the **Settings** dialog show the macOS symbols (⌘) on every system.

## The window

The window has two parts: the sidebar and the page area.

| Area | What it holds |
|---|---|
| Top of the sidebar | The workspace switcher, then **Search**, **Settings** and **New page** |
| Middle of the sidebar | **Favorites** (when you have any) and **Private**, the page tree |
| Bottom of the sidebar | **Trash** |
| Top bar of the page area | Sidebar toggle, back and forward buttons, the breadcrumb path, **Edited …** time, the star, and the **…** page menu |
| Page area | The open page or database |

To hide or show the sidebar, press **Cmd+\\** or click the sidebar button in the top bar. To change its width, drag its right edge. Redbook remembers both for the next launch.

On macOS, the menu bar has the same commands: **File > New Page**, **File > Search**, **View > Toggle Sidebar**, **View > Toggle Theme**, **Go > Back** and **Go > Forward**.

## The blank page at launch

Each time Redbook starts, it opens a new, empty page with the cursor in the title. You can start typing without any click.

The launch page becomes a normal page as soon as you type a title or any text. If you open another page or quit while the launch page is still empty, Redbook deletes it permanently. An empty launch page left behind by a crash is deleted at the next start.

## Creating a page

- To create a page at the top level, press **Cmd+N** or click **New page** in the sidebar.
- To create a page at the top level from the tree, point to **Private** and click **+** (**Add a page**).
- To create a sub-page, point to a page in the sidebar and click **+** (**Add a page inside**).
- To create a sub-page from inside a page, type `/page` and press **Enter**. [Writing on a page](03-writing.md#adding-blocks-with-the-slash-menu) describes the slash menu.

Redbook opens the new page. A page with no title shows as **Untitled**.

## Giving a page an icon and a cover

Point to the area above the page title to show **Add icon** and **Add cover**.

- **Add icon** opens an emoji list. Click an emoji to use it. To change it later, click the icon. To remove it, click the icon and then **Remove**.
- **Add cover** adds a colour gradient at the top of the page. Point to the cover and click **Change cover** to pick another gradient, **Choose image…** to use a picture file, or **Remove**. Redbook copies the picture file into its data folder, so the cover stays when you move or delete the original.

## Using the sidebar tree

The sidebar shows top-level pages under **Private**. Click a page to open it. Click the arrow before a page to show or hide its sub-pages. A page without sub-pages shows **No pages inside** when you open its arrow.

Databases show in the tree with a grid icon, or with their emoji. Their rows don't show in the sidebar; you see them in the database itself.

## Moving pages by drag and drop

Drag a page in the sidebar to put it somewhere else. Where you drop it on another page decides the result.

| Drop position on the target row | Result |
|---|---|
| Top quarter (a line shows above the row) | The page moves before the target, at the same level |
| Bottom quarter (a line shows below the row) | The page moves after the target, at the same level |
| Middle (the row is highlighted) | The page moves inside the target, as its last sub-page |
| On the **Private** heading | The page moves to the top level, at the end |

You can't drop a page inside a database. Pages in **Favorites** can't be dragged.

## Moving a page with Move to

**Move to** moves a page without drag and drop.

1. Open the **…** menu of the page, in the sidebar or in the top bar.
2. Click **Move to**.
3. Type part of the target page's title.
4. Select the target and press **Enter**, or click it.

The list doesn't offer the page itself, its own sub-pages, or databases. Choose **Private** to move the page to the top level of the workspace.

## Using the page menu

The **…** button in the top bar opens the page menu. The **…** button on a sidebar row opens the same menu without the first three items.

| Menu item | What it does |
|---|---|
| **Small text** | Shows the page in smaller text. Click again to turn it off |
| **Full width** | Lets the page use the whole window width instead of a centred column |
| **Lock page** | Stops edits on the page. The top bar shows **🔒 Locked**. Click again to unlock |
| **Add to Favorites** / **Remove from Favorites** | Adds or removes the page in **Favorites** |
| **Duplicate** | Copies the page with its sub-pages and databases, places the copy after it, and opens the copy |
| **Rename** | Opens a dialog to change the title. Click **Save** or press **Enter** |
| **Move to** | Moves the page; see [Moving a page with Move to](#moving-a-page-with-move-to) |
| **Export Markdown** | Saves the page as a Markdown file |
| **Move to Trash** | Moves the page and everything inside it to the Trash |

A locked page can't be typed in. Its blocks show no handles, its to-do boxes can't be ticked, and its title is read-only. AI agents connected over MCP can't change it either. Unlock it to edit again.

## Exporting a page as Markdown

**Export Markdown** writes one page to a `.md` file.

1. Open the **…** page menu.
2. Click **Export Markdown**.
3. Choose a folder and a file name. The dialog starts in your Downloads folder with the page title as the name.
4. Click **Save**.

Redbook shows "Exported to" followed by the file path. The file has the page title as a heading, then the page content. A database exports as a Markdown table.

Sub-pages aren't exported with their content. They appear as links that start with `redbook://page/`, and only Redbook understands them. Images keep the path of the picture file on your computer.

## Using Favorites

Favorites give quick access to pages you use often. To add a page, click the star in the top bar or choose **Add to Favorites** in the page menu. **Favorites** appears at the top of the sidebar, with the most recently edited page first. To remove a page, click the star again.

## Deleting and restoring pages with the Trash

**Move to Trash** removes a page and everything inside it from the sidebar. Nothing is deleted yet; the Trash keeps it until you empty it.

To open the Trash, click **Trash** at the bottom of the sidebar. The Trash shows the trashed pages of the current workspace, newest first.

| Action in the Trash | Result |
|---|---|
| Click a page | Opens the page with a red bar that says **This page is in Trash.** The page's content shows again only after you restore it |
| Click the restore button (**Restore**) | Puts the page back where it was, with its sub-pages |
| Click the delete button (**Delete permanently**) | Asks **Delete permanently?**, then deletes the page and everything inside it |
| Click **Empty trash** | Asks **Empty trash?**, then deletes every page in the Trash of this workspace |

The red bar on a trashed page has the same choices: **Restore page** and **Delete from Trash**.

If the page's parent is still in the Trash, a restored page goes to the top level instead.

Permanent deletion can't be undone. The Trash never empties by itself.

## Searching pages

The search palette finds pages by the words in their titles and text, in the cells of simple tables, and in database property values. A match in a property value finds the row.

1. Press **Cmd+P** or **Cmd+K**, or click **Search** in the sidebar.
2. Type one or more words.
3. Use **Up** and **Down** to select a result.
4. Press **Enter**, or click the result, to open the page.

With an empty search field, the palette lists your recently opened pages, marked **Recent**. Each typed word matches the start of a word, so `plan` finds "planning". A result shows the page once, with a short extract of the matching text. If nothing matches, the palette shows **No results**.

Search looks only in the current workspace and ignores pages in the Trash. **Cmd+K** opens the search only when no text is selected on the page; with selected text, it adds a link instead.

## Going back and forward

Redbook remembers the pages you open, like a web browser.

- To go back, press **Cmd+[** or click the back arrow in the top bar.
- To go forward, press **Cmd+]** or click the forward arrow.
- To jump to a parent page, click its name in the breadcrumb path in the top bar.

The history starts empty at each launch. Pages deleted permanently in the meantime are skipped. A page in the Trash opens with the Trash bar.

## Working with workspaces

Each workspace has its own page tree, Favorites and Trash. The workspace switcher at the top of the sidebar shows the current one.

1. Click the workspace switcher.
2. Choose a workspace from the list to switch to it.

The same menu has these items:

| Menu item | What it does |
|---|---|
| **New workspace** | Asks for a name and opens the new workspace. An empty name gives **Untitled workspace** |
| **Rename workspace** | Changes the name of the current workspace |
| **Change icon** | Picks an emoji for the current workspace, or **Remove icon** |
| **Delete workspace** | Deletes the current workspace and all its pages permanently, after a confirmation |

A deleted workspace doesn't go to the Trash. If you delete the last workspace, Redbook creates a new empty **Personal** workspace.

## Changing the theme

Redbook follows the light or dark mode of your system. To change it, open **Settings** and click **System**, **Light** or **Dark** next to **Appearance**. **Cmd+Shift+L** switches between light and dark.

Redbook saves the choice and uses it at the next launch. With **Light** or **Dark**, Redbook keeps that theme when the system mode changes. With **System**, it follows the system mode again.

## Keyboard shortcuts for the window

| Action | macOS | Linux and Windows |
|---|---|---|
| New page | Cmd+N | Ctrl+N |
| Search | Cmd+P or Cmd+K | Ctrl+P or Ctrl+K |
| Show or hide the sidebar | Cmd+\\ | Ctrl+\\ |
| Back | Cmd+[ | Ctrl+[ |
| Forward | Cmd+] | Ctrl+] |
| Switch light and dark | Cmd+Shift+L | Ctrl+Shift+L |
| Quit | Cmd+Q | Ctrl+Q |

The shortcuts for writing are in [Writing on a page](03-writing.md#keyboard-shortcuts-for-writing).

## What pages and workspaces don't do

- There is no undo for page operations. A permanent delete is final.
- Search doesn't look across workspaces, and it doesn't search inside the Trash.
- **Move to** and drag and drop work only inside the current workspace. The app can't move a page to another workspace; an AI agent can, with `move_page`.
- Redbook has one window. Closing it quits the app.
- Export works on one page at a time. There is no export of a whole workspace from the app. Sync keeps a readable Markdown copy of every page in the sync repository; see [Sync between devices](06-sync-between-devices.md#what-is-in-the-sync-repository).

Next: [Writing on a page](03-writing.md).
