//! Device sync through a private git remote: lossless JSONL files in a working tree, the git
//! CLI as transport (spawned only while syncing) and record-level 3-way merges.
//!
//! Tree: `pages/<id>.jsonl` (page record, then its non-page blocks in tree order),
//! `collections/<id>.jsonl` (schema props + views), `workspaces.jsonl`, `files/` (attachments),
//! generated `README.md` + `read/<id>.md`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, anyhow, bail};
use gpui_kit::{App, Global, Task};
use rusqlite::{OptionalExtension as _, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::db;
use crate::import::files_dir;
use crate::model::Workspace;
use crate::store::Store;

const PAGE_LIKE: &str = "('page','collection_view','collection_view_page')";
const USAGE: &str = "usage: redbook sync init <remote> [--dir PATH] | clone <remote> [--dir PATH] [--force] | now | status | dir <PATH>";
const ATTRIBUTES: &str = "* text=auto eol=lf\nfiles/** -text\nREADME.md linguist-generated merge=binary\nread/** linguist-generated merge=binary\n";
const DEBOUNCE: Duration = Duration::from_secs(10);
const PULL_EVERY: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    /// Export, pull, push.
    Full,
    /// Export and push; pulls only when the push is rejected.
    Push,
    /// Export and push within 5 s.
    Quit,
}

#[derive(Default, Debug)]
pub struct Report {
    pub exported: usize,
    pub imported: usize,
    pub conflicts: usize,
}

/// Last run, kept in `.git/redbook-status.json` (outside the DB so it never wakes the watcher).
#[derive(Default, Serialize, Deserialize)]
pub struct Status {
    pub time: i64,
    pub error: Option<String>,
    pub summary: String,
    /// Local commits not pushed yet.
    pub ahead: bool,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

// ---- configuration

fn settings_path(db: &Path) -> PathBuf {
    db.with_extension("settings.json")
}

fn read_settings(db: &Path) -> Value {
    fs::read_to_string(settings_path(db))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

/// Expands a leading `~` and makes the path absolute.
pub fn expand(p: &str) -> PathBuf {
    let home = || dirs::home_dir().unwrap_or_default();
    let p = match p.strip_prefix("~") {
        Some("") => home(),
        Some(r) if r.starts_with(['/', '\\']) => home().join(&r[1..]),
        _ => PathBuf::from(p),
    };
    std::path::absolute(&p).unwrap_or(p)
}

/// Working tree: `sync_dir` from the settings file, else `<data dir>/sync`.
pub fn dir(db: &Path) -> PathBuf {
    match read_settings(db)["sync_dir"].as_str() {
        Some(d) => expand(d),
        None => db.parent().unwrap_or(Path::new(".")).join("sync"),
    }
}

fn set_dir(db: &Path, dir: &Path) -> Result<()> {
    let mut s = read_settings(db);
    s["sync_dir"] = json!(dir.to_string_lossy());
    fs::write(settings_path(db), serde_json::to_string_pretty(&s)?)?;
    Ok(())
}

fn status_path(dir: &Path) -> PathBuf {
    dir.join(".git").join("redbook-status.json")
}

pub fn status(db: &Path) -> Option<Status> {
    let s = fs::read_to_string(status_path(&dir(db))).ok()?;
    serde_json::from_str(&s).ok()
}

/// This device's name once sync is set up; None while sync is off.
pub fn host(store: &Store) -> Option<String> {
    store
        .conn()
        .query_row("SELECT value FROM sync_state WHERE key = 'host'", [], |r| {
            r.get(0)
        })
        .optional()
        .ok()
        .flatten()
}

fn machine_name() -> String {
    let out = Command::new("hostname").output().ok();
    let name = out
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_else(|| "device".into());
    name.split('.').next().unwrap_or(&name).to_string()
}

/// Turns change logging on.
fn enable(store: &Store) -> Result<String> {
    let host = machine_name();
    store.conn().execute(
        "INSERT OR REPLACE INTO sync_state(key, value) VALUES ('host', ?1)",
        [&host],
    )?;
    Ok(host)
}

fn mark_all_dirty(store: &Store) -> Result<()> {
    store.conn().execute(
        &format!(
            "INSERT OR REPLACE INTO sync_dirty(id) SELECT id FROM blocks WHERE parent_id IS NULL OR type IN {PAGE_LIKE}"
        ),
        [],
    )?;
    Ok(())
}

// ---- git CLI

struct Git {
    dir: PathBuf,
    deadline: Instant,
    /// No credential prompts (the app has no terminal to answer them).
    batch: bool,
}

impl Git {
    fn new(dir: &Path, limit: Duration, batch: bool) -> Self {
        Git {
            dir: dir.to_path_buf(),
            deadline: Instant::now() + limit,
            batch,
        }
    }

    /// (success, stdout, stderr); killed at the deadline.
    fn cmd(&self, args: &[&str]) -> Result<(bool, String, String)> {
        let mut c = Command::new("git");
        c.arg("-C")
            .arg(&self.dir)
            .args(["-c", "commit.gpgsign=false", "-c", "core.quotepath=off"])
            .args(args)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if self.batch {
            c.env("GIT_TERMINAL_PROMPT", "0");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = c.spawn().context("cannot run git; is it installed?")?;
        let pipe = |mut r: Box<dyn Read + Send>| {
            std::thread::spawn(move || {
                let mut v = vec![];
                let _ = r.read_to_end(&mut v);
                String::from_utf8_lossy(&v).into_owned()
            })
        };
        let out = pipe(Box::new(child.stdout.take().unwrap()));
        let err = pipe(Box::new(child.stderr.take().unwrap()));
        let status = loop {
            if let Some(s) = child.try_wait()? {
                break s;
            }
            if Instant::now() >= self.deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("git {} timed out", args[0]);
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        Ok((
            status.success(),
            out.join().unwrap_or_default(),
            err.join().unwrap_or_default(),
        ))
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let (ok, out, err) = self.cmd(args)?;
        if !ok {
            bail!("git {}: {}", args[0], err.trim());
        }
        Ok(out)
    }

    fn lines(&self, args: &[&str]) -> Result<Vec<String>> {
        Ok(self.run(args)?.lines().map(str::to_string).collect())
    }

    fn ok(&self, args: &[&str]) -> bool {
        self.cmd(args).is_ok_and(|r| r.0)
    }

    /// File at a revision; None when it does not exist there.
    fn show(&self, rev: &str, path: &str) -> Result<Option<String>> {
        let (ok, out, _) = self.cmd(&["show", &format!("{rev}:{path}")])?;
        Ok(ok.then_some(out))
    }

    fn head(&self) -> Result<String> {
        Ok(self.run(&["rev-parse", "HEAD"])?.trim().to_string())
    }

    /// Commits everything; false when there was nothing to commit.
    fn commit(&self, msg: &str) -> Result<bool> {
        self.run(&["add", "-A"])?;
        if self.run(&["status", "--porcelain"])?.trim().is_empty() {
            return Ok(false);
        }
        self.run(&["commit", "-q", "--no-verify", "-m", msg])?;
        Ok(true)
    }

    /// Ok(false) when the remote has commits we lack.
    fn push(&self) -> Result<bool> {
        let (ok, _, err) = self.cmd(&["push", "-q", "-u", "origin", "HEAD"])?;
        if ok {
            return Ok(true);
        }
        if ["rejected", "fetch first", "non-fast-forward"]
            .iter()
            .any(|s| err.contains(s))
        {
            return Ok(false);
        }
        bail!("git push: {}", err.trim())
    }

    /// Sets a commit identity for this repo when git has none.
    fn ensure_identity(&self, host: &str) -> Result<()> {
        if !self.ok(&["config", "user.email"]) {
            self.run(&["config", "user.name", host])?;
            self.run(&["config", "user.email", &format!("redbook@{host}")])?;
        }
        Ok(())
    }
}

/// One sync at a time per working tree, across processes.
struct Lock(PathBuf);

impl Lock {
    fn take(dir: &Path, wait: Duration) -> Result<Lock> {
        let path = dir.join(".git").join("redbook-sync.lock");
        let until = Instant::now() + wait;
        loop {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => return Ok(Lock(path)),
                Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e.into()),
                Err(_) => {}
            }
            let stale = fs::metadata(&path)
                .and_then(|m| m.modified())
                .is_ok_and(|t| t.elapsed().unwrap_or_default() > Duration::from_secs(300));
            if stale {
                let _ = fs::remove_file(&path);
            } else if Instant::now() >= until {
                bail!("another sync is running");
            } else {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

// ---- records

/// One block; the first line of a page file also carries its workspace.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Rec {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace: Option<String>,
    parent: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    sort: f64,
    created: i64,
    edited: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trashed: Option<i64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    favorite: bool,
    title: Value,
    props: Value,
}

fn parse_json(s: &str, default: Value) -> Value {
    serde_json::from_str(s).unwrap_or(default)
}

/// `root` and its blocks up to (not into) nested page-like blocks.
fn owned(store: &Store, root: &str) -> Result<Vec<Rec>> {
    let mut st = store.conn().prepare_cached(&format!(
        "WITH RECURSIVE s(id) AS (SELECT ?1 UNION ALL
           SELECT b.id FROM blocks b JOIN s ON b.parent_id = s.id WHERE b.type NOT IN {PAGE_LIKE})
         SELECT b.id, b.workspace_id, b.parent_id, b.type, b.sort, b.title, b.props, b.created,
                b.edited, b.trashed, b.favorite FROM s JOIN blocks b ON b.id = s.id"
    ))?;
    let recs = st
        .query_map([root], |r| {
            let (title, props): (String, String) = (r.get(5)?, r.get(6)?);
            Ok(Rec {
                id: r.get(0)?,
                workspace: Some(r.get(1)?),
                parent: r.get(2)?,
                kind: r.get(3)?,
                sort: r.get(4)?,
                title: parse_json(&title, json!([])),
                props: parse_json(&props, json!({})),
                created: r.get(7)?,
                edited: r.get(8)?,
                trashed: r.get(9)?,
                favorite: r.get(10)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(recs)
}

/// Page-like (or root) block a block belongs to; None when it no longer exists.
fn owner(store: &Store, id: &str) -> Result<Option<String>> {
    let mut st = store.conn().prepare_cached(&format!(
        "WITH RECURSIVE a(id, parent_id, type, depth) AS (
           SELECT id, parent_id, type, 0 FROM blocks WHERE id = ?1
           UNION ALL SELECT b.id, b.parent_id, b.type, a.depth + 1 FROM blocks b JOIN a ON b.id = a.parent_id
           WHERE a.type NOT IN {PAGE_LIKE})
         SELECT id FROM a ORDER BY depth DESC LIMIT 1"
    ))?;
    Ok(st.query_row([id], |r| r.get(0)).optional()?)
}

/// Tree order from `root` (siblings by sort, then id); unreachable records last.
fn order(mut recs: Vec<Rec>, root: &str) -> Vec<Rec> {
    recs.sort_by(|a, b| a.sort.total_cmp(&b.sort).then_with(|| a.id.cmp(&b.id)));
    let mut kids: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, r) in recs.iter().enumerate() {
        if r.id != root
            && let Some(p) = &r.parent
        {
            kids.entry(p.as_str()).or_default().push(i);
        }
    }
    let mut seen = vec![false; recs.len()];
    let mut out = vec![];
    let mut stack: Vec<usize> = recs.iter().position(|r| r.id == root).into_iter().collect();
    while let Some(i) = stack.pop() {
        if std::mem::replace(&mut seen[i], true) {
            continue;
        }
        out.push(i);
        if let Some(k) = kids.get(recs[i].id.as_str()) {
            stack.extend(k.iter().rev());
        }
    }
    out.extend((0..recs.len()).filter(|i| !seen[*i]));
    let mut slots: Vec<Option<Rec>> = recs.into_iter().map(Some).collect();
    out.into_iter().filter_map(|i| slots[i].take()).collect()
}

fn page_text(recs: Vec<Rec>, root: &str) -> String {
    order(recs, root)
        .into_iter()
        .map(|mut r| {
            if r.id != root {
                r.workspace = None;
            }
            serde_json::to_string(&r).unwrap_or_default() + "\n"
        })
        .collect()
}

fn parse_recs(text: &str) -> Result<Vec<Rec>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).with_context(|| format!("bad record: {l}")))
        .collect()
}

fn collection_text(store: &Store, id: &str) -> Result<Option<String>> {
    let conn = store.conn();
    let schema: Option<String> = conn
        .query_row("SELECT schema FROM collections WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(schema) = schema else {
        return Ok(None);
    };
    let mut out = String::new();
    if let Value::Object(m) = parse_json(&schema, json!({})) {
        let mut props: Vec<_> = m.into_iter().collect();
        props.sort_by(|a, b| a.0.cmp(&b.0));
        for (k, v) in props {
            out += &(json!({ "id": format!("prop:{k}"), "def": v }).to_string() + "\n");
        }
    }
    let mut st = conn.prepare_cached(
        "SELECT id, type, name, sort, query, format FROM views WHERE collection_id = ?1 ORDER BY sort, id",
    )?;
    let views = st.query_map([id], |r| {
        let (q, f): (String, String) = (r.get(4)?, r.get(5)?);
        Ok(json!({
            "id": format!("view:{}", r.get::<_, String>(0)?),
            "type": r.get::<_, String>(1)?,
            "name": r.get::<_, String>(2)?,
            "sort": r.get::<_, f64>(3)?,
            "query": parse_json(&q, json!({})),
            "format": parse_json(&f, json!({})),
        }))
    })?;
    for v in views {
        out += &(v?.to_string() + "\n");
    }
    Ok(Some(out))
}

fn workspaces_text(store: &Store) -> Result<String> {
    Ok(store
        .workspaces()?
        .iter()
        .map(|w| serde_json::to_string(w).unwrap_or_default() + "\n")
        .collect())
}

// ---- export

/// Writes only when the content differs; true when it wrote.
fn write(path: &Path, text: &str) -> Result<bool> {
    if fs::read(path).is_ok_and(|old| old == text.as_bytes()) {
        return Ok(false);
    }
    if let Some(d) = path.parent() {
        fs::create_dir_all(d)?;
    }
    fs::write(path, text)?;
    Ok(true)
}

fn remove(path: &Path) -> bool {
    fs::remove_file(path).is_ok()
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x100_0000_01b3)
    })
}

/// Props that may hold a local file path.
const FILE_PROPS: [&str; 2] = ["source", "cover"];

/// Local attachment paths become `files/<name>` and are copied into the tree.
fn export_files(recs: &mut [Rec], dir: &Path, files: &Path) -> Result<()> {
    for r in recs {
        for key in FILE_PROPS {
            let Some(src) = r.props.get(key).and_then(Value::as_str) else {
                continue;
            };
            let path = Path::new(src);
            if !path.is_absolute() || !path.is_file() {
                continue;
            }
            let base = path.file_name().unwrap_or_default().to_string_lossy();
            let name = if path.parent() == Some(files) {
                base.into_owned()
            } else {
                let safe: String = base
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || ".-_".contains(c) {
                            c
                        } else {
                            '-'
                        }
                    })
                    .collect();
                format!("{:016x}-{safe}", fnv(&fs::read(path)?))
            };
            let dest = dir.join("files").join(&name);
            if !dest.exists() {
                fs::create_dir_all(dest.parent().unwrap())?;
                fs::copy(path, &dest)?;
            }
            r.props[key] = json!(format!("files/{name}"));
        }
    }
    Ok(())
}

/// Exports pages changed since the last export; returns how many page files changed and
/// whether anything changed.
fn export(store: &Store, dir: &Path, files: &Path) -> Result<(usize, bool)> {
    let dirty: Vec<(i64, String)> = store
        .conn()
        .prepare("SELECT seq, id FROM sync_dirty ORDER BY seq")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let (mut pages, mut gone) = (BTreeSet::new(), BTreeSet::new());
    for (_, id) in &dirty {
        match owner(store, id)? {
            Some(o) => pages.insert(o),
            None => gone.insert(id.clone()),
        };
    }
    let (mut n, mut render) = (0, BTreeSet::new());
    for p in &pages {
        let mut recs = owned(store, p)?;
        let Some(root) = recs.iter().find(|r| r.id == *p) else {
            continue;
        };
        // The app's untouched launch page.
        if root.props.get("draft") == Some(&Value::Bool(true)) {
            continue;
        }
        if let Some(parent) = &root.parent
            && store.block(parent).is_ok_and(|b| b.kind.is_collection())
        {
            render.insert(parent.clone()); // a database's render shows its rows
        }
        if root.kind.starts_with("collection_view") {
            let path = dir.join("collections").join(format!("{p}.jsonl"));
            match collection_text(store, p)? {
                Some(t) => write(&path, &t)?,
                None => remove(&path),
            };
        }
        export_files(&mut recs, dir, files)?;
        n += write(&dir.join(format!("pages/{p}.jsonl")), &page_text(recs, p))? as usize;
        render.insert(p.clone());
    }
    for g in &gone {
        if remove(&dir.join(format!("pages/{g}.jsonl"))) {
            n += 1;
        }
        remove(&dir.join(format!("collections/{g}.jsonl")));
        render.insert(g.clone());
    }
    let ws = write(&dir.join("workspaces.jsonl"), &workspaces_text(store)?)?;
    if n > 0 || ws {
        render_pages(store, dir, &render)?;
    }
    if let Some((max, _)) = dirty.last() {
        store
            .conn()
            .execute("DELETE FROM sync_dirty WHERE seq <= ?1", [max])?;
    }
    Ok((n, n > 0 || ws))
}

/// Regenerates `read/<id>.md` for `ids` (removed when gone, trashed or not page-like) and README.
fn render_pages<'a>(
    store: &Store,
    dir: &Path,
    ids: impl IntoIterator<Item = &'a String>,
) -> Result<()> {
    for id in ids {
        let path = dir.join("read").join(format!("{id}.md"));
        match store.block(id) {
            Ok(b) if b.trashed.is_none() && b.kind.is_page_like() => {
                let md = store.page_to_markdown(id)?;
                write(
                    &path,
                    &format!(
                        "<!-- Generated by Redbook from pages/{id}.jsonl; edits are overwritten. -->\n\n{md}"
                    ),
                )?;
            }
            _ => {
                remove(&path);
            }
        }
    }
    let mut out = String::from(
        "# Redbook\n\nGenerated on every sync; edits are overwritten. Notes live in `pages/` (one JSON record per block).\n",
    );
    for w in store.workspaces()? {
        out += &format!(
            "\n## {}{}\n\n",
            w.icon.map(|i| i + " ").unwrap_or_default(),
            w.name
        );
        let nodes = store.page_tree(&w.id)?;
        let mut kids: HashMap<Option<&str>, Vec<usize>> = HashMap::new();
        for (i, n) in nodes.iter().enumerate() {
            kids.entry(n.parent_id.as_deref()).or_default().push(i);
        }
        let mut stack: Vec<(usize, usize)> = kids
            .get(&None)
            .into_iter()
            .flatten()
            .rev()
            .map(|i| (*i, 0))
            .collect();
        while let Some((i, depth)) = stack.pop() {
            let n = &nodes[i];
            let title = if n.title.trim().is_empty() {
                "Untitled"
            } else {
                n.title.trim()
            };
            let icon = n.icon.as_ref().map(|i| format!("{i} ")).unwrap_or_default();
            out += &format!(
                "{}- [{icon}{}](read/{}.md)\n",
                "  ".repeat(depth),
                title.replace('[', "\\[").replace(']', "\\]"),
                n.id
            );
            for k in kids.get(&Some(n.id.as_str())).into_iter().flatten().rev() {
                stack.push((*k, depth + 1));
            }
        }
    }
    write(&dir.join("README.md"), &out)?;
    Ok(())
}

// ---- import

fn upsert(store: &Store, r: &Rec, ws: &str, dir: &Path, files: &Path) -> Result<()> {
    let mut props = r.props.clone();
    for key in FILE_PROPS {
        let Some(name) = props
            .get(key)
            .and_then(Value::as_str)
            .and_then(|s| s.strip_prefix("files/"))
            .map(str::to_string)
        else {
            continue;
        };
        let local = files.join(&name);
        if !local.exists() {
            fs::create_dir_all(files)?;
            fs::copy(dir.join("files").join(&name), &local)
                .with_context(|| format!("attachment files/{name}"))?;
        }
        props[key] = json!(local.to_string_lossy());
    }
    store
        .conn()
        .prepare_cached(
            "INSERT INTO blocks(id, workspace_id, parent_id, type, sort, title, props, created, edited, trashed, favorite)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET workspace_id = excluded.workspace_id, parent_id = excluded.parent_id,
               type = excluded.type, sort = excluded.sort, title = excluded.title, props = excluded.props,
               created = excluded.created, edited = excluded.edited, trashed = excluded.trashed,
               favorite = excluded.favorite",
        )?
        .execute(params![
            r.id,
            ws,
            r.parent,
            r.kind,
            r.sort,
            r.title.to_string(),
            props.to_string(),
            r.created,
            r.edited,
            r.trashed,
            r.favorite
        ])?;
    Ok(())
}

fn delete_block(store: &Store, id: &str) -> Result<()> {
    let conn = store.conn();
    conn.execute("DELETE FROM blocks WHERE id = ?1", [id])?;
    conn.execute("DELETE FROM views WHERE collection_id = ?1", [id])?;
    conn.execute("DELETE FROM collections WHERE id = ?1", [id])?;
    Ok(())
}

fn import_collection(store: &Store, id: &str, text: &str) -> Result<()> {
    let conn = store.conn();
    let mut schema = serde_json::Map::new();
    conn.execute("DELETE FROM views WHERE collection_id = ?1", [id])?;
    for l in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: Value = serde_json::from_str(l)?;
        let key = v["id"].as_str().unwrap_or_default();
        if let Some(p) = key.strip_prefix("prop:") {
            schema.insert(p.to_string(), v["def"].clone());
        } else if let Some(view) = key.strip_prefix("view:") {
            conn.execute(
                "INSERT OR REPLACE INTO views(id, collection_id, type, name, sort, query, format)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    view,
                    id,
                    v["type"].as_str().unwrap_or("table"),
                    v["name"].as_str().unwrap_or_default(),
                    v["sort"].as_f64().unwrap_or(1.0),
                    v["query"].to_string(),
                    v["format"].to_string()
                ],
            )?;
        }
    }
    conn.execute(
        "INSERT INTO collections(id, schema) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET schema = excluded.schema",
        params![id, Value::Object(schema).to_string()],
    )?;
    Ok(())
}

/// Ids of `root`'s own blocks currently in the DB.
fn owned_ids(store: &Store, root: &str) -> Result<Vec<String>> {
    Ok(owned(store, root)?.into_iter().map(|r| r.id).collect())
}

/// Inserts `text` (a page file) as a new page next to the original, titled "(conflict <label>)".
fn conflict_copy(
    store: &Store,
    text: &str,
    page: &str,
    label: &str,
    dir: &Path,
    files: &Path,
) -> Result<String> {
    let recs = parse_recs(text)?;
    let ids: HashMap<&str, String> = recs
        .iter()
        .map(|r| (r.id.as_str(), uuid::Uuid::new_v4().to_string()))
        .collect();
    let root = recs
        .iter()
        .find(|r| r.id == page)
        .ok_or_else(|| anyhow!("page {page} has no page record"))?;
    let ws = root.workspace.clone().unwrap_or_default();
    let next: Option<f64> = store.conn().query_row(
        "SELECT min(sort) FROM blocks WHERE parent_id IS ?1 AND workspace_id = ?2 AND sort > ?3",
        params![root.parent, ws, root.sort],
        |r| r.get(0),
    )?;
    for r in &recs {
        let mut c = r.clone();
        c.id = ids[r.id.as_str()].clone();
        if r.id == page {
            c.sort = next.map_or(r.sort + 1.0, |n| (r.sort + n) / 2.0);
            c.favorite = false;
            if c.kind.starts_with("collection_view") {
                c.kind = "page".into(); // keeps their title/icon, not a second database
            }
            if let Value::Array(t) = &mut c.title {
                t.push(json!([format!(" (conflict {label})")]));
            }
        } else {
            c.parent = Some(
                r.parent
                    .as_deref()
                    .and_then(|p| ids.get(p).cloned())
                    .unwrap_or_else(|| ids[page].clone()),
            );
        }
        upsert(store, &c, &ws, dir, files)?;
    }
    Ok(ids[page].clone())
}

/// Applies changed tree paths to the DB. `conflicts` are (page id, their file) pairs whose
/// version is kept as a conflict copy. Local unexported edits are never overwritten.
fn import(
    store: &Store,
    dir: &Path,
    files: &Path,
    paths: &[String],
    conflicts: Vec<(String, String)>,
    label: &str,
) -> Result<usize> {
    store.tx(|| {
        let conn = store.conn();
        let dirty: Vec<String> = conn
            .prepare("SELECT id FROM sync_dirty")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let before: i64 = conn.query_row("SELECT coalesce(max(seq), 0) FROM sync_dirty", [], |r| r.get(0))?;
        let mut local = HashSet::new();
        for id in &dirty {
            local.extend(owner(store, id)?);
        }
        let mut copies = conflicts;
        let (mut pages, mut gone, mut workspaces) = (vec![], vec![], None);
        let mut file_owner: HashMap<String, String> = HashMap::new();
        for path in paths {
            let text = fs::read_to_string(dir.join(path)).ok();
            if path == "workspaces.jsonl" {
                let mut keep = HashSet::new();
                for l in text.iter().flat_map(|t| t.lines()).filter(|l| !l.trim().is_empty()) {
                    let w: Workspace = serde_json::from_str(l)?;
                    conn.execute(
                        "INSERT INTO workspaces(id, name, icon, sort, created) VALUES (?1, ?2, ?3, ?4, ?5)
                         ON CONFLICT(id) DO UPDATE SET name = excluded.name, icon = excluded.icon, sort = excluded.sort",
                        params![w.id, w.name, w.icon, w.sort, w.created],
                    )?;
                    keep.insert(w.id);
                }
                workspaces = Some(keep);
            } else if let Some(id) = path.strip_prefix("pages/").and_then(|p| p.strip_suffix(".jsonl")) {
                match text {
                    Some(t) if local.contains(id) => copies.push((id.to_string(), t)),
                    Some(t) => {
                        let recs = parse_recs(&t)?;
                        let ws = recs
                            .iter()
                            .find(|r| r.id == id)
                            .and_then(|r| r.workspace.clone())
                            .ok_or_else(|| anyhow!("{path}: no page record"))?;
                        for r in &recs {
                            upsert(store, r, &ws, dir, files)?;
                            file_owner.insert(r.id.clone(), id.to_string());
                        }
                        pages.push(id.to_string());
                    }
                    None if !local.contains(id) => gone.push(id.to_string()),
                    None => {}
                }
            } else if let Some(id) = path.strip_prefix("collections/").and_then(|p| p.strip_suffix(".jsonl")) {
                match text {
                    Some(t) => import_collection(store, id, &t)?,
                    None => {
                        conn.execute("DELETE FROM views WHERE collection_id = ?1", [id])?;
                        conn.execute("DELETE FROM collections WHERE id = ?1", [id])?;
                    }
                }
            }
        }
        // Blocks that left a page; their surviving children get re-parented below.
        let mut stale_owner: HashMap<String, Option<String>> = HashMap::new();
        for p in &pages {
            for b in owned_ids(store, p)? {
                if file_owner.get(&b) != Some(p) {
                    delete_block(store, &b)?;
                    stale_owner.insert(b, Some(p.clone()));
                }
            }
        }
        for g in &gone {
            let parent: Option<Option<String>> = conn
                .query_row("SELECT parent_id FROM blocks WHERE id = ?1", [g], |r| r.get(0))
                .optional()?;
            for b in owned_ids(store, g)? {
                delete_block(store, &b)?;
                stale_owner.insert(b, parent.clone().flatten());
            }
        }
        let orphans: Vec<(String, String)> = conn
            .prepare("SELECT id, parent_id FROM blocks b WHERE parent_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM blocks p WHERE p.id = b.parent_id)")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let exists = |id: &str| -> bool {
            conn.query_row("SELECT 1 FROM blocks WHERE id = ?1", [id], |_| Ok(()))
                .optional()
                .ok()
                .flatten()
                .is_some()
        };
        let mut mark: Vec<String> = dirty;
        for (id, missing) in orphans {
            let to = stale_owner
                .get(&missing)
                .cloned()
                .flatten()
                .or_else(|| file_owner.get(&id).filter(|o| **o != id).cloned())
                .filter(|t| exists(t));
            conn.execute("UPDATE blocks SET parent_id = ?2 WHERE id = ?1", params![id, to])?;
            mark.push(id);
        }
        if let Some(keep) = workspaces {
            for w in store.workspaces()? {
                let used: bool = conn.query_row(
                    "SELECT EXISTS (SELECT 1 FROM blocks WHERE workspace_id = ?1)",
                    [&w.id],
                    |r| r.get(0),
                )?;
                if !keep.contains(&w.id) && !used {
                    conn.execute("DELETE FROM workspaces WHERE id = ?1", [&w.id])?;
                }
            }
        }
        let n = pages.len() + gone.len() + copies.len();
        for (page, text) in copies {
            mark.push(conflict_copy(store, &text, &page, label, dir, files)?);
        }
        // What the import itself wrote is already in the tree; keep only local changes.
        conn.execute("DELETE FROM sync_dirty WHERE seq > ?1", [before])?;
        for id in mark {
            conn.execute("INSERT OR REPLACE INTO sync_dirty(id) VALUES (?1)", [id])?;
        }
        Ok(n)
    })
}

// ---- merge

fn key(line: &str) -> String {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|v| v.get("id")?.as_str().map(str::to_string))
        .unwrap_or_else(|| line.to_string())
}

/// 3-way merge of record files keyed by `id`: a record changed on one side takes that
/// side; edited beats deleted; changed differently on both sides keeps ours and flags it.
fn merge_records(base: &str, ours: &str, theirs: &str) -> (String, bool) {
    let map = |s: &'_ str| -> Vec<(String, String)> {
        s.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| (key(l), l.to_string()))
            .collect()
    };
    let (b, o, t) = (map(base), map(ours), map(theirs));
    let find = |v: &[(String, String)], k: &str| v.iter().find(|x| x.0 == k).map(|x| x.1.clone());
    let mut keys: Vec<&String> = o.iter().map(|x| &x.0).collect();
    keys.extend(t.iter().map(|x| &x.0).filter(|k| find(&o, k).is_none()));
    let (mut out, mut conflict) = (String::new(), false);
    for k in keys {
        let (bv, ov, tv) = (find(&b, k), find(&o, k), find(&t, k));
        let v = if ov == tv || tv == bv {
            ov
        } else if ov == bv || ov.is_none() {
            tv
        } else if tv.is_none() {
            ov
        } else {
            conflict = true;
            ov
        };
        if let Some(l) = v {
            out += &(l + "\n");
        }
    }
    (out, conflict)
}

/// Whole-file merge: one-sided changes win, a deletion loses to an edit.
fn merge_file(b: Option<&str>, o: Option<&str>, t: Option<&str>) -> (Option<String>, bool) {
    if o == t || t == b {
        return (o.map(str::to_string), false);
    }
    if o == b {
        return (t.map(str::to_string), false);
    }
    match (o, t) {
        (Some(o), Some(t)) => {
            let (m, c) = merge_records(b.unwrap_or_default(), o, t);
            (Some(m), c)
        }
        (o, t) => (o.or(t).map(str::to_string), false),
    }
}

fn page_id(path: &str) -> Option<&str> {
    path.strip_prefix("pages/")?.strip_suffix(".jsonl")
}

/// Fetches and merges the upstream branch, then imports what changed. Returns pages imported.
fn pull(git: &Git, store: &Store, files: &Path, host: &str, rep: &mut Report) -> Result<()> {
    git.run(&["fetch", "-q", "origin"])?;
    let branch = git
        .run(&["symbolic-ref", "--short", "HEAD"])?
        .trim()
        .to_string();
    let up = format!("origin/{branch}");
    if !git.ok(&["rev-parse", "-q", "--verify", &up])
        || git
            .run(&["rev-list", "--count", &format!("HEAD..{up}")])?
            .trim()
            == "0"
    {
        return Ok(());
    }
    let dir = &git.dir;
    let old = git.head()?;
    let (ok, _, err) = git.cmd(&[
        "merge",
        "-q",
        "--no-commit",
        "--no-edit",
        "-Xno-renames",
        &up,
    ])?;
    let merging = git.ok(&["rev-parse", "-q", "--verify", "MERGE_HEAD"]);
    if !ok && !merging {
        bail!("git merge: {}", err.trim());
    }
    let their_host = git.run(&["log", "-1", "--format=%s", &up])?;
    let their_host = their_host.split(':').next().unwrap_or("remote").trim();
    let label = format!("{their_host} {}", chrono::Local::now().format("%Y-%m-%d"));
    let result = (|| -> Result<usize> {
        let mut conflicts = vec![];
        if merging {
            let base = git
                .run(&["merge-base", "HEAD", "MERGE_HEAD"])?
                .trim()
                .to_string();
            let changed =
                |rev: &str| git.lines(&["diff", "--name-only", "--no-renames", &base, rev]);
            let theirs: HashSet<String> = changed("MERGE_HEAD")?.into_iter().collect();
            for path in changed("HEAD")?
                .into_iter()
                .filter(|p| theirs.contains(p) && p.ends_with(".jsonl"))
            {
                let (b, o, t) = (
                    git.show(&base, &path)?,
                    git.show("HEAD", &path)?,
                    git.show("MERGE_HEAD", &path)?,
                );
                let (mut m, c) = merge_file(b.as_deref(), o.as_deref(), t.as_deref());
                if let Some(id) = page_id(&path) {
                    m = m
                        .map(|m| parse_recs(&m).map(|r| page_text(r, id)))
                        .transpose()?;
                    if c && let Some(t) = t {
                        conflicts.push((id.to_string(), t));
                    }
                }
                match m {
                    Some(text) => {
                        write(&dir.join(&path), &text)?;
                    }
                    None => {
                        remove(&dir.join(&path));
                    }
                }
            }
            // Generated files get rewritten below; attachments are content-named.
            for path in git.lines(&["diff", "--name-only", "--diff-filter=U"])? {
                if !path.ends_with(".jsonl")
                    && !git.ok(&["checkout", "--ours", "--", &path])
                    && !git.ok(&["checkout", "--theirs", "--", &path])
                {
                    remove(&dir.join(&path));
                }
            }
            git.run(&["add", "-A"])?;
        }
        let changed = git.lines(&["diff", "--cached", "--name-only", "--no-renames", &old])?;
        rep.conflicts += conflicts.len();
        let n = import(store, dir, files, &changed, conflicts, &label)?;
        let ids: Vec<String> = changed
            .iter()
            .filter_map(|p| page_id(p))
            .map(str::to_string)
            .collect();
        render_pages(store, dir, &ids)?;
        Ok(n)
    })();
    match result {
        Ok(n) => {
            rep.imported += n;
            git.run(&["add", "-A"])?;
            if merging {
                git.run(&[
                    "commit",
                    "-q",
                    "--no-verify",
                    "-m",
                    &format!("{host}: merge"),
                ])?;
            }
            Ok(())
        }
        Err(e) => {
            let _ = if merging {
                git.cmd(&["merge", "--abort"])
            } else {
                git.cmd(&["reset", "-q", "--keep", &old])
            };
            Err(e)
        }
    }
}

// ---- commands

fn open(db: &Path) -> Result<Store> {
    let store = Store::open(db)?;
    // Imports re-parent orphans themselves; cascades would delete subpages.
    store.conn().execute_batch("PRAGMA foreign_keys = OFF")?;
    Ok(store)
}

/// One sync run; the outcome is saved for `status`.
pub fn run(db: &Path, mode: Mode, interactive: bool) -> Result<Report> {
    let dir = dir(db);
    let prev = status(db).unwrap_or_default();
    let mut ahead = prev.ahead;
    let r = cycle(db, &dir, mode, interactive, &mut ahead);
    let changed = r.as_ref().map_or(true, |r| r.exported + r.imported > 0);
    if dir.join(".git").is_dir()
        && (changed || ahead != prev.ahead || prev.error.is_some() || mode == Mode::Full)
    {
        let st = Status {
            time: now_ms(),
            error: r.as_ref().err().map(|e| format!("{e:#}")),
            summary: r.as_ref().map_or_else(
                |_| prev.summary.clone(),
                |r| {
                    format!(
                        "{} exported, {} imported, {} conflicts",
                        r.exported, r.imported, r.conflicts
                    )
                },
            ),
            ahead,
        };
        let _ = fs::write(status_path(&dir), serde_json::to_string_pretty(&st)?);
    }
    r
}

fn cycle(db: &Path, dir: &Path, mode: Mode, interactive: bool, ahead: &mut bool) -> Result<Report> {
    if !dir.join(".git").is_dir() {
        bail!(
            "sync folder {} is missing; run `redbook sync clone`",
            dir.display()
        );
    }
    let quit = mode == Mode::Quit;
    let _lock = Lock::take(
        dir,
        if quit {
            Duration::from_secs(4)
        } else {
            Duration::ZERO
        },
    )?;
    let store = open(db)?;
    let host = host(&store).ok_or_else(|| anyhow!("sync is not set up for this database"))?;
    let files = files_dir(db);
    let limit = Duration::from_secs(if quit { 5 } else { 180 });
    let git = Git::new(dir, limit, !interactive);
    let _ = fs::remove_file(dir.join(".git/index.lock")); // only left by a killed git
    let mut rep = Report::default();
    let export_commit = |rep: &mut Report, ahead: &mut bool| -> Result<()> {
        let (n, any) = export(&store, dir, &files)?;
        rep.exported += n;
        if any && git.commit(&format!("{host}: {n} pages"))? {
            *ahead = true;
        }
        Ok(())
    };
    export_commit(&mut rep, ahead)?;
    if mode == Mode::Full {
        pull(&git, &store, &files, &host, &mut rep)?;
        export_commit(&mut rep, ahead)?;
        let unpushed = git.cmd(&["rev-list", "--count", "@{u}..HEAD"])?;
        *ahead = !unpushed.0 || unpushed.1.trim() != "0";
    }
    if *ahead {
        if !git.push()? {
            if quit {
                bail!("remote has new changes; they are merged at next launch");
            }
            pull(&git, &store, &files, &host, &mut rep)?;
            export_commit(&mut rep, ahead)?;
            if !git.push()? {
                bail!("push rejected twice; try again");
            }
        }
        *ahead = false;
    }
    Ok(rep)
}

/// Every record file in the tree, for a full import.
fn all_paths(git: &Git) -> Result<Vec<String>> {
    Ok(git
        .lines(&["ls-files"])?
        .into_iter()
        .filter(|p| p.ends_with(".jsonl"))
        .collect())
}

fn check_target(dir: &Path) -> Result<()> {
    if dir.join(".git").exists() {
        bail!("{} is already a sync folder", dir.display());
    }
    if fs::read_dir(dir).is_ok_and(|mut d| d.next().is_some()) {
        bail!("{} is not empty", dir.display());
    }
    Ok(())
}

pub fn init(db: &Path, remote: &str, dir_arg: Option<PathBuf>, interactive: bool) -> Result<()> {
    let dir = dir_arg.unwrap_or_else(|| dir(db));
    check_target(&dir)?;
    fs::create_dir_all(&dir)?;
    let git = Git::new(&dir, Duration::from_secs(600), !interactive);
    git.run(&["init", "-q", "-b", "main"])?;
    fs::write(dir.join(".gitattributes"), ATTRIBUTES)?;
    let store = open(db)?;
    let host = enable(&store)?;
    git.ensure_identity(&host)?;
    set_dir(db, &dir)?;
    mark_all_dirty(&store)?;
    let (n, _) = export(&store, &dir, &files_dir(db))?;
    git.commit(&format!("{host}: {n} pages"))?;
    git.run(&["remote", "add", "origin", remote])?;
    git.run(&["push", "-q", "-u", "origin", "main"])
        .context("set up locally, but the push failed; fix it and run `redbook sync now`")?;
    println!(
        "Sync set up in {}: {n} pages pushed to {remote}",
        dir.display()
    );
    Ok(())
}

pub fn clone(
    db: &Path,
    remote: &str,
    dir_arg: Option<PathBuf>,
    force: bool,
    interactive: bool,
) -> Result<()> {
    let dir = dir_arg.unwrap_or_else(|| dir(db));
    check_target(&dir)?;
    let store = open(db)?;
    let pages: i64 = store.conn().query_row(
        &format!("SELECT count(*) FROM blocks WHERE type IN {PAGE_LIKE} AND json_extract(props, '$.draft') IS NOT 1"),
        [],
        |r| r.get(0),
    )?;
    if pages > 0 && !force {
        bail!(
            "this database already has {pages} pages; quit Redbook and use --force to merge them in"
        );
    }
    if !force {
        // Only untouched launch pages and empty workspaces are here.
        store
            .conn()
            .execute_batch("DELETE FROM blocks; DELETE FROM workspaces;")?;
    }
    let parent = dir
        .parent()
        .ok_or_else(|| anyhow!("bad folder {}", dir.display()))?;
    fs::create_dir_all(parent)?;
    let git = Git::new(parent, Duration::from_secs(600), !interactive);
    git.run(&["clone", "-q", remote, &dir.to_string_lossy()])?;
    let git = Git::new(&dir, Duration::from_secs(600), !interactive);
    let host = enable(&store)?;
    git.ensure_identity(&host)?;
    set_dir(db, &dir)?;
    let n = import(&store, &dir, &files_dir(db), &all_paths(&git)?, vec![], "")?;
    if force {
        mark_all_dirty(&store)?;
    }
    println!("Cloned {remote} into {}: {n} pages imported", dir.display());
    Ok(())
}

/// Points sync at `new`: an existing clone of the same remote, or an empty folder that the
/// working tree moves into (re-cloned when the move fails).
pub fn switch_dir(db: &Path, new: &Path) -> Result<String> {
    let cur = dir(db);
    let new = expand(&new.to_string_lossy());
    if new == cur {
        return Ok("Sync folder unchanged".into());
    }
    if !cur.join(".git").is_dir() {
        check_target(&new)?;
        set_dir(db, &new)?;
        return Ok(format!("Sync folder set to {}", new.display()));
    }
    let git = Git::new(&cur, Duration::from_secs(600), true);
    let remote = git
        .run(&["remote", "get-url", "origin"])?
        .trim()
        .to_string();
    if new.join(".git").exists() {
        let theirs =
            Git::new(&new, Duration::from_secs(30), true).run(&["remote", "get-url", "origin"]);
        if theirs.is_ok_and(|r| r.trim() == remote) {
            set_dir(db, &new)?;
            // The DB is the truth: its pages get re-exported into that tree.
            mark_all_dirty(&open(db)?)?;
            return Ok(format!("Switched to {} (same remote)", new.display()));
        }
        bail!("{} holds a different repository", new.display());
    }
    check_target(&new)?;
    let lock = Lock::take(&cur, Duration::ZERO)?;
    let _ = fs::remove_dir(&new);
    drop(lock);
    if fs::rename(&cur, &new).is_ok() {
        set_dir(db, &new)?;
        return Ok(format!("Moved the sync folder to {}", new.display()));
    }
    fs::create_dir_all(new.parent().unwrap_or(&new))?;
    Git::new(new.parent().unwrap_or(&new), Duration::from_secs(600), true)
        .run(&["clone", "-q", &remote, &new.to_string_lossy()])
        .context("could not move the folder or clone into it")?;
    Git::new(&new, Duration::from_secs(30), true).ensure_identity(&machine_name())?;
    set_dir(db, &new)?;
    mark_all_dirty(&open(db)?)?;
    Ok(format!(
        "Cloned into {}; the old folder {} can be deleted",
        new.display(),
        cur.display()
    ))
}

pub fn cli(args: &[String]) -> Result<()> {
    let db = Store::default_path();
    let mut pos = vec![];
    let (mut dir_arg, mut force) = (None, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dir" => dir_arg = Some(expand(it.next().ok_or_else(|| anyhow!(USAGE))?)),
            "--force" => force = true,
            _ => pos.push(a.as_str()),
        }
    }
    match pos.as_slice() {
        ["init", remote] => init(&db, remote, dir_arg, true),
        ["clone", remote] => clone(&db, remote, dir_arg, force, true),
        ["now"] => {
            let r = run(&db, Mode::Full, true)?;
            println!(
                "{} exported, {} imported, {} conflicts",
                r.exported, r.imported, r.conflicts
            );
            Ok(())
        }
        ["dir", path] => {
            println!("{}", switch_dir(&db, &expand(path))?);
            Ok(())
        }
        ["status"] => {
            let dir = dir(&db);
            let store = Store::open(&db)?;
            let host = host(&store);
            println!("folder:  {}", dir.display());
            let Some(host) = host else {
                println!("sync is off; set it up with `redbook sync init <remote>`");
                return Ok(());
            };
            let remote =
                Git::new(&dir, Duration::from_secs(10), true).run(&["remote", "get-url", "origin"]);
            println!(
                "remote:  {}",
                remote.map_or_else(|e| format!("{e:#}"), |r| r.trim().to_string())
            );
            println!("device:  {host}");
            let pending: i64 =
                store
                    .conn()
                    .query_row("SELECT count(*) FROM sync_dirty", [], |r| r.get(0))?;
            println!("pending: {pending} changed blocks");
            if let Some(st) = status(&db) {
                let when = chrono::DateTime::from_timestamp_millis(st.time)
                    .map(|t| {
                        t.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_default();
                println!(
                    "last:    {when}: {}{}",
                    st.summary,
                    if st.ahead { " (not pushed)" } else { "" }
                );
                if let Some(e) = st.error {
                    println!("error:   {e}");
                }
            }
            Ok(())
        }
        _ => bail!(USAGE),
    }
}

// ---- app: background runs on change (debounced), on activation and on quit

struct Auto {
    db: PathBuf,
    busy: bool,
    queued: Option<Mode>,
    debounce: Option<Task<()>>,
    pulled: Option<Instant>,
}

impl Global for Auto {}

/// Turns automatic sync on when this database is set up for it, and pulls once.
pub fn start(cx: &mut App) {
    if host(&db::store(cx)).is_none() {
        return;
    }
    cx.set_global(Auto {
        db: Store::default_path(),
        busy: false,
        queued: None,
        debounce: None,
        pulled: None,
    });
    launch(Mode::Full, cx);
}

pub fn enabled(cx: &App) -> bool {
    cx.has_global::<Auto>()
}

pub fn busy(cx: &App) -> bool {
    cx.try_global::<Auto>().is_some_and(|a| a.busy)
}

/// After any DB change: export and push once things are quiet.
pub fn changed(cx: &mut App) {
    // `redbook sync init/clone` from a terminal turns sync on while the app runs.
    if !enabled(cx) {
        start(cx);
        return;
    }
    let task = cx.spawn(async move |cx| {
        cx.background_executor().timer(DEBOUNCE).await;
        cx.update(|cx| launch(Mode::Push, cx));
    });
    cx.global_mut::<Auto>().debounce = Some(task);
}

pub fn activated(cx: &mut App) {
    if !enabled(cx) {
        start(cx);
    } else if cx
        .try_global::<Auto>()
        .is_some_and(|a| a.pulled.is_none_or(|t| t.elapsed() >= PULL_EVERY))
    {
        launch(Mode::Full, cx);
    }
}

pub fn launch(mode: Mode, cx: &mut App) {
    let a = cx.global_mut::<Auto>();
    if a.busy {
        a.queued = Some(if a.queued == Some(Mode::Full) {
            Mode::Full
        } else {
            mode
        });
        return;
    }
    a.busy = true;
    if mode == Mode::Full {
        a.pulled = Some(Instant::now());
    }
    let db = a.db.clone();
    cx.spawn(async move |cx| {
        let r = cx
            .background_executor()
            .spawn(async move { run(&db, mode, false) })
            .await;
        cx.update(|cx| {
            if r.is_ok_and(|r| r.imported > 0) {
                db::sync_external(cx);
            }
            let a = cx.global_mut::<Auto>();
            a.busy = false;
            if let Some(m) = a.queued.take() {
                launch(m, cx);
            }
        });
    })
    .detach();
}

/// Final export and push, bounded to ~5 s; blocks the quitting app.
pub fn quit(cx: &mut App) {
    if let Some(a) = cx.try_global::<Auto>() {
        let _ = run(&a.db.clone(), Mode::Quit, false);
    }
}

/// Runs `f` with automatic sync paused (folder switch).
pub fn paused<T>(cx: &mut App, f: impl FnOnce() -> T) -> T {
    let had = cx.try_global::<Auto>().map(|a| a.busy);
    if had.is_some() {
        cx.global_mut::<Auto>().busy = true;
    }
    let out = f();
    if let Some(b) = had {
        cx.global_mut::<Auto>().busy = b;
    }
    out
}

#[cfg(test)]
mod tests;
