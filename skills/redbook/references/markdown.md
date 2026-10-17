# Markdown and values

## Blocks

`create_page`, `append_blocks` and `update_page` take markdown. `get_page` returns the same forms.

| Markdown | Block |
|---|---|
| `# `, `## `, `### ` | Headings |
| plain lines | Text |
| `- ` or `* ` | Bulleted list; indent to nest |
| `1. ` | Numbered list; indent to nest |
| `- [ ] ` / `- [x] ` | To-do, open or done |
| `> ` | Quote |
| ```` ```lang ```` … ```` ``` ```` | Code block with a language |
| `\| a \| b \|` rows | Table; the first row is the header |
| `![alt](url)` | Image |
| `<details><summary>Title</summary>…</details>` | Toggle with its content |
| `<aside>💡 text</aside>` | Callout with an emoji |
| `---` | Divider |
| `$$ … $$` | Equation |

Inline: `**bold**`, `*italic*`, `~~strike~~`, `` `code` ``, `[text](url)`. A link to
`redbook://page/<id>` opens that page in the app.

## Database values

`add_db_row` and `update_db_row` take `values`: property name (or id) to value. Strings are parsed
by the property's type.

| Type | Value |
|---|---|
| title, text, url, email, phone_number | A string |
| number | A number, or a string such as `"1,200"` |
| checkbox | `true` / `false`, or `"yes"` / `"no"` |
| date | `"2026-10-11"`, `"2026-10-11T15:30"`, `"October 11, 2026"`; a range is `"A → B"` |
| select, status | An option name; a new name creates the option |
| multi_select | An array, or a comma-separated string |
| any | `null` clears the value |

`create_database` property types: `title`, `text`, `number`, `select`, `multi_select`, `status`,
`date`, `checkbox`, `url`, `email`, `phone_number`, `created_time`, `last_edited_time`, `relation`,
`file`. Select, multi-select and status take `options`.

## Filters

`query_database` filter conditions are `{property, op, value}`; `op` is one of `is`, `is_not`,
`contains`, `does_not_contain`, `starts_with`, `ends_with`, `is_empty`, `is_not_empty`,
`greater_than`, `less_than`, `greater_than_or_equal_to`, `less_than_or_equal_to`. A list of
conditions must all match; `{"operator": "or", "conditions": [...]}` matches any.
