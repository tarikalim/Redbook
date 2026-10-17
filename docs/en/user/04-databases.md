# Databases

A database is a collection of rows with typed properties, like a spreadsheet whose rows are also pages. You see the same rows through one or more views: a table, a board, a list, a gallery or a calendar.

## Creating a database

A database is either inline, inside a page, or a full page of its own.

1. Open the page that will hold the database.
2. Type `/database` in an empty block.
3. Choose **Database - Inline** or **Database - Full page** and press **Enter**.

| Kind | Result |
|---|---|
| **Database - Inline** | The database appears in the page in place of the empty block |
| **Database - Full page** | Redbook creates the database as a sub-page and opens it |

A new database has one property, **Name**, and one view, **Table**. Click the name at the top of the database to give it a title. Both kinds also show in the sidebar tree, under their page.

An inline table or list shows up to 14 rows before it scrolls. An inline gallery shows up to three rows of cards. A full-page database uses the whole window.

## Adding and editing rows

Each row is a page. Its **Name** is the page title, and its other properties sit at the top of the page.

- To add a row, click **New** in the toolbar, or click **New** under the last row of a table.
- To edit a value in a table, click the cell.
- To open a row as a page, click **Open** in its **Name** cell, or click the row in a board, list, gallery or calendar.

In a table view, a new row starts with the cursor in its **Name** cell. In the other views, Redbook opens the new row as a page.

On a row page, the properties appear between the title and the page content. Empty values show **Empty**; click one to set it. **Add a property** adds a new property to the whole database.

To delete a row, right-click it in a table, board, list or gallery view and choose **Delete**. You can also open the row and choose **Move to Trash** in the **…** page menu. The row goes to the Trash like any page; [Pages and workspaces](02-pages-and-workspaces.md#deleting-and-restoring-pages-with-the-trash) describes how to restore it.

## Property types

To add a property in a table, click **+** at the end of the header row and choose a type. The new property is named after its type, and its name is ready to edit. Type a name and press **Enter**.

| Type | Holds | How you set it |
|---|---|---|
| **Title** | The row name; every database has exactly one | Click and type |
| **Text** | Free text | Click and type |
| **Number** | A number, shown in the chosen format | Click and type |
| **Select** | One option | Pick or create an option |
| **Multi-select** | Several options | Pick or create options; the list stays open |
| **Status** | One option; new ones start with **Not started**, **In progress**, **Done** | Pick or create an option |
| **Date** | One day | Pick a day, or click **Today** or **Clear** |
| **Checkbox** | Yes or no | Click the box |
| **URL** | A web address, shown underlined | Click and type |
| **Email** | An email address, shown underlined | Click and type |
| **Phone** | A phone number | Click and type |
| **Created time** | When the row was created | Set by Redbook |
| **Last edited time** | When the row last changed | Set by Redbook |

To finish a text, number or other typed value, press **Enter**.

## Using select, multi-select and status options

When you click an option property, a field and the list of options appear.

1. Type part of an option name to filter the list.
2. Click an option, or press **Enter** to take the first match.

If nothing matches, press **Enter** or click **Create** to add your text as a new option. Redbook gives each new option a colour. Clicking the option that is already set clears the value of a select or status property. In a multi-select, each click adds or removes one option.

## Changing a property

Click a column header in a table to open the property menu. On a row page, click the property name.

| Menu item | What it does |
|---|---|
| **Rename** | Edits the property name in place |
| **Sort ascending**, **Sort descending** | Sorts the view by this property only (table header only) |
| **Hide** | Hides the column in this view (table header only) |
| **Type** | Changes the property type |
| **Number format** | **Number**, **Number with commas**, **Percent**, **US dollar**, **Euro**, **Pound**, **Yen** or **Lira** (number properties only) |
| **Delete property** | Removes the property and its values from every row |

The **Name** property can be renamed, but it can't change type or be deleted.

To change a column's width, drag the right edge of its header. Each view keeps its own widths.

## Adding views

A view is one way to look at the rows. Every view has its own filters, sorts, hidden properties and settings. The tabs at the top of the database list the views.

1. Click **+** after the last view tab.
2. Choose **Table**, **Board**, **List**, **Gallery** or **Calendar**.

| View | Shows |
|---|---|
| **Table** | Rows and columns, with editable cells and calculations |
| **Board** | Cards in columns, one column per option of a select or status property |
| **List** | One line per row, with up to four property values |
| **Gallery** | Cards in a grid of four columns |
| **Calendar** | A month grid, rows placed on the day of a date property |

A new board groups by one of the select or status properties. If the database has none, Redbook adds a **Status** property. A new calendar uses one of the date properties. If there is none, Redbook adds a **Date** property.

Right-click the open view's tab to **Rename view**, **Duplicate view** or **Delete view**. These items act on the open view, so click a tab first to open it. The **…** button in the toolbar has the same items. The last view of a database can't be deleted.

## Using a board view

The board shows one column per option, plus a column for rows with no value. The **Group** button in the toolbar shows and changes the grouping property. Only select and status properties can group a board.

- Drag a card to another column to change its value.
- Click **New** at the bottom of a column to add a row with that column's value.
- Click a card to open the row.

## Using a calendar view

The calendar shows one month, with weeks starting on Sunday. The **By** button in the toolbar sets the date property that places the rows.

- Use the arrow buttons to go to the previous or next month, and **Today** to come back.
- Click a day to add a row with that date. Redbook opens the new row.
- Click a row on a day to open it.

A day shows up to three rows and then **+N more**. Rows without a date don't appear.

## Filtering rows

Filters hide the rows that don't match. A view shows rows that match all of its filters.

1. Click **Filter** in the toolbar.
2. Choose a property.
3. In the bar under the toolbar, click the operator and choose one.
4. Type or pick the value.

The **Filter** button shows the number of filters, for example **Filter 2**. A filter without a value doesn't hide any rows yet. A checkbox filter starts as **Is** **Checked**, so it hides rows at once. To remove a filter, click its **x**.

| Property type | Operators |
|---|---|
| Text, Title, URL, Email, Phone | **Is**, **Is not**, **Contains**, **Does not contain**, **Starts with**, **Ends with**, **Is empty**, **Is not empty** |
| Number | **=**, **≠**, **>**, **<**, **≥**, **≤**, **Is empty**, **Is not empty** |
| Select, Status | **Is**, **Is not**, **Is empty**, **Is not empty** |
| Multi-select | **Contains**, **Does not contain**, **Is empty**, **Is not empty** |
| Checkbox | **Is** (**Checked** or **Unchecked**) |
| Date | **Is**, **Is before**, **Is after**, **Is on or before**, **Is on or after**, **Is empty**, **Is not empty** |
| Created time, Last edited time | **Is before**, **Is on or after** |

Type dates in filters as `YYYY-MM-DD`, for example `2026-10-11`.

## Sorting rows

1. Click **Sort** in the toolbar.
2. Choose a property.

The sort appears in the bar under the toolbar. Click **Ascending** or **Descending** to switch the direction. Add more sorts the same way; the first one decides first. To remove a sort, click its **x**.

**Sort ascending** and **Sort descending** in a column header replace all sorts of the view with that one column.

## Searching rows in a database

Click the search button in the toolbar and type. The view shows only rows whose values contain the text, without regard to capital letters. Click the search button again to close the field and show all rows.

## Showing and hiding properties

To hide a column, choose **Hide** in its header menu. To show it again, open the **…** menu in the toolbar, point to **Properties**, and click the property. A tick marks the properties that the view shows. Board cards, list lines and gallery cards show the same properties as the view, without empty values. Board cards leave out the property that groups the board, and list lines show at most the first four properties.

## Adding calculations to a table

The last line of a table view has a **Calculate** button under each column. Click it and choose a calculation for that column. Choose **None** to remove it. The result covers the rows that the view shows.

| Property type | Calculations |
|---|---|
| Number | **Count all**, **Count values**, **Count unique**, **Count empty**, **Count not empty**, **Percent empty**, **Percent not empty**, **Sum**, **Average**, **Median**, **Min**, **Max**, **Range** |
| Checkbox | **Count all**, **Checked**, **Unchecked**, **Percent checked** |
| All other types | **Count all**, **Count values**, **Count unique**, **Count empty**, **Count not empty**, **Percent empty**, **Percent not empty** |

## What databases don't do

- The app can't add relation, rollup, formula, person or file properties. An AI agent can create relation and file properties; the app shows them as plain values. An imported database with these keeps their content as plain values.
- Filters always combine with "and". There are no "or" groups or nested filter groups in the app.
- Board views group only by select or status properties.
- Rows can't be reordered by drag and drop. Use a sort.
- A table has no row selection and no bulk delete. Delete rows one at a time.
- URL and email values are shown underlined, but a click edits them; it doesn't open them.
- Dates that you enter hold a single day. You can't enter a time or an end date in the app. Imported dates can keep a time or an end date.
- Calendar weeks always start on Sunday.

Next: [Importing a Markdown & CSV export](05-import.md).
