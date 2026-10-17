# Contributing

Thanks for helping. Redbook is a small project, so the process is short.

## Build and run

You need Rust 1.92 or newer; on Linux, also the packages `scripts/install-linux-deps.sh` installs.

```sh
cargo run                  # the window, with a debug build
cargo test
scripts/bundle.sh          # macOS: release build installed as /Applications/Redbook.app
```

The window reads `redbook.db` in the app's data folder (`~/Library/Application Support/RedBook` on
macOS). Point `REDBOOK_DB` at another file to try changes without touching your own notes.

## Before a pull request

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo deny check           # cargo install cargo-deny
```

The pull request check runs these, and also checks:

- **Branch name**: [Conventional Branch](https://conventionalbranch.org/): `feat/`, `fix/`,
  `hotfix/` or `chore/` + lower-case words joined by hyphens (`feat/table-resize`), or
  `release/<version>`. Documentation changes go on `chore/`. Rule: `scripts/check-branch-name.sh`.
- **Commit messages and the PR title**: [Conventional Commits](https://www.conventionalcommits.org/),
  `feat: …`, `fix: …`, `docs: …`, at most 100 characters in the subject. Rule: `committed.toml`.
  The body says why, with any measurement that led to the change.
- **No AI attribution**: no AI tool as co-author (`Co-authored-by:`), no `Claude-Session:`
  trailer, no "Generated with …" line in the PR body.
- **No secrets** in the new commits (gitleaks).
- **Dependencies**: no known vulnerability (RustSec), only the licences in `deny.toml`.

Clippy and the tests run on macOS and are skipped while the pull request is a draft; `ci.yml` also
builds and tests on Linux and Windows.

- One change per pull request, with a test when it changes behaviour.
- Code follows `cargo fmt`'s defaults. Keep code comments to a line or two; the reasoning goes in
  the commit message.
- Add a line under `Unreleased` in `CHANGELOG.md` for anything a user would notice.

## Where things are

| Path | What |
|---|---|
| `src/model.rs`, `src/store.rs` | Data model; SQLite storage, schema, markdown in and out |
| `src/app.rs`, `src/sidebar.rs`, `src/search.rs` | The window, sidebar and search |
| `src/editor/` | The block editor |
| `src/database/` | Databases: views, filters, sorts, calculations |
| `src/sync.rs`, `src/sync/` | Sync through a git repository |
| `src/import.rs` | Markdown & CSV import |
| `src/mcp.rs` | MCP server |
| `src/mem.rs` | Memory and idle work |
| `vendor/` | Vendored GPUI crates (Apache-2.0) |
| `skills/redbook/` | The skill served over MCP; update it when an MCP tool changes |
| `docs/en/` | User and developer documentation; update it with the behaviour it describes |

## Releases

The maintainer bumps the version in `Cargo.toml`, moves `Unreleased` in `CHANGELOG.md` under the
new version, and pushes a `v*` tag. The release workflow builds and publishes the app.
