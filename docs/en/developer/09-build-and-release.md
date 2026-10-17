# Build and release

Redbook builds with stock Cargo on macOS, Linux and Windows. CI builds and
tests all three on every push to `main`; a `v*` tag builds the release
artifacts and publishes a GitHub release. This page covers the local build,
the macOS bundle script, the icons, both workflows and what the published
artifacts do and do not include.

## What you need to build

| Platform | Needs |
|---|---|
| All | Rust stable 1.92 or later: the code needs 1.88 (let-chains), but the locked dependencies need more (`uuid` 1.28.0 needs 1.89 everywhere, `oo7` 0.6.0 needs 1.92 on Linux); `git` on `PATH` for the sync tests |
| macOS | Xcode Command Line Tools. gpui-kit enables `runtime_shaders`, so `gpui-pre-apple` compiles `shaders.metal` at app start, not with `xcrun metal` at build time |
| Linux (Debian/Ubuntu) | The packages in [`scripts/install-linux-deps.sh`](../../../scripts/install-linux-deps.sh): compilers, fontconfig, Wayland, xkbcommon, X11-xcb, OpenSSL, zstd, ALSA, Vulkan loader |
| Windows | MSVC toolchain; `build.rs` embeds `assets/redbook.ico` with `winresource` |

## How to build and run locally

| Task | Command |
|---|---|
| Run the app (debug) | `cargo run` |
| Run against a throwaway database | `REDBOOK_DB=/tmp/rb/redbook.db cargo run` |
| Run the MCP server | `cargo run -- mcp` |
| Test | `cargo test` |
| Lint as CI does | `cargo clippy --locked --all-targets -- -D warnings` |
| Release binary | `cargo build --release` |
| Install the macOS app | `scripts/bundle.sh` |

The profiles are set for this project's two uses:

| Profile | Setting | Why |
|---|---|---|
| `dev` | `package."*" = { opt-level = 2 }` | Dependencies (GPUI, SQLite) optimised so a debug build stays responsive; the app crate itself rebuilds fast |
| `release` | `lto = "thin"`, `codegen-units = 1`, `strip = true` | Smaller, faster binary for distribution |

Never measure memory or wakeups on a debug build; see
[04-memory-and-idle.md](04-memory-and-idle.md).

## What `scripts/bundle.sh` does

[`scripts/bundle.sh`](../../../scripts/bundle.sh) has two modes:

| Invocation | Mode |
|---|---|
| `scripts/bundle.sh` | Build release with `cargo build --release -j 4`, install `/Applications/Redbook.app`, register it with Launch Services |
| `scripts/bundle.sh <binary> <out dir>` | Package a prebuilt binary into `<out dir>/Redbook.app` without building or registering (used by the release workflow) |

In both modes it updates the bundle in place: it moves the new binary over
`Contents/MacOS/redbook` (a running `redbook mcp` keeps its old file), copies
`assets/AppIcon.icns`, writes `Info.plist`, signs the bundle ad hoc
(`codesign --force --deep -s -`) and touches it, so Finder and the Dock drop a
cached icon. Deleting the bundle first would drop it from Spotlight until macOS
indexed it again. Local mode also registers the app with Launch Services and
Spotlight, and unregisters and deletes a leftover `/Applications/Red Book.app`
from before the rename.

`Info.plist` keys that matter:

| Key | Value | Why |
|---|---|---|
| `CFBundleIdentifier` | `com.tarikalim.redbook` | |
| `CFBundleShortVersionString` | `version` from `Cargo.toml` | One version source |
| `LSMinimumSystemVersion` | `12.0` | |
| `LSEnvironment` → `MallocSpaceEfficient` | `1` | The allocator setting reaches the process before it starts, so a Finder launch does not need the self-`exec` in `mem.rs` |
| `NSSupportsAutomaticGraphicsSwitching` | true | Lets a dual-GPU Mac stay on the integrated GPU |

## Which icon assets exist

| File | Used by |
|---|---|
| `assets/icon.svg` | The source of the others |
| `assets/AppIcon.icns` | The macOS bundle |
| `assets/redbook.ico` | Embedded into `redbook.exe` by `build.rs` |
| `assets/redbook.png` | The Linux tarball, next to `packaging/redbook.desktop` |

On Linux the window's `app_id` is `redbook`, matching the desktop file's
`StartupWMClass`, so the dock groups the window with the launcher. `scripts/render-icon.swift` renders `assets/icon.svg` on a transparent
background (`qlmanage` paints it white); `AppIcon.icns` and `redbook.png` are
made from that render with `iconutil` and `sips`. `sips` cannot write `.ico`,
so `redbook.ico` is converted by hand when the icon changes.

## What CI runs

[`.github/workflows/check.yml`](../../../.github/workflows/check.yml) runs on
every pull request, as in Ithildin and Mellon:

| Job | Runner | Steps |
|---|---|---|
| `checks` | Ubuntu | Branch name (`scripts/check-branch-name.sh`), commit messages and PR title (`committed.toml`), no AI co-author or attribution, gitleaks, `cargo fmt --check`, `cargo-deny` (`deny.toml`) |
| `test` | macOS | `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`; skipped while the pull request is a draft |

[`.github/workflows/ci.yml`](../../../.github/workflows/ci.yml) builds and tests
on Ubuntu (after `scripts/install-linux-deps.sh`) and Windows, on pushes and
pull requests to `main`, because Redbook also ships for those systems.
`fail-fast: false` lets one platform's failure leave the other's result
visible, `concurrency` with `cancel-in-progress` cancels a run when a newer
commit lands on the same ref, and `CARGO_PROFILE_DEV_DEBUG: 0` drops debug
info, which shrinks the `Swatinem/rust-cache` cache and speeds up linking.

CodeQL's default setup scans the Rust code and the workflows on every push and
pull request.

The GUI test (`app::tests::opens_pages_rows_and_databases`) runs headless
through gpui-kit's `test-support` feature, so it runs on every runner.

## How a release is made

Push a tag that starts with `v`:

```sh
git tag v0.2.0 && git push origin v0.2.0
```

[`.github/workflows/release.yml`](../../../.github/workflows/release.yml)
then builds on three runners and publishes:

| Runner | Builds | Artifact |
|---|---|---|
| `macos-latest` | `aarch64-apple-darwin` and `x86_64-apple-darwin`, joined with `lipo`, bundled with `scripts/bundle.sh` | `Redbook-<version>-macos-universal.zip` (`ditto`) and `.dmg` (`hdiutil`, UDZO) |
| `ubuntu-22.04` | `x86_64` release | `Redbook-<version>-linux-x86_64.tar.gz`: binary, `.desktop`, `.png` |
| `windows-latest` | `x86_64` release | `Redbook-<version>-windows-x86_64.zip`: `redbook.exe` |

The Linux build runs on Ubuntu 22.04 rather than the latest image to keep the
glibc version the binary requires low, so it starts on older distributions.

A final job downloads the three artifact sets and runs `gh release create`
with the title `Redbook <version>` and, as notes, the commit subjects since the previous
tag (`git log --pretty='- %s' <prev>..<tag>`). `<version>` is the tag
without its `v`.

## What the artifacts are not

- **Not notarised or Developer-ID signed.** The macOS app carries an ad-hoc
  signature only. A downloaded copy is quarantined, and Gatekeeper refuses to
  open it until the user approves it in System Settings → Privacy & Security,
  or removes the quarantine with
  `xattr -dr com.apple.quarantine /Applications/Redbook.app`.
- **Not signed on Windows.** SmartScreen warns on first launch.
- **Not packaged for Linux.** There is no `.deb`, AppImage or Flatpak; the
  user places the binary and the desktop file. There is no ARM Linux or ARM
  Windows build.
- **Not auto-updating.** A new version is a manual download.

## What Actions minutes cost

The repository is public, so GitHub-hosted runner minutes on standard runners
are free. It was private until 2026-10-11, when minutes counted against the
account quota with multipliers (Linux 1×, Windows 2×, macOS 10×). Observed
durations on 2026-10-11:

| Workflow | macOS | Ubuntu | Windows |
|---|---|---|---|
| CI (`1efb00a`) | 7 min | 9 min | 18 min |
| Release (`v0.1.0`) | 17 min | 12 min | 17 min |

GitHub rounds each job up to a whole minute. The `1efb00a` CI jobs took
7m15s, 9m25s and 18m05s, so that run billed 8 × 10 + 10 + 19 × 2 = 128
minutes, dominated by macOS. One release bills at least 170 + 12 + 34 ≈ 216,
plus the short `release` job on Ubuntu. The cache
(`Swatinem/rust-cache`) is what keeps these from being far longer; a cache
miss after a `Cargo.lock` change costs a full GPUI build on every runner.

## Known gaps

- **The tag and `Cargo.toml` version are not checked against each other.**
  The artifact names use the tag; `Info.plist` and the MCP `serverInfo` use
  `Cargo.toml`. Bump both.
- **The release workflow runs no tests or lints.** It trusts that CI passed on
  the tagged commit.
- **Icon regeneration is manual.** Nothing rebuilds `.ico` and `.png` from the
  `.icns`.
- **Clippy runs only on macOS.** Lints in `cfg(windows)` or Linux-only code
  are not checked.
- **CI cannot see memory regressions.** No job measures footprint or wakeups;
  see [04-memory-and-idle.md](04-memory-and-idle.md#how-to-keep-it-from-regressing).
- **`bundle.sh` builds with `-j 4`** regardless of the machine.
