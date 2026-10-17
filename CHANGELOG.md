# Changelog

All notable changes to Redbook. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- MIT OR Apache-2.0 licence, contributing guide, code of conduct, security policy, issue and pull
  request templates, Dependabot, and the pull request check of Ithildin and Mellon: branch name,
  commit messages and PR title, no AI co-author or attribution, gitleaks, `cargo fmt`,
  `cargo-deny`, clippy and tests.
- The `redbook` skill (`skills/redbook`), served over the MCP Skills extension; MCP also answers
  `server/discover`, speaks protocol 2025-11-25 and annotates its tools.
- `assets/icon.svg` as the icon's source, rendered by `scripts/render-icon.swift`.

### Changed

- The import is described as a Markdown & CSV import; the docs page is `user/05-import.md`.
- `scripts/bundle.sh` updates the app in place, so Spotlight keeps it and a running `redbook mcp`
  keeps its file.
- `rustls` 0.23.46, for RUSTSEC-2026-0346 to 0349.

## [0.1.1] - 2026-10-15

### Added

- Select several blocks and copy, cut or delete them.
- MCP: `update_page` can replace content without moving the old blocks to the Trash.

### Fixed

- Links in table cells open with Cmd+click, as in text.

### Changed

- GPU memory is trimmed after 2 s idle instead of when the display link stops.
