# Writing on a page

A page is a list of blocks. Each paragraph, heading, list item, image or table is one block. You edit one block at a time, and Redbook saves each change by itself.

Keyboard shortcuts on this page use **Cmd** on macOS and **Ctrl** on Linux and Windows. **Option** on macOS is **Alt** on Linux and Windows.

## Typing and moving between blocks

Click a block to type in it. Click the empty area below the last block to add a new text block at the end.

| Key | Result |
|---|---|
| **Enter** | Splits the block at the cursor. The new block is a text block, or the same kind for to-do, bulleted, numbered and toggle lists |
| **Enter** on an empty list item | Turns the item into a text block, which ends the list |
| **Enter** at the start of a block with text | Adds an empty block before it |
| **Enter** in an open toggle | Adds a block inside the toggle |
| **Shift+Enter** | Adds a line break inside the block |
| **Backspace** at the start of a heading, list item, quote or other non-text block | Turns the block into a text block (not a code block that has text) |
| **Backspace** at the start of a text block | Joins the block to the one before it |
| **Backspace** at the start of a text block after a divider | Deletes the divider |
| **Tab** | Indents the block under the block before it |
| **Shift+Tab** | Moves an indented block one level out |
| **Up** or **Down** on the first or last line | Moves to the block before or after |
| **Left** or **Right** at the start or end | Moves to the block before or after |
| **Escape** | Stops editing the block |

In the page title, **Enter** or **Down** moves to the first block.

**Backspace** doesn't join a text block into an image, page, table or database before it. If the text block is empty, **Backspace** deletes it.

**Tab** indents only under a text block, a list item, a toggle, a quote or a callout. In code and equation blocks, **Enter** adds a line break instead of a new block.

## Adding blocks with the slash menu

Type `/` in a block to open the block menu (not in a code block). Type more letters to filter it, for example `/h2` or `/todo`.

1. Type `/` in a block.
2. Optional: type part of the block name.
3. Select an item with **Up** and **Down**.
4. Press **Enter**, or click the item.

To close the menu, press **Escape**. The menu also closes when you type a space after a filter that matches nothing.

If the block is empty, the new block replaces it. Otherwise the new block goes after the current one.

| Slash menu item | Block |
|---|---|
| **Text** | A plain paragraph |
| **Page** | A sub-page. Redbook creates it and opens it |
| **To-do list** | A line with a tick box |
| **Heading 1**, **Heading 2**, **Heading 3** | Headings in three sizes |
| **Bulleted list** | A list item with a bullet |
| **Numbered list** | A list item with a number |
| **Toggle list** | A line that opens and closes to show the blocks inside it |
| **Quote** | Text with a line at its left |
| **Callout** | Text in a shaded box with a 💡 icon |
| **Divider** | A horizontal line |
| **Code** | Monospaced text with a language label |
| **Block equation** | Monospaced text for a formula |
| **Image** | A picture from a file on your computer |
| **Table** | A simple grid of cells |
| **Database - Inline** | A database inside the page |
| **Database - Full page** | A database as its own sub-page |

[Databases](04-databases.md) describes the two database items.

## Using Markdown shortcuts

Type these characters at the start of an empty block, or at the start of its text. The block changes as soon as you type the final space or character.

| Type at the start | Block |
|---|---|
| `# ` | Heading 1 |
| `## ` | Heading 2 |
| `### ` | Heading 3 |
| `#### ` | Heading 4 (no slash menu item) |
| `[] ` or `[ ] ` | To-do |
| `[x] ` | To-do, already ticked |
| `- `, `* ` or `+ ` | Bulleted list |
| `1. ` or `1) ` | Numbered list |
| `> ` | Toggle list |
| `" ` or `\| ` | Quote |
| ```` ``` ```` | Code |
| `---` | Divider |

A numbered list started with another number, for example `5. `, counts on from that number. Nested numbered lists use letters at the second level and roman numerals at the third.

`> ` makes a toggle, not a quote. The shortcuts don't work inside code blocks, callouts or equations.

## Formatting text

Select text in a block, then press a shortcut. Press it again to remove the format.

| Format | macOS | Linux and Windows | Markdown while typing |
|---|---|---|---|
| Bold | Cmd+B | Ctrl+B | `**text**` or `__text__` |
| Italic | Cmd+I | Ctrl+I | `*text*` or `_text_` |
| Underline | Cmd+U | Ctrl+U | none |
| Strikethrough | Cmd+Shift+S or Cmd+Shift+X | Ctrl+Shift+S or Ctrl+Shift+X | `~text~` |
| Inline code | Cmd+E | Ctrl+E | `` `text` `` |

The Markdown forms apply when you type the closing character. The opening character must follow a space or start the block.

## Adding and opening links

A link can point to a web page or to another Redbook page.

1. Select the text for the link.
2. Press **Cmd+K**.
3. Paste the address.
4. Press **Enter**.

To remove a link, select the text, press **Cmd+K**, clear the field and press **Enter**. To cancel, press **Escape**.

To open a link, hold the modifier key and click the link, in the text or in a table cell. On macOS, the key is **Cmd**. On Linux and Windows, the key is the **Super** or **Windows** key, not **Ctrl**. A plain click puts the cursor in the text.

## Linking to other Redbook pages

A link whose address starts with `redbook://page/` opens that Redbook page instead of a browser. Links of this kind come from the import and from AI agents.

Redbook has no command that copies the link of a page. To point to a sub-page in the page text, use a **Page** block. A **Page** block shows the sub-page's icon and title, and a click opens it.

## Changing a block's type with Turn into

**Turn into** changes a text-type block to another kind and keeps its text.

1. Point to the block and click its handle (**⋮⋮**).
2. In the **Turn into** section, click the new kind.

While you edit a block, a shortcut does the same.

| Turn into | macOS | Linux and Windows |
|---|---|---|
| Text | Cmd+Option+0 | Ctrl+Alt+0 |
| Heading 1 | Cmd+Option+1 | Ctrl+Alt+1 |
| Heading 2 | Cmd+Option+2 | Ctrl+Alt+2 |
| Heading 3 | Cmd+Option+3 | Ctrl+Alt+3 |
| To-do list | Cmd+Option+4 | Ctrl+Alt+4 |
| Bulleted list | Cmd+Option+5 | Ctrl+Alt+5 |
| Numbered list | Cmd+Option+6 | Ctrl+Alt+6 |
| Toggle list | Cmd+Option+7 | Ctrl+Alt+7 |
| Code | Cmd+Option+8 | Ctrl+Alt+8 |
| Page | Cmd+Option+9 | Ctrl+Alt+9 |

**Turn into** also offers **Quote** and **Callout**, which have no shortcut.

## Using the block handle

Point to a block to show two buttons at its left.

- **+** adds an empty block after this one and opens the slash menu in it.
- **⋮⋮** is the block handle. Click it to open the block menu, or drag it to move the block.

| Block menu section | Items |
|---|---|
| Top | **Delete**, **Duplicate** |
| **Turn into** | The block kinds in [Changing a block's type with Turn into](#changing-a-blocks-type-with-turn-into); text-type blocks only |
| **Color** | Text colour: **Default**, **Gray**, **Brown**, **Orange**, **Yellow**, **Green**, **Blue**, **Purple**, **Pink**, **Red** |
| **Background** | The same colours, as a background |

**Cmd+D** duplicates the block that you are editing. **Delete** in the block menu removes the block from the page.

## Moving blocks by drag and drop

1. Point to the block.
2. Drag its handle (**⋮⋮**).
3. Drop it on another block.

A line shows where the block will go. The block moves before the block that you drop it on, at the same indent level. To move a block to the end of the page, drop it on the empty area below the last block.

## Working with to-dos and toggles

- To tick or untick a to-do, click its box, or press **Cmd+Enter** while you edit it.
- To open or close a toggle, click its arrow, or press **Cmd+Enter** while you edit it.

Ticked to-dos show their text struck through. A toggle starts closed each time you open the page.

## Working with tables

**Table** in the slash menu adds a grid of two rows and three columns. The cursor starts in the first cell. Each cell holds one line of text.

| Key in a cell | Result |
|---|---|
| **Tab** | Moves to the next cell; past the last cell, adds a row |
| **Shift+Tab** | Moves to the previous cell |
| **Enter** | Moves to the cell below; in the last row, adds a row |
| **Up** or **Down** | Moves to the cell above or below |
| **Escape** | Stops editing the cell |

Point to the table to show a **+** bar under it and a **+** bar at its right. They add a row at the end and a column at the end.

Right-click a cell to open the table menu.

| Table menu item | What it does |
|---|---|
| **Insert row above**, **Insert row below** | Adds an empty row next to the cell's row |
| **Delete row** | Deletes the cell's row |
| **Insert column left**, **Insert column right** | Adds an empty column next to the cell's column |
| **Delete column** | Deletes the cell's column |
| **Header row: off** or **Header row: on** | Shows the current setting; click to switch it |

The header row shows the first row in bold on a shaded background. Deleting the only row deletes the table.

## Adding images

1. Type `/image` and press **Enter**.
2. Choose a picture file in the dialog.

The picture shows at most at the width of the page and at most 600 pixels high. If you close the dialog, the block shows **Add an image**; click it to choose a file.

Redbook copies the picture into its data folder and uses the copy. You can move, rename or delete the original file, and the picture still shows. Images from the import are copied the same way.

## Writing code blocks

Type ```` ``` ```` at the start of a block, or choose **Code** in the slash menu. A new code block has the label **Plain Text**. Click the label to choose another language from the list.

Code blocks use a monospaced font, and **Enter** adds a new line. The language is a label only; Redbook doesn't colour the code.

## Using callouts, quotes and dividers

- A **Callout** shows its text in a shaded box with a 💡 icon. Use **Background** in the block menu to change its colour.
- A **Quote** shows a line at the left of its text.
- A **Divider** is a horizontal line. Type `---` or choose **Divider**. You continue typing in the block after it.

Callouts and quotes can hold indented blocks. Indent a block under them with **Tab**.

## Keyboard shortcuts for writing

| Action | macOS | Linux and Windows |
|---|---|---|
| Bold | Cmd+B | Ctrl+B |
| Italic | Cmd+I | Ctrl+I |
| Underline | Cmd+U | Ctrl+U |
| Strikethrough | Cmd+Shift+S or Cmd+Shift+X | Ctrl+Shift+S or Ctrl+Shift+X |
| Inline code | Cmd+E | Ctrl+E |
| Add or edit a link (text selected) | Cmd+K | Ctrl+K |
| Open a link | Cmd+click | Super+click (Windows key+click) |
| Duplicate the block | Cmd+D | Ctrl+D |
| Turn into a kind | Cmd+Option+0 to 9 | Ctrl+Alt+0 to 9 |
| Tick a to-do, open or close a toggle | Cmd+Enter | Ctrl+Enter |
| Line break inside a block | Shift+Enter | Shift+Enter |
| Indent, outdent | Tab, Shift+Tab | Tab, Shift+Tab |
| Stop editing | Escape | Escape |

The shortcuts for the window are in [Pages and workspaces](02-pages-and-workspaces.md#keyboard-shortcuts-for-the-window).

## What the editor doesn't do

- You edit and select text in one block at a time. You can't select, copy or delete several blocks together.
- There is no undo for block operations such as delete, move or turn into. A block removed with **Delete** doesn't appear in the Trash, unless it is a sub-page or a database. A sub-page or database inside a deleted block can't be restored from the Trash.
- Drag and drop doesn't nest a block under another one. Use **Tab** for that.
- Bold, italic, links and colours don't show in the block that you edit. They show again when you leave the block.
- Equations show as their source text. Redbook doesn't render the formula.
- Code blocks have no syntax colours.
- A callout's 💡 icon can't be changed in the app.
- Table cells hold plain single-line text.
- Redbook has no command to copy a link to a page.

Next: [Databases](04-databases.md).
