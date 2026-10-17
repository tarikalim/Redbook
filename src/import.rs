//! `redbook import`: a Markdown & CSV export (zip or folder) into the store: pages as `.md`,
//! databases as `.csv`, each name ending in a 32-hex id.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::model::*;
use crate::store::Store;

const USAGE: &str = "usage: redbook import <Export.zip|folder> [--workspace NAME]";
const COLORS: &[&str] = &[
    "default", "gray", "brown", "orange", "yellow", "green", "blue", "purple", "pink", "red",
];

pub fn run(args: &[String]) -> Result<()> {
    let (mut path, mut ws) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--workspace" => ws = Some(it.next().context(USAGE)?.clone()),
            _ if path.is_none() && !a.starts_with("--") => path = Some(PathBuf::from(a)),
            _ => bail!(USAGE),
        }
    }
    let path = path.context(USAGE)?;
    let db = Store::default_path();
    let store = Store::open(&db)?;
    let files = files_dir(&db);
    let summary = import(&store, &path, ws.as_deref(), &files)?;
    println!("{summary}");
    Ok(())
}

/// Attachment copies live next to the database: `<data dir>/RedBook/files`.
pub fn files_dir(db: &Path) -> PathBuf {
    db.parent().unwrap_or(Path::new(".")).join("files")
}

#[derive(Debug, Default)]
pub struct Summary {
    pub workspace: String,
    pub pages: usize,
    pub databases: usize,
    pub rows: usize,
    pub files: usize,
    pub skipped: usize,
    pub warnings: Vec<String>,
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Imported into \"{}\": {} pages, {} databases, {} rows, {} files; {} already imported (skipped)",
            self.workspace, self.pages, self.databases, self.rows, self.files, self.skipped
        )?;
        if !self.warnings.is_empty() {
            write!(f, "\nWarnings ({}):", self.warnings.len())?;
            for w in &self.warnings {
                write!(f, "\n  - {w}")?;
            }
        }
        Ok(())
    }
}

/// Workspace by id or name (case-insensitive); the first one when `name` is None.
/// Creates it (or "Personal") when missing and `create` is set.
pub fn resolve_workspace(store: &Store, name: Option<&str>, create: bool) -> Result<String> {
    let all = store.workspaces()?;
    let found = match name {
        Some(n) => all
            .iter()
            .find(|w| w.id == n || w.name.eq_ignore_ascii_case(n)),
        None => all.first(),
    };
    match found {
        Some(w) => Ok(w.id.clone()),
        None if create => Ok(store.create_workspace(name.unwrap_or("Personal"))?.id),
        None => bail!("no workspace {}", name.unwrap_or("exists")),
    }
}

pub fn import(store: &Store, path: &Path, ws: Option<&str>, files: &Path) -> Result<Summary> {
    let tmp;
    let root = if path.is_dir() {
        path.to_path_buf()
    } else {
        tmp = TempDir::new()?;
        unzip(path, &tmp.0).with_context(|| format!("extract {}", path.display()))?;
        tmp.0.clone()
    };
    let ws = resolve_workspace(store, ws, true)?;
    let mut imp = Import {
        store,
        ws: ws.clone(),
        files: files.to_path_buf(),
        ids: HashMap::new(),
        dbs: HashSet::new(),
        fill: vec![],
        sum: Summary {
            workspace: store.workspace(&ws)?.name,
            ..Default::default()
        },
    };
    store.tx(|| {
        for n in scan(&root)? {
            imp.create(&n, None)?;
        }
        for f in std::mem::take(&mut imp.fill) {
            imp.fill(f)?;
        }
        Ok(())
    })?;
    Ok(imp.sum)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self> {
        let p = std::env::temp_dir().join(format!("redbook-import-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&p)?;
        Ok(TempDir(p))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Extracts `zip` into `dest`, then any zips inside it (multi-part exports).
fn unzip(zip: &Path, dest: &Path) -> Result<()> {
    let mut ar = zip::ZipArchive::new(fs::File::open(zip)?)?;
    let mut nested = vec![];
    for i in 0..ar.len() {
        let mut f = ar.by_index(i)?;
        // Exporters write UTF-8 names but do not always set the UTF-8 flag.
        let name = String::from_utf8_lossy(f.name_raw()).replace('\\', "/");
        let rel: PathBuf = name
            .split('/')
            .filter(|c| !c.is_empty() && *c != "." && *c != "..")
            .collect();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(rel);
        if f.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(p) = out.parent() {
            fs::create_dir_all(p)?;
        }
        std::io::copy(&mut f, &mut fs::File::create(&out)?)?;
        if out.extension().is_some_and(|e| e == "zip") {
            nested.push(out);
        }
    }
    for z in nested {
        unzip(&z, z.parent().unwrap_or(dest))?;
        fs::remove_file(&z)?;
    }
    Ok(())
}

// ---- export tree

/// One exported page or database: `Title <id>.md`, `Title <id>[_all].csv`, children folder.
#[derive(Debug)]
struct Node {
    sid: String,
    title: String,
    stem: String,
    md: Option<PathBuf>,
    csv: Option<PathBuf>,
    dir: Option<PathBuf>,
}

fn is_hex_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `Title 0123…` → (title, id).
fn split_name(stem: &str) -> (String, Option<String>) {
    match stem.rsplit_once(' ') {
        Some((t, id)) if is_hex_id(id) => (t.to_string(), Some(id.to_lowercase())),
        _ => (stem.to_string(), None),
    }
}

/// Pages and databases of a folder in name order; unmatched folders are flattened in.
fn scan(dir: &Path) -> Result<Vec<Node>> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<_>>()?;
    entries.sort();
    let ext = |p: &Path, e: &str| p.is_file() && p.extension().is_some_and(|x| x == e);
    let stem = |p: &Path| {
        p.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let mut nodes: Vec<Node> = vec![];
    for p in entries.iter().filter(|p| ext(p, "md")) {
        let s = stem(p);
        let (title, id) = split_name(&s);
        nodes.push(Node {
            sid: id.unwrap_or_else(|| p.to_string_lossy().into_owned()),
            title,
            stem: s,
            md: Some(p.clone()),
            csv: None,
            dir: None,
        });
    }
    for p in entries.iter().filter(|p| ext(p, "csv")) {
        let s = stem(p);
        let all = s.ends_with("_all");
        let base = s.strip_suffix("_all").unwrap_or(&s).to_string();
        match nodes.iter_mut().find(|n| n.stem == base) {
            // `_all.csv` holds every row; the plain one only the default view's.
            Some(n) if n.csv.is_none() || all => n.csv = Some(p.clone()),
            Some(_) => {}
            None => {
                let (title, id) = split_name(&base);
                nodes.push(Node {
                    sid: id.unwrap_or_else(|| p.to_string_lossy().into_owned()),
                    title,
                    stem: base,
                    md: None,
                    csv: Some(p.clone()),
                    dir: None,
                });
            }
        }
    }
    for p in entries.iter().filter(|p| p.is_dir()) {
        let name = p.file_name().unwrap_or_default().to_string_lossy();
        // Older exports name the folder like the file, newer ones drop the id.
        let owner = nodes
            .iter()
            .position(|n| n.dir.is_none() && n.stem == name)
            .or_else(|| {
                nodes
                    .iter()
                    .position(|n| n.dir.is_none() && n.title == name)
            });
        match owner {
            Some(i) => nodes[i].dir = Some(p.clone()),
            None => nodes.extend(scan(p)?),
        }
    }
    Ok(nodes)
}

fn read(p: &Path) -> Result<String> {
    let s = fs::read_to_string(p).with_context(|| format!("read {}", p.display()))?;
    Ok(s.trim_start_matches('\u{feff}').to_string())
}

/// Splits an exported page into H1 title, leading `Key: value` lines (rows only) and body.
fn split_page(text: &str, props: bool) -> (Option<String>, Vec<(String, String)>, String) {
    let mut lines = text.lines().peekable();
    while lines.peek().is_some_and(|l| l.trim().is_empty()) {
        lines.next();
    }
    let title = lines
        .peek()
        .and_then(|l| l.strip_prefix("# "))
        .map(|t| t.trim().to_string());
    if title.is_some() {
        lines.next();
    }
    let mut kv = vec![];
    if props {
        while lines.peek().is_some_and(|l| l.trim().is_empty()) {
            lines.next();
        }
        while let Some((k, v)) = lines.peek().and_then(|l| prop_line(l)) {
            kv.push((k, v));
            lines.next();
        }
    }
    (title, kv, lines.collect::<Vec<_>>().join("\n"))
}

fn prop_line(l: &str) -> Option<(String, String)> {
    let (k, v) = l.split_once(':')?;
    let ok = !k.is_empty()
        && k.len() <= 60
        && k == k.trim()
        && !k.starts_with(['#', '-', '*', '>', '|', '!', '[', '`', '<'])
        && (v.is_empty() || v.starts_with(' '));
    ok.then(|| (k.to_string(), v.trim().to_string()))
}

/// Export id in a link target: `Page%20<id>.md`, `…_all.csv`, `https://…/Title-<id>?v=…`.
fn target_id(t: &str) -> Option<String> {
    let t = percent_decode(t);
    let path = t.split(['?', '#']).next()?;
    let seg = path.trim_end_matches('/').rsplit('/').next()?;
    let seg = seg
        .strip_suffix(".md")
        .or_else(|| seg.strip_suffix(".csv"))
        .unwrap_or(seg);
    let seg = seg.strip_suffix("_all").unwrap_or(seg);
    let tail = seg.get(seg.len().checked_sub(32)?..)?;
    let before = seg[..seg.len() - 32].chars().last();
    (is_hex_id(tail) && before.is_none_or(|c| c == ' ' || c == '-')).then(|| tail.to_lowercase())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b[i], hex) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Index of the `)` closing a link target (balanced parens allowed inside).
fn close_paren(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(i),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// `[label](target)` filling the whole line.
fn link_only(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix('[')?;
    let (label, rest) = rest.split_once("](")?;
    let end = close_paren(rest)?;
    (end + 1 == rest.len()).then(|| (label, &rest[..end]))
}

// ---- import

struct Fill {
    block: String,
    title: String,
    md: PathBuf,
    /// (source id, block id) of child pages, placed where the body links them.
    children: Vec<(String, String)>,
    row: bool,
}

struct Import<'a> {
    store: &'a Store,
    ws: String,
    files: PathBuf,
    /// Export id -> block id.
    ids: HashMap<String, String>,
    dbs: HashSet<String>,
    fill: Vec<Fill>,
    sum: Summary,
}

impl Import<'_> {
    /// Creates the node (and its subtree) under `parent`; bodies are filled later so links resolve.
    fn create(&mut self, n: &Node, parent: Option<&str>) -> Result<Option<String>> {
        if let Some(id) = self.store.find_by_source(&n.sid)? {
            self.sum.skipped += 1;
            self.ids.insert(n.sid.clone(), id.clone());
            if n.csv.is_some() {
                self.dbs.insert(n.sid.clone());
            } else if let Some(d) = &n.dir {
                for c in scan(d)? {
                    self.create(&c, Some(&id))?;
                }
            }
            return Ok(Some(id));
        }
        let text = n.md.as_deref().map(read).transpose()?;
        let title = text
            .as_deref()
            .and_then(|t| split_page(t, false).0)
            .unwrap_or_else(|| n.title.clone());
        if let Some(csv) = &n.csv {
            return match self.database(n, &title, csv, parent) {
                Ok(id) => Ok(Some(id)),
                Err(e) => {
                    self.sum
                        .warnings
                        .push(format!("database \"{title}\": {e:#}"));
                    Ok(None)
                }
            };
        }
        let b = self
            .store
            .create_block(&self.ws, parent, None, BlockType::Page)?;
        self.store
            .update_title(&b.id, &RichText::from_plain(&title))?;
        self.sum.pages += 1;
        self.page(n, &title, &b.id, false)?;
        Ok(Some(b.id))
    }

    fn page(&mut self, n: &Node, title: &str, block: &str, row: bool) -> Result<()> {
        self.store.set_prop(block, "source_id", json!(n.sid))?;
        self.ids.insert(n.sid.clone(), block.to_string());
        let mut children = vec![];
        if let Some(d) = &n.dir {
            for c in scan(d)? {
                if let Some(id) = self.create(&c, Some(block))? {
                    children.push((c.sid.clone(), id));
                }
            }
        }
        if let Some(md) = &n.md {
            self.fill.push(Fill {
                block: block.to_string(),
                title: title.to_string(),
                md: md.clone(),
                children,
                row,
            });
        }
        Ok(())
    }

    /// CSV + row pages → database; row `Key: value` lines fill columns the CSV lacks.
    fn database(
        &mut self,
        n: &Node,
        title: &str,
        csv: &Path,
        parent: Option<&str>,
    ) -> Result<String> {
        let mut records = parse_csv(&read(csv)?).into_iter();
        let header = records
            .next()
            .filter(|h| !h.is_empty())
            .context("empty CSV")?;
        let row_nodes = match &n.dir {
            Some(d) => scan(d)?,
            None => vec![],
        };
        let mut metas = vec![];
        for rn in &row_nodes {
            let text = rn.md.as_deref().map(read).transpose()?.unwrap_or_default();
            let (t, kv, _) = split_page(&text, true);
            metas.push((t.unwrap_or_else(|| rn.title.clone()), kv));
        }
        let mut cols: Vec<String> = header[1..].to_vec();
        let mut used = vec![false; row_nodes.len()];
        // (title, row page index, column -> cell)
        let mut rows: Vec<(String, Option<usize>, HashMap<String, String>)> = vec![];
        let md_cells = |i: usize, cells: &mut HashMap<String, String>| {
            for (k, v) in &metas[i].1 {
                cells.entry(k.clone()).or_insert_with(|| v.clone());
            }
        };
        for rec in records {
            let rt = rec.first().cloned().unwrap_or_default();
            let mut cells: HashMap<String, String> = header
                .iter()
                .cloned()
                .zip(rec.iter().cloned())
                .skip(1)
                .collect();
            let page = (0..metas.len()).find(|&i| !used[i] && metas[i].0 == rt);
            if let Some(i) = page {
                used[i] = true;
                md_cells(i, &mut cells);
            }
            rows.push((rt, page, cells));
        }
        for i in (0..metas.len()).filter(|&i| !used[i]) {
            let mut cells = HashMap::new();
            md_cells(i, &mut cells);
            rows.push((metas[i].0.clone(), Some(i), cells));
        }
        for (_, _, cells) in &rows {
            let mut extra: Vec<&String> = cells.keys().filter(|k| !cols.contains(k)).collect();
            extra.sort();
            cols.extend(extra.into_iter().cloned());
        }

        let db = self.store.create_database(&self.ws, parent, None, true)?.id;
        self.store.update_title(&db, &RichText::from_plain(title))?;
        self.store.set_prop(&db, "source_id", json!(n.sid))?;
        self.store
            .update_property(&db, "title", Property::new(&header[0], PropType::Title))?;
        self.ids.insert(n.sid.clone(), db.clone());
        self.dbs.insert(n.sid.clone());
        self.sum.databases += 1;
        let mut props = vec![];
        for c in &cols {
            let vals: Vec<&str> = rows
                .iter()
                .filter_map(|r| r.2.get(c).map(String::as_str))
                .collect();
            let kind = infer(&vals);
            props.push((
                self.store.add_property(&db, c, kind)?,
                Property::new(c, kind),
            ));
        }
        for (rt, page, cells) in &rows {
            let row = self.store.add_row(&db)?.id;
            self.store.update_title(&row, &RichText::from_plain(rt))?;
            for (pid, prop) in &mut props {
                if let Some(v) = cells.get(&prop.name).and_then(|s| cell_value(prop, s)) {
                    self.store.set_row_value(&row, pid, v)?;
                }
            }
            if let Some(i) = page {
                self.page(&row_nodes[*i], rt, &row, true)?;
            }
            self.sum.rows += 1;
        }
        for (pid, prop) in props {
            if !prop.options.is_empty() {
                self.store.update_property(&db, &pid, prop)?;
            }
        }
        Ok(db)
    }

    fn fill(&mut self, f: Fill) -> Result<()> {
        let text = read(&f.md)?;
        let (_, _, body) = split_page(&text, f.row);
        let dir = f.md.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut chunk = String::new();
        let mut placed = HashSet::new();
        for line in body.lines() {
            if let Some((label, target)) = link_only(line.trim())
                && let Some(sid) = target_id(target)
            {
                if let Some((_, child)) = f.children.iter().find(|c| c.0 == sid) {
                    self.flush(&f.block, &mut chunk)?;
                    self.store.move_block(child, Some(&f.block), usize::MAX)?;
                    placed.insert(child.clone());
                    continue;
                }
                if percent_decode(target).ends_with(".csv") && !self.dbs.contains(&sid) {
                    self.sum.warnings.push(format!(
                        "\"{}\": database \"{label}\" is not in the export (linked view?), skipped",
                        f.title
                    ));
                    continue;
                }
            }
            chunk.push_str(&self.rewrite(line, &dir, &f.title));
            chunk.push('\n');
        }
        self.flush(&f.block, &mut chunk)?;
        for (_, child) in &f.children {
            if !placed.contains(child) {
                self.store.move_block(child, Some(&f.block), usize::MAX)?;
            }
        }
        Ok(())
    }

    fn flush(&mut self, block: &str, chunk: &mut String) -> Result<()> {
        if !chunk.trim().is_empty() {
            self.store.markdown_to_blocks(block, chunk)?;
        }
        chunk.clear();
        Ok(())
    }

    /// Rewrites link targets: exported pages → redbook://page/<id>, local files → copies.
    fn rewrite(&mut self, line: &str, dir: &Path, page: &str) -> String {
        let mut out = String::new();
        let mut rest = line;
        while let Some(k) = rest.find("](") {
            let image = rest[..k]
                .rfind('[')
                .is_some_and(|s| rest[..s].ends_with('!'));
            out.push_str(&rest[..k + 2]);
            let after = &rest[k + 2..];
            let Some(end) = close_paren(after) else {
                rest = after;
                continue;
            };
            out.push_str(&self.target(&after[..end], dir, image, page));
            out.push(')');
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        out
    }

    fn target(&mut self, t: &str, dir: &Path, image: bool, page: &str) -> String {
        if let Some(id) = target_id(t).and_then(|s| self.ids.get(&s)) {
            return format!("redbook://page/{id}");
        }
        if t.contains("://") || t.starts_with("mailto:") || t.starts_with('#') {
            return t.to_string();
        }
        let src = dir.join(percent_decode(t));
        match self.copy_file(&src) {
            Ok(p) => p,
            Err(e) => {
                let what = if image { "image" } else { "file" };
                self.sum
                    .warnings
                    .push(format!("\"{page}\": {what} {t}: {e:#}"));
                t.to_string()
            }
        }
    }

    fn copy_file(&mut self, src: &Path) -> Result<String> {
        if !src.is_file() {
            bail!("not found in export");
        }
        let dest = copy_to_files(src, &self.files)?;
        self.sum.files += 1;
        Ok(dest.to_string_lossy().into_owned())
    }
}

/// Copies `src` into the app's files dir under a unique, filesystem-safe name.
pub fn copy_to_files(src: &Path, files: &Path) -> Result<PathBuf> {
    let name: String = src
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    fs::create_dir_all(files)?;
    let dest = files.join(format!(
        "{}-{name}",
        &Uuid::new_v4().simple().to_string()[..8]
    ));
    fs::copy(src, &dest)?;
    Ok(dest)
}

// ---- CSV and values

fn parse_csv(s: &str) -> Vec<Vec<String>> {
    let (mut rows, mut row, mut field) = (vec![], vec![], String::new());
    let (mut quoted, mut chars) = (false, s.chars().peekable());
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' if quoted => quoted = false,
            '"' if field.is_empty() => quoted = true,
            ',' if !quoted => row.push(std::mem::take(&mut field)),
            '\n' if !quoted => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            '\r' if !quoted => {}
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.retain(|r: &Vec<String>| r.iter().any(|c| !c.is_empty()));
    rows
}

fn parse_number(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', "");
    if !s.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.') {
        return None;
    }
    s.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// `October 11, 2026[ 3:45 PM]`, `2026-10-11[T15:45]`, `2026/10/11`; ranges joined by ` → `.
pub fn parse_date(s: &str) -> Option<Value> {
    let (a, b) = match s.split_once('→') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let mut v = json!({ "start": one_date(a)? });
    if let Some(b) = b {
        v["end"] = json!(one_date(b)?);
    }
    Some(v)
}

fn one_date(s: &str) -> Option<String> {
    let mut s = s.trim();
    if s.ends_with(')')
        && let Some(i) = s.rfind(" (")
    {
        s = s[..i].trim(); // "(GMT+3)"
    }
    let (date, time) = if s.len() >= 10
        && s.as_bytes()[4].is_ascii_punctuation()
        && s[..4].bytes().all(|b| b.is_ascii_digit())
    {
        let p: Vec<&str> = s[..10].split(['-', '/']).collect();
        let n = |x: &&str| x.parse::<u32>().ok();
        let (y, m, d) = (n(p.first()?)?, n(p.get(1)?)?, n(p.get(2)?)?);
        (
            ymd(y, m, d)?,
            s[10..].trim_start_matches(['T', ' ']).to_string(),
        )
    } else {
        const MONTHS: [&str; 12] = [
            "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
        ];
        let mut it = s.split_whitespace();
        let mon = it.next()?.to_lowercase();
        let m = MONTHS
            .iter()
            .position(|x| mon.starts_with(x) && mon.len() >= 3)? as u32
            + 1;
        let d = it.next()?.trim_end_matches(',').parse().ok()?;
        let y = it.next()?.parse().ok()?;
        (ymd(y, m, d)?, it.collect::<Vec<_>>().join(" "))
    };
    if time.is_empty() {
        return Some(date);
    }
    let t = time.to_lowercase();
    let (hm, pm) = match (t.strip_suffix("pm"), t.strip_suffix("am")) {
        (Some(x), _) => (x.trim(), Some(true)),
        (_, Some(x)) => (x.trim(), Some(false)),
        _ => (t.trim(), None),
    };
    let (h, m) = hm.split_once(':')?;
    let (mut h, m): (u32, u32) = (h.parse().ok()?, m.get(..2)?.parse().ok()?);
    if let Some(pm) = pm {
        h = h % 12 + if pm { 12 } else { 0 };
    }
    (h < 24 && m < 60).then(|| format!("{date}T{h:02}:{m:02}"))
}

fn ymd(y: u32, m: u32, d: u32) -> Option<String> {
    ((1..=12).contains(&m) && (1..=31).contains(&d) && y > 0)
        .then(|| format!("{y:04}-{m:02}-{d:02}"))
}

fn is_email(s: &str) -> bool {
    s.split_once('@')
        .is_some_and(|(a, b)| !a.is_empty() && b.contains('.') && !s.contains(char::is_whitespace))
}

/// Property type guessed from a column's cells.
fn infer(vals: &[&str]) -> PropType {
    let v: Vec<&str> = vals
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let all = |f: &dyn Fn(&str) -> bool| v.iter().all(|s| f(s));
    if v.is_empty() {
        return PropType::Text;
    }
    if all(&|s| s == "Yes" || s == "No") {
        return PropType::Checkbox;
    }
    if all(&|s| parse_number(s).is_some()) {
        return PropType::Number;
    }
    if all(&|s| parse_date(s).is_some()) {
        return PropType::Date;
    }
    if all(&|s| (s.starts_with("http://") || s.starts_with("https://")) && !s.contains(' ')) {
        return PropType::Url;
    }
    if all(&is_email) {
        return PropType::Email;
    }
    let items: Vec<&str> = v.iter().flat_map(|s| s.split(", ")).collect();
    let distinct: HashSet<&str> = items.iter().copied().collect();
    if !all(&|s| s.chars().count() <= 60 && !s.contains('\n')) || distinct.len() > 30 {
        return PropType::Text;
    }
    if v.iter().any(|s| s.contains(", ")) && items.iter().all(|s| s.chars().count() <= 30) {
        return PropType::MultiSelect;
    }
    let values: HashSet<&str> = v.iter().copied().collect();
    if values.len() < v.len() && values.len() <= 12 {
        return PropType::Select;
    }
    PropType::Text
}

pub fn ensure_option(prop: &mut Property, value: &str) {
    if prop.option_index(value).is_none() {
        prop.options.push(SelectOption {
            id: Uuid::new_v4().simple().to_string()[..8].to_string(),
            value: value.to_string(),
            color: COLORS[prop.options.len() % COLORS.len()].to_string(),
        });
    }
}

/// Stored value for a text cell; adds missing select options to `prop`.
pub fn cell_value(prop: &mut Property, s: &str) -> Option<Value> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    Some(match prop.kind {
        PropType::Checkbox => json!(matches!(
            s.to_lowercase().as_str(),
            "yes" | "true" | "1" | "x" | "checked"
        )),
        PropType::Number => json!(parse_number(s)?),
        PropType::Date => parse_date(s)?,
        PropType::Select | PropType::Status => {
            ensure_option(prop, s);
            json!(s)
        }
        PropType::MultiSelect => {
            let items: Vec<&str> = s
                .split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .collect();
            for i in &items {
                ensure_option(prop, i);
            }
            json!(items)
        }
        _ => json!(s),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(
            target_id("Sub%20Page%200123456789abcdef0123456789ABCDEF.md").as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(
            target_id("https://example.com/My-Page-0123456789abcdef0123456789abcdef?pvs=4")
                .as_deref(),
            Some("0123456789abcdef0123456789abcdef")
        );
        assert_eq!(target_id("https://example.com/x"), None);
        assert_eq!(
            link_only("[a](W%20(1%E2%80%932)%20x.md)"),
            Some(("a", "W%20(1%E2%80%932)%20x.md"))
        );
        assert_eq!(percent_decode("a%20b%E2%80%93c"), "a b–c");
        assert_eq!(
            parse_csv("\u{feff}A,B\n\"x, \"\"y\"\"\",2\n\n"),
            [vec!["\u{feff}A", "B"], vec!["x, \"y\"", "2"]]
        );
        assert_eq!(
            parse_date("October 11, 2026"),
            Some(json!({"start": "2026-10-11"}))
        );
        assert_eq!(
            parse_date("October 11, 2026 3:05 PM (GMT+3) → October 12, 2026"),
            Some(json!({"start": "2026-10-11T15:05", "end": "2026-10-12"}))
        );
        assert_eq!(
            parse_date("2026/01/02"),
            Some(json!({"start": "2026-01-02"}))
        );
        assert_eq!(infer(&["Yes", "No", ""]), PropType::Checkbox);
        assert_eq!(infer(&["1", "2.5"]), PropType::Number);
        assert_eq!(infer(&["a, b", "c"]), PropType::MultiSelect);
        assert_eq!(infer(&["Done", "Todo", "Done"]), PropType::Select);
        assert_eq!(infer(&["x@y.dev"]), PropType::Email);
        assert_eq!(infer(&["free text", "more text"]), PropType::Text);
        assert_eq!(
            prop_line("Email: a@b.c"),
            Some(("Email".into(), "a@b.c".into()))
        );
        assert_eq!(prop_line("https://x.dev"), None);
    }

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/markdown-export")
    }

    #[test]
    fn imports_fixture_export() {
        let store = Store::open_in_memory().unwrap();
        let files = std::env::temp_dir().join(format!("redbook-files-{}", Uuid::new_v4()));
        let sum = import(&store, &fixture(), Some("Imported"), &files).unwrap();
        assert_eq!(
            (sum.pages, sum.databases, sum.rows, sum.files),
            (5, 1, 3, 1),
            "{sum}"
        );
        assert_eq!(sum.warnings.len(), 1, "{sum}"); // linked view
        let ws = resolve_workspace(&store, Some("Imported"), false).unwrap();
        let tree = store.page_tree(&ws).unwrap();
        let find = |t: &str| {
            tree.iter()
                .find(|n| n.title == t)
                .unwrap_or_else(|| panic!("{t}: {tree:?}"))
        };
        let (proj, sub, db) = (find("Projects"), find("Sub page – ı"), find("Tasks"));
        assert_eq!(sub.parent_id.as_deref(), Some(proj.id.as_str()));
        assert_eq!(db.parent_id.as_deref(), Some(proj.id.as_str()));

        // Children sit where the body linked them.
        let kids = store.children(&proj.id).unwrap();
        let kinds: Vec<&str> = kids.iter().map(|b| b.kind.as_str()).collect();
        assert_eq!(
            kinds,
            [
                "text",
                "page",
                "toggle",
                "callout",
                "collection_view_page",
                "bulleted_list",
                "bulleted_list",
                "text",
                "page",
                "image",
                "text",
                "page"
            ]
        );
        let titles: Vec<String> = [1, 7, 8, 11]
            .iter()
            .map(|&i| kids[i].title.plain_text())
            .collect();
        assert_eq!(
            titles,
            ["Sub page – ı", "Between text.", "Second", "Unlinked"]
        );
        assert_eq!(kids[3].icon(), Some("💡"));
        assert_eq!(
            store.children(&kids[2].id).unwrap()[0].title.plain_text(),
            "Hidden text"
        );
        assert_eq!(store.children(&kids[5].id).unwrap().len(), 1);
        let img = kids[9].str_prop("source").unwrap();
        assert!(img.starts_with(files.to_str().unwrap()) && Path::new(img).exists());
        let link = kids[10].title.0.iter().find_map(|s| s.link()).unwrap();
        assert_eq!(link, format!("redbook://page/{}", find("Ideas").id));

        // Database: inferred types, CSV merged with row pages.
        let schema = store.schema(&db.id).unwrap();
        let kind = |n: &str| schema.values().find(|p| p.name == n).unwrap().kind;
        assert_eq!(kind("Task"), PropType::Title);
        assert_eq!(kind("Done"), PropType::Checkbox);
        assert_eq!(kind("Points"), PropType::Number);
        assert_eq!(kind("Due"), PropType::Date);
        assert_eq!(kind("Status"), PropType::Select);
        assert_eq!(kind("Tags"), PropType::MultiSelect);
        assert_eq!(kind("Owner"), PropType::Email);
        let rows = store.rows(&db.id).unwrap();
        let write = rows
            .iter()
            .find(|r| r.title.plain_text() == "Write docs")
            .unwrap();
        let (due, _) = schema.iter().find(|(_, p)| p.name == "Due").unwrap();
        assert_eq!(
            write.props["properties"][due],
            json!({"start": "2026-10-11"})
        );
        assert_eq!(
            store.children(&write.id).unwrap()[0].title.plain_text(),
            "Row body text."
        );

        // Re-import creates nothing new.
        let again = import(&store, &fixture(), Some("Imported"), &files).unwrap();
        assert_eq!(
            (again.pages, again.databases, again.rows, again.skipped),
            (0, 0, 0, 6),
            "{again}"
        );
        fs::remove_dir_all(files).ok();
    }
}
