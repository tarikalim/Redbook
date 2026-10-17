//! SQLite storage: schema, migrations, block CRUD, trash, search, databases, markdown.
#![allow(dead_code)] // UI wiring comes later

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension as _, Row, params};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::model::*;

macro_rules! page_like {
    () => {
        "('page','collection_view','collection_view_page')"
    };
}

/// Plain text of a rich-text JSON column, for FTS triggers.
macro_rules! plain_sql {
    ($col:literal) => {
        concat!(
            "(SELECT coalesce(group_concat(json_extract(value,'$[0]'),''),'') FROM json_each(",
            $col,
            "))"
        )
    };
}

/// FTS text of a block row `$r`: title, simple-table cells and database property values.
macro_rules! fts_text {
    ($r:literal) => {
        concat!(
            "((SELECT coalesce(group_concat(json_extract(value,'$[0]'),''),'') FROM json_each(",
            $r,
            ".title)) || ' ' || (SELECT coalesce(group_concat(t, ' '), '') FROM (SELECT (SELECT group_concat(json_extract(s.value, '$[0]'), '') FROM json_each(c.value) s) t FROM json_each(",
            $r,
            ".props, '$.cells') c)) || ' ' || (SELECT coalesce(group_concat(value, ' '), '') FROM json_tree(",
            $r,
            ".props, '$.properties') WHERE type IN ('text', 'integer', 'real')))"
        )
    };
}

/// Index i holds the migration to user_version i+1.
const MIGRATIONS: &[&str] = &[
    concat!(
        "
CREATE TABLE workspaces(
  id TEXT PRIMARY KEY, name TEXT NOT NULL, icon TEXT, sort REAL NOT NULL, created INTEGER NOT NULL
);
CREATE TABLE blocks(
  rid INTEGER PRIMARY KEY,  -- stable rowid for FTS
  id TEXT NOT NULL UNIQUE,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  parent_id TEXT REFERENCES blocks(id) ON DELETE CASCADE,
  type TEXT NOT NULL,
  sort REAL NOT NULL,
  title TEXT NOT NULL DEFAULT '[]',
  props TEXT NOT NULL DEFAULT '{}',
  created INTEGER NOT NULL,
  edited INTEGER NOT NULL,
  trashed INTEGER,
  favorite INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX blocks_parent ON blocks(parent_id, sort);
CREATE INDEX blocks_root ON blocks(workspace_id, parent_id, sort);
CREATE TABLE collections(id TEXT PRIMARY KEY REFERENCES blocks(id) ON DELETE CASCADE, schema TEXT NOT NULL);
CREATE TABLE views(
  id TEXT PRIMARY KEY,
  collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  type TEXT NOT NULL, name TEXT NOT NULL, sort REAL NOT NULL,
  query TEXT NOT NULL DEFAULT '{}', format TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX views_collection ON views(collection_id, sort);
CREATE VIRTUAL TABLE blocks_fts USING fts5(text, tokenize = 'unicode61 remove_diacritics 2');
CREATE TRIGGER blocks_ai AFTER INSERT ON blocks BEGIN
  INSERT INTO blocks_fts(rowid, text) VALUES (NEW.rid, ",
    plain_sql!("NEW.title"),
    ");
END;
CREATE TRIGGER blocks_au AFTER UPDATE OF title ON blocks BEGIN
  UPDATE blocks_fts SET text = ",
    plain_sql!("NEW.title"),
    " WHERE rowid = NEW.rid;
END;
CREATE TRIGGER blocks_ad AFTER DELETE ON blocks BEGIN
  DELETE FROM blocks_fts WHERE rowid = OLD.rid;
END;
"
    ),
    // Sync change log, active once `redbook sync init/clone` wrote sync_state.
    // DELETE + INSERT, not OR REPLACE: an outer upsert would override the trigger's conflict clause.

        "
CREATE TABLE sync_state(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE sync_dirty(seq INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE);
CREATE TRIGGER sync_bi AFTER INSERT ON blocks WHEN EXISTS (SELECT 1 FROM sync_state) BEGIN
  DELETE FROM sync_dirty WHERE id = NEW.id;
  INSERT INTO sync_dirty(id) VALUES (NEW.id);
END;
CREATE TRIGGER sync_bu AFTER UPDATE ON blocks WHEN EXISTS (SELECT 1 FROM sync_state) BEGIN
  DELETE FROM sync_dirty WHERE id = NEW.id;
  INSERT INTO sync_dirty(id) VALUES (NEW.id);
  DELETE FROM sync_dirty WHERE id = OLD.parent_id AND OLD.parent_id IS NOT NEW.parent_id;
  INSERT INTO sync_dirty(id) SELECT OLD.parent_id
    WHERE OLD.parent_id IS NOT NULL AND OLD.parent_id IS NOT NEW.parent_id;
END;
CREATE TRIGGER sync_bd AFTER DELETE ON blocks WHEN EXISTS (SELECT 1 FROM sync_state) BEGIN
  DELETE FROM sync_dirty WHERE id = OLD.id;
  INSERT INTO sync_dirty(id) VALUES (OLD.id);
  DELETE FROM sync_dirty WHERE id = OLD.parent_id;
  INSERT INTO sync_dirty(id) SELECT OLD.parent_id WHERE OLD.parent_id IS NOT NULL;
END;
CREATE TRIGGER sync_ci AFTER INSERT ON collections WHEN EXISTS (SELECT 1 FROM sync_state) BEGIN
  DELETE FROM sync_dirty WHERE id = NEW.id;
  INSERT INTO sync_dirty(id) VALUES (NEW.id);
END;
CREATE TRIGGER sync_cu AFTER UPDATE ON collections WHEN EXISTS (SELECT 1 FROM sync_state) BEGIN
  DELETE FROM sync_dirty WHERE id = NEW.id;
  INSERT INTO sync_dirty(id) VALUES (NEW.id);
END;
CREATE TRIGGER sync_vi AFTER INSERT ON views WHEN EXISTS (SELECT 1 FROM sync_state) BEGIN
  DELETE FROM sync_dirty WHERE id = NEW.collection_id;
  INSERT INTO sync_dirty(id) VALUES (NEW.collection_id);
END;
CREATE TRIGGER sync_vu AFTER UPDATE ON views WHEN EXISTS (SELECT 1 FROM sync_state) BEGIN
  DELETE FROM sync_dirty WHERE id = NEW.collection_id;
  INSERT INTO sync_dirty(id) VALUES (NEW.collection_id);
END;
CREATE TRIGGER sync_vd AFTER DELETE ON views WHEN EXISTS (SELECT 1 FROM sync_state) BEGIN
  DELETE FROM sync_dirty WHERE id = OLD.collection_id;
  INSERT INTO sync_dirty(id) VALUES (OLD.collection_id);
END;
",
    // Search also covers simple-table cells and database property values.
    concat!(
        "
DROP TRIGGER blocks_ai;
DROP TRIGGER blocks_au;
CREATE TRIGGER blocks_ai AFTER INSERT ON blocks BEGIN
  INSERT INTO blocks_fts(rowid, text) VALUES (NEW.rid, ",
        fts_text!("NEW"),
        ");
END;
CREATE TRIGGER blocks_au AFTER UPDATE OF title, props ON blocks BEGIN
  UPDATE blocks_fts SET text = ",
        fts_text!("NEW"),
        " WHERE rowid = NEW.rid;
END;
DELETE FROM blocks_fts;
INSERT INTO blocks_fts(rowid, text) SELECT rid, ",
        fts_text!("blocks"),
        " FROM blocks;
"
    ),
];

pub struct Store {
    conn: Connection,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

fn short_id() -> String {
    Uuid::new_v4().simple().to_string()[..8].to_string()
}

fn block_from_row(r: &Row) -> rusqlite::Result<Block> {
    let kind: String = r.get("type")?;
    let title: String = r.get("title")?;
    let props: String = r.get("props")?;
    Ok(Block {
        id: r.get("id")?,
        workspace_id: r.get("workspace_id")?,
        parent_id: r.get("parent_id")?,
        kind: BlockType::parse(&kind),
        sort: r.get("sort")?,
        title: RichText::from_json_str(&title),
        props: serde_json::from_str(&props).unwrap_or_else(|_| json!({})),
        created: r.get("created")?,
        edited: r.get("edited")?,
        trashed: r.get("trashed")?,
        favorite: r.get("favorite")?,
    })
}

/// Row with columns id, parent, type, title, props.
fn node_from_row(r: &Row) -> rusqlite::Result<PageNode> {
    let kind: String = r.get(2)?;
    let title: String = r.get(3)?;
    let props: String = r.get(4)?;
    let props: Value = serde_json::from_str(&props).unwrap_or(Value::Null);
    Ok(PageNode {
        id: r.get(0)?,
        parent_id: r.get(1)?,
        kind: BlockType::parse(&kind),
        title: RichText::from_json_str(&title).plain_text(),
        icon: props
            .get("icon")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn workspace_from_row(r: &Row) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: r.get("id")?,
        name: r.get("name")?,
        icon: r.get("icon")?,
        sort: r.get("sort")?,
        created: r.get("created")?,
    })
}

fn view_from_row(r: &Row) -> rusqlite::Result<View> {
    let kind: String = r.get("type")?;
    let query: String = r.get("query")?;
    let format: String = r.get("format")?;
    Ok(View {
        id: r.get("id")?,
        collection_id: r.get("collection_id")?,
        kind: ViewType::parse(&kind),
        name: r.get("name")?,
        sort: r.get("sort")?,
        query: serde_json::from_str(&query).unwrap_or_default(),
        format: serde_json::from_str(&format).unwrap_or_else(|_| json!({})),
    })
}

/// Sort key strictly between neighbours, or None when the gap is exhausted.
fn sort_between(prev: Option<f64>, next: Option<f64>) -> Option<f64> {
    let v = match (prev, next) {
        (None, None) => 1.0,
        (Some(p), None) => p + 1.0,
        (None, Some(n)) => n - 1.0,
        (Some(p), Some(n)) => (p + n) / 2.0,
    };
    let ok =
        prev.is_none_or(|p| v > p && v - p > 1e-9) && next.is_none_or(|n| v < n && n - v > 1e-9);
    ok.then_some(v)
}

impl Store {
    /// $REDBOOK_DB, else <data dir>/RedBook/redbook.db: ~/Library/Application Support on
    /// macOS, $XDG_DATA_HOME (~/.local/share) on Linux, %APPDATA% on Windows.
    pub fn default_path() -> PathBuf {
        if let Some(p) = std::env::var_os("REDBOOK_DB") {
            return PathBuf::from(p);
        }
        dirs::data_dir()
            .unwrap_or_default()
            .join("RedBook")
            .join("redbook.db")
    }

    pub fn open_default() -> Result<Self> {
        Self::open(&Self::default_path())
    }

    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000;
             PRAGMA cache_size = -64; PRAGMA mmap_size = 268435456;",
        )?;
        conn.set_prepared_statement_cache_capacity(128);
        let store = Store { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            let tx = self.conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Runs `f` in a transaction, or inline when one is already open.
    pub fn tx<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<T> {
        if !self.conn.is_autocommit() {
            return f();
        }
        let tx = self.conn.unchecked_transaction()?;
        let out = f()?;
        tx.commit()?;
        Ok(out)
    }

    /// Raw connection for modules with their own tables (sync).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Changes when another connection (e.g. the MCP server) commits.
    pub fn data_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA data_version", [], |r| r.get(0))?)
    }

    // ---- workspaces

    pub fn workspaces(&self) -> Result<Vec<Workspace>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT * FROM workspaces ORDER BY sort")?;
        Ok(st
            .query_map([], workspace_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn workspace(&self, id: &str) -> Result<Workspace> {
        let mut st = self
            .conn
            .prepare_cached("SELECT * FROM workspaces WHERE id = ?1")?;
        st.query_row([id], workspace_from_row)
            .optional()?
            .ok_or_else(|| anyhow!("no workspace {id}"))
    }

    pub fn create_workspace(&self, name: &str) -> Result<Workspace> {
        let sort: f64 = self.conn.query_row(
            "SELECT coalesce(max(sort), 0) + 1 FROM workspaces",
            [],
            |r| r.get(0),
        )?;
        let ws = Workspace {
            id: new_id(),
            name: name.to_string(),
            icon: None,
            sort,
            created: now(),
        };
        self.conn
            .prepare_cached(
                "INSERT INTO workspaces(id, name, icon, sort, created) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![ws.id, ws.name, ws.icon, ws.sort, ws.created])?;
        Ok(ws)
    }

    pub fn rename_workspace(&self, id: &str, name: &str) -> Result<()> {
        self.conn
            .prepare_cached("UPDATE workspaces SET name = ?2 WHERE id = ?1")?
            .execute(params![id, name])?;
        Ok(())
    }

    pub fn set_workspace_icon(&self, id: &str, icon: Option<&str>) -> Result<()> {
        self.conn
            .prepare_cached("UPDATE workspaces SET icon = ?2 WHERE id = ?1")?
            .execute(params![id, icon])?;
        Ok(())
    }

    /// Deletes the workspace and all its blocks.
    pub fn delete_workspace(&self, id: &str) -> Result<()> {
        self.tx(|| {
            self.conn
                .prepare_cached("DELETE FROM blocks WHERE workspace_id = ?1")?
                .execute([id])?;
            self.conn
                .prepare_cached("DELETE FROM workspaces WHERE id = ?1")?
                .execute([id])?;
            Ok(())
        })
    }

    pub fn reorder_workspace(&self, id: &str, index: usize) -> Result<()> {
        self.tx(|| {
            let mut sibs: Vec<(String, f64)> = self
                .workspaces()?
                .into_iter()
                .filter(|w| w.id != id)
                .map(|w| (w.id, w.sort))
                .collect();
            let sort = self.place("workspaces", &mut sibs, index)?;
            self.conn
                .prepare_cached("UPDATE workspaces SET sort = ?2 WHERE id = ?1")?
                .execute(params![id, sort])?;
            Ok(())
        })
    }

    // ---- ordering

    /// Sort key for `index` among `sibs` (ordered), renumbering the table rows if the gap is gone.
    fn place(&self, table: &str, sibs: &mut [(String, f64)], index: usize) -> Result<f64> {
        let index = index.min(sibs.len());
        let neighbours = |s: &[(String, f64)]| {
            (
                index.checked_sub(1).map(|i| s[i].1),
                s.get(index).map(|x| x.1),
            )
        };
        let (prev, next) = neighbours(sibs);
        if let Some(v) = sort_between(prev, next) {
            return Ok(v);
        }
        let mut st = self
            .conn
            .prepare_cached(&format!("UPDATE {table} SET sort = ?2 WHERE id = ?1"))?;
        for (i, (id, sort)) in sibs.iter_mut().enumerate() {
            *sort = (i + 1) as f64;
            st.execute(params![&*id, *sort])?;
        }
        let (prev, next) = neighbours(sibs);
        sort_between(prev, next).ok_or_else(|| anyhow!("sort renumbering failed"))
    }

    fn sibling_sorts(
        &self,
        ws: &str,
        parent: Option<&str>,
        exclude: Option<&str>,
    ) -> Result<Vec<(String, f64)>> {
        let mut st = self.conn.prepare_cached(
            "SELECT id, sort FROM blocks WHERE workspace_id = ?1 AND parent_id IS ?2 AND trashed IS NULL
             AND id IS NOT ?3 ORDER BY sort",
        )?;
        Ok(st
            .query_map(params![ws, parent, exclude], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    // ---- blocks

    pub fn block(&self, id: &str) -> Result<Block> {
        let mut st = self
            .conn
            .prepare_cached("SELECT * FROM blocks WHERE id = ?1")?;
        st.query_row([id], block_from_row)
            .optional()?
            .ok_or_else(|| anyhow!("no block {id}"))
    }

    fn insert_block(&self, b: &Block) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT INTO blocks(id, workspace_id, parent_id, type, sort, title, props, created, edited, trashed, favorite)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?
            .execute(params![
                b.id,
                b.workspace_id,
                b.parent_id,
                b.kind.as_str(),
                b.sort,
                b.title.to_json_string(),
                b.props.to_string(),
                b.created,
                b.edited,
                b.trashed,
                b.favorite
            ])?;
        Ok(())
    }

    /// New empty block under `parent` (None = workspace root), after `after` or at the end.
    pub fn create_block(
        &self,
        ws: &str,
        parent: Option<&str>,
        after: Option<&str>,
        kind: BlockType,
    ) -> Result<Block> {
        self.tx(|| {
            let ws = match parent {
                Some(p) => self.block(p)?.workspace_id,
                None => ws.to_string(),
            };
            let mut sibs = self.sibling_sorts(&ws, parent, None)?;
            let index = match after {
                Some(a) => {
                    sibs.iter()
                        .position(|s| s.0 == a)
                        .ok_or_else(|| anyhow!("{a} is not a sibling"))?
                        + 1
                }
                None => sibs.len(),
            };
            let sort = self.place("blocks", &mut sibs, index)?;
            let t = now();
            let b = Block {
                id: new_id(),
                workspace_id: ws,
                parent_id: parent.map(str::to_string),
                kind,
                sort,
                title: RichText::default(),
                props: json!({}),
                created: t,
                edited: t,
                trashed: None,
                favorite: false,
            };
            self.insert_block(&b)?;
            Ok(b)
        })
    }

    /// Non-trashed children in order.
    pub fn children(&self, parent: &str) -> Result<Vec<Block>> {
        let mut st = self.conn.prepare_cached(
            "SELECT * FROM blocks WHERE parent_id = ?1 AND trashed IS NULL ORDER BY sort",
        )?;
        Ok(st
            .query_map([parent], block_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Topmost non-trashed page-like blocks below `id` (not `id` itself).
    pub fn nested_pages(&self, id: &str) -> Result<Vec<String>> {
        let mut out = vec![];
        for c in self.children(id)? {
            if c.kind.is_page_like() {
                out.push(c.id);
            } else {
                out.extend(self.nested_pages(&c.id)?);
            }
        }
        Ok(out)
    }

    pub fn update_title(&self, id: &str, title: &RichText) -> Result<()> {
        self.conn
            .prepare_cached("UPDATE blocks SET title = ?2, edited = ?3 WHERE id = ?1")?
            .execute(params![id, title.to_json_string(), now()])?;
        Ok(())
    }

    /// Replaces the whole props object.
    pub fn update_props(&self, id: &str, props: &Value) -> Result<()> {
        self.conn
            .prepare_cached("UPDATE blocks SET props = ?2, edited = ?3 WHERE id = ?1")?
            .execute(params![id, props.to_string(), now()])?;
        Ok(())
    }

    /// Sets one props key; `Value::Null` removes it.
    pub fn set_prop(&self, id: &str, key: &str, value: Value) -> Result<()> {
        self.tx(|| {
            let mut props = self.block(id)?.props;
            let obj = props
                .as_object_mut()
                .ok_or_else(|| anyhow!("props of {id} is not an object"))?;
            if value.is_null() {
                obj.remove(key);
            } else {
                obj.insert(key.to_string(), value);
            }
            self.update_props(id, &props)
        })
    }

    pub fn update_type(&self, id: &str, kind: BlockType) -> Result<()> {
        self.conn
            .prepare_cached("UPDATE blocks SET type = ?2, edited = ?3 WHERE id = ?1")?
            .execute(params![id, kind.as_str(), now()])?;
        Ok(())
    }

    // ---- simple tables (`table` + `table_row` children with `cells`)

    /// Fills an empty `table` block with `rows` x `cols` empty cells, header row on.
    pub fn fill_table(&self, table: &str, rows: usize, cols: usize) -> Result<()> {
        self.tx(|| {
            self.set_prop(table, "width", json!(cols))?;
            self.set_prop(table, "header", json!(true))?;
            for _ in 0..rows {
                self.add_table_row(table, usize::MAX)?;
            }
            Ok(())
        })
    }

    /// Column count: the stored width or the widest row.
    pub fn table_width(&self, table: &str) -> Result<usize> {
        let w = self
            .block(table)?
            .props
            .get("width")
            .and_then(Value::as_u64);
        let rows = self.children(table)?;
        Ok(rows
            .iter()
            .map(|r| r.cells().len())
            .chain(w.map(|w| w as usize))
            .max()
            .unwrap_or(0))
    }

    /// Empty row at `index` (clamped to the end).
    pub fn add_table_row(&self, table: &str, index: usize) -> Result<Block> {
        self.tx(|| {
            let width = self.table_width(table)?.max(1);
            let rows = self.children(table)?;
            let index = index.min(rows.len());
            let after = index.checked_sub(1).map(|i| rows[i].id.as_str());
            let mut b = self.create_block("", Some(table), after, BlockType::TableRow)?;
            if index == 0 && !rows.is_empty() {
                self.move_block(&b.id, Some(table), 0)?;
            }
            b.props = json!({ "cells": vec![json!([]); width] });
            self.update_props(&b.id, &b.props)?;
            Ok(b)
        })
    }

    /// Inserts an empty column at `index` (clamped), padding short rows.
    pub fn add_table_column(&self, table: &str, index: usize) -> Result<()> {
        self.tx(|| {
            let width = self.table_width(table)?;
            for r in self.children(table)? {
                let mut cells: Vec<Value> = r.cells().into_iter().map(Value::from).collect();
                cells.resize(width, json!([]));
                cells.insert(index.min(width), json!([]));
                self.set_prop(&r.id, "cells", json!(cells))?;
            }
            self.set_prop(table, "width", json!(width + 1))
        })
    }

    pub fn delete_table_column(&self, table: &str, col: usize) -> Result<()> {
        self.tx(|| {
            let width = self.table_width(table)?;
            if col >= width || width <= 1 {
                return Ok(());
            }
            for r in self.children(table)? {
                let mut cells: Vec<Value> = r.cells().into_iter().map(Value::from).collect();
                cells.resize(width, json!([]));
                cells.remove(col);
                self.set_prop(&r.id, "cells", json!(cells))?;
            }
            self.set_prop(table, "width", json!(width - 1))
        })
    }

    /// Block ids of the subtree rooted at `id` (included), parents before children.
    fn subtree_ids(&self, id: &str) -> Result<Vec<String>> {
        let mut st = self.conn.prepare_cached(
            "WITH RECURSIVE s(id, depth) AS (
               SELECT ?1, 0 UNION ALL SELECT b.id, s.depth + 1 FROM blocks b JOIN s ON b.parent_id = s.id)
             SELECT id FROM s ORDER BY depth",
        )?;
        Ok(st
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Moves `id` under `new_parent` (None = workspace root) at `index` among its new siblings.
    pub fn move_block(&self, id: &str, new_parent: Option<&str>, index: usize) -> Result<()> {
        self.move_block_in(id, None, new_parent, index)
    }

    /// Like `move_block`; `ws` picks the workspace for a root move (default: the block's own).
    pub fn move_block_in(
        &self,
        id: &str,
        ws: Option<&str>,
        new_parent: Option<&str>,
        index: usize,
    ) -> Result<()> {
        self.tx(|| {
            let b = self.block(id)?;
            let ws = match new_parent {
                Some(p) => {
                    if self.subtree_ids(id)?.iter().any(|s| s == p) {
                        bail!("cannot move a block into itself");
                    }
                    self.block(p)?.workspace_id
                }
                None => ws.map_or_else(|| b.workspace_id.clone(), str::to_string),
            };
            let mut sibs = self.sibling_sorts(&ws, new_parent, Some(id))?;
            let sort = self.place("blocks", &mut sibs, index)?;
            self.conn
                .prepare_cached(
                    "UPDATE blocks SET parent_id = ?2, sort = ?3, edited = ?4 WHERE id = ?1",
                )?
                .execute(params![id, new_parent, sort, now()])?;
            if ws != b.workspace_id {
                let mut st = self
                    .conn
                    .prepare_cached("UPDATE blocks SET workspace_id = ?2 WHERE id = ?1")?;
                for sid in self.subtree_ids(id)? {
                    st.execute(params![sid, ws])?;
                }
            }
            Ok(())
        })
    }

    /// Block imported from the given source id (`props.source_id`), trashed ones included.
    pub fn find_by_source(&self, source_id: &str) -> Result<Option<String>> {
        let mut st = self.conn.prepare_cached(
            "SELECT id FROM blocks WHERE json_extract(props, '$.source_id') = ?1",
        )?;
        Ok(st.query_row([source_id], |r| r.get(0)).optional()?)
    }

    // ---- sidebar

    /// Page-like nodes of a workspace (database rows excluded), parent = nearest page-like ancestor.
    pub fn page_tree(&self, ws: &str) -> Result<Vec<PageNode>> {
        let mut st = self.conn.prepare_cached(concat!(
            "WITH RECURSIVE t(id, type, page_parent) AS (
               SELECT id, type, NULL FROM blocks WHERE workspace_id = ?1 AND parent_id IS NULL AND trashed IS NULL
               UNION ALL
               SELECT b.id, b.type, CASE WHEN t.type IN ",
            page_like!(),
            " THEN t.id ELSE t.page_parent END
               FROM t JOIN blocks b ON b.parent_id = t.id
               WHERE b.trashed IS NULL AND t.type NOT IN ('collection_view', 'collection_view_page'))
             SELECT b.id, t.page_parent, b.type, b.title, b.props FROM t JOIN blocks b ON b.id = t.id
             WHERE b.type IN ",
            page_like!(),
            " ORDER BY b.sort"
        ))?;
        Ok(st
            .query_map([ws], node_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Breadcrumb: page-like ancestors root first, including `id` itself if page-like.
    pub fn ancestors(&self, id: &str) -> Result<Vec<PageNode>> {
        let mut st = self.conn.prepare_cached(concat!(
            "WITH RECURSIVE a(id, depth) AS (
               SELECT ?1, 0 UNION ALL SELECT b.parent_id, a.depth + 1 FROM blocks b JOIN a ON b.id = a.id
               WHERE b.parent_id IS NOT NULL)
             SELECT b.id, b.parent_id, b.type, b.title, b.props FROM a JOIN blocks b ON b.id = a.id
             WHERE b.type IN ",
            page_like!(),
            " ORDER BY a.depth DESC"
        ))?;
        Ok(st
            .query_map([id], node_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn toggle_favorite(&self, id: &str) -> Result<bool> {
        self.conn
            .prepare_cached("UPDATE blocks SET favorite = 1 - favorite WHERE id = ?1")?
            .execute([id])?;
        Ok(self.block(id)?.favorite)
    }

    pub fn favorites(&self, ws: &str) -> Result<Vec<PageNode>> {
        let mut st = self.conn.prepare_cached(
            "SELECT id, parent_id, type, title, props FROM blocks
             WHERE workspace_id = ?1 AND favorite = 1 AND trashed IS NULL ORDER BY edited DESC",
        )?;
        Ok(st
            .query_map([ws], node_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    // ---- trash

    /// Marks the subtree trashed with one timestamp, so restore brings back exactly it.
    pub fn trash(&self, id: &str) -> Result<()> {
        self.tx(|| {
            // Unique stamp so nested trash/restore never collide.
            let t: i64 = self.conn.query_row(
                "SELECT max(coalesce(max(trashed), 0) + 1, ?1) FROM blocks",
                [now()],
                |r| r.get(0),
            )?;
            let mut st = self.conn.prepare_cached(
                "UPDATE blocks SET trashed = ?2 WHERE id = ?1 AND trashed IS NULL",
            )?;
            for sid in self.subtree_ids(id)? {
                st.execute(params![sid, t])?;
            }
            Ok(())
        })
    }

    /// Restores a trashed subtree; moves it to the root if its parent is still trashed.
    pub fn restore(&self, id: &str) -> Result<()> {
        self.tx(|| {
            let b = self.block(id)?;
            let Some(t) = b.trashed else { return Ok(()) };
            let mut st = self.conn.prepare_cached(
                "UPDATE blocks SET trashed = NULL WHERE id = ?1 AND trashed = ?2",
            )?;
            for sid in self.subtree_ids(id)? {
                st.execute(params![sid, t])?;
            }
            if let Some(p) = &b.parent_id
                && self.block(p)?.trashed.is_some()
            {
                self.move_block(id, None, usize::MAX)?;
            }
            Ok(())
        })
    }

    pub fn delete_forever(&self, id: &str) -> Result<()> {
        self.tx(|| {
            let ids = self.subtree_ids(id)?;
            let mut st = self
                .conn
                .prepare_cached("DELETE FROM blocks WHERE id = ?1")?;
            for sid in ids.iter().rev() {
                st.execute([sid])?;
            }
            Ok(())
        })
    }

    /// Top-level trashed pages, newest first.
    pub fn list_trash(&self, ws: &str) -> Result<Vec<Block>> {
        let mut st = self.conn.prepare_cached(concat!(
            "SELECT b.* FROM blocks b LEFT JOIN blocks p ON p.id = b.parent_id
             WHERE b.workspace_id = ?1 AND b.trashed IS NOT NULL AND b.type IN ",
            page_like!(),
            " AND (p.id IS NULL OR p.trashed IS NULL OR p.trashed != b.trashed) ORDER BY b.trashed DESC"
        ))?;
        Ok(st
            .query_map([ws], block_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn empty_trash(&self, ws: &str) -> Result<()> {
        self.tx(|| {
            for b in self.list_trash(ws)? {
                self.delete_forever(&b.id)?;
            }
            Ok(())
        })
    }

    // ---- duplicate

    /// Deep copy of a page (blocks, databases, views), placed right after the original.
    pub fn duplicate_page(&self, id: &str) -> Result<String> {
        self.tx(|| {
            let src = self.block(id)?;
            let mut st = self.conn.prepare_cached(
                "WITH RECURSIVE s(id, depth) AS (
                   SELECT ?1, 0 UNION ALL
                   SELECT b.id, s.depth + 1 FROM blocks b JOIN s ON b.parent_id = s.id WHERE b.trashed IS NULL)
                 SELECT b.* FROM s JOIN blocks b ON b.id = s.id ORDER BY s.depth, b.sort",
            )?;
            let blocks: Vec<Block> = st.query_map([id], block_from_row)?.collect::<rusqlite::Result<_>>()?;
            drop(st);
            let mut ids: HashMap<String, String> = blocks.iter().map(|b| (b.id.clone(), new_id())).collect();
            let mut sibs = self.sibling_sorts(&src.workspace_id, src.parent_id.as_deref(), None)?;
            let index = sibs.iter().position(|s| s.0 == id).map_or(sibs.len(), |i| i + 1);
            let root_sort = self.place("blocks", &mut sibs, index)?;
            let t = now();
            for b in &blocks {
                let mut copy = b.clone();
                copy.id = ids[&b.id].clone();
                if b.id == id {
                    copy.sort = root_sort;
                } else {
                    copy.parent_id = b.parent_id.as_ref().map(|p| ids[p].clone());
                }
                (copy.created, copy.edited, copy.favorite) = (t, t, false);
                self.insert_block(&copy)?;
                if b.kind.is_collection() {
                    self.copy_collection(&b.id, &copy.id)?;
                }
            }
            Ok(ids.remove(id).unwrap())
        })
    }

    fn copy_collection(&self, from: &str, to: &str) -> Result<()> {
        let Some(schema) = self.schema_opt(from)? else {
            return Ok(());
        };
        self.save_schema(to, &schema, true)?;
        for mut v in self.views(from)? {
            v.id = new_id();
            v.collection_id = to.to_string();
            self.insert_view(&v)?;
        }
        Ok(())
    }

    // ---- search

    /// Prefix search over block text; one hit per page, best first.
    pub fn search(&self, ws: &str, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let fts: Vec<String> = query
            .split_whitespace()
            .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
            .collect();
        if fts.is_empty() {
            return Ok(vec![]);
        }
        let mut st = self.conn.prepare_cached(
            "SELECT b.id, snippet(blocks_fts, 0, '', '', '…', 12) FROM blocks_fts
             JOIN blocks b ON b.rid = blocks_fts.rowid
             WHERE blocks_fts MATCH ?1 AND b.workspace_id = ?2 AND b.trashed IS NULL
             ORDER BY rank LIMIT 500",
        )?;
        let hits: Vec<(String, String)> = st
            .query_map(params![fts.join(" "), ws], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut page_of = self.conn.prepare_cached(concat!(
            "WITH RECURSIVE a(id, type, parent_id, title, depth) AS (
               SELECT id, type, parent_id, title, 0 FROM blocks WHERE id = ?1
               UNION ALL
               SELECT b.id, b.type, b.parent_id, b.title, a.depth + 1 FROM blocks b JOIN a ON b.id = a.parent_id
               WHERE a.type NOT IN ",
            page_like!(),
            ")
             SELECT id, title FROM a WHERE type IN ",
            page_like!(),
            " ORDER BY depth LIMIT 1"
        ))?;
        let mut out: Vec<SearchHit> = vec![];
        for (block_id, snippet) in hits {
            let page: Option<(String, String)> = page_of
                .query_row([&block_id], |r| Ok((r.get(0)?, r.get(1)?)))
                .optional()?;
            let Some((page_id, title)) = page else {
                continue;
            };
            if out.iter().any(|h| h.page_id == page_id) {
                continue;
            }
            out.push(SearchHit {
                page_id,
                title: RichText::from_json_str(&title).plain_text(),
                snippet,
            });
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    // ---- databases

    /// Database block with a `Name` title property and a default table view.
    pub fn create_database(
        &self,
        ws: &str,
        parent: Option<&str>,
        after: Option<&str>,
        full_page: bool,
    ) -> Result<Block> {
        self.tx(|| {
            let kind = if full_page {
                BlockType::CollectionViewPage
            } else {
                BlockType::CollectionView
            };
            let b = self.create_block(ws, parent, after, kind)?;
            let mut schema = Schema::new();
            schema.insert("title".into(), Property::new("Name", PropType::Title));
            self.save_schema(&b.id, &schema, true)?;
            self.create_view(&b.id, ViewType::Table, "Table")?;
            Ok(b)
        })
    }

    fn schema_opt(&self, coll: &str) -> Result<Option<Schema>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT schema FROM collections WHERE id = ?1")?;
        let s: Option<String> = st.query_row([coll], |r| r.get(0)).optional()?;
        Ok(s.map(|s| serde_json::from_str(&s)).transpose()?)
    }

    pub fn schema(&self, coll: &str) -> Result<Schema> {
        self.schema_opt(coll)?
            .ok_or_else(|| anyhow!("no collection {coll}"))
    }

    fn save_schema(&self, coll: &str, schema: &Schema, insert: bool) -> Result<()> {
        let sql = if insert {
            "INSERT INTO collections(id, schema) VALUES (?1, ?2)"
        } else {
            "UPDATE collections SET schema = ?2 WHERE id = ?1"
        };
        self.conn
            .prepare_cached(sql)?
            .execute(params![coll, serde_json::to_string(schema)?])?;
        Ok(())
    }

    /// Returns the new property id.
    pub fn add_property(&self, coll: &str, name: &str, kind: PropType) -> Result<String> {
        if kind == PropType::Title {
            bail!("a database has exactly one title property");
        }
        self.tx(|| {
            let mut schema = self.schema(coll)?;
            let id = short_id();
            schema.insert(id.clone(), Property::new(name, kind));
            self.save_schema(coll, &schema, false)?;
            Ok(id)
        })
    }

    pub fn update_property(&self, coll: &str, prop_id: &str, prop: Property) -> Result<()> {
        if (prop_id == "title") != (prop.kind == PropType::Title) {
            bail!("only the `title` property can have type title");
        }
        self.tx(|| {
            let mut schema = self.schema(coll)?;
            if !schema.contains_key(prop_id) {
                bail!("no property {prop_id}");
            }
            schema.insert(prop_id.to_string(), prop);
            self.save_schema(coll, &schema, false)
        })
    }

    pub fn remove_property(&self, coll: &str, prop_id: &str) -> Result<()> {
        if prop_id == "title" {
            bail!("the title property cannot be removed");
        }
        self.tx(|| {
            let mut schema = self.schema(coll)?;
            schema.remove(prop_id);
            self.save_schema(coll, &schema, false)?;
            for row in self.children(coll)? {
                if row
                    .props
                    .get("properties")
                    .and_then(|p| p.get(prop_id))
                    .is_some()
                {
                    self.set_row_value(&row.id, prop_id, Value::Null)?;
                }
            }
            Ok(())
        })
    }

    pub fn add_row(&self, coll: &str) -> Result<Block> {
        let ws = self.block(coll)?.workspace_id;
        self.create_block(&ws, Some(coll), None, BlockType::Page)
    }

    /// Unfiltered rows in manual order.
    pub fn rows(&self, coll: &str) -> Result<Vec<Block>> {
        Ok(self
            .children(coll)?
            .into_iter()
            .filter(|b| b.kind == BlockType::Page)
            .collect())
    }

    /// `rows` in the compact form views keep; cells are not parsed.
    pub fn db_rows(&self, coll: &str) -> Result<Vec<DbRow>> {
        let mut st = self.conn.prepare_cached(
            "SELECT id, title, coalesce(json_extract(props, '$.properties'), '{}'), created, edited
             FROM blocks WHERE parent_id = ?1 AND trashed IS NULL AND type = 'page' ORDER BY sort",
        )?;
        Ok(st
            .query_map([coll], |r| {
                let title: String = r.get(1)?;
                let cells: String = r.get(2)?;
                Ok(DbRow {
                    id: r.get(0)?,
                    title: RichText::from_json_str(&title),
                    cells: cells.into(),
                    created: r.get(3)?,
                    edited: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// (row count, max edited, sum of sorts): changes whenever rows are added, edited, moved or trashed.
    pub fn rows_stamp(&self, coll: &str) -> Result<(i64, i64, f64)> {
        Ok(self.conn.prepare_cached(
            "SELECT count(*), coalesce(max(edited), 0), coalesce(sum(sort), 0) FROM blocks WHERE parent_id = ?1 AND trashed IS NULL",
        )?
        .query_row([coll], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?)
    }

    /// Sets a row's property value (`title` sets the title); Null clears it.
    pub fn set_row_value(&self, row: &str, prop_id: &str, value: Value) -> Result<()> {
        if prop_id == "title" {
            return self.update_title(
                row,
                &RichText::from_plain(value.as_str().unwrap_or_default()),
            );
        }
        self.tx(|| {
            let mut props = self.block(row)?.props;
            let obj = props
                .as_object_mut()
                .ok_or_else(|| anyhow!("props of {row} is not an object"))?;
            let values = obj
                .entry("properties")
                .or_insert_with(|| Value::Object(Map::new()));
            let values = values
                .as_object_mut()
                .ok_or_else(|| anyhow!("properties of {row} is not an object"))?;
            if value.is_null() {
                values.remove(prop_id);
            } else {
                values.insert(prop_id.to_string(), value);
            }
            self.update_props(row, &props)
        })
    }

    /// Rows filtered and sorted by the view's query (grouped first when `group_by` is set).
    pub fn query_rows(&self, view_id: &str) -> Result<Vec<Block>> {
        let view = self.view(view_id)?;
        let schema = self.schema(&view.collection_id)?;
        Ok(apply_query(
            self.rows(&view.collection_id)?,
            &view.query,
            &schema,
        ))
    }

    // ---- views

    pub fn views(&self, coll: &str) -> Result<Vec<View>> {
        let mut st = self
            .conn
            .prepare_cached("SELECT * FROM views WHERE collection_id = ?1 ORDER BY sort")?;
        Ok(st
            .query_map([coll], view_from_row)?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn view(&self, id: &str) -> Result<View> {
        let mut st = self
            .conn
            .prepare_cached("SELECT * FROM views WHERE id = ?1")?;
        st.query_row([id], view_from_row)
            .optional()?
            .ok_or_else(|| anyhow!("no view {id}"))
    }

    fn insert_view(&self, v: &View) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT INTO views(id, collection_id, type, name, sort, query, format) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?
            .execute(params![
                v.id,
                v.collection_id,
                v.kind.as_str(),
                v.name,
                v.sort,
                serde_json::to_string(&v.query)?,
                v.format.to_string()
            ])?;
        Ok(())
    }

    pub fn create_view(&self, coll: &str, kind: ViewType, name: &str) -> Result<View> {
        let sort: f64 = self.conn.query_row(
            "SELECT coalesce(max(sort), 0) + 1 FROM views WHERE collection_id = ?1",
            [coll],
            |r| r.get(0),
        )?;
        let v = View {
            id: new_id(),
            collection_id: coll.to_string(),
            kind,
            name: name.to_string(),
            sort,
            query: Query::default(),
            format: json!({}),
        };
        self.insert_view(&v)?;
        Ok(v)
    }

    /// Saves type, name, sort, query and format.
    pub fn update_view(&self, v: &View) -> Result<()> {
        self.conn
            .prepare_cached("UPDATE views SET type = ?2, name = ?3, sort = ?4, query = ?5, format = ?6 WHERE id = ?1")?
            .execute(params![v.id, v.kind.as_str(), v.name, v.sort, serde_json::to_string(&v.query)?, v.format.to_string()])?;
        Ok(())
    }

    pub fn delete_view(&self, id: &str) -> Result<()> {
        self.conn
            .prepare_cached("DELETE FROM views WHERE id = ?1")?
            .execute([id])?;
        Ok(())
    }

    // ---- markdown

    /// `# Title` followed by the page body.
    pub fn page_to_markdown(&self, id: &str) -> Result<String> {
        let b = self.block(id)?;
        let mut out = format!("# {}\n", inline_md(&b.title));
        let body = if b.kind.is_collection() {
            let mut s = String::new();
            self.db_markdown(&b, "", &mut s)?;
            s
        } else {
            self.children_to_markdown(id)?
        };
        if !body.is_empty() {
            out.push('\n');
            out.push_str(&body);
        }
        Ok(out)
    }

    /// Markdown of a block's children (no title line).
    pub fn children_to_markdown(&self, parent: &str) -> Result<String> {
        let mut out = String::new();
        self.render_children(parent, 0, &mut out)?;
        Ok(out)
    }

    /// Markdown of the given blocks with their children, in the given order.
    pub fn blocks_to_markdown(&self, ids: &[String]) -> Result<String> {
        let blocks = ids
            .iter()
            .map(|id| self.block(id))
            .collect::<Result<Vec<_>>>()?;
        let mut out = String::new();
        self.render_blocks(&blocks, 0, &mut out)?;
        Ok(out)
    }

    fn render_children(&self, parent: &str, indent: usize, out: &mut String) -> Result<()> {
        let kids = self.children(parent)?;
        self.render_blocks(&kids, indent, out)
    }

    fn render_blocks(&self, kids: &[Block], indent: usize, out: &mut String) -> Result<()> {
        let pad = " ".repeat(indent);
        let mut num = 0;
        for (i, b) in kids.iter().enumerate() {
            num = if b.kind == BlockType::NumberedList {
                num + 1
            } else {
                0
            };
            let text = inline_md(&b.title);
            let mut child_indent = indent + 2;
            let mut recurse = true;
            match &b.kind {
                BlockType::Header => line(out, &pad, "# ", &text),
                BlockType::SubHeader => line(out, &pad, "## ", &text),
                BlockType::SubSubHeader => line(out, &pad, "### ", &text),
                BlockType::Header4 => line(out, &pad, "#### ", &text),
                BlockType::ToDo => line(
                    out,
                    &pad,
                    if b.checked() { "- [x] " } else { "- [ ] " },
                    &text,
                ),
                BlockType::BulletedList | BlockType::Toggle => line(out, &pad, "- ", &text),
                BlockType::NumberedList => {
                    let marker = format!("{num}. ");
                    child_indent = indent + marker.len();
                    line(out, &pad, &marker, &text);
                }
                BlockType::Quote => quote(out, &pad, "", &text),
                BlockType::Callout => quote(
                    out,
                    &pad,
                    &b.icon().map(|i| format!("{i} ")).unwrap_or_default(),
                    &text,
                ),
                BlockType::Code => {
                    recurse = false;
                    let lang = b.str_prop("language").unwrap_or_default();
                    out.push_str(&format!("{pad}```{lang}\n"));
                    for l in b.title.plain_text().lines() {
                        out.push_str(&format!("{pad}{l}\n"));
                    }
                    out.push_str(&format!("{pad}```\n"));
                }
                BlockType::Divider => line(out, &pad, "", "---"),
                BlockType::Image => {
                    let src = b.str_prop("source").unwrap_or_default();
                    line(
                        out,
                        &pad,
                        "",
                        &format!("![{}]({src})", b.title.plain_text()),
                    );
                }
                BlockType::Video
                | BlockType::Audio
                | BlockType::File
                | BlockType::Pdf
                | BlockType::Embed => {
                    let src = b.str_prop("source").unwrap_or_default();
                    let label = if b.title.is_empty() {
                        src.to_string()
                    } else {
                        b.title.plain_text()
                    };
                    line(out, &pad, "", &format!("[{}]({src})", escape_md(&label)));
                }
                BlockType::Bookmark => {
                    let url = b.str_prop("link").unwrap_or_default();
                    let label = if b.title.is_empty() {
                        url.to_string()
                    } else {
                        b.title.plain_text()
                    };
                    line(out, &pad, "", &format!("[{label}]({url})"));
                }
                BlockType::Equation => {
                    line(out, &pad, "", &format!("$$ {} $$", b.title.plain_text()))
                }
                BlockType::Page => {
                    recurse = false;
                    line(
                        out,
                        &pad,
                        "",
                        &format!(
                            "[{}](redbook://page/{})",
                            escape_md(&b.title.plain_text()),
                            b.id
                        ),
                    );
                }
                BlockType::CollectionView | BlockType::CollectionViewPage => {
                    recurse = false;
                    self.db_markdown(b, &pad, out)?;
                }
                BlockType::Table => {
                    recurse = false;
                    let rows: Vec<Vec<String>> = self
                        .children(&b.id)?
                        .iter()
                        .map(|r| {
                            let cells = r.props.get("cells").and_then(Value::as_array);
                            cells
                                .into_iter()
                                .flatten()
                                .map(|c| inline_md(&RichText::from(c.clone())))
                                .collect()
                        })
                        .collect();
                    md_table(out, &pad, &rows);
                }
                BlockType::TableOfContents | BlockType::ColumnList | BlockType::Column => {
                    child_indent = indent
                }
                _ => {
                    if !text.is_empty() || b.kind == BlockType::Text {
                        line(out, &pad, "", &text)
                    }
                }
            }
            if recurse {
                self.render_children(&b.id, child_indent, out)?;
            }
            let list = matches!(
                b.kind,
                BlockType::BulletedList
                    | BlockType::NumberedList
                    | BlockType::ToDo
                    | BlockType::Toggle
            );
            if let Some(next) = kids.get(i + 1)
                && !(list && next.kind == b.kind)
            {
                out.push('\n');
            }
        }
        Ok(())
    }

    /// Database as `title` + markdown table of the first view's rows.
    fn db_markdown(&self, b: &Block, pad: &str, out: &mut String) -> Result<()> {
        let Some(schema) = self.schema_opt(&b.id)? else {
            return Ok(());
        };
        let rows = match self.views(&b.id)?.first() {
            Some(v) => self.query_rows(&v.id)?,
            None => self.rows(&b.id)?,
        };
        let mut props: Vec<(&String, &Property)> = schema.iter().collect();
        props.sort_by_key(|(id, _)| *id != "title");
        if !b.title.is_empty() {
            out.push_str(&format!("{pad}{}\n\n", inline_md(&b.title)));
        }
        let mut table = vec![
            props
                .iter()
                .map(|(_, p)| p.name.clone())
                .collect::<Vec<_>>(),
        ];
        for r in &rows {
            table.push(
                props
                    .iter()
                    .map(|(id, p)| value_text(&r.row_value(id, p)))
                    .collect(),
            );
        }
        md_table(out, pad, &table);
        Ok(())
    }

    /// Parses markdown into blocks appended under `parent`; returns the top-level ids.
    pub fn markdown_to_blocks(&self, parent: &str, md: &str) -> Result<Vec<String>> {
        self.tx(|| {
            let ws = self.block(parent)?.workspace_id;
            let mut imp = Importer {
                store: self,
                ws,
                root: parent.to_string(),
                next_sort: HashMap::new(),
                stack: vec![],
                pending: None,
                top: vec![],
            };
            imp.run(md)?;
            Ok(imp.top)
        })
    }
}

/// Paragraph/quote lines waiting to be merged into one block.
struct Pending {
    kind: BlockType,
    indent: usize,
    lines: Vec<String>,
}

struct Importer<'a> {
    store: &'a Store,
    ws: String,
    root: String,
    next_sort: HashMap<String, f64>,
    /// (indent, block id) of open containers.
    stack: Vec<(usize, String)>,
    pending: Option<Pending>,
    top: Vec<String>,
}

impl Importer<'_> {
    fn run(&mut self, md: &str) -> Result<()> {
        let lines: Vec<&str> = md.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            i = self.line(&lines, i)?;
        }
        self.flush()
    }

    /// `<details>` (toggle) or `<aside>` (callout) starting at line `i`; returns the next unread line.
    fn html_block(&mut self, lines: &[&str], i: usize, indent: usize, tag: &str) -> Result<usize> {
        let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
        let (mut depth, mut j, mut body) = (0i64, i, vec![]);
        while j < lines.len() {
            let l = lines[j];
            depth += l.matches(&open).count() as i64 - l.matches(&close).count() as i64;
            body.push(l);
            j += 1;
            if depth <= 0 {
                break;
            }
        }
        let text = body.join("\n");
        let inner = text.trim();
        let inner = inner[inner.find('>').map_or(0, |k| k + 1)..].trim();
        let inner = inner.strip_suffix(close.as_str()).unwrap_or(inner).trim();
        let (kind, title, rest, mut props) = if tag == "details" {
            let (title, rest) = inner
                .strip_prefix("<summary>")
                .and_then(|r| r.split_once("</summary>"))
                .unwrap_or(("", inner));
            (BlockType::Toggle, title.trim(), rest, json!({}))
        } else {
            let (first, rest) = inner.split_once("\n\n").unwrap_or((inner, ""));
            (BlockType::Callout, first.trim(), rest, json!({}))
        };
        let mut title = title;
        // Callouts start with their emoji icon.
        if kind == BlockType::Callout
            && let Some((icon, r)) = title.split_once(char::is_whitespace)
            && icon.chars().next().is_some_and(|c| !c.is_ascii())
        {
            props["icon"] = json!(icon);
            title = r.trim();
        }
        let id = self.add(indent, kind, parse_inline(title), props)?;
        self.stack.pop();
        let mut sub = Importer {
            store: self.store,
            ws: self.ws.clone(),
            root: id,
            next_sort: HashMap::new(),
            stack: vec![],
            pending: None,
            top: vec![],
        };
        sub.run(rest)?;
        Ok(j)
    }

    /// Handles the line at `i`, returns the index of the next unread line.
    fn line(&mut self, lines: &[&str], i: usize) -> Result<usize> {
        let raw = lines[i].replace('\t', "    ");
        let content = raw.trim_start();
        if content.is_empty() {
            self.flush()?;
            return Ok(i + 1);
        }
        let indent = raw.len() - content.len();
        let (kind, text) = if let Some(q) = content.strip_prefix('>') {
            (BlockType::Quote, q.strip_prefix(' ').unwrap_or(q))
        } else {
            (BlockType::Text, content)
        };
        if kind == BlockType::Text
            && let Some(tag) = ["details", "aside"]
                .into_iter()
                .find(|t| content.starts_with(&format!("<{t}")))
        {
            self.flush()?;
            while self.stack.last().is_some_and(|(ind, _)| *ind >= indent) {
                self.stack.pop();
            }
            return self.html_block(lines, i, indent, tag);
        }
        let starts_block = kind == BlockType::Text && block_marker(content);
        if let Some(p) = &mut self.pending
            && p.kind == kind
            && p.indent == indent
            && !starts_block
        {
            p.lines.push(text.to_string());
            return Ok(i + 1);
        }
        self.flush()?;
        while self.stack.last().is_some_and(|(ind, _)| *ind >= indent) {
            self.stack.pop();
        }
        if !starts_block {
            self.pending = Some(Pending {
                kind,
                indent,
                lines: vec![text.to_string()],
            });
            return Ok(i + 1);
        }

        if let Some(lang) = content.strip_prefix("```") {
            let mut j = i + 1;
            let mut body = vec![];
            while j < lines.len() && !lines[j].trim_start().starts_with("```") {
                let l = lines[j];
                let strip = l.len() - l.trim_start().len();
                body.push(&l[strip.min(indent)..]);
                j += 1;
            }
            let rt = RichText::from_plain(&body.join("\n"));
            self.add(
                indent,
                BlockType::Code,
                rt,
                json!({ "language": lang.trim() }),
            )?;
            return Ok(j + 1);
        }
        if content.starts_with('|') {
            let mut j = i;
            let mut rows = vec![];
            while j < lines.len() && lines[j].trim_start().starts_with('|') {
                let cells = split_row(lines[j].trim());
                if !cells
                    .iter()
                    .all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
                {
                    rows.push(cells);
                }
                j += 1;
            }
            let width = rows.iter().map(Vec::len).max().unwrap_or(0);
            let table = self.add(
                indent,
                BlockType::Table,
                RichText::default(),
                json!({ "width": width, "header": true }),
            )?;
            for cells in rows {
                let cells: Vec<Value> =
                    cells.iter().map(|c| Value::from(parse_inline(c))).collect();
                self.insert(
                    &table,
                    BlockType::TableRow,
                    RichText::default(),
                    json!({ "cells": cells }),
                )?;
            }
            return Ok(j);
        }

        let (kind, rest, props) = classify(content);
        self.add(indent, kind, parse_inline(rest), props)?;
        Ok(i + 1)
    }

    fn flush(&mut self) -> Result<()> {
        if let Some(p) = self.pending.take() {
            let rt = parse_inline(&p.lines.join("\n"));
            while self.stack.last().is_some_and(|(ind, _)| *ind >= p.indent) {
                self.stack.pop();
            }
            self.add(p.indent, p.kind, rt, json!({}))?;
        }
        Ok(())
    }

    /// Adds a block under the innermost container and opens it for deeper lines.
    fn add(
        &mut self,
        indent: usize,
        kind: BlockType,
        title: RichText,
        props: Value,
    ) -> Result<String> {
        let parent = self
            .stack
            .last()
            .map(|(_, id)| id.clone())
            .unwrap_or_else(|| self.root.clone());
        let id = self.insert(&parent, kind, title, props)?;
        if parent == self.root {
            self.top.push(id.clone());
        }
        self.stack.push((indent, id.clone()));
        Ok(id)
    }

    fn insert(
        &mut self,
        parent: &str,
        kind: BlockType,
        title: RichText,
        props: Value,
    ) -> Result<String> {
        let sort = match self.next_sort.get(parent) {
            Some(s) => *s,
            None => self
                .store
                .sibling_sorts(&self.ws, Some(parent), None)?
                .last()
                .map_or(1.0, |s| s.1 + 1.0),
        };
        self.next_sort.insert(parent.to_string(), sort + 1.0);
        let t = now();
        let b = Block {
            id: new_id(),
            workspace_id: self.ws.clone(),
            parent_id: Some(parent.to_string()),
            kind,
            sort,
            title,
            props,
            created: t,
            edited: t,
            trashed: None,
            favorite: false,
        };
        self.store.insert_block(&b)?;
        Ok(b.id)
    }
}

/// True when a line starts a non-paragraph block.
fn block_marker(s: &str) -> bool {
    s.starts_with("```") || s.starts_with('|') || classify(s).0 != BlockType::Text
}

/// Block type, remaining text and props of a single markdown line.
fn classify(s: &str) -> (BlockType, &str, Value) {
    let t = s.trim_end();
    if matches!(t, "---" | "***" | "___") {
        return (BlockType::Divider, "", json!({}));
    }
    for (prefix, kind) in [
        ("#### ", BlockType::Header4),
        ("### ", BlockType::SubSubHeader),
        ("## ", BlockType::SubHeader),
        ("# ", BlockType::Header),
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return (kind, rest, json!({}));
        }
    }
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = s.strip_prefix(bullet) {
            for (box_, checked) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] {
                if let Some(r) = rest.strip_prefix(box_) {
                    return (BlockType::ToDo, r, json!({ "checked": checked }));
                }
            }
            return (BlockType::BulletedList, rest, json!({}));
        }
    }
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && s[digits..].starts_with(". ") {
        return (BlockType::NumberedList, &s[digits + 2..], json!({}));
    }
    if let Some(eq) = t.strip_prefix("$$").and_then(|r| r.strip_suffix("$$"))
        && t.len() >= 4
    {
        return (BlockType::Equation, eq.trim(), json!({}));
    }
    if let Some(rest) = t.strip_prefix("![")
        && let Some((alt, url)) = rest.split_once("](")
        && let Some(url) = url.strip_suffix(')')
    {
        return (BlockType::Image, alt, json!({ "source": url }));
    }
    (BlockType::Text, s, json!({}))
}

fn split_row(s: &str) -> Vec<String> {
    let inner = s.trim_matches('|');
    let mut cells = vec![String::new()];
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cells.last_mut().unwrap().push(chars.next().unwrap())
            }
            '|' => cells.push(String::new()),
            _ => cells.last_mut().unwrap().push(c),
        }
    }
    cells.into_iter().map(|c| c.trim().to_string()).collect()
}

fn line(out: &mut String, pad: &str, marker: &str, text: &str) {
    let mut lines = text.split('\n');
    out.push_str(&format!(
        "{pad}{marker}{}\n",
        lines.next().unwrap_or_default()
    ));
    let cont = " ".repeat(marker.chars().count());
    for l in lines {
        out.push_str(&format!("{pad}{cont}{l}\n"));
    }
}

fn quote(out: &mut String, pad: &str, prefix: &str, text: &str) {
    for (i, l) in text.split('\n').enumerate() {
        let p = if i == 0 { prefix } else { "" };
        out.push_str(&format!("{pad}> {p}{l}\n"));
    }
}

fn md_table(out: &mut String, pad: &str, rows: &[Vec<String>]) {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return;
    }
    for (i, row) in rows.iter().enumerate() {
        let cells: Vec<String> = (0..width)
            .map(|c| {
                row.get(c)
                    .map_or(String::new(), |s| s.replace('|', "\\|").replace('\n', " "))
            })
            .collect();
        out.push_str(&format!("{pad}| {} |\n", cells.join(" | ")));
        if i == 0 {
            out.push_str(&format!("{pad}|{}\n", " --- |".repeat(width)));
        }
    }
}

/// Database cell as display text.
pub fn value_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => if *b { "Yes" } else { "No" }.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(a) => a.iter().map(value_text).collect::<Vec<_>>().join(", "),
        Value::Object(o) => match (o.get("start"), o.get("end")) {
            (Some(s), Some(e)) => format!("{} → {}", value_text(s), value_text(e)),
            (Some(s), None) => value_text(s),
            _ => v.to_string(),
        },
    }
}

// ---- inline markdown

fn escape_md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '*' | '`' | '[' | ']' | '~') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn inline_md(rt: &RichText) -> String {
    let mut out = String::new();
    for seg in &rt.clone().normalized().0 {
        let mut s = if seg.has(&Annotation::Code) {
            format!("`{}`", seg.text)
        } else {
            escape_md(&seg.text)
        };
        if seg.has(&Annotation::Strike) {
            s = format!("~~{s}~~");
        }
        if seg.has(&Annotation::Italic) {
            s = format!("*{s}*");
        }
        if seg.has(&Annotation::Bold) {
            s = format!("**{s}**");
        }
        if let Some(url) = seg.link() {
            s = format!("[{s}]({url})");
        }
        out.push_str(&s);
    }
    out
}

fn ann_rank(a: &Annotation) -> u8 {
    match a {
        Annotation::Bold => 0,
        Annotation::Italic => 1,
        Annotation::Strike => 2,
        Annotation::Underline => 3,
        Annotation::Code => 4,
        Annotation::Color(_) => 5,
        Annotation::Link(_) => 6,
        _ => 7,
    }
}

fn canon(mut v: Vec<Annotation>) -> Vec<Annotation> {
    v.sort_by_key(ann_rank);
    v.dedup();
    v
}

/// Parses **bold**, *italic*, ~~strike~~, `code` and [text](url).
fn parse_inline(s: &str) -> RichText {
    let chars: Vec<char> = s.chars().collect();
    let mut segs: Vec<Segment> = vec![];
    let mut buf = String::new();
    let (mut bold, mut italic, mut strike) = (false, false, false);
    let anns = |b: bool, i: bool, s: bool| {
        let mut v = vec![];
        if b {
            v.push(Annotation::Bold)
        }
        if i {
            v.push(Annotation::Italic)
        }
        if s {
            v.push(Annotation::Strike)
        }
        v
    };
    let at = |i: usize, pat: &str| {
        pat.chars()
            .enumerate()
            .all(|(k, c)| chars.get(i + k) == Some(&c))
    };
    // Opening needs a non-space after, closing a non-space before.
    let can_toggle = |i: usize, len: usize, active: bool| {
        if active {
            i > 0 && !chars[i - 1].is_whitespace()
        } else {
            chars.get(i + len).is_some_and(|c| !c.is_whitespace())
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let flush = |buf: &mut String, segs: &mut Vec<Segment>, a: Vec<Annotation>| {
            if !buf.is_empty() {
                segs.push(Segment {
                    text: std::mem::take(buf),
                    annotations: a,
                });
            }
        };
        if c == '\\' && i + 1 < chars.len() {
            buf.push(chars[i + 1]);
            i += 2;
        } else if at(i, "**") && can_toggle(i, 2, bold) {
            flush(&mut buf, &mut segs, anns(bold, italic, strike));
            bold = !bold;
            i += 2;
        } else if at(i, "~~") && can_toggle(i, 2, strike) {
            flush(&mut buf, &mut segs, anns(bold, italic, strike));
            strike = !strike;
            i += 2;
        } else if c == '*' && can_toggle(i, 1, italic) {
            flush(&mut buf, &mut segs, anns(bold, italic, strike));
            italic = !italic;
            i += 1;
        } else if c == '`'
            && let Some(end) = chars[i + 1..].iter().position(|&ch| ch == '`')
        {
            flush(&mut buf, &mut segs, anns(bold, italic, strike));
            let mut a = anns(bold, italic, strike);
            a.push(Annotation::Code);
            segs.push(Segment {
                text: chars[i + 1..i + 1 + end].iter().collect(),
                annotations: canon(a),
            });
            i += end + 2;
        } else if c == '['
            && let Some((text, url, len)) = parse_link(&chars[i..])
        {
            flush(&mut buf, &mut segs, anns(bold, italic, strike));
            for seg in parse_inline(&text).0 {
                let mut a = anns(bold, italic, strike);
                a.extend(seg.annotations);
                a.push(Annotation::Link(url.clone()));
                segs.push(Segment {
                    text: seg.text,
                    annotations: canon(a),
                });
            }
            i += len;
        } else {
            buf.push(c);
            i += 1;
        }
    }
    if !buf.is_empty() {
        segs.push(Segment {
            text: buf,
            annotations: anns(bold, italic, strike),
        });
    }
    RichText(segs).normalized()
}

/// `[text](url)` at the start of `chars`: (text, url, consumed chars).
fn parse_link(chars: &[char]) -> Option<(String, String, usize)> {
    let mut depth = 0;
    let mut close = None;
    let mut k = 0;
    while k < chars.len() {
        match chars[k] {
            '\\' => k += 1,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(k);
                    break;
                }
            }
            _ => {}
        }
        k += 1;
    }
    let close = close?;
    if chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = close + 2 + chars[close + 2..].iter().position(|&c| c == ')')?;
    Some((
        chars[1..close].iter().collect(),
        chars[close + 2..end].iter().collect(),
        end + 1,
    ))
}

// ---- query evaluation

/// Filters and sorts rows by a view query.
pub fn apply_query<R: RowValue>(rows: Vec<R>, q: &Query, schema: &Schema) -> Vec<R> {
    let rows = rows.into_iter().filter(|r| {
        let results: Vec<bool> = q
            .filters
            .iter()
            .filter_map(|f| {
                Some(filter_match(
                    &r.row_value(&f.property, schema.get(&f.property)?),
                    f,
                ))
            })
            .collect();
        match q.operator {
            FilterOperator::And => results.iter().all(|m| *m),
            FilterOperator::Or => results.is_empty() || results.iter().any(|m| *m),
        }
    });
    // Sort columns: the group (no empty-last rule), then the sorts; None direction = group.
    let group = q
        .group_by
        .as_ref()
        .and_then(|g| schema.get(g).map(|p| (g, p, None)));
    let cols: Vec<(&String, &Property, Option<Direction>)> = group
        .into_iter()
        .chain(q.sorts.iter().filter_map(|s| {
            schema
                .get(&s.property)
                .map(|p| (&s.property, p, Some(s.direction)))
        }))
        .collect();
    if cols.is_empty() {
        return rows.collect();
    }
    // Keys read once per row: `row_value` may parse.
    let mut keyed: Vec<(Vec<Value>, R)> = rows
        .map(|r| {
            (
                cols.iter().map(|(id, p, _)| r.row_value(id, p)).collect(),
                r,
            )
        })
        .collect();
    keyed.sort_by(|(ka, _), (kb, _)| {
        for ((va, vb), (_, p, dir)) in ka.iter().zip(kb).zip(&cols) {
            let ord = match dir {
                None => cmp_prop(va, vb, p),
                Some(d) => match (empty(va), empty(vb)) {
                    (true, false) => Ordering::Greater,
                    (false, true) => Ordering::Less,
                    _ if *d == Direction::Descending => cmp_prop(va, vb, p).reverse(),
                    _ => cmp_prop(va, vb, p),
                },
            };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        Ordering::Equal
    });
    keyed.into_iter().map(|(_, r)| r).collect()
}

/// Date objects compare by their start.
fn scalar(v: &Value) -> &Value {
    v.get("start").unwrap_or(v)
}

fn empty(v: &Value) -> bool {
    match scalar(v) {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        _ => false,
    }
}

fn lower(v: &Value) -> String {
    value_text(scalar(v)).to_lowercase()
}

fn cmp_values(a: &Value, b: &Value) -> Ordering {
    match (scalar(a), scalar(b)) {
        (Value::Number(x), Value::Number(y)) => x
            .as_f64()
            .unwrap_or(0.)
            .partial_cmp(&y.as_f64().unwrap_or(0.))
            .unwrap_or(Ordering::Equal),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (x, y) => lower(x).cmp(&lower(y)),
    }
}

/// Select/status sort by option order, everything else by value.
fn cmp_prop(a: &Value, b: &Value, p: &Property) -> Ordering {
    if matches!(p.kind, PropType::Select | PropType::Status) {
        let idx = |v: &Value| {
            v.as_str()
                .and_then(|s| p.option_index(s))
                .unwrap_or(usize::MAX)
        };
        return idx(a).cmp(&idx(b));
    }
    cmp_values(a, b)
}

fn eq_loose(a: &Value, b: &Value) -> bool {
    match (scalar(a), scalar(b)) {
        (Value::Number(_), Value::Number(_)) | (Value::Bool(_), Value::Bool(_)) => {
            cmp_values(a, b).is_eq()
        }
        (Value::Bool(x), Value::Null) | (Value::Null, Value::Bool(x)) => !x,
        _ => lower(a) == lower(b),
    }
}

fn filter_match(v: &Value, f: &Filter) -> bool {
    let arr = v.as_array();
    let has = |x: &Value| arr.map_or(eq_loose(v, x), |a| a.iter().any(|e| eq_loose(e, x)));
    let contains = || arr.map_or_else(|| lower(v).contains(&lower(&f.value)), |_| has(&f.value));
    let cmp = || (!empty(v)).then(|| cmp_values(v, &f.value));
    match f.op {
        FilterOp::Is => has(&f.value),
        FilterOp::IsNot => !has(&f.value),
        FilterOp::Contains => contains(),
        FilterOp::DoesNotContain => !contains(),
        FilterOp::StartsWith => lower(v).starts_with(&lower(&f.value)),
        FilterOp::EndsWith => lower(v).ends_with(&lower(&f.value)),
        FilterOp::IsEmpty => empty(v),
        FilterOp::IsNotEmpty => !empty(v),
        FilterOp::GreaterThan => cmp().is_some_and(Ordering::is_gt),
        FilterOp::LessThan => cmp().is_some_and(Ordering::is_lt),
        FilterOp::GreaterThanOrEqualTo => cmp().is_some_and(Ordering::is_ge),
        FilterOp::LessThanOrEqualTo => cmp().is_some_and(Ordering::is_le),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Store, String) {
        let s = Store::open_in_memory().unwrap();
        let ws = s.create_workspace("Personal").unwrap().id;
        (s, ws)
    }

    fn page(s: &Store, ws: &str, parent: Option<&str>, title: &str) -> String {
        let b = s.create_block(ws, parent, None, BlockType::Page).unwrap();
        s.update_title(&b.id, &RichText::from_plain(title)).unwrap();
        b.id
    }

    fn text(s: &Store, ws: &str, parent: &str, t: &str) -> String {
        let b = s
            .create_block(ws, Some(parent), None, BlockType::Text)
            .unwrap();
        s.update_title(&b.id, &RichText::from_plain(t)).unwrap();
        b.id
    }

    fn ids(blocks: &[Block]) -> Vec<String> {
        blocks.iter().map(|b| b.id.clone()).collect()
    }

    #[test]
    fn rich_text_format() {
        let json = r#"[["plain"],["bold link",[["b"],["a","https://x.dev"]]]]"#;
        let rt: RichText = serde_json::from_str(json).unwrap();
        assert_eq!(rt.plain_text(), "plainbold link");
        assert_eq!(rt.0[1].link(), Some("https://x.dev"));
        assert_eq!(serde_json::to_string(&rt).unwrap(), json);
        let kind: BlockType = serde_json::from_str("\"sub_header\"").unwrap();
        assert_eq!(kind, BlockType::SubHeader);
        assert_eq!(
            BlockType::parse("alias"),
            BlockType::Unknown("alias".into())
        );
    }

    #[test]
    fn create_move_and_tree() {
        let (s, ws) = setup();
        let a = page(&s, &ws, None, "A");
        let b = page(&s, &ws, None, "B");
        let t1 = text(&s, &ws, &a, "one");
        let t2 = text(&s, &ws, &a, "two");
        let t3 = text(&s, &ws, &a, "three");
        let t0 = s
            .create_block(&ws, Some(&a), Some(&t1), BlockType::Text)
            .unwrap()
            .id;
        assert_eq!(
            ids(&s.children(&a).unwrap()),
            [t1.clone(), t0, t2.clone(), t3.clone()]
        );

        s.move_block(&t3, Some(&a), 0).unwrap();
        let order = ids(&s.children(&a).unwrap());
        assert_eq!([&order[0], &order[1], &order[3]], [&t3, &t1, &t2]);
        s.move_block(&t1, Some(&b), 0).unwrap();
        assert_eq!(ids(&s.children(&b).unwrap()), std::slice::from_ref(&t1));

        // Repeated inserts at the same spot exhaust the gap and force renumbering.
        let first = s.children(&a).unwrap()[0].id.clone();
        for _ in 0..80 {
            s.create_block(&ws, Some(&a), Some(&first), BlockType::Text)
                .unwrap();
        }
        let sorts: Vec<f64> = s.children(&a).unwrap().iter().map(|b| b.sort).collect();
        assert!(sorts.windows(2).all(|w| w[0] < w[1]));

        let sub = page(&s, &ws, Some(&a), "Sub");
        assert!(s.move_block(&a, Some(&sub), 0).is_err());
        let toggle = s
            .create_block(&ws, Some(&b), None, BlockType::Toggle)
            .unwrap()
            .id;
        let nested = page(&s, &ws, Some(&toggle), "Nested");

        let tree = s.page_tree(&ws).unwrap();
        let parent_of = |id: &str| tree.iter().find(|n| n.id == id).unwrap().parent_id.clone();
        assert_eq!(tree.len(), 4);
        assert_eq!(parent_of(&sub), Some(a.clone()));
        assert_eq!(parent_of(&nested), Some(b.clone()));
        let crumbs: Vec<String> = s
            .ancestors(&nested)
            .unwrap()
            .into_iter()
            .map(|n| n.title)
            .collect();
        assert_eq!(crumbs, ["B", "Nested"]);

        s.move_block(&b, None, 0).unwrap();
        let roots: Vec<String> = s
            .page_tree(&ws)
            .unwrap()
            .into_iter()
            .filter(|n| n.parent_id.is_none())
            .map(|n| n.id)
            .collect();
        assert_eq!(roots, [b.clone(), a.clone()]);
        assert!(s.toggle_favorite(&sub).unwrap());
        assert_eq!(s.favorites(&ws).unwrap()[0].id, sub);
    }

    #[test]
    fn trash_restore_delete() {
        let (s, ws) = setup();
        let a = page(&s, &ws, None, "Alpha");
        let child = page(&s, &ws, Some(&a), "Child");
        text(&s, &ws, &child, "zebra");
        s.trash(&a).unwrap();
        assert!(s.page_tree(&ws).unwrap().is_empty());
        assert_eq!(ids(&s.list_trash(&ws).unwrap()), std::slice::from_ref(&a));
        assert!(s.search(&ws, "zebra", 10).unwrap().is_empty());

        s.restore(&a).unwrap();
        assert_eq!(s.page_tree(&ws).unwrap().len(), 2);

        // Restoring a child whose parent is still trashed moves it to the root.
        s.trash(&child).unwrap();
        s.trash(&a).unwrap();
        assert_eq!(s.list_trash(&ws).unwrap().len(), 2);
        s.restore(&child).unwrap();
        assert_eq!(s.block(&child).unwrap().parent_id, None);

        s.delete_forever(&child).unwrap();
        assert!(s.block(&child).is_err());
        assert!(s.search(&ws, "zebra", 10).unwrap().is_empty());
        s.empty_trash(&ws).unwrap();
        assert!(s.block(&a).is_err());
    }

    #[test]
    fn search_prefix_and_snippet() {
        let (s, ws) = setup();
        let p = page(&s, &ws, None, "Rust notes");
        let toggle = s
            .create_block(&ws, Some(&p), None, BlockType::Toggle)
            .unwrap()
            .id;
        text(&s, &ws, &toggle, "ownership and the borrow checker");
        page(&s, &ws, None, "Groceries");

        let hits = s.search(&ws, "borr", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].page_id, p);
        assert_eq!(hits[0].title, "Rust notes");
        assert!(hits[0].snippet.contains("borrow"));
        assert_eq!(s.search(&ws, "rust no", 10).unwrap()[0].page_id, p);
        assert!(s.search(&ws, "\"quoted", 10).unwrap().is_empty());

        let other = s.create_workspace("Work").unwrap().id;
        assert!(s.search(&other, "rust", 10).unwrap().is_empty());
    }

    #[test]
    fn duplicate_deep_copy() {
        let (s, ws) = setup();
        let p = page(&s, &ws, None, "Original");
        let next = page(&s, &ws, None, "Next");
        let list = s
            .create_block(&ws, Some(&p), None, BlockType::BulletedList)
            .unwrap()
            .id;
        text(&s, &ws, &list, "nested");
        let db = s.create_database(&ws, Some(&p), None, false).unwrap().id;
        s.add_property(&db, "Points", PropType::Number).unwrap();
        s.add_row(&db).unwrap();

        let copy = s.duplicate_page(&p).unwrap();
        let roots: Vec<String> = s
            .page_tree(&ws)
            .unwrap()
            .into_iter()
            .filter(|n| n.parent_id.is_none())
            .map(|n| n.id)
            .collect();
        assert_eq!(roots, [p.clone(), copy.clone(), next]);
        let kids = s.children(&copy).unwrap();
        assert_eq!(kids.len(), 2);
        assert_ne!(kids[0].id, list);
        assert_eq!(
            s.children(&kids[0].id).unwrap()[0].title.plain_text(),
            "nested"
        );
        let db_copy = &kids[1].id;
        assert_eq!(s.schema(db_copy).unwrap().len(), 2);
        assert_eq!(s.views(db_copy).unwrap().len(), 1);
        assert_eq!(s.rows(db_copy).unwrap().len(), 1);
        assert_eq!(s.search(&ws, "original", 10).unwrap().len(), 2);
    }

    #[test]
    fn database_query() {
        let (s, ws) = setup();
        let db = s.create_database(&ws, None, None, true).unwrap().id;
        let status = s.add_property(&db, "Status", PropType::Select).unwrap();
        let points = s.add_property(&db, "Points", PropType::Number).unwrap();
        let tags = s.add_property(&db, "Tags", PropType::MultiSelect).unwrap();
        let mut p = s.schema(&db).unwrap()[&status].clone();
        p.options = ["Todo", "Doing", "Done"]
            .iter()
            .map(|v| SelectOption {
                id: short_id(),
                value: v.to_string(),
                color: "gray".into(),
            })
            .collect();
        s.update_property(&db, &status, p).unwrap();

        for (name, st, pts, tag) in [
            ("a", "Done", 5, "x"),
            ("b", "Todo", 1, "y"),
            ("c", "Doing", 3, "x"),
            ("d", "Todo", 8, "z"),
        ] {
            let r = s.add_row(&db).unwrap().id;
            s.set_row_value(&r, "title", json!(name)).unwrap();
            s.set_row_value(&r, &status, json!(st)).unwrap();
            s.set_row_value(&r, &points, json!(pts)).unwrap();
            s.set_row_value(&r, &tags, json!([tag])).unwrap();
        }
        let titles = |v: &str| -> Vec<String> {
            s.query_rows(v)
                .unwrap()
                .iter()
                .map(|b| b.title.plain_text())
                .collect()
        };
        let mut view = s.views(&db).unwrap().remove(0);
        view.query.filters = vec![Filter {
            property: points.clone(),
            op: FilterOp::GreaterThan,
            value: json!(2),
        }];
        view.query.sorts = vec![Sort {
            property: points.clone(),
            direction: Direction::Descending,
        }];
        s.update_view(&view).unwrap();
        assert_eq!(titles(&view.id), ["d", "a", "c"]);

        view.query.filters = vec![Filter {
            property: tags.clone(),
            op: FilterOp::Contains,
            value: json!("x"),
        }];
        view.query.sorts = vec![Sort {
            property: "title".into(),
            direction: Direction::Ascending,
        }];
        s.update_view(&view).unwrap();
        assert_eq!(titles(&view.id), ["a", "c"]);

        let board = s.create_view(&db, ViewType::Board, "Board").unwrap();
        let mut board = s.view(&board.id).unwrap();
        board.query.group_by = Some(status.clone());
        board.query.sorts = vec![Sort {
            property: points.clone(),
            direction: Direction::Ascending,
        }];
        s.update_view(&board).unwrap();
        assert_eq!(titles(&board.id), ["b", "d", "c", "a"]);
        // Compact rows answer like parsed blocks.
        let schema = s.schema(&db).unwrap();
        let compact: Vec<String> = apply_query(s.db_rows(&db).unwrap(), &board.query, &schema)
            .iter()
            .map(|r| r.title.plain_text())
            .collect();
        assert_eq!(compact, ["b", "d", "c", "a"]);
        for (r, b) in s.db_rows(&db).unwrap().iter().zip(s.rows(&db).unwrap()) {
            for (id, p) in &schema {
                assert_eq!(r.row_value(id, p), b.row_value(id, p));
            }
        }

        s.remove_property(&db, &tags).unwrap();
        assert!(
            s.rows(&db)
                .unwrap()
                .iter()
                .all(|r| r.props["properties"].get(&tags).is_none())
        );
        assert!(s.remove_property(&db, "title").is_err());
        s.delete_view(&board.id).unwrap();
        assert_eq!(s.views(&db).unwrap().len(), 1);

        let md = s.page_to_markdown(&db).unwrap();
        assert!(md.contains("| Name |") && md.contains("| a |"), "{md}");

        let stamp = s.rows_stamp(&db).unwrap();
        assert_eq!(stamp.0, 4);
        let first = s.rows(&db).unwrap()[0].id.clone();
        s.trash(&first).unwrap();
        assert_ne!(s.rows_stamp(&db).unwrap(), stamp);
    }

    #[test]
    fn selected_blocks_markdown() {
        let (s, ws) = setup();
        let p = page(&s, &ws, None, "Doc");
        let ids = s
            .markdown_to_blocks(&p, "intro\n\n- one\n  - nested\n- two\n\nend\n")
            .unwrap();
        assert_eq!(
            s.blocks_to_markdown(&ids[1..3]).unwrap(),
            "- one\n  - nested\n- two\n"
        );
    }

    #[test]
    fn markdown_round_trip() {
        let (s, ws) = setup();
        let p = page(&s, &ws, None, "Doc");
        let md = "## Heading

Hello **bold** and *italic* with `code`, ~~gone~~ and [a link](https://x.dev)
second line

- one
  - nested **deep**
- two

1. first
2. second
   1. inner

- [ ] todo
- [x] done

> a quote
> continues

```rust
fn main() {
    println!(\"hi\");
}
```

---

![cat](https://img.dev/cat.png)

$$ e = mc^2 $$

| A | B |
| --- | --- |
| 1 | **2** |
";
        let top = s.markdown_to_blocks(&p, md).unwrap();
        assert_eq!(top.len(), 14);
        assert_eq!(s.children_to_markdown(&p).unwrap(), md);

        let kids = s.children(&p).unwrap();
        assert_eq!(kids[0].kind, BlockType::SubHeader);
        assert_eq!(kids[1].title.0[1].annotations, [Annotation::Bold]);
        assert_eq!(kids[9].kind, BlockType::Code);
        assert_eq!(kids[9].str_prop("language"), Some("rust"));
        assert!(
            s.page_to_markdown(&p)
                .unwrap()
                .starts_with("# Doc\n\n## Heading\n")
        );
        assert_eq!(s.search(&ws, "nested deep", 5).unwrap()[0].page_id, p);
    }

    #[test]
    fn table_markdown_round_trip() {
        let (s, ws) = setup();
        let p = page(&s, &ws, None, "T");
        let md = "| Key | Value |\n| --- | --- |\n| a \\| b | *x* |\n| c |  |\n";
        s.markdown_to_blocks(&p, md).unwrap();
        assert_eq!(s.children_to_markdown(&p).unwrap(), md);

        let table = s.children(&p).unwrap()[0].clone();
        assert_eq!(table.kind, BlockType::Table);
        assert_eq!(table.props["header"], json!(true));
        let rows = s.children(&table.id).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].cells()[0].plain_text(), "a | b");

        s.add_table_column(&table.id, 1).unwrap();
        s.add_table_row(&table.id, 1).unwrap();
        s.delete_table_column(&table.id, 2).unwrap();
        assert_eq!(
            s.children_to_markdown(&p).unwrap(),
            "| Key |  |\n| --- | --- |\n|  |  |\n| a \\| b |  |\n| c |  |\n"
        );
    }

    #[test]
    fn file_store_and_data_version() {
        let dir = std::env::temp_dir().join(format!("redbook-test-{}", short_id()));
        let path = dir.join("test.db");
        let a = Store::open(&path).unwrap();
        let b = Store::open(&path).unwrap();
        let mode: String = a
            .conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        let v = a.data_version().unwrap();
        b.create_workspace("From MCP").unwrap();
        assert_ne!(a.data_version().unwrap(), v);
        assert_eq!(a.workspaces().unwrap().len(), 1);
        drop((a, b));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn search_table_cells_and_db_values() {
        let dir = std::env::temp_dir().join(format!("redbook-test-{}", short_id()));
        let path = dir.join("redbook.db");
        let s = Store::open(&path).unwrap();
        let ws = s.create_workspace("W").unwrap().id;
        let p = page(&s, &ws, None, "Sheet");
        s.markdown_to_blocks(&p, "| Fruit |\n| --- |\n| kumquat |\n")
            .unwrap();
        let db = s.create_database(&ws, None, None, true).unwrap().id;
        let pid = s.add_property(&db, "Tags", PropType::MultiSelect).unwrap();
        let row = s.add_row(&db).unwrap().id;
        s.set_row_value(&row, &pid, json!(["zeppelin"])).unwrap();
        assert_eq!(s.search(&ws, "kumq", 5).unwrap()[0].page_id, p);
        assert_eq!(s.search(&ws, "zeppel", 5).unwrap()[0].page_id, row);

        // An index built before migration 3 is backfilled.
        s.conn
            .execute_batch("DELETE FROM blocks_fts; PRAGMA user_version = 2;")
            .unwrap();
        drop(s);
        let s = Store::open(&path).unwrap();
        assert_eq!(s.search(&ws, "zeppel", 5).unwrap()[0].page_id, row);
        assert_eq!(s.search(&ws, "kumq", 5).unwrap().len(), 1);
        drop(s);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
