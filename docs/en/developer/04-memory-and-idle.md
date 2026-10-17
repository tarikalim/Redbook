# Memory and idle cost

Redbook's defining constraint is that an open window it is not using costs
next to nothing: little memory, no CPU, no wakeups. Every change to the GUI is
judged against that, and every `perf(mem)` commit carries its own
before → after measurement. This page records the measurement method, each
optimisation with its reason and its number, the ideas that were rejected,
where the floor is, and how to keep it there.

## Where the numbers stand

| Scenario | Footprint | Idle wakeups | Source |
|---|---|---|---|
| Blank page, 150 pages in the sidebar, window in front, 20 s idle | 30.78 MB | — | commit `aa078e7`, 3 runs |
| 100-row database page, window in front, 20 s idle | 32.21 MB | — | commit `1efb00a`, 3 runs |
| Installed release build, window in background, ~10 min after launch | 31 MB | 0 over 2 × 60 s | `footprint` and `top -c d`, 2026-10-11, macOS 26.6 |
| Window in front, idle, before the blink cap | — | 0.3/s, 0.00% CPU | commit `b52f565` |
| App in background, before the blink cap | — | 0.4/s, 0.00% CPU | commit `b52f565` |

The start line was 121 MB idle in the first footprint measurement (commit
`194c5c0`) and about 30–35 wakeups/s (commit `b52f565`); a focused window
still held 117 MB until the blink cap (commit `ddec256`). The method on this page — a release build,
a temporary database, a 1200×800 window on a 2× display (a 2400×1600
drawable), the mean of 3 runs — is what the later commits state. Some
earlier commits (`b52f565`, `194c5c0`, `3d8dea8`, `7871780`, `c1517e0`,
`ddec256`) do not record their setup.

## How to measure footprint on macOS

Measure the number the system charges the process — `phys_footprint` — not
RSS. Mapped clean file pages, reclaimable malloc pages and shared libraries'
clean pages are in RSS but not in the footprint.

1. Build release: `cargo build --release`, or install with
   `scripts/bundle.sh`. A debug build's numbers are meaningless here.
2. Point it at a throwaway database so content is fixed:
   `REDBOOK_DB=/tmp/rb-measure/redbook.db target/release/redbook`.
3. Keep the window at 1200×800. Window size moves the graphics numbers
   directly (one surface is width × height × 4 bytes at the backing scale).
4. Do the thing you are measuring, then leave the app untouched for 20 s. The
   GPU trim runs about 0.1–0.2 s after the last frame and the frame-memory trim
   2 s after it; the cursor stops blinking after 10 blinks (about 5 s).
5. Run `footprint <pid>`, three times on three launches, and compare means.

`footprint` prints the total and a table per category. Only the **Dirty**
column counts toward the total; **Reclaimable** is memory the kernel may take
back (with `MallocSpaceEfficient` set, freed malloc pages land there). The
categories that matter:

| Category | What it is in Redbook | Moved by |
|---|---|---|
| `IOSurface` | Window surfaces: the drawable on screen and any pooled drawables | Drawable pool drain, autorelease pool |
| `IOAccelerator (graphics)` | GPU driver memory: textures, buffers, command-queue scratch | Command queue recreation, atlas sizes, path textures, instance buffer |
| `MALLOC_SMALL` | Most of the Rust heap: GPUI frames, layout, text caches, entities, SQLite cache | MallocSpaceEfficient, frame trim, glyph cache cap, SQLite settings, row representation |
| `__DATA_DIRTY`, `__DATA`, `__DATA_CONST` | Written pages of the binary and the system frameworks | Nothing in this repository |

The commits also quote **non-graphics footprint**: the total minus
`IOSurface` and `IOAccelerator`. It isolates heap work from window-size and
GPU-driver noise.

A snapshot of the installed build in the background (2026-10-11):

| Category | Dirty | Reclaimable |
|---|---|---|
| Total footprint | 31 MB | — |
| `IOSurface` | 15 MB | 0 |
| `MALLOC_SMALL` | 6.5 MB | 10 MB |
| `__DATA_DIRTY` | 1.8 MB | 0 |
| `IOAccelerator (graphics)` | 1.2 MB | 2.3 MB |
| `__DATA` + `__DATA_CONST` | 2.0 MB | 0 |
| `MALLOC metadata` | 0.8 MB | 0 |
| page table | 0.7 MB | 0 |

To attribute heap to call sites, launch with `MallocStackLogging=lite` and
run `malloc_history <pid> -allBySize`. That is how the glyph cache
(`CGGlyphBitmapCreate` under `rasterize_glyph`), the AppKit tab group and the
per-frame buffers were found. `vmmap --summary <pid>` gives the same
categories with region detail.

## How to measure idle wakeups on macOS

Use `top` in **delta mode**:

```sh
top -l 4 -s 10 -c d -pid <pid> -stats pid,command,cpu,idlew,mem
```

Discard the first sample. Each following line's `IDLEW` is the number of
times the process was woken from idle during that 10 s interval; divide by 10
for a rate.

The pitfall: without `-c d`, `IDLEW` is a counter that only grows (since the
process started, or since `top` started with `-c a`). A long-running app shows
a large, slowly rising number that looks like a problem and is not, and a
fresh launch shows a small number that hides one. Only the delta between two
samples is a rate. In delta mode the `MEM` column also turns into a delta and
reads `0B` when nothing changed; read memory from `footprint`, not from that
column.

Measure three states, because each has a different wakeup source:

| State | What keeps it awake if something regresses |
|---|---|
| Window in front, nothing focused | Display link, timers, polling |
| Window in front, a text block focused | Cursor blink (until `MAX_BLINKS`) |
| Window in background | Anything that ignores window activation |

## Where the 117 MB came from

The pre-optimisation footprint was mostly memory nobody was using between
frames:

| Holder | Size | Found in |
|---|---|---|
| Metal command-queue scratch memory, per queue, after rendering 2400×1600 | ~49 MB | `194c5c0` |
| Pooled `CAMetalLayer` drawables, up to three at 15 MB | up to 45 MB | `3d8dea8` |
| Last frame's drawable kept alive by an autorelease pool that never drained, plus command-buffer memory | 15 MB + ~4 MB | `7871780` |
| Free but dirty small-malloc pages (38% of `MALLOC_SMALL`) | 7 MB | `ddfb002` |
| Window-sized path texture plus a 4× MSAA target, allocated on every resize | window-sized | `86e90bb` |
| Sprite atlas pages at 1024×1024 even for one emoji | 4 MB per BGRA page | `7cff26a` |
| Spare GPUI `Frame`, taffy tree, layout caches and arenas at peak size | ~3.5 MB | `a356682` |

A focused text block made all of it permanent: the blinking cursor drew a
frame every 500 ms, so neither the GPU trim nor the frame trim ever ran.
Commit `ddec256` measured 117 MB focused against 32 MB in the background.

## What each optimisation does, and what it saved

| Change | Commit | Where | Measured |
|---|---|---|---|
| Stop the display link after 10 idle vsyncs | `b52f565` | `vendor/gpui-pre-macos/src/window.rs`, `IDLE_FRAMES` | Front idle 29.6 → 0.3 wakeups/s, 0.70 → 0.00% CPU |
| Cached sidebar, focus parked while inactive | `b52f565` | `app.rs` | Not quoted separately; the whole commit took background 35.1 → 0.4 wakeups/s |
| File watcher instead of 1 s poll | `b52f565` | `db.rs` | No idle poll; external write → UI 160–190 ms |
| Path textures on demand | `86e90bb` | `metal_renderer.rs` | Not quoted |
| SQLite page cache 2 MB → 256 KB | `c1517e0` | `store.rs` | Live heap −0.5 MB, `MALLOC_SMALL` −0.3 MB |
| `MallocSpaceEfficient=1` via self-relaunch | `ddfb002` | `mem.rs`, `bundle.sh` | `MALLOC_SMALL` 17.8 → 8.5 MB; non-graphics 29.9 → 20.7 MB |
| New Metal command queue when idle | `194c5c0` | `metal_renderer.rs`, `trim_idle` | 121 → 68 MB |
| Empty the drawable pool when idle | `3d8dea8` | `metal_renderer.rs`, `trim_idle` | 83 → 68 MB after hovering |
| Autorelease pool around each frame | `7871780` | `metal_renderer.rs`, `draw` | 68 → 51 MB |
| Skip the AppKit tab group | `477c040` | `vendor/gpui-pre-macos/src/window.rs`, `tab_bar_visible` | `MALLOC_SMALL` 8.5 → 7.65 MB; non-graphics 20.7 → 19.6 MB |
| Atlas pages start at 256 / 512 px | `7cff26a` | `metal_atlas.rs` | Not quoted; a 1024 BGRA page is 4 MB |
| Instance buffer starts at 64 KB | `39dca2e` | `metal_renderer.rs` | Not quoted; was 2 MB |
| CoreGraphics glyph cache capped to 0 | `9116eb1` | `mem.rs`, `cap_glyph_cache` | Content page `MALLOC_SMALL` 10.06 → 9.61 MB |
| Trim per-frame buffers after 2 s | `a356682` | `vendor/gpui-pre/src/window.rs`, `trim_frame_memory` | Content page 9.61 → 7.34 MB; blank page 7.65 → 7.50 MB (`MALLOC_SMALL`) |
| SQLite reads through mmap, cache 64 KB | `1e054f3` | `store.rs` | Content page `MALLOC_SMALL` 7.34 → 7.03 MB |
| Cursor stops after 10 blinks | `ddec256` | `vendor/gpui-base/…/blink_cursor.rs`, `MAX_BLINKS` | Focused idle 117 → 31 MB |
| Sidebar row buttons only on the hovered row | `aa078e7` | `sidebar.rs` | 31.18 → 30.78 MB; `MALLOC_SMALL` 6.50 → 6.15 MB |
| Database rows as raw JSON | `1efb00a` | `model.rs` `DbRow`, `store.rs` `db_rows` | 100-row page 33.21 → 32.21 MB; `MALLOC_SMALL` 8.31 → 7.34 MB |

`git show <commit>` has the full argument for each row.

### Display link stop

GPUI's macOS backend drives frames from a `CVDisplayLink` frame source
(`vendor/gpui-pre-macos/src/display_link.rs`) whose callback fires every
refresh while the window is visible, whether or not anything changed. Upstream
already skips starting it for an occluded window. The patch counts vsyncs since the last `draw`
(`idle_frames`); past `IDLE_FRAMES` (10, about 0.1 s at 120 Hz and 0.2 s at 60 Hz) it stops
the link. To start again it implements
`PlatformWindow::frame_waker`, which GPUI already calls when a window becomes
dirty or requests a frame; the waker restarts the link from a spawned task,
because the caller may hold the window-state lock.

### GPU trims when the window goes idle

`MetalRenderer::trim_idle` runs from `Window::trim_frame_memory` through
`PlatformWindow::trim_idle`, once no frame has been drawn for `TRIM_AFTER` (2 s).
It used to run when the display link stopped; at 0.1 s that hit almost every pause
between keystrokes, and the next frame paid for a cold queue and new drawables:

- **Command queue.** The driver keeps per-queue scratch memory (about three
  times the drawable size) and never shrinks it. Nothing is in flight once the
  window is idle, so the renderer swaps in a new queue; the next frame regrows
  what it needs.
- **Drawable pool.** `CAMetalLayer` pools up to three drawables. Setting
  `drawableSize` to a different value and back empties the pool; the surface
  on screen stays as the layer's contents.
- **Path textures.** Released; they are recreated by the first path batch.

### Autorelease pool per frame

Metal returns the drawable, command buffer and pass descriptors autoreleased.
The display-link callback has no autorelease pool, so they lived until some
unrelated pool drained — in practice the last frame's 15 MB drawable stayed
owned by the process. `draw` now wraps `draw_inner` in
`objc2::rc::autoreleasepool`.

### MallocSpaceEfficient relaunch

macOS's xzone malloc keeps freed small-allocation pages dirty, so they count
in the footprint. `MallocSpaceEfficient=1` makes it mark them reusable. The
setting is read when libmalloc initialises, before `main`, so the binary
re-executes itself once with the variable set
(`mem::ensure_space_efficient_malloc`), and the app bundle sets it through
`LSEnvironment` so a Finder launch needs no `exec`. In the snapshot table in
[How to measure footprint on macOS](#how-to-measure-footprint-on-macos) the
10 MB of reclaimable `MALLOC_SMALL` is this setting at work.

### Frame memory trim

Between frames GPUI keeps the spare `Frame` (scene primitive vectors, dispatch
tree, hitboxes, element states), the taffy layout tree, both line-layout frame
caches and the element and event arenas at their peak size. The patch makes
`Window::draw` record the time and keep one timer task; once no frame has
been drawn for `TRIM_AFTER` (2 s) it calls `trim_frame_memory`, which drops
the spare frame and the taffy tree, shrinks the layout caches and swaps the
empty arenas for one untouched chunk. The next frame re-grows them with
one-off allocations.

### Cursor blink cap

gpui-base's `BlinkCursor` toggles every 500 ms forever. The patch counts
blinks; after `MAX_BLINKS` (10) the cursor stays solid and the blink task
ends. Any input resets the count. This is the change that made the other
trims reachable in the most common state — the app in front with the caret in
a block.

### Glyph cache cap

CoreText keeps every rasterised glyph bitmap in CoreGraphics' local font cache
(16 MB cap), while GPUI copies each glyph into its own atlas once and never
asks again. `mem::cap_glyph_cache(0)` sets the cap to zero through
`CGFontCacheSetMaxSize`, a private function looked up with `dlsym` and skipped
when absent. The cache grows with every font size, weight and page browsed;
on the 100-block test page it held 580 KB.

### Window tabbing

When GPUI opens a window it asks `NSWindow.tabGroup` for the tab-bar state.
The getter builds AppKit's tab-group machinery (about 490 KB of heap per
`malloc_history`). Redbook opens windows without a tabbing identifier, so
automatic tabbing is off; the patched `tab_bar_visible` returns `false`
without touching `tabGroup` when `allowsAutomaticWindowTabbing` is off.

## Which ideas were rejected, and why

| Idea | Why not |
|---|---|
| `malloc_zone_pressure_relief()` from inside the process | Released 0 bytes (commit `ddfb002`); the environment variable is the only lever that worked |
| Setting `MallocSpaceEfficient` with `setenv` at startup | Too late: libmalloc reads it before `main` runs |
| Polling `PRAGMA data_version` every 1 s for MCP writes | A wakeup per second for a check that almost never finds anything (commit `b52f565`) |
| One text input entity per block | An entity, focus handle, input state and blink task per block; the editor shares one input instead ([03-ui-and-editor.md](03-ui-and-editor.md#why-the-editor-uses-one-shared-text-input)) |
| Database rows as parsed `serde_json::Value` | About 14× the size of the JSON text (343 KB for 25 KB, commit `1efb00a`); rows stay as text and are parsed per cell |
| gpui-kit's full icon set (`AllAssets`) | About 1 MiB more binary (+1.02 MiB per the gpui-kit-assets 0.7.1 README) for icons the app does not use |
| The SQLite default page cache (2 MB) | Held the whole test database in malloc; file pages belong in the kernel's cache, which is not process footprint |

## Where the floor is

The installed build's background snapshot is 31 MB, and about half of it is
one surface: `IOSurface` 15 MB is the 2400×1600 BGRA image the window server
composites (2400 × 1600 × 4 B = 15.4 MB). That surface cannot be freed while
the window is visible, and it scales with window area and display scale.

Of the remaining ~16 MB, roughly 4 MB is dirty data pages of the binary and
the system frameworks (`__DATA*`, `__AUTH*`), plus page tables, stacks and
malloc metadata — the price of linking AppKit, Metal and CoreText at all.
The live Rust heap in `MALLOC_SMALL` is about 6.5 MB. That heap is the only
part this repository still controls, and it holds working state: font data,
the GPUI entity graph, the page tree, the SQLite connection.

These shares are arithmetic on one snapshot, not a measured lower bound.
Treat "about 25 MB at this window size" as the practical floor: below it you
are removing working state, not waste.

## How to keep it from regressing

No CI job measures memory or wakeups. The guard is the habit every `perf`
commit follows:

1. **Measure before and after**, with the method on this page, 3 runs, and put
   both numbers in the commit message, as every `perf(mem)` commit does.
2. **Check all three wakeup states** with `top -c d` after any change that
   adds a timer, a task, an animation or a subscription.
3. **Never leave work running while idle.** A periodic task must stop when
   the window is inactive or there is nothing to do; prefer an event (the file
   watcher, `cx.notify`) to a timer.
4. **Do not keep frames coming.** Anything that redraws on a timer — a
   spinner, a blink, a hover effect driven by polling — keeps the display link
   running and blocks both trims.
5. **Keep per-row cost flat.** Build buttons, menus and editors for the
   hovered or focused item only; share data with `Rc`; virtualise lists.
6. **Keep cached data compact.** Hold JSON as text and parse on demand; hold
   ids, not cloned blocks.
7. **Put new GPU allocations behind `trim_idle`** if they are window-sized or
   only needed while drawing.

## Known gaps

- **Most of this is macOS-only.** The display-link stop, the GPU trims, the
  autorelease pool and the tab group patch exist only in the vendored macOS
  backend; `MallocSpaceEfficient` and the glyph cache cap are macOS-only code
  in `src/mem.rs`. On Linux and Windows the only
  GPUI patches that apply are the frame-memory trim and the blink cap; the
  app-level changes (SQLite settings, file watcher, focus parking, sidebar
  rows, raw-JSON rows) apply everywhere. Neither footprint nor wakeups have
  ever been measured there.
- **Numbers come from one machine and one window size.** A larger window or a
  3× display raises `IOSurface` and the GPU numbers in proportion.
- **The first frame after idle pays to regrow** the command queue scratch, the
  path textures and the frame buffers. That latency has not been measured.
- **The glyph-cache cap depends on private API.** If
  `CGFontCacheSetMaxSize` disappears, `dlsym` returns null and the cap is
  skipped without a message.
- **Board and calendar views are not virtualised**, so their memory grows
  with the number of cards.
- **Nothing fails when it regresses.** A change that keeps the display link
  running passes every test.
