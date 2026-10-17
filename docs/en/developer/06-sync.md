# Sync

Sync keeps several devices' databases in step through a private git
repository. Each device exports its notes into a git working tree as text,
commits, and pushes; pulling merges the other devices' commits record by
record and imports the result. All of it is in
[`src/sync.rs`](../../../src/sync.rs), with tests in
[`src/sync/tests.rs`](../../../src/sync/tests.rs).

Sync is off until `redbook sync init` or `redbook sync clone` runs. Until
then the change-log triggers do nothing and the app starts no sync work.

## Why git and why text files

The transport is the `git` command-line tool, spawned only while a sync runs.
That choice keeps three things out of the app: a resident sync process, a
network stack, and an embedded git library. Credentials, SSH keys and proxies
are whatever the user's git already uses, and the remote can be any private
repository the user controls.

The files are JSON Lines, one block per line, one page per file. That
format is what makes merging cheap and safe:

- A line is a record with an `id`, so a three-way merge can work per block
  instead of per text hunk.
- A page per file means two devices editing different pages never touch the
  same file.
- Records are written in a fixed tree order (siblings by `sort`, then `id`)
  with a fixed field order, so an unchanged page exports to the same bytes and
  produces no diff.
- Every column of `blocks` is carried, so a round trip is lossless; markdown
  is not ([02-data-model-and-storage.md](02-data-model-and-storage.md#how-markdown-goes-in-and-out)).

## What the working tree contains

| Path | Content | Written by |
|---|---|---|
| `pages/<id>.jsonl` | A page-like block, then every non-page block it owns, one record per line, in tree order | export |
| `collections/<id>.jsonl` | A database's schema: one `{"id":"prop:<prop id>","def":{…}}` line per property (sorted by id), then one `{"id":"view:<view id>",…}` line per view | export |
| `workspaces.jsonl` | One `Workspace` record per line | export |
| `files/<name>` | Attachments: a file already in the local `files/` folder keeps its name; any other file is named `<FNV-64 hash of content>-<sanitised name>` | export |
| `README.md` | Generated index of all workspaces and pages, linking `read/` | export |
| `read/<id>.md` | Generated markdown of each page, for reading on the git host | export |
| `.gitattributes` | `* text=auto eol=lf`, `files/** -text`, generated files `linguist-generated merge=binary` | `sync init` |
| `.git/redbook-sync.lock` | One sync at a time per working tree, across processes | each run |
| `.git/redbook-status.json` | Last run's time, error, summary and "not pushed yet" flag | each `Full` run, any run that changed something or failed, changed the "not pushed" flag, or follows a failed run |

Each page file's line is a `Rec`, with fields in this order: `id`,
`workspace` (first line only — the page itself), `parent`, `type`, `sort`,
`created`, `edited`, `trashed` and `favorite` (only when set), `title` (rich
text), `props`. A database
row is a page and gets its own file; a database block's file holds the block
and its own blocks, and its schema and views are in `collections/`.

```
{"id":"9f…","workspace":"3c…","parent":null,"type":"page","sort":2.0,"created":1760190000000,"edited":1760190100000,"title":[["Ideas"]],"props":{"icon":"💡"}}
{"id":"a1…","parent":"9f…","type":"to_do","sort":1.0,"created":1760190005000,"edited":1760190090000,"title":[["Call Ada"]],"props":{"checked":true}}
```

Attachments are rewritten on export: a `props.source` or `props.cover` (`FILE_PROPS`) that is an absolute path
to an existing file becomes `files/<name>` and the file is copied into the
tree once. On import the reverse happens: the file is copied into the local
`files/` folder and the prop becomes that absolute path again.

`redbook-status.json` and the lock live inside `.git/` because the database's file
watcher watches the database folder, and the working tree defaults to a
subfolder of it; nothing a sync writes for its own bookkeeping should look
like a database change.

## How changes are tracked

Migration 2 created `sync_state` and `sync_dirty` and the triggers that fill
it: every insert, update or delete on `blocks` or `views`, and every insert or
update on `collections`, records the affected id (and, for a block move or delete, the old parent) in
`sync_dirty`, but only `WHEN EXISTS (SELECT 1 FROM sync_state)`. Triggers
rather than `Store` methods record changes because four writers exist — the
GUI, the MCP server, the importer and sync's own SQL — and a trigger covers
every one of them, including writes made while the app is closed.

`export` reads `sync_dirty` in `seq` order, maps each id to the page that
owns it (`owner`: walk up `parent_id` to the nearest page-like block) or marks
it gone, and rewrites only those page files. A file is written only when its
bytes differ. Then it rewrites `workspaces.jsonl`, regenerates `read/` for the
touched pages and `README.md` when anything changed, and deletes the processed
`sync_dirty` rows up to the highest `seq` it read. A row inserted during the
export survives for the next run.

Draft pages (the untouched launch page, `props.draft = true`) are skipped.

## When a sync runs

| Trigger | Mode | Condition |
|---|---|---|
| App launch | `Full` | Sync is set up for this database (`sync::start`) |
| First DB change or window activation after `sync init/clone` from a terminal | `Full` | `changed` / `activated` call `start` while sync is off, which reads `sync_state` again |
| Window activated | `Full` | At least `PULL_EVERY` (60 s) since the last full run |
| Any database change in the app | `Push` | After `DEBOUNCE` (10 s) without further changes |
| App quit | `Quit` | Blocks the quit; waits up to 4 s for the lock, 5 s git deadline |
| Settings → **Sync now** | `Full` | Sync is set up (`sync::launch`); shows "Syncing…" |
| `redbook sync now` | `Full` | Interactive: git may prompt for credentials |

| Mode | Does |
|---|---|
| `Full` | Export and commit, pull and merge, export again, push if ahead |
| `Push` | Export and commit, push if ahead; pull only when the push is rejected |
| `Quit` | Export and commit, push with a 5 s deadline; a rejected push is left for the next launch |

In the app, runs execute on GPUI's background executor with their own
`Store` connection. One run at a time: a request while busy is queued, and a
queued `Full` is never downgraded to `Push`. When a run imported anything,
`db::sync_external` reloads every view. Nothing is scheduled while there is
nothing to do; the 10 s debounce exists only after a change. A run that
exported schedules one follow-up `Push` (its own `sync_dirty` cleanup is
seen as an external write), which finds nothing to export.

A push rejected because the remote moved on triggers one pull, re-export and
second push; a second rejection is an error ("push rejected twice"). The
`ahead` flag in the status file remembers unpushed commits across runs, so a
failed push is retried by the next run.

## How the git subprocess is run

Every call is `git -C <tree> -c commit.gpgsign=false -c core.quotepath=off …`
with `LC_ALL=C` (stable messages to match on), stdin closed, stdout and stderr
read on threads, and a deadline after which the child is killed.

| Setting | Value | Why |
|---|---|---|
| Deadline | 180 s per run; 5 s at quit; 600 s for `init`, `clone`, `dir` (30 s for `dir`'s check of the target's remote and identity); 10 s for `status` | A hung network must not hang a run or the quit |
| `GIT_TERMINAL_PROMPT=0` | Background runs | The app has no terminal to answer a prompt |
| `CREATE_NO_WINDOW` | Windows | No console window flashes per git call |
| `--no-verify` on commit | always | User hooks in the sync tree must not block a sync |
| Stale `.git/index.lock` | removed before each run | Only a killed git leaves one, and a deadline kills git |
| Identity | `user.name = <host>`, `user.email = redbook@<host>` in the repo, if git has no `user.email` | A machine without a global identity can still commit |

Commit messages are `<host>: <n> pages` and `<host>: merge`. `<host>` is the
machine's short host name, stored in `sync_state` at set-up; the pull code
reads it back from the other side's last commit subject to label conflict
copies.

## How a pull merges

`pull` fetches, and stops if `origin/<branch>` has nothing new. Otherwise it
runs `git merge --no-commit --no-edit -Xno-renames`. Git's own line merge is
not trusted for record files: for every `.jsonl` file changed on both sides
since the merge base, Redbook reads base, ours and theirs with `git show` and
merges them itself (`merge_file`, `merge_records`), then rewrites page files in
canonical tree order. Other conflicted files are resolved to ours, else
theirs, else removed; generated files are regenerated anyway and attachments
are content-named.

Record-level rules (`merge_records`), per `id`:

| Base → ours, theirs | Result |
|---|---|
| Same on both sides, or theirs unchanged | Ours |
| Ours unchanged, or ours deleted and theirs changed | Theirs |
| Theirs deleted, ours changed | Ours |
| Both changed differently | Ours, and the page is flagged as a conflict |

File-level rules (`merge_file`): a change on one side wins; a deletion loses
to an edit. An edit always beats a deletion, at both levels, because losing a
deletion is recoverable and losing an edit is not.

A flagged page keeps the local (ours) version in place, and the remote's (theirs) whole page file is
imported as a new page next to it, titled `<title> (conflict <host> <date>)`.
A conflicting database is copied as a plain page, so a conflict never creates
a second database.

## How imported files reach the database

`import` applies the changed paths inside one transaction:

1. **Local edits first.** Pages that own an id still in `sync_dirty` — local
   changes not yet exported — are never overwritten. If the incoming file
   touches one, its version becomes a conflict copy instead.
2. **Pages** are upserted record by record (`INSERT … ON CONFLICT(id) DO
   UPDATE`). Blocks that the page file no longer lists are deleted.
3. **Removed page files** delete the page's own blocks.
4. **Orphans** — blocks whose parent no longer exists — are re-parented to
   where the removed block lived, or to the page file that lists them.
5. **Workspaces** are upserted; a local workspace missing from the file is
   deleted only if no block uses it.
6. **Collections** replace the schema and all views of that database.
7. The import's own trigger output is discarded, and only genuinely local
   changes (plus re-parented orphans and conflict copies) stay in `sync_dirty`.

The sync connection runs with `PRAGMA foreign_keys = OFF`. With cascades on,
deleting a block that moved to another page file in the same import would
delete its subpages before step 4 could re-parent them.

## Where the working tree lives

The tree is `sync_dir` from `redbook.settings.json` (a leading `~` is
expanded), else `sync/` next to the database. `redbook sync dir <PATH>` moves
it (`switch_dir`):

| Target | What happens |
|---|---|
| Same as now | Nothing |
| Sync not set up yet | The path is recorded, if the folder is empty or missing |
| An existing clone of the same remote | Switch to it, mark everything dirty so the database's content is re-exported into it |
| A clone of a different remote | Refused |
| Empty or missing folder | The tree is moved there; if the move fails (another volume), it is cloned there and everything is marked dirty |

The Settings dialog's "Change folder…" calls the same function with automatic
runs paused.

## What the command line offers

```
redbook sync init <remote> [--dir PATH]
redbook sync clone <remote> [--dir PATH] [--force]
redbook sync now
redbook sync status
redbook sync dir <PATH>
```

| Command | Does | Refuses when |
|---|---|---|
| `init` | Creates the tree, writes `.gitattributes`, enables tracking, exports everything, commits, adds `origin`, pushes `main` | The target is already a sync folder or not empty |
| `clone` | Clones the remote and imports every file | The database has pages that are not drafts, unless `--force` |
| `clone --force` | Imports into a database that already has pages, then marks everything dirty so the next run pushes the merge | — |
| `now` | One `Full` run, printing exported / imported / conflict counts | Sync is not set up, the sync folder is missing, or another sync holds the lock |
| `status` | Folder, remote, device name, pending changed blocks, last run and error | — |
| `dir` | Moves or switches the working tree | See the folder table |

Without `--force`, `clone` empties `blocks` and `workspaces` first, which at
that point hold only drafts and empty workspaces. Quit the app before
`clone --force`, as its error message says: the import writes while the app
might be exporting.

## How sync is tested

The four tests in `src/sync/tests.rs` use temporary databases and no network;
`two_devices` and `switch_sync_folder` also use a local bare repository and the
real `git` binary:

| Test | Covers |
|---|---|
| `lossless_round_trip` | Export into a tree, then `import` into a fresh database (no git), reproduces every block, schema, view, workspace and attachment; re-exporting gives byte-identical page files |
| `two_devices` | Two devices editing different pages, adjacent blocks of one page, the same block (conflict copy), delete versus edit, a database schema and rows, an attachment, trash |
| `switch_sync_folder` | `sync dir` moves, switches to a clone of the same remote, refuses a non-empty folder and a bare repository; the "different repository" refusal has no test |
| `record_merge` | `merge_records` rules, including delete-versus-edit |

They need `git` on `PATH`; CI runners have it.

## Known gaps

- **Conflicts are per block, resolved per page.** Two devices editing the
  same block produce a whole-page conflict copy, not a merged paragraph, and
  the user merges by hand.
- **Not encrypted.** The repository holds every note in plain text. Privacy
  is the remote's access control.
- **Attachments only accumulate.** A file removed from every page stays in
  `files/` and in history; there is no garbage collection and no LFS.
- **Changes made while the app is closed wait.** MCP and importer writes are
  logged by triggers but exported only at the next app launch or
  `redbook sync now`.
- **The quit push is bounded.** If 5 s is not enough, the commit stays local
  (`ahead`) until the next run; a device that never relaunches never pushes it.
- **No interactive credentials in the app.** Background runs cannot answer a
  password prompt; HTTPS remotes need a credential helper, SSH remotes an
  agent or an unencrypted key.
- **Settings are per device.** Sidebar width, recent pages and the sync folder
  are not synced; workspaces, pages, databases and trash are.
- **Host names are labels, not identities.** Two machines with the same short
  host name label conflicts identically.
- **A deleted page loses to any concurrent edit of it** on another device —
  by design, but a page trashed on one device and edited on another comes back.
