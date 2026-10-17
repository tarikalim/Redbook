# Importing a Markdown & CSV export

Redbook imports a **Markdown & CSV** export from another notes app: pages with their sub-pages, databases with their rows, and the images and files they contain. You run the import once from a terminal, and the pages appear in a Redbook workspace.

## The export

The export is a zip file. It holds each page as a Markdown file and each database as a CSV file, and each file name ends with a 32-character id, for example `Projects 0123456789abcdef0123456789abcdef.md`. The sub-pages of a page are in a folder with the page's name.

When you export, include the sub-pages, the images and files, and a folder for the sub-pages of each page.

Keep the file as a zip. Redbook can also import the folder that you get when you extract it. If the zip contains more zip files, Redbook extracts those too.

## Running the import

Prerequisites:

- Redbook is installed. You know the program path for your system; see [Running Redbook commands in a terminal](01-install.md#running-redbook-commands-in-a-terminal).
- The export zip file or its extracted folder is on this computer.

Procedure:

1. Open a terminal.
2. Run `redbook import <path to the export zip>`.
3. Wait for the summary line.

To import into a particular workspace, add `--workspace` and its name, for example `redbook import Export.zip --workspace Work`. If no workspace has that name, Redbook creates it. Without `--workspace`, the pages go into the first workspace.

The summary looks like this: Imported into "Personal": 42 pages, 3 databases, 120 rows, 15 files; 0 already imported (skipped). If there are warnings, a line **Warnings (N):** follows, then one warning per line.

Redbook can stay open during the import. The new pages appear in the sidebar a moment after the import ends.

## What comes over

| In the export | In Redbook |
|---|---|
| Pages and sub-pages | Pages in the same tree, at the top level of the workspace |
| Headings, paragraphs, bulleted, numbered and to-do lists | The same blocks, with nesting kept |
| Bold, italic, strikethrough, inline code and links | The same formatting |
| Quotes, toggles and callouts | The same blocks; callouts keep their emoji |
| Code blocks | Code blocks with their language label |
| Simple tables | Tables with a header row |
| Images and attached files | Copies in the `files` folder of the data folder |
| Links between pages of the export | Internal Redbook links that open the page |
| Databases (from their CSV files) | Full-page databases with one **Table** view |
| Database rows and their page content | Rows, with their content on each row page |

Redbook guesses each database property's type from its values in the CSV file.

| Values in the column | Property type in Redbook |
|---|---|
| Only `Yes` and `No` | **Checkbox** |
| Only numbers | **Number** |
| Only dates | **Date** |
| Only `http://` or `https://` addresses | **URL** |
| Only email addresses | **Email** |
| Short values separated by commas | **Multi-select** |
| A few short values that repeat | **Select** |
| Anything else | **Text** |

An inline database becomes a full-page database in Redbook. It stays at its place in the parent page, as a link to the database.

## What doesn't come over

- **Linked database views.** A linked view isn't a database in the export, so Redbook skips it. The summary shows a warning such as: "Projects": database "Tasks" is not in the export (linked view?), skipped.
- **Database views.** Only one **Table** view is created. Boards, filters, sorts and calendars aren't in the export.
- **Property types without an equivalent.** Status, relation, rollup, formula, person and file properties arrive as plain values. Redbook guesses their type like any other column.
- **Page icons, covers and text colours.** The Markdown export doesn't contain them.
- **Comments and page history.**
- **Links to pages outside the export.** They stay as web links.
- **Files missing from the export.** Each one gives a warning that names the page and the file.

## Importing again

Redbook remembers which page of the export each imported page came from. If you import the same export again, Redbook skips the pages that it already has and adds only new ones. The summary counts them as "already imported (skipped)". New rows in a database that was already imported aren't added.

A second import doesn't update pages that changed in the other app after the first import. Pages in the Redbook Trash count as already imported. To import such a page again, delete it permanently from the Trash first.

## What the import doesn't do

- The import has no window in the app. It runs only from a terminal.
- It doesn't connect to the other app. It reads an export file that you made.
- It doesn't keep Redbook and the other app in step. Later changes there need a new export, and existing pages don't update.
- It doesn't import exports in HTML or PDF format.

Next: [Sync between devices](06-sync-between-devices.md).
