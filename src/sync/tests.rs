// Sync tests: local bare remote, two temp databases, real git CLI, no network.
use super::*;
use crate::model::*;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("redbook-sync-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn bare(t: &Path) -> String {
    let r = t.join("remote.git");
    let ok = Command::new("git")
        .args(["init", "-q", "--bare", "-b", "main"])
        .arg(&r)
        .status()
        .unwrap();
    assert!(ok.success());
    r.to_string_lossy().into_owned()
}

fn set_host(db: &Path, name: &str) {
    Store::open(db)
        .unwrap()
        .conn()
        .execute(
            "UPDATE sync_state SET value = ?1 WHERE key = 'host'",
            [name],
        )
        .unwrap();
}

fn sync(db: &Path) -> Report {
    run(db, Mode::Full, false).unwrap()
}

fn text(s: &Store, parent: &str, t: &str) -> String {
    let ws = s.block(parent).unwrap().workspace_id;
    let b = s
        .create_block(&ws, Some(parent), None, BlockType::Text)
        .unwrap();
    s.update_title(&b.id, &RichText::from_plain(t)).unwrap();
    b.id
}

fn page(s: &Store, ws: &str, title: &str) -> String {
    let p = s.create_block(ws, None, None, BlockType::Page).unwrap();
    s.update_title(&p.id, &RichText::from_plain(title)).unwrap();
    p.id
}

fn title(s: &Store, id: &str) -> String {
    s.block(id).unwrap().title.plain_text()
}

/// Page ids whose title contains `needle`.
fn pages_titled(s: &Store, needle: &str) -> Vec<String> {
    let mut st = s
        .conn()
        .prepare("SELECT id, title FROM blocks WHERE type = 'page'")
        .unwrap();
    st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .filter(|(_, t)| RichText::from_json_str(t).plain_text().contains(needle))
        .map(|(id, _)| id)
        .collect()
}

fn texts(s: &Store, page: &str) -> Vec<String> {
    s.children(page)
        .unwrap()
        .iter()
        .map(|b| b.title.plain_text())
        .collect()
}

fn all_ids(s: &Store, table: &str) -> Vec<String> {
    let mut st = s
        .conn()
        .prepare(&format!("SELECT id FROM {table} ORDER BY id"))
        .unwrap();
    st.query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// Every block type, rich text, props, favorites, trash, tables and a database.
fn fill(a: &Store, ws: &str, files: &Path) {
    let p = page(a, ws, "Hello");
    let rich = RichText(vec![
        Segment {
            text: "plain ".into(),
            annotations: vec![],
        },
        Segment {
            text: "bold link".into(),
            annotations: vec![
                Annotation::Bold,
                Annotation::Link("https://x.y".into()),
                Annotation::Color("red".into()),
            ],
        },
        Segment {
            text: "⁍".into(),
            annotations: vec![Annotation::Equation("e^x".into())],
        },
        Segment {
            text: "‣".into(),
            annotations: vec![Annotation::Date(
                json!({"type": "date", "start_date": "2026-10-11"}),
            )],
        },
    ]);
    a.update_title(&p, &rich).unwrap();
    a.set_prop(&p, "icon", json!("📝")).unwrap();
    a.set_prop(&p, "cover", json!("gradient_2")).unwrap();
    a.toggle_favorite(&p).unwrap();
    let toggle = a
        .create_block(ws, Some(&p), None, BlockType::Toggle)
        .unwrap()
        .id;
    text(a, &toggle, "inside toggle");
    let todo = a
        .create_block(ws, Some(&p), None, BlockType::ToDo)
        .unwrap()
        .id;
    a.set_prop(&todo, "checked", json!(true)).unwrap();
    let first = a.children(&p).unwrap()[0].id.clone();
    let mid = a
        .create_block(ws, Some(&p), Some(&first), BlockType::Code)
        .unwrap()
        .id; // fractional sort
    a.set_prop(&mid, "language", json!("Rust")).unwrap();
    fs::create_dir_all(files).unwrap();
    fs::write(files.join("abcd1234-pic.png"), b"png bytes").unwrap();
    let img = a
        .create_block(ws, Some(&p), None, BlockType::Image)
        .unwrap()
        .id;
    a.set_prop(
        &img,
        "source",
        json!(files.join("abcd1234-pic.png").to_string_lossy()),
    )
    .unwrap();
    let table = a
        .create_block(ws, Some(&p), None, BlockType::Table)
        .unwrap()
        .id;
    a.fill_table(&table, 2, 2).unwrap();
    let row = a.children(&table).unwrap()[0].id.clone();
    a.set_prop(
        &row,
        "cells",
        serde_json::from_str(r#"[[["a",[["b"]]]],[["c"]]]"#).unwrap(),
    )
    .unwrap();
    let sub = a
        .create_block(ws, Some(&p), None, BlockType::Page)
        .unwrap()
        .id;
    a.markdown_to_blocks(&sub, "# Head\n\n- one\n  - nested\n\n> quote")
        .unwrap();
    let dbk = a.create_database(ws, Some(&p), None, false).unwrap().id;
    let status = a.add_property(&dbk, "Status", PropType::Select).unwrap();
    let mut prop = a.schema(&dbk).unwrap()[&status].clone();
    prop.options = vec![SelectOption {
        id: "o1".into(),
        value: "Done".into(),
        color: "green".into(),
    }];
    a.update_property(&dbk, &status, prop).unwrap();
    let r1 = a.add_row(&dbk).unwrap().id;
    a.set_row_value(&r1, "title", json!("Row 1")).unwrap();
    a.set_row_value(&r1, &status, json!("Done")).unwrap();
    text(a, &r1, "row body");
    let mut view = a.create_view(&dbk, ViewType::Board, "Board").unwrap();
    view.query.sorts = vec![Sort {
        property: status.clone(),
        direction: Direction::Descending,
    }];
    view.format = json!({"group_by": status});
    a.update_view(&view).unwrap();
    let gone = page(a, ws, "Trashed");
    text(a, &gone, "trashed body");
    a.trash(&gone).unwrap();
}

#[test]
fn lossless_round_trip() {
    let t = tmp("roundtrip");
    let (db_a, db_b) = (t.join("a/redbook.db"), t.join("b/redbook.db"));
    let a = Store::open(&db_a).unwrap();
    let ws = a.create_workspace("Work").unwrap().id;
    a.set_workspace_icon(&ws, Some("💼")).unwrap();
    let files = t.join("a/files");
    fill(&a, &ws, &files);
    enable(&a).unwrap();
    mark_all_dirty(&a).unwrap();
    let dir = t.join("tree");
    export(&a, &dir, &files).unwrap();
    let paths: Vec<String> = ["pages", "collections"]
        .iter()
        .flat_map(|d| fs::read_dir(dir.join(d)).unwrap())
        .map(|e| {
            let e = e.unwrap();
            let d = e
                .path()
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            format!("{d}/{}", e.file_name().to_string_lossy())
        })
        .chain(["workspaces.jsonl".to_string()])
        .collect();
    let b = open(&db_b).unwrap();
    let files_b = t.join("b/files");
    import(&b, &dir, &files_b, &paths, vec![], "").unwrap();

    assert_eq!(a.workspaces().unwrap(), b.workspaces().unwrap());
    let ids = all_ids(&a, "blocks");
    assert_eq!(ids, all_ids(&b, "blocks"));
    for id in &ids {
        let mut x = a.block(id).unwrap();
        if let Some(src) = x.str_prop("source").map(str::to_string) {
            let moved = src.replace(&*files.to_string_lossy(), &files_b.to_string_lossy());
            x.props["source"] = json!(moved);
            assert_eq!(fs::read(&moved).unwrap(), b"png bytes");
        }
        assert_eq!(x, b.block(id).unwrap());
    }
    for c in all_ids(&a, "collections") {
        assert_eq!(a.schema(&c).unwrap(), b.schema(&c).unwrap());
        assert_eq!(a.views(&c).unwrap(), b.views(&c).unwrap());
    }
    // Deterministic: exporting B gives byte-identical page files.
    enable(&b).unwrap();
    mark_all_dirty(&b).unwrap();
    let dir_b = t.join("tree-b");
    export(&b, &dir_b, &files_b).unwrap();
    for p in paths.iter().filter(|p| p.starts_with("pages/")) {
        assert_eq!(
            fs::read_to_string(dir.join(p)).unwrap(),
            fs::read_to_string(dir_b.join(p)).unwrap(),
            "{p}"
        );
    }
    assert!(
        fs::read_to_string(dir.join("README.md"))
            .unwrap()
            .contains("bold link")
    );
    fs::remove_dir_all(&t).ok();
}

#[test]
fn two_devices() {
    let t = tmp("two");
    let remote = bare(&t);
    let (db_a, db_b) = (t.join("a/redbook.db"), t.join("b/redbook.db"));
    let a = Store::open(&db_a).unwrap();
    let ws = a.create_workspace("Personal").unwrap().id;
    let x = page(&a, &ws, "X");
    let y = page(&a, &ws, "Y");
    let y1 = text(&a, &y, "y one");
    let z = page(&a, &ws, "Z");
    let zs: Vec<String> = (1..=3).map(|i| text(&a, &z, &format!("z{i}"))).collect();
    init(&db_a, &remote, Some(t.join("a/sync")), false).unwrap();
    set_host(&db_a, "devA");
    clone(&db_b, &remote, Some(t.join("b/sync")), false, false).unwrap();
    set_host(&db_b, "devB");
    let b = Store::open(&db_b).unwrap();
    assert_eq!(texts(&b, &z), ["z1", "z2", "z3"]);
    assert_eq!(b.workspaces().unwrap().len(), 1);

    // Different pages.
    a.update_title(&x, &RichText::from_plain("X by A")).unwrap();
    b.update_title(&y, &RichText::from_plain("Y by B")).unwrap();
    sync(&db_a);
    sync(&db_b);
    sync(&db_a);
    for s in [&a, &b] {
        assert_eq!(
            (title(s, &x), title(s, &y)),
            ("X by A".into(), "Y by B".into())
        );
    }

    // Same page, adjacent blocks.
    a.update_title(&zs[0], &RichText::from_plain("z1 by A"))
        .unwrap();
    b.update_title(&zs[1], &RichText::from_plain("z2 by B"))
        .unwrap();
    sync(&db_a);
    let r = sync(&db_b);
    assert_eq!(r.conflicts, 0);
    sync(&db_a);
    for s in [&a, &b] {
        assert_eq!(texts(s, &z), ["z1 by A", "z2 by B", "z3"]);
        assert!(pages_titled(s, "conflict").is_empty());
    }

    // Same block: B keeps its text, A's lands in a conflict copy; nothing lost.
    a.update_title(&zs[2], &RichText::from_plain("z3 by A"))
        .unwrap();
    b.update_title(&zs[2], &RichText::from_plain("z3 by B"))
        .unwrap();
    sync(&db_a);
    assert_eq!(sync(&db_b).conflicts, 1);
    sync(&db_a);
    for s in [&a, &b] {
        assert_eq!(texts(s, &z)[2], "z3 by B");
        let copies = pages_titled(s, "(conflict devA");
        assert_eq!(copies.len(), 1, "conflict copy");
        assert_eq!(texts(s, &copies[0]), ["z1 by A", "z2 by B", "z3 by A"]);
        assert_eq!(s.block(&copies[0]).unwrap().parent_id, None);
    }

    // Deleted on A, edited on B: the edit wins on both.
    a.delete_forever(&y).unwrap();
    b.update_title(&y1, &RichText::from_plain("y one edited"))
        .unwrap();
    sync(&db_a);
    sync(&db_b);
    sync(&db_a);
    for s in [&a, &b] {
        assert_eq!(texts(s, &y), ["y one edited"]);
    }

    // Database: schema change on A, row edits on both.
    let dbk = a.create_database(&ws, None, None, true).unwrap().id;
    let st = a.add_property(&dbk, "Status", PropType::Text).unwrap();
    let r1 = a.add_row(&dbk).unwrap().id;
    let r2 = a.add_row(&dbk).unwrap().id;
    sync(&db_a);
    sync(&db_b);
    a.set_row_value(&r1, &st, json!("a")).unwrap();
    a.add_property(&dbk, "Due", PropType::Date).unwrap();
    b.set_row_value(&r2, &st, json!("b")).unwrap();
    sync(&db_a);
    sync(&db_b);
    sync(&db_a);
    for s in [&a, &b] {
        let p = &s.schema(&dbk).unwrap()[&st];
        assert_eq!(s.block(&r1).unwrap().row_value(&st, p), json!("a"));
        assert_eq!(s.block(&r2).unwrap().row_value(&st, p), json!("b"));
        assert_eq!(s.schema(&dbk).unwrap().len(), 3);
        assert_eq!(s.views(&dbk).unwrap().len(), 1);
    }

    // Image picked from anywhere on A arrives as a file on B.
    let ext = t.join("photo.jpg");
    fs::write(&ext, b"jpeg bytes").unwrap();
    let img = a
        .create_block(&ws, Some(&x), None, BlockType::Image)
        .unwrap()
        .id;
    a.set_prop(&img, "source", json!(ext.to_string_lossy()))
        .unwrap();
    sync(&db_a);
    sync(&db_b);
    let src = b
        .block(&img)
        .unwrap()
        .str_prop("source")
        .unwrap()
        .to_string();
    // Canonicalize: Windows may report the temp dir in 8.3 short form.
    let dir = fs::canonicalize(Path::new(&src).parent().unwrap()).unwrap();
    assert_eq!(dir, fs::canonicalize(t.join("b/files")).unwrap(), "{src}");
    assert_eq!(fs::read(&src).unwrap(), b"jpeg bytes");

    // A cover image travels the same way.
    a.set_prop(&x, "cover", json!(ext.to_string_lossy()))
        .unwrap();
    sync(&db_a);
    sync(&db_b);
    let cover = b.block(&x).unwrap().str_prop("cover").unwrap().to_string();
    let cdir = fs::canonicalize(Path::new(&cover).parent().unwrap()).unwrap();
    assert_eq!(
        cdir,
        fs::canonicalize(t.join("b/files")).unwrap(),
        "{cover}"
    );

    // Trash on B, then nothing left to sync.
    b.trash(&x).unwrap();
    sync(&db_b);
    sync(&db_a);
    assert!(a.block(&x).unwrap().trashed.is_some());
    assert_eq!(sync(&db_a).exported, 0);
    let pending: i64 = a
        .conn()
        .query_row("SELECT count(*) FROM sync_dirty", [], |r| r.get(0))
        .unwrap();
    assert_eq!(pending, 0);
    assert!(status(&db_a).unwrap().error.is_none());
    fs::remove_dir_all(&t).ok();
}

#[test]
fn switch_sync_folder() {
    let t = tmp("switch");
    let remote = bare(&t);
    let (db_a, db_b) = (t.join("a/redbook.db"), t.join("b/redbook.db"));
    let a = Store::open(&db_a).unwrap();
    let ws = a.create_workspace("W").unwrap().id;
    let p = page(&a, &ws, "P");
    init(&db_a, &remote, Some(t.join("one")), false).unwrap();

    let two = t.join("two");
    fs::create_dir_all(&two).unwrap();
    switch_dir(&db_a, &two).unwrap();
    assert_eq!(dir(&db_a), two);
    assert!(two.join(".git").is_dir() && !t.join("one").exists());
    let saved = fs::read_to_string(settings_path(&db_a)).unwrap();
    assert!(saved.contains("two"), "{saved}");
    a.update_title(&p, &RichText::from_plain("P2")).unwrap();
    sync(&db_a);

    clone(&db_b, &remote, Some(t.join("b-tree")), false, false).unwrap();
    switch_dir(&db_a, &t.join("b-tree")).unwrap();
    assert_eq!(dir(&db_a), t.join("b-tree"));
    sync(&db_a);

    let other = t.join("other");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("x.txt"), "x").unwrap();
    assert!(switch_dir(&db_a, &other).is_err());
    let foreign = t.join("foreign");
    bare(&foreign);
    assert!(switch_dir(&db_a, &foreign.join("remote.git")).is_err());
    assert_eq!(dir(&db_a), t.join("b-tree"));
    fs::remove_dir_all(&t).ok();
}

#[test]
fn record_merge() {
    let l = |id: &str, v: &str| format!("{{\"id\":\"{id}\",\"v\":\"{v}\"}}\n");
    let base = l("a", "0") + &l("b", "0") + &l("c", "0");
    let ours = l("a", "1") + &l("b", "0") + &l("c", "0");
    let theirs = l("a", "0") + &l("b", "2") + &l("d", "2");
    assert_eq!(
        merge_records(&base, &ours, &theirs),
        (l("a", "1") + &l("b", "2") + &l("d", "2"), false)
    );
    // Deleted vs edited keeps the edit; both edited keeps ours and flags it.
    let theirs = l("a", "2") + &l("b", "0");
    let ours2 = l("a", "1") + &l("c", "1");
    assert_eq!(
        merge_records(&base, &ours2, &theirs),
        (l("a", "1") + &l("c", "1"), true)
    );
    assert_eq!(expand("~"), dirs::home_dir().unwrap());
}
