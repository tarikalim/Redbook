//! `redbook mcp`: MCP server over stdio (newline-delimited JSON-RPC 2.0), no auth.
//! The `redbook` skill (skills/redbook) is served through the Skills extension.

use std::collections::HashMap;
use std::io::{BufRead as _, Write as _};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::import::{cell_value, ensure_option, resolve_workspace};
use crate::model::*;
use crate::store::{Store, apply_query, value_text};

const PROTOCOL: &str = "2025-06-18";
const SUPPORTED: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const SKILLS: &str = "io.modelcontextprotocol/skills";

/// The skill's files: (path inside the skill, text).
const SKILL_FILES: &[(&str, &str)] = &[
    ("SKILL.md", include_str!("../skills/redbook/SKILL.md")),
    (
        "references/markdown.md",
        include_str!("../skills/redbook/references/markdown.md"),
    ),
];

const INSTRUCTIONS: &str = "Local Redbook notes: pages, blocks (as markdown) and databases. \
Use search or list_pages to find ids, get_page to read. The `redbook` skill explains writing pages and databases.";

pub fn run() -> Result<()> {
    let store = Store::open_default()?;
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle(&store, &line) {
            writeln!(out, "{resp}")?;
            out.flush()?;
        }
    }
    Ok(())
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// One JSON-RPC message in, the response out (None for notifications).
pub fn handle(store: &Store, line: &str) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(rpc_error(Value::Null, -32700, &format!("parse error: {e}"))),
    };
    let id = msg.get("id").cloned();
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        // Responses to our (nonexistent) requests are ignored.
        return match id {
            Some(id) if msg.get("result").is_none() && msg.get("error").is_none() => {
                Some(rpc_error(id, -32600, "invalid request"))
            }
            _ => None,
        };
    };
    let id = id?;
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
    let capabilities =
        json!({ "tools": { "listChanged": false }, "resources": {}, "extensions": { SKILLS: {} } });
    let cache = |mut v: Value| {
        v["resultType"] = json!("complete");
        v["ttlMs"] = json!(300_000);
        v["cacheScope"] = json!("public");
        v
    };
    let result = match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str();
            let version = asked.filter(|v| SUPPORTED.contains(v)).unwrap_or(PROTOCOL);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": capabilities,
                "serverInfo": { "name": "redbook", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS
            }))
        }
        "server/discover" => Ok(
            json!({ "capabilities": capabilities, "serverInfo": { "name": "redbook", "version": env!("CARGO_PKG_VERSION") }, "instructions": INSTRUCTIONS }),
        ),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or_default();
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if tools().iter().any(|t| t["name"] == name) {
                let (text, is_error) = match call(store, name, &args) {
                    Ok(t) => (t, false),
                    Err(e) => (format!("{e:#}"), true),
                };
                Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
            } else {
                Err((-32602, format!("unknown tool: {name}")))
            }
        }
        "skills/list" => Ok(cache(json!({ "skills": [skill_entry()] }))),
        "skills/get" => match params["uri"].as_str() {
            Some(uri) if uri == skill_uri("SKILL.md") => {
                Ok(cache(json!({ "skill": skill_entry() })))
            }
            _ => Err((-32602, "unknown skill".into())),
        },
        "resources/list" => Ok(
            json!({ "resources": SKILL_FILES.iter().map(|(path, text)| json!({
            "uri": skill_uri(path), "name": format!("redbook/{path}"), "mimeType": "text/markdown", "size": text.len()
        })).collect::<Vec<_>>() }),
        ),
        "resources/read" => match SKILL_FILES
            .iter()
            .find(|(path, _)| params["uri"].as_str() == Some(&skill_uri(path)))
        {
            Some((path, text)) => Ok(cache(
                json!({ "contents": [{ "uri": skill_uri(path), "mimeType": "text/markdown", "text": text }] }),
            )),
            None => Err((-32602, "unknown resource".into())),
        },
        _ => Err((-32601, format!("method not found: {method}"))),
    };
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err((code, m)) => rpc_error(id, code, &m),
    })
}

fn skill_uri(path: &str) -> String {
    format!("skill://redbook/{path}")
}

/// The skills/list entry: frontmatter and a manifest of every file with its digest and size.
fn skill_entry() -> Value {
    let skill = SKILL_FILES[0].1;
    let mut frontmatter = serde_json::Map::new();
    for line in skill.split("---").nth(1).unwrap_or_default().lines() {
        if let Some((k, v)) = line.split_once(':') {
            frontmatter.insert(k.trim().into(), json!(v.trim()));
        }
    }
    json!({
        "uri": skill_uri("SKILL.md"),
        "frontmatter": frontmatter,
        "resources": SKILL_FILES.iter().map(|(path, text)| json!({
            "uri": skill_uri(path),
            "digest": sha256(text),
            "size": text.len()
        })).collect::<Vec<_>>()
    })
}

/// `sha256:<hex>` of a file's bytes, as the Skills manifest wants it.
fn sha256(text: &str) -> String {
    let hex: String = Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hex}")
}

const READ_ONLY: &[&str] = &[
    "list_workspaces",
    "search",
    "get_page",
    "list_pages",
    "query_database",
];

fn tool(name: &str, description: &str, props: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": props, "required": required, "additionalProperties": false },
        "annotations": {
            "readOnlyHint": READ_ONLY.contains(&name),
            "destructiveHint": matches!(name, "update_page" | "trash_page"),
            "openWorldHint": false
        }
    })
}

fn tools() -> Vec<Value> {
    let ws = json!({ "type": "string", "description": "Workspace name or id (default: the first workspace)" });
    let id = |d: &str| json!({ "type": "string", "description": d });
    let md = |d: &str| json!({ "type": "string", "description": d });
    let values = json!({
        "type": "object",
        "description": "Property name (or id) -> value. Strings are parsed by property type: \
            checkbox yes/no/true/false, number, date `2026-10-11` / `2026-10-11T15:30` / `October 11, 2026` \
            (range: `A → B`), select (new options are created), multi_select comma list or array. \
            null clears a value. The title property sets the row title.",
        "additionalProperties": true
    });
    vec![
        tool(
            "list_workspaces",
            "List all workspaces with their ids.",
            json!({}),
            &[],
        ),
        tool(
            "search",
            "Full-text prefix search over page titles, block text, table cells and database values. Returns one hit per page: title, page id, snippet.",
            json!({
                "query": { "type": "string" },
                "workspace": ws,
                "limit": { "type": "integer", "description": "Max hits (default 20)" }
            }),
            &["query"],
        ),
        tool(
            "get_page",
            "Read a page as markdown with a header (id, title, type, parent, breadcrumb, child pages). \
             Database rows include their property values; databases list their schema, views and rows. \
             Links to other pages look like redbook://page/<id>.",
            json!({ "page_id": id("Page, database or row id") }),
            &["page_id"],
        ),
        tool(
            "list_pages",
            "Page tree (pages and databases, not rows) of a workspace, or of one page's subtree.",
            json!({ "workspace": ws, "parent_id": id("Only list the subtree under this page") }),
            &[],
        ),
        tool(
            "create_page",
            "Create a page under parent_id (a page, or a database to add a row), or at the root of a workspace. \
             Content is markdown: headings, lists (nested by indent), - [ ] todos, > quotes, ``` code, tables, \
             ![alt](url) images, <details><summary>toggle</summary>…</details>, <aside>💡 callout</aside>.",
            json!({
                "parent_id": id("Parent page id; omit for a workspace root page"),
                "workspace": ws,
                "title": { "type": "string" },
                "markdown": md("Page content"),
                "icon": { "type": "string", "description": "Emoji icon" }
            }),
            &["title"],
        ),
        tool(
            "update_page",
            "Change a page's title or icon (empty string removes it), and/or replace its content with markdown. \
             Replacing content keeps child pages and databases (also ones nested in toggles) and places them after \
             the new content in their order; the replaced blocks go to the trash as one restorable page, or are \
             deleted when trash_replaced is false. Locked pages are refused.",
            json!({
                "page_id": id("Page id"),
                "title": { "type": "string" },
                "icon": { "type": "string" },
                "markdown": md("New content (replaces the existing blocks)"),
                "trash_replaced": { "type": "boolean", "description": "Keep the replaced blocks in the trash (default true); false deletes them" }
            }),
            &["page_id"],
        ),
        tool(
            "append_blocks",
            "Append markdown content to the end of a page (same markdown as create_page).",
            json!({ "page_id": id("Page id"), "markdown": md("Content to append") }),
            &["page_id", "markdown"],
        ),
        tool(
            "move_page",
            "Move a page under new_parent_id, or to the root of a workspace (its own when none given). \
             index = position among the new siblings (default: last).",
            json!({
                "page_id": id("Page id"),
                "new_parent_id": id("New parent page id"),
                "workspace": ws,
                "index": { "type": "integer", "minimum": 0 }
            }),
            &["page_id"],
        ),
        tool(
            "trash_page",
            "Move a page and its subtree to the trash (restorable).",
            json!({ "page_id": id("Page id") }),
            &["page_id"],
        ),
        tool(
            "restore_page",
            "Restore a trashed page; it goes to the workspace root if its parent is still trashed.",
            json!({ "page_id": id("Page id") }),
            &["page_id"],
        ),
        tool(
            "create_database",
            "Create a full-page database under parent_id (or at a workspace root). It always has a title \
             property (`Name` unless a property of type title is given) and a Table view.",
            json!({
                "parent_id": id("Parent page id"),
                "workspace": ws,
                "title": { "type": "string" },
                "properties": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "type": {
                                "type": "string",
                                "enum": ["title", "text", "number", "select", "multi_select", "status", "date",
                                         "checkbox", "url", "email", "phone_number", "created_time",
                                         "last_edited_time", "relation", "file"]
                            },
                            "options": { "type": "array", "items": { "type": "string" },
                                         "description": "Select / multi_select / status options" }
                        },
                        "required": ["name", "type"]
                    }
                }
            }),
            &["title"],
        ),
        tool(
            "query_database",
            "Rows of a database as a markdown table (with row ids). Without filter and sorts the first view's \
             query is used. Filter conditions: {property, op, value}; op is one of is, is_not, contains, \
             does_not_contain, starts_with, ends_with, is_empty, is_not_empty, greater_than, less_than, \
             greater_than_or_equal_to, less_than_or_equal_to. Property names or ids are accepted.",
            json!({
                "database_id": id("Database id"),
                "filter": {
                    "description": "Array of conditions (all must match), or {operator: \"and\"|\"or\", conditions: [...]}",
                    "type": ["array", "object"]
                },
                "sorts": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "property": { "type": "string" },
                            "direction": { "type": "string", "enum": ["ascending", "descending"] }
                        },
                        "required": ["property"]
                    }
                },
                "limit": { "type": "integer", "description": "Max rows (default 100)" }
            }),
            &["database_id"],
        ),
        tool(
            "add_db_row",
            "Add a row to a database.",
            json!({ "database_id": id("Database id"), "values": values }),
            &["database_id"],
        ),
        tool(
            "update_db_row",
            "Set property values of a database row (only the given properties change).",
            json!({ "row_id": id("Row id"), "values": values }),
            &["row_id", "values"],
        ),
    ]
}

fn opt<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str)
}

fn req<'a>(a: &'a Value, k: &str) -> Result<&'a str> {
    opt(a, k).with_context(|| format!("missing string argument `{k}`"))
}

fn num(a: &Value, k: &str) -> Option<usize> {
    a.get(k).and_then(Value::as_u64).map(|n| n as usize)
}

fn call(store: &Store, name: &str, a: &Value) -> Result<String> {
    match name {
        "list_workspaces" => {
            let ws = store.workspaces()?;
            Ok(ws
                .iter()
                .map(|w| format!("- {} (id: {})", w.name, w.id))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        "search" => search(store, a),
        "get_page" => get_page(store, req(a, "page_id")?),
        "list_pages" => list_pages(store, a),
        "create_page" => store.tx(|| {
            let (ws, parent) = target(store, a)?;
            let b = store.create_block(&ws, parent, None, BlockType::Page)?;
            let title = req(a, "title")?;
            store.update_title(&b.id, &RichText::from_plain(title))?;
            if let Some(icon) = opt(a, "icon").filter(|i| !i.is_empty()) {
                store.set_prop(&b.id, "icon", json!(icon))?;
            }
            if let Some(md) = opt(a, "markdown") {
                store.markdown_to_blocks(&b.id, md)?;
            }
            Ok(format!("Created page \"{title}\" (id: {})", b.id))
        }),
        "update_page" => store.tx(|| {
            let id = req(a, "page_id")?;
            let page = unlocked(store, id)?;
            if let Some(t) = opt(a, "title") {
                store.update_title(id, &RichText::from_plain(t))?;
            }
            if let Some(i) = opt(a, "icon") {
                let icon = if i.is_empty() { Value::Null } else { json!(i) };
                store.set_prop(id, "icon", icon)?;
            }
            if let Some(md) = opt(a, "markdown") {
                let trash = a
                    .get("trash_replaced")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                clear_content(store, &page, trash)?;
                store.markdown_to_blocks(id, md)?;
                // Kept sub-pages go below the new content, in their order.
                for c in store
                    .children(id)?
                    .into_iter()
                    .filter(|c| c.kind.is_page_like())
                {
                    store.move_block(&c.id, Some(id), usize::MAX)?;
                }
            }
            Ok(format!("Updated page {id}"))
        }),
        "append_blocks" => {
            let id = req(a, "page_id")?;
            unlocked(store, id)?;
            let n = store.markdown_to_blocks(id, req(a, "markdown")?)?.len();
            Ok(format!("Appended {n} blocks to {id}"))
        }
        "move_page" => {
            let id = req(a, "page_id")?;
            let index = num(a, "index").unwrap_or(usize::MAX);
            match opt(a, "new_parent_id") {
                Some(p) => store.move_block(id, Some(p), index)?,
                None => {
                    let ws = match opt(a, "workspace") {
                        Some(w) => resolve_workspace(store, Some(w), false)?,
                        None => store.block(id)?.workspace_id,
                    };
                    store.move_block_in(id, Some(&ws), None, index)?
                }
            }
            Ok(format!("Moved {id}"))
        }
        "trash_page" => {
            let id = req(a, "page_id")?;
            unlocked(store, id)?;
            store.trash(id)?;
            Ok(format!("Trashed {id}"))
        }
        "restore_page" => {
            let id = req(a, "page_id")?;
            store.restore(id)?;
            Ok(format!("Restored {id}"))
        }
        "create_database" => store.tx(|| create_database(store, a)),
        "query_database" => query_database(store, a),
        "add_db_row" => store.tx(|| {
            let db = req(a, "database_id")?;
            let mut schema = store.schema(db)?;
            let row = store.add_row(db)?.id;
            set_values(store, db, &mut schema, &row, a.get("values"))?;
            Ok(format!("Added row {row}"))
        }),
        "update_db_row" => store.tx(|| {
            let row = req(a, "row_id")?;
            let db = unlocked(store, row)?
                .parent_id
                .filter(|p| store.schema(p).is_ok())
                .ok_or_else(|| anyhow!("{row} is not a database row"))?;
            let mut schema = store.schema(&db)?;
            set_values(store, &db, &mut schema, row, a.get("values"))?;
            Ok(format!("Updated row {row}"))
        }),
        _ => bail!("unknown tool {name}"),
    }
}

/// The block, or an error when the page is locked in the app.
fn unlocked(store: &Store, id: &str) -> Result<Block> {
    let b = store.block(id)?;
    if b.props.get("locked").and_then(Value::as_bool) == Some(true) {
        bail!(
            "page {id} is locked; unlock it in Redbook (page menu > Lock page) before changing it"
        );
    }
    Ok(b)
}

/// Moves the page's non-page blocks into one trashed page (or deletes them); sub-pages inside them stay on the page.
fn clear_content(store: &Store, page: &Block, trash: bool) -> Result<()> {
    let old: Vec<Block> = store
        .children(&page.id)?
        .into_iter()
        .filter(|c| !c.kind.is_page_like())
        .collect();
    if old.is_empty() {
        return Ok(());
    }
    for c in &old {
        for p in store.nested_pages(&c.id)? {
            store.move_block(&p, Some(&page.id), usize::MAX)?;
        }
    }
    if !trash {
        for c in &old {
            store.delete_forever(&c.id)?;
        }
        return Ok(());
    }
    let bin = store.create_block(&page.workspace_id, Some(&page.id), None, BlockType::Page)?;
    let title = format!("{} (replaced content)", page.title.plain_text());
    store.update_title(&bin.id, &RichText::from_plain(&title))?;
    for c in &old {
        store.move_block(&c.id, Some(&bin.id), usize::MAX)?;
    }
    store.trash(&bin.id)
}

/// (workspace, parent) from `parent_id` or `workspace` arguments.
fn target<'a>(store: &Store, a: &'a Value) -> Result<(String, Option<&'a str>)> {
    Ok(match opt(a, "parent_id") {
        Some(p) => (store.block(p)?.workspace_id, Some(p)),
        None => (resolve_workspace(store, opt(a, "workspace"), true)?, None),
    })
}

fn workspaces_arg(store: &Store, a: &Value) -> Result<Vec<Workspace>> {
    let all = store.workspaces()?;
    match opt(a, "workspace") {
        Some(w) => {
            let id = resolve_workspace(store, Some(w), false)?;
            Ok(all.into_iter().filter(|x| x.id == id).collect())
        }
        None => Ok(all),
    }
}

fn search(store: &Store, a: &Value) -> Result<String> {
    let q = req(a, "query")?;
    let limit = num(a, "limit").unwrap_or(20);
    let mut out = vec![];
    for w in workspaces_arg(store, a)? {
        for h in store.search(&w.id, q, limit)? {
            out.push(format!(
                "- {} (id: {}) [{}] — {}",
                h.title, h.page_id, w.name, h.snippet
            ));
        }
    }
    out.truncate(limit);
    Ok(if out.is_empty() {
        "No results.".into()
    } else {
        out.join("\n")
    })
}

fn kind_name(b: &Block, store: &Store) -> &'static str {
    if b.kind.is_collection() {
        "database"
    } else if b
        .parent_id
        .as_deref()
        .is_some_and(|p| store.schema(p).is_ok())
    {
        "database row"
    } else {
        "page"
    }
}

fn get_page(store: &Store, id: &str) -> Result<String> {
    let b = store.block(id)?;
    if !b.kind.is_page_like() {
        bail!("{id} is a {} block, not a page", b.kind.as_str());
    }
    let ws = store.workspace(&b.workspace_id)?;
    let crumbs = store.ancestors(id)?;
    let kind = kind_name(&b, store);
    let mut out = format!(
        "---\nid: {id}\ntitle: {}\ntype: {kind}\nworkspace: {}\n",
        b.title.plain_text(),
        ws.name
    );
    match crumbs.len().checked_sub(2).map(|i| &crumbs[i]) {
        Some(p) => out.push_str(&format!("parent: {} ({})\n", p.id, p.title)),
        None => out.push_str("parent: workspace root\n"),
    }
    let path: Vec<&str> = crumbs.iter().map(|c| c.title.as_str()).collect();
    out.push_str(&format!("breadcrumb: {}\n", path.join(" / ")));
    if let Some(i) = b.icon() {
        out.push_str(&format!("icon: {i}\n"));
    }
    if b.trashed.is_some() {
        out.push_str("trashed: true\n");
    }
    out.push_str("---\n\n");

    if kind == "database" {
        let schema = store.schema(id)?;
        out.push_str(&format!("# {}\n\n## Properties\n", b.title.plain_text()));
        for (pid, p) in ordered(&schema) {
            let opts: Vec<&str> = p.options.iter().map(|o| o.value.as_str()).collect();
            let opts = if opts.is_empty() {
                String::new()
            } else {
                format!(": {}", opts.join(", "))
            };
            out.push_str(&format!(
                "- {} ({}{opts}) [id: {pid}]\n",
                p.name,
                type_name(p.kind)
            ));
        }
        out.push_str("\n## Views\n");
        for v in store.views(id)? {
            out.push_str(&format!(
                "- {} ({}) query: {}\n",
                v.name,
                v.kind.as_str(),
                serde_json::to_string(&v.query)?
            ));
        }
        let rows = first_view_rows(store, id, &schema)?;
        out.push_str(&format!("\n## Rows ({})\n\n", rows.len()));
        out.push_str(&rows_table(&rows, &schema, 100));
        return Ok(out);
    }
    if kind == "database row" {
        let db = b.parent_id.as_deref().unwrap_or_default();
        let schema = store.schema(db)?;
        out.push_str("## Properties\n");
        for (pid, p) in ordered(&schema).into_iter().skip(1) {
            out.push_str(&format!(
                "- {}: {}\n",
                p.name,
                value_text(&b.row_value(pid, p))
            ));
        }
        out.push('\n');
    }
    out.push_str(&store.page_to_markdown(id)?);
    let kids: Vec<String> = store
        .page_tree(&b.workspace_id)?
        .into_iter()
        .filter(|n| n.parent_id.as_deref() == Some(id))
        .map(|n| {
            format!(
                "- {} (id: {}){}",
                n.title,
                n.id,
                if n.kind.is_collection() {
                    " [database]"
                } else {
                    ""
                }
            )
        })
        .collect();
    if !kids.is_empty() {
        out.push_str(&format!("\n## Child pages\n{}\n", kids.join("\n")));
    }
    Ok(out)
}

fn list_pages(store: &Store, a: &Value) -> Result<String> {
    let root = opt(a, "parent_id");
    let wss = match root {
        Some(p) => vec![store.workspace(&store.block(p)?.workspace_id)?],
        None => workspaces_arg(store, a)?,
    };
    let mut out = String::new();
    for w in wss {
        let tree = store.page_tree(&w.id)?;
        let mut kids: HashMap<Option<&str>, Vec<&PageNode>> = HashMap::new();
        for n in &tree {
            kids.entry(n.parent_id.as_deref()).or_default().push(n);
        }
        fn walk(
            kids: &HashMap<Option<&str>, Vec<&PageNode>>,
            parent: Option<&str>,
            depth: usize,
            out: &mut String,
        ) {
            for n in kids.get(&parent).into_iter().flatten() {
                let icon = n
                    .icon
                    .as_deref()
                    .map(|i| format!("{i} "))
                    .unwrap_or_default();
                let db = if n.kind.is_collection() {
                    " [database]"
                } else {
                    ""
                };
                out.push_str(&format!(
                    "{}- {icon}{} (id: {}){db}\n",
                    "  ".repeat(depth),
                    n.title,
                    n.id
                ));
                walk(kids, Some(&n.id), depth + 1, out);
            }
        }
        out.push_str(&format!("# {} (workspace id: {})\n", w.name, w.id));
        walk(&kids, root, 0, &mut out);
    }
    Ok(out)
}

fn type_name(k: PropType) -> String {
    serde_json::to_value(k)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn parse_type(s: &str) -> Result<PropType> {
    let s = match s {
        "rich_text" => "text",
        "multi-select" => "multi_select",
        s => s,
    };
    match serde_json::from_value(json!(s)) {
        Ok(PropType::Unknown) | Err(_) => bail!("unknown property type {s}"),
        Ok(k) => Ok(k),
    }
}

/// Title first, then by name.
fn ordered(schema: &Schema) -> Vec<(&String, &Property)> {
    let mut v: Vec<_> = schema.iter().collect();
    v.sort_by_key(|(id, p)| (*id != "title", p.name.to_lowercase()));
    v
}

/// Property id for a name or id.
fn prop_id(schema: &Schema, key: &str) -> Result<String> {
    if schema.contains_key(key) {
        return Ok(key.to_string());
    }
    schema
        .iter()
        .find(|(_, p)| p.name.eq_ignore_ascii_case(key))
        .map(|(id, _)| id.clone())
        .ok_or_else(|| {
            let names: Vec<&str> = schema.values().map(|p| p.name.as_str()).collect();
            anyhow!("no property \"{key}\" (have: {})", names.join(", "))
        })
}

fn create_database(store: &Store, a: &Value) -> Result<String> {
    let (ws, parent) = target(store, a)?;
    let title = req(a, "title")?;
    let db = store.create_database(&ws, parent, None, true)?.id;
    store.update_title(&db, &RichText::from_plain(title))?;
    for p in a
        .get("properties")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = req(p, "name")?;
        let kind = parse_type(req(p, "type")?)?;
        let (pid, mut prop) = if kind == PropType::Title {
            ("title".to_string(), Property::new(name, kind))
        } else {
            (
                store.add_property(&db, name, kind)?,
                Property::new(name, kind),
            )
        };
        for o in p
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            ensure_option(&mut prop, o.as_str().context("options must be strings")?);
        }
        store.update_property(&db, &pid, prop)?;
    }
    let schema = store.schema(&db)?;
    let props: Vec<String> = ordered(&schema)
        .iter()
        .map(|(id, p)| format!("{} ({}, id: {id})", p.name, type_name(p.kind)))
        .collect();
    Ok(format!(
        "Created database \"{title}\" (id: {db}) with properties: {}",
        props.join(", ")
    ))
}

/// JSON value as stored for `prop`; strings are parsed by type, select options added.
fn coerce(prop: &mut Property, v: &Value) -> Result<Value> {
    let bad = |p: &Property| anyhow!("invalid value {v} for {} ({})", p.name, type_name(p.kind));
    match (prop.kind, v) {
        (_, Value::Null) => Ok(Value::Null),
        (
            PropType::CreatedTime | PropType::LastEditedTime | PropType::Formula | PropType::Rollup,
            _,
        ) => {
            bail!("{} is read-only", prop.name)
        }
        (_, Value::String(s)) if s.trim().is_empty() => Ok(Value::Null),
        (_, Value::String(s)) => cell_value(prop, s).ok_or_else(|| bad(prop)),
        (PropType::Checkbox, Value::Bool(_)) | (PropType::Number, Value::Number(_)) => {
            Ok(v.clone())
        }
        (PropType::Date, Value::Object(o)) if o.get("start").is_some_and(Value::is_string) => {
            Ok(v.clone())
        }
        (PropType::MultiSelect | PropType::Relation | PropType::File, Value::Array(items)) => {
            let mut out = vec![];
            for i in items {
                let s = i.as_str().ok_or_else(|| bad(prop))?;
                if prop.kind == PropType::MultiSelect {
                    ensure_option(prop, s);
                }
                out.push(json!(s));
            }
            Ok(Value::Array(out))
        }
        _ => Err(bad(prop)),
    }
}

fn set_values(
    store: &Store,
    db: &str,
    schema: &mut Schema,
    row: &str,
    values: Option<&Value>,
) -> Result<()> {
    let Some(values) = values else { return Ok(()) };
    let values = values.as_object().context("`values` must be an object")?;
    let mut changed = vec![];
    for (k, v) in values {
        let pid = prop_id(schema, k)?;
        let prop = schema.get_mut(&pid).unwrap();
        let before = prop.options.len();
        let val = coerce(prop, v)?;
        if prop.options.len() != before {
            changed.push(pid.clone());
        }
        store.set_row_value(row, &pid, val)?;
    }
    for pid in changed {
        store.update_property(db, &pid, schema[&pid].clone())?;
    }
    Ok(())
}

fn first_view_rows(store: &Store, db: &str, schema: &Schema) -> Result<Vec<Block>> {
    let q = store
        .views(db)?
        .into_iter()
        .next()
        .map(|v| v.query)
        .unwrap_or_default();
    Ok(apply_query(store.rows(db)?, &q, schema))
}

fn rows_table(rows: &[Block], schema: &Schema, limit: usize) -> String {
    let props = ordered(schema);
    let cell = |s: String| s.replace('|', "\\|").replace('\n', " ");
    let mut out = format!(
        "| id | {} |\n|{}\n",
        props
            .iter()
            .map(|(_, p)| cell(p.name.clone()))
            .collect::<Vec<_>>()
            .join(" | "),
        " --- |".repeat(props.len() + 1)
    );
    for r in rows.iter().take(limit) {
        let vals: Vec<String> = props
            .iter()
            .map(|(id, p)| cell(value_text(&r.row_value(id, p))))
            .collect();
        out.push_str(&format!("| {} | {} |\n", r.id, vals.join(" | ")));
    }
    if rows.len() > limit {
        out.push_str(&format!("\n({} more rows not shown)\n", rows.len() - limit));
    }
    out
}

fn condition(schema: &Schema, c: &Value) -> Result<Filter> {
    let op = req(c, "op").or_else(|_| req(c, "operator"))?;
    Ok(Filter {
        property: prop_id(schema, req(c, "property")?)?,
        op: serde_json::from_value(json!(op)).map_err(|_| anyhow!("unknown filter op {op}"))?,
        value: c.get("value").cloned().unwrap_or(Value::Null),
    })
}

fn query_database(store: &Store, a: &Value) -> Result<String> {
    let db = req(a, "database_id")?;
    let schema = store.schema(db)?;
    let limit = num(a, "limit").unwrap_or(100);
    let (filter, sorts) = (
        a.get("filter").filter(|v| !v.is_null()),
        a.get("sorts").filter(|v| !v.is_null()),
    );
    let rows = if filter.is_none() && sorts.is_none() {
        first_view_rows(store, db, &schema)?
    } else {
        let mut q = Query::default();
        let conds = match filter {
            Some(Value::Array(c)) => c.clone(),
            Some(o @ Value::Object(m)) => match m.get("conditions") {
                Some(Value::Array(c)) => {
                    if opt(o, "operator") == Some("or") {
                        q.operator = FilterOperator::Or;
                    }
                    c.clone()
                }
                _ => vec![o.clone()],
            },
            Some(_) => bail!("`filter` must be an array or object"),
            None => vec![],
        };
        q.filters = conds
            .iter()
            .map(|c| condition(&schema, c))
            .collect::<Result<_>>()?;
        for s in sorts.and_then(Value::as_array).into_iter().flatten() {
            let dir = opt(s, "direction").unwrap_or("ascending");
            q.sorts.push(Sort {
                property: prop_id(&schema, req(s, "property")?)?,
                direction: if dir.starts_with("desc") {
                    Direction::Descending
                } else {
                    Direction::Ascending
                },
            });
        }
        apply_query(store.rows(db)?, &q, &schema)
    };
    Ok(format!(
        "{} rows\n\n{}",
        rows.len(),
        rows_table(&rows, &schema, limit)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpc(store: &Store, method: &str, params: Value) -> Value {
        let line =
            json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }).to_string();
        handle(store, &line).unwrap()
    }

    fn tool_call(store: &Store, name: &str, args: Value) -> (String, bool) {
        let r = rpc(
            store,
            "tools/call",
            json!({ "name": name, "arguments": args }),
        );
        let res = &r["result"];
        (
            res["content"][0]["text"].as_str().unwrap().to_string(),
            res["isError"] == true,
        )
    }

    fn id_in(text: &str) -> String {
        let i = text.find("id: ").unwrap() + 4;
        text[i..i + 36].to_string()
    }

    #[test]
    fn skill_is_served_with_a_matching_manifest() {
        let s = Store::open_in_memory().unwrap();
        let init = rpc(&s, "initialize", json!({ "protocolVersion": "2025-06-18" }));
        assert!(init["result"]["capabilities"]["extensions"][SKILLS].is_object());
        let list = rpc(&s, "skills/list", json!({}));
        let skill = &list["result"]["skills"][0];
        assert_eq!(skill["frontmatter"]["name"], "redbook");
        for file in skill["resources"].as_array().unwrap() {
            let read = rpc(&s, "resources/read", json!({ "uri": file["uri"] }));
            let text = read["result"]["contents"][0]["text"].as_str().unwrap();
            assert_eq!(file["size"], text.len());
            assert_eq!(file["digest"], sha256(text));
        }
        assert_eq!(
            rpc(&s, "skills/get", json!({ "uri": "skill://nope/SKILL.md" }))["error"]["code"],
            -32602
        );
        // A known vector, so the manifest's digest is not only checked against itself.
        assert_eq!(
            sha256("hello"),
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn protocol_and_tools() {
        let s = Store::open_in_memory().unwrap();
        let init = rpc(
            &s,
            "initialize",
            json!({ "protocolVersion": "2024-11-05", "capabilities": {} }),
        );
        assert_eq!(init["result"]["protocolVersion"], "2024-11-05");
        let init = rpc(&s, "initialize", json!({ "protocolVersion": "1999-01-01" }));
        assert_eq!(init["result"]["protocolVersion"], PROTOCOL);
        assert!(
            handle(
                &s,
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
            )
            .is_none()
        );
        assert_eq!(rpc(&s, "nope", json!({}))["error"]["code"], -32601);
        assert_eq!(handle(&s, "{oops").unwrap()["error"]["code"], -32700);
        assert_eq!(
            rpc(&s, "tools/list", json!({}))["result"]["tools"]
                .as_array()
                .unwrap()
                .len(),
            14
        );

        let (t, err) = tool_call(
            &s,
            "create_page",
            json!({ "title": "Plans", "markdown": "- [ ] buy milk\n\nzebra text", "icon": "📝" }),
        );
        assert!(!err, "{t}");
        let page = id_in(&t);
        let (t, _) = tool_call(&s, "search", json!({ "query": "zebr" }));
        assert!(t.contains(&page), "{t}");
        let (t, _) = tool_call(&s, "get_page", json!({ "page_id": page }));
        assert!(
            t.contains("title: Plans") && t.contains("- [ ] buy milk") && t.contains("icon: 📝"),
            "{t}"
        );

        let (t, err) = tool_call(
            &s,
            "create_database",
            json!({
                "parent_id": page, "title": "Tasks",
                "properties": [{ "name": "Status", "type": "select", "options": ["Todo", "Done"] },
                               { "name": "Points", "type": "number" }, { "name": "Due", "type": "date" }]
            }),
        );
        assert!(!err, "{t}");
        let db = id_in(&t);
        for (name, st, pts) in [("a", "Done", 3), ("b", "Todo", 5), ("c", "Later", 1)] {
            let (t, err) = tool_call(
                &s,
                "add_db_row",
                json!({
                    "database_id": db, "values": { "Name": name, "status": st, "Points": pts, "Due": "October 11, 2026" }
                }),
            );
            assert!(!err, "{t}");
        }
        let (t, _) = tool_call(
            &s,
            "query_database",
            json!({
                "database_id": db, "filter": [{ "property": "Points", "op": "greater_than", "value": 2 }],
                "sorts": [{ "property": "Points", "direction": "descending" }]
            }),
        );
        let (b, a) = (t.find("| b |").unwrap(), t.find("| a |").unwrap());
        assert!(t.starts_with("2 rows") && b < a, "{t}");
        assert_eq!(
            s.schema(&db)
                .unwrap()
                .values()
                .find(|p| p.name == "Status")
                .unwrap()
                .options
                .len(),
            3
        );
        let (_, err) = tool_call(
            &s,
            "add_db_row",
            json!({ "database_id": db, "values": { "Points": "many" } }),
        );
        assert!(err);
        let (t, _) = tool_call(&s, "get_page", json!({ "page_id": db }));
        assert!(
            t.contains("type: database") && t.contains("Status (select: Todo, Done, Later)"),
            "{t}"
        );

        let (t, _) = tool_call(&s, "list_pages", json!({}));
        assert!(t.contains("- 📝 Plans") && t.contains("  - Tasks"), "{t}");
        let (t, err) = tool_call(
            &s,
            "update_page",
            json!({ "page_id": page, "markdown": "# New", "title": "Plans 2" }),
        );
        assert!(!err, "{t}");
        let md = s.page_to_markdown(&page).unwrap();
        assert!(
            md.starts_with("# Plans 2\n") && md.contains("# New") && !md.contains("milk"),
            "{md}"
        );
        assert_eq!(s.children(&page).unwrap().len(), 2); // database kept
        let (_, err) = tool_call(&s, "trash_page", json!({ "page_id": page }));
        assert!(!err);
        assert!(
            tool_call(&s, "search", json!({ "query": "new" }))
                .0
                .contains("No results")
        );
        let (_, err) = tool_call(&s, "get_page", json!({ "page_id": "missing" }));
        assert!(err);
    }

    #[test]
    fn update_page_trashes_content_and_respects_lock() {
        let s = Store::open_in_memory().unwrap();
        let (t, _) = tool_call(
            &s,
            "create_page",
            json!({ "title": "P", "markdown": "old text" }),
        );
        let page = id_in(&t);
        let ws = s.block(&page).unwrap().workspace_id;
        let toggle = s
            .create_block(&ws, Some(&page), None, BlockType::Toggle)
            .unwrap()
            .id;
        let sub = s
            .create_block(&ws, Some(&toggle), None, BlockType::Page)
            .unwrap()
            .id;
        let (t, err) = tool_call(
            &s,
            "update_page",
            json!({ "page_id": page, "markdown": "new text" }),
        );
        assert!(!err, "{t}");
        let sub_b = s.block(&sub).unwrap();
        assert!(sub_b.parent_id.as_deref() == Some(page.as_str()) && sub_b.trashed.is_none());
        let trash = s.list_trash(&ws).unwrap();
        assert_eq!(trash.len(), 1);
        assert!(s.block(&toggle).unwrap().trashed.is_some());
        s.restore(&trash[0].id).unwrap();
        assert!(
            s.page_to_markdown(&trash[0].id)
                .unwrap()
                .contains("old text")
        );
        s.trash(&trash[0].id).unwrap();

        s.set_prop(&page, "locked", json!(true)).unwrap();
        for (tool, args) in [
            ("update_page", json!({ "page_id": page, "title": "X" })),
            ("append_blocks", json!({ "page_id": page, "markdown": "x" })),
            ("trash_page", json!({ "page_id": page })),
        ] {
            let (t, err) = tool_call(&s, tool, args);
            assert!(err && t.contains("locked"), "{tool}: {t}");
        }
        assert!(s.page_to_markdown(&page).unwrap().contains("new text"));
    }

    #[test]
    fn update_page_without_trash_keeps_sub_pages_after_content() {
        let s = Store::open_in_memory().unwrap();
        let (t, _) = tool_call(
            &s,
            "create_page",
            json!({ "title": "Week", "markdown": "## Days" }),
        );
        let page = id_in(&t);
        let ws = s.block(&page).unwrap().workspace_id;
        let days: Vec<String> = ["Mon", "Tue"]
            .iter()
            .map(|d| {
                id_in(&tool_call(&s, "create_page", json!({ "title": d, "parent_id": page })).0)
            })
            .collect();
        for md in ["| a | b |\n|---|---|\n| 1 | 2 |\n\n## Days", "## Days 2"] {
            let (t, err) = tool_call(
                &s,
                "update_page",
                json!({ "page_id": page, "markdown": md, "trash_replaced": false }),
            );
            assert!(!err, "{t}");
        }
        assert!(s.list_trash(&ws).unwrap().is_empty());
        let kids: Vec<Block> = s.children(&page).unwrap();
        assert_eq!(kids.len(), 3);
        assert_eq!(kids[0].kind, BlockType::SubHeader);
        assert_eq!([&kids[1].id, &kids[2].id], [&days[0], &days[1]]);
    }
}
