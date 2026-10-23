# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-10-24

First public release.

### Added

- egui/eframe GUI: category sidebar, sortable target list with safety badges,
  size totals, per-item checkboxes, error reporting
- Scanners:
  - **Developer caches** — npm, npx, bun, pnpm, yarn, uv, pip, Go modules,
    Cargo registry, Gradle, Maven, Deno, Xcode `DerivedData`, simulator caches
  - **App caches** — `~/Library/Caches` and cache subdirectories
    (`Cache`, `Code Cache`, `GPUCache`, `Service Worker`, `Crashpad`, …) inside
    `Application Support` of *installed* apps only
  - **App leftovers** — directories in `Application Support`, `Containers`,
    `Group Containers`, `HTTPStorages`, `Saved Application State` that match no
    installed `.app` (bundle-ID + name matching)
  - **Logs** — `~/Library/Logs` and Homebrew service logs
- Safety model:
  - `safe` — 100% regenerable data, pre-selected
  - `review` — uninstalled-app leftovers, opt-in
  - never offers session data, cookies, `IndexedDB`, `LocalStorage`, keychains,
    or anything under `/System`
- Deletion via Trash (`NSFileManager` — recoverable with Finder *Put Back*);
  live service logs are truncated in place instead so daemons keep their file
  handle
- CI workflow (fmt, clippy, tests, release build on `macos-latest`)
- MIT license

### Fixed during development

- Nondeterministic scan results — nested thread pools (jwalk inside rayon
  `par_iter`) silently exhausted threads and made directories report 0 bytes;
  sizing now uses a serial `walkdir` walk
- fd exhaustion — macOS soft limit of 256 open files is now raised to 65 536
  at startup
- False positive: `Zed.app` flagged as leftover (3-char names bypassed the
  length guard) — exact match now runs before containment checks
- False positive: `Codex` matched VS Code (`"codex"` contains `"code"`) —
  containment now requires ≥5 chars on both sides
- `com.apple.*` entries excluded from scans (TCC-protected, untrashable anyway)

[Unreleased]: https://github.com/ranb27/macclean/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ranb27/macclean/releases/tag/v0.1.0
