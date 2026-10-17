# Vendored GPUI crates

Four crates from gpui-kit's dependency tree are copied into `vendor/` and
patched. Every patch exists for the idle-cost goal in
[04-memory-and-idle.md](04-memory-and-idle.md); none changes behaviour a user
would notice except that an idle cursor stops blinking. This page says what
each patch does, how to find it, and how to carry the patches to a new
gpui-kit release.

## Why the crates are vendored instead of forked

The patches touch code no public API reaches — the macOS display link, the
Metal renderer's queue and drawable pool, GPUI's per-frame buffers, the
cursor-blink task. The options were a git fork or path copies, and path copies
won:

- The source of every patch is in this repository's history, next to the
  measurement that justified it (`git log -- vendor`).
- Builds need no network access to a fork, and `Cargo.lock` stays a plain
  crates.io lock for everything else.
- Each crate is replaced individually; the Linux, Windows and wgpu backends
  stay on crates.io untouched.

The cost is about 230,000 lines of copied upstream Rust and a manual upgrade
procedure.

## How the patches are wired in

`Cargo.toml` replaces the crates with `[patch.crates-io]`:

```toml
[patch.crates-io]
# gpui-pre-macos 0.3.8 + idle stop of the display link (search "idle_frames" in window.rs).
gpui-pre-macos = { path = "vendor/gpui-pre-macos" }
# gpui-pre-apple 0.3.8 (Metal renderer) vendored for memory work.
gpui-pre-apple = { path = "vendor/gpui-pre-apple" }
# gpui-pre 0.3.8 + idle trim of per-frame buffers (search "trim_frame_memory" in window.rs).
gpui-pre = { path = "vendor/gpui-pre" }
# gpui-base 0.7.1 + cursor stops blinking after 10 blinks without input (search "MAX_BLINKS").
gpui-base = { path = "vendor/gpui-base" }
```

A `[patch]` entry is used only when the vendored crate's version satisfies
the version the dependency graph asks for. gpui-kit 0.7.1 requires
`gpui-pre =0.3.8` (exact) and `gpui-base 0.7.1` (caret), and the vendored manifests carry
exactly those versions. If they ever disagree, Cargo prints
`Patch … was not used in the crate graph` and builds the unpatched
crates.io copy — the build succeeds and the idle work silently disappears.
To confirm the patches are live, check that `Cargo.lock` has no `source =`
line for the four crates (path dependencies have none).

`[workspace]` has `exclude = ["vendor"]`, so the vendored crates are not
workspace members: `cargo test` and `cargo clippy --all-targets` in CI do not
run on them. `vendor/gpui-pre-macos/src/gpui_macos.rs` also carries
`#![allow(warnings)]`, because a path dependency is built without the
`--cap-lints` that Cargo applies to registry crates.

## What each vendored crate patches

| Crate | Version | Platform | Patch | Commit |
|---|---|---|---|---|
| `gpui-pre-macos` | 0.3.8 | macOS | Stop the display link once more than `IDLE_FRAMES` (10) vsyncs pass without a draw, restart through `frame_waker` | `b52f565` |
| | | | `PlatformWindow::trim_idle` calls `renderer.trim_idle()`; GPUI calls it after 2 s idle | `194c5c0` |
| | | | Answer `tab_bar_visible` without building AppKit's tab group when automatic tabbing is off | `477c040` |
| `gpui-pre-apple` | 0.3.8 | macOS (Metal) | `trim_idle`: new command queue, empty drawable pool, drop path textures | `194c5c0`, `3d8dea8` |
| | | | Autorelease pool around each `draw` | `7871780` |
| | | | Path intermediate textures allocated on the first path batch, dropped after a frame without paths | `86e90bb` |
| | | | Atlas pages start at 512 (monochrome) / 256 (polychrome) px, doubling to 1024 | `7cff26a` |
| | | | Instance buffer pool starts at 64 KB instead of 2 MB | `39dca2e` |
| `gpui-pre` | 0.3.8 | all | `Window::schedule_trim` / `trim_frame_memory` after `TRIM_AFTER` (2 s) without a frame; `Arena::trim`; line-layout cache `trim`; `PlatformWindow::trim_idle` hook called from it | `a356682` |
| | | | `Cargo.toml`: `[[example]]` and `[[test]]` targets removed, because the copy omits those files | `f9d9605` |
| `gpui-base` | 0.7.1 | all | `BlinkCursor` stops after `MAX_BLINKS` (10) blinks without input | `ddec256` |

The macOS patch calls `MetalRenderer::trim_idle`, which only the Apple patch
defines. The two crates must be upgraded together.

## How to find a patch in the code

Upstream code has no markers, so search for these identifiers:

| Crate | Search for |
|---|---|
| `gpui-pre-macos` | `Red Book patch` (four comments in `src/window.rs`), `IDLE_FRAMES`, `idle_frames`, `link_idle`, `frame_waker`, `allowsAutomaticWindowTabbing` in `tab_bar_visible` |
| `gpui-pre-apple` | `trim_idle`, `autoreleasepool`, `draw_inner`, `ensure_path_intermediate_textures`, `release_path_intermediate_textures` in `src/metal_renderer.rs`; `buffer_size: 64 * 1024`; `default_atlas_size` in `src/metal_atlas.rs` |
| `gpui-pre` | `schedule_trim`, `trim_frame_memory`, `TRIM_AFTER`, `last_draw_at` in `src/window.rs`; `pub fn trim` in `src/arena.rs`; `shrink_to_fit` and `pub fn trim` in `src/text_system/line_layout.rs`; `fn trim` in `src/text_system.rs` |
| `gpui-base` | `MAX_BLINKS`, `blinks` in `src/input/base/blink_cursor.rs` |

The authoritative list is a diff against the registry copy of the same
version. With the crates fetched (`cargo fetch` with the patch section
commented out, or any earlier build), this prints every patch:

```sh
R=$(ls -d ~/.cargo/registry/src/*/ | head -1)
for c in gpui-pre-macos-0.3.8 gpui-pre-apple-0.3.8 gpui-pre-0.3.8 gpui-base-0.7.1; do
  diff -ru -x .cargo-ok -x .cargo_vcs_info.json -x Cargo.toml.orig -x Cargo.lock \
    -x examples -x docs -x tests "$R/$c" "vendor/${c%-*}"
done
```

## How to upgrade gpui-kit and re-apply the patches

Prerequisites: a clean working tree, and the measurement setup from
[04-memory-and-idle.md](04-memory-and-idle.md) to verify the result.

1. **Read the new requirements.** Bump `gpui-kit = "=<new>"` in both
   `[dependencies]` and `[dev-dependencies]`, comment out the
   `[patch.crates-io]` section, run `cargo update -p gpui-kit` and
   `cargo fetch`, and note the versions `Cargo.lock` now has for
   `gpui-pre`, `gpui-pre-macos`, `gpui-pre-apple` and `gpui-base`.
2. **Vendor the new versions unchanged, in their own commit.** First save the
   patches of the two crates that have no separate patch commit, with the
   registry diff from
   [How to find a patch in the code](#how-to-find-a-patch-in-the-code)
   redirected per crate: `… "$R/gpui-pre-macos-<old>" vendor/gpui-pre-macos > gpui-pre-macos.patch`,
   and the same for `gpui-base`. Then, for each crate,
   replace `vendor/<crate>` with the registry copy minus `.cargo-ok`,
   `.cargo_vcs_info.json` and `Cargo.toml.orig`. For `gpui-pre`, leave out
   `examples/`, `docs/` and `tests/` and delete the `[[example]]` and `[[test]]`
   targets from its `Cargo.toml`, as commit `f9d9605` did. An
   unchanged-copy commit makes the next step's diffs readable.
3. **Re-apply each patch commit.** The vendored paths do not change between
   versions, so a patch commit applies with three-way merge:
   `git show 194c5c0 -- vendor/gpui-pre-apple | git apply --3way`. Apply
   them in history order (`git log --reverse --format=%h -- vendor`), skipping
   the pure-copy commits (`56b845b`, `f9d9605`), and resolve conflicts by
   intent, using the commit messages. This works for `gpui-pre-apple` and
   `gpui-pre` only: `gpui-pre-macos` (`b52f565`) and `gpui-base` (`ddec256`)
   arrived as the whole crate plus the patch in one commit, so for those two
   limit each replayed commit to the other crates (`git show <commit> --
   vendor/gpui-pre-apple vendor/gpui-pre`; the `gpui-pre-macos` parts of
   `194c5c0` and `477c040` are already in the saved diff) and
   apply the patch file saved in step 2 with
   `patch -p2 -d vendor/<crate> < <crate>.patch`. Re-add `#![allow(warnings)]` to `gpui_macos.rs` if it is lost.
4. **Restore `[patch.crates-io]`**, update the version numbers in its
   comments, and build. A `Patch … was not used` warning means a version
   mismatch: fix it before anything else.
5. **Re-measure** footprint and wakeups in the three states, and put the
   numbers in the commit message. A patch that applies cleanly can still be
   dead if upstream moved the code it hooks into — for example, if GPUI stops
   calling `frame_waker`, the display link never restarts and the window
   freezes after 0.1–0.2 s of idleness, which only running the app shows.

## Known gaps

- **Nothing is upstreamed.** Every gpui-kit upgrade repeats the procedure on
  this page, and conflicts grow with upstream churn.
- **No test covers the patches.** CI does not exercise the vendored crates
  (they are excluded from the workspace) and has no macOS GUI session, so a
  broken patch is found by measuring or using the app.
- **`gpui-pre-apple/vendor/gpui/` is upstream's**, not this repository's: it
  is part of the published crate and is copied, not patched.
- **The Linux and Windows backends are not patched**, so the display-link and
  GPU trims have no equivalent there.
