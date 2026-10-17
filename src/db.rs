// Shared store + change signal for all views.
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use futures::StreamExt as _;
use gpui_kit::*;
use notify::Watcher as _;

use crate::store::Store;

/// Bumped on every write; views `cx.observe(&db::revision(cx), ..)` to reload.
pub struct Revision(pub u64);

struct Db {
    store: Rc<Store>,
    revision: Entity<Revision>,
    /// `PRAGMA data_version` at the last sync; only other connections' commits change it.
    seen: Cell<Option<i64>>,
}

impl Global for Db {}

pub fn init(store: Store, cx: &mut App) {
    let revision = cx.new(|_| Revision(0));
    let seen = Cell::new(store.data_version().ok());
    cx.set_global(Db {
        store: Rc::new(store),
        revision,
        seen,
    });
}

pub fn store(cx: &App) -> Rc<Store> {
    cx.global::<Db>().store.clone()
}

pub fn revision(cx: &App) -> Entity<Revision> {
    cx.global::<Db>().revision.clone()
}

/// Call after any write so other views refresh.
pub fn changed(cx: &mut App) {
    revision(cx).update(cx, |r, cx| {
        r.0 += 1;
        cx.notify();
    });
}

/// Reloads all views if another connection (the MCP server) committed since the last call.
pub fn sync_external(cx: &mut App) {
    let db = cx.global::<Db>();
    let v = db.store.data_version().ok();
    if db.seen.replace(v) != v {
        changed(cx);
    }
}

/// Runs `sync_external` shortly after `db` or its WAL changes on disk; None if it can't watch.
pub fn watch(db: &Path, cx: &mut App) -> Option<Task<()>> {
    let dir = db.parent()?.to_path_buf();
    let name = db.file_name()?.to_os_string();
    let mut wal = name.clone();
    wal.push("-wal");
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let watcher = notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
        let ours = ev.is_ok_and(|e| {
            e.paths
                .iter()
                .any(|p| p.file_name().is_some_and(|n| n == name || n == wal))
        });
        if ours {
            let _ = tx.unbounded_send(());
        }
    })
    .and_then(|mut w| {
        w.watch(&dir, notify::RecursiveMode::NonRecursive)
            .map(|_| w)
    })
    .inspect_err(|e| eprintln!("db watcher unavailable, falling back to polling: {e}"))
    .ok()?;
    Some(cx.spawn(async move |cx| {
        let _watcher = watcher;
        while rx.next().await.is_some() {
            // Coalesce a commit's burst of file events into one check.
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            while rx.try_recv().is_ok() {}
            cx.update(sync_external);
        }
    }))
}

/// Currently open page; the shell observes it, any view calls `open_page` to navigate.
pub struct Nav {
    pub page: Option<String>,
    pub workspace: Option<String>,
}

struct NavGlobal(Entity<Nav>);
impl Global for NavGlobal {}

pub fn nav(cx: &mut App) -> Entity<Nav> {
    if !cx.has_global::<NavGlobal>() {
        let e = cx.new(|_| Nav {
            page: None,
            workspace: None,
        });
        cx.set_global(NavGlobal(e));
    }
    cx.global::<NavGlobal>().0.clone()
}

pub fn open_page(id: impl Into<String>, cx: &mut App) {
    let id = id.into();
    nav(cx).update(cx, |n, cx| {
        n.page = Some(id);
        cx.notify();
    });
}
