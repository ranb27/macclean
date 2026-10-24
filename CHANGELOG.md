# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `macclean --mcp` — Model Context Protocol server on stdio so an AI
  assistant can inspect/clean the Mac: `disk_summary`, `scan_targets`,
  `clean_targets`. Cleaning is gated to targets a fresh scan reports as
  regenerable — review-level and arbitrary paths are always skipped.
- **Connect AI** button — detects installed MCP clients (Claude Desktop,
  Claude Code, Cursor, Windsurf, Codex, Gemini CLI, Zed) and merges the
  server into their config with one click; copy-snippet fallback for
  everything else.
- System-following dark/light theme, embedded Inter font, reclaimable
  ring, background update check (top-right), centered scan spinner.
- Landing page rework: dual themes via `prefers-color-scheme`,
  glassmorphism, three.js hero, scroll reveals, EN/TH/ZH/JA picker,
  theme-paired screenshots.

### Fixed

- Translucent panel tints rendered opaque white in dark mode — `Color32`
  stores premultiplied alpha, invalid `from_rgba_premultiplied` input.
- App icon drawn full-bleed — now on Apple's ~824 px icon grid.

## [0.2.0] - 2026-10-24

### Added

- `Action::Command` — whitelisted cleanup commands offered as targets:
  `brew cleanup -s` (when Homebrew is installed) and
  `xcrun simctl delete unavailable` (when CoreSimulator data exists). Fixed
  argv and absolute binary paths; no shell involved.
- New scanners:
  - `~/Library/LaunchAgents` orphan agents — plist `Label` matched against
    installed apps, plus a program-path existence check so agents whose
    binary still exists are never flagged
  - `~/Library/Developer` review-level data — `CoreSimulator/Devices`,
    `Xcode/Archives`, `iOS DeviceSupport`
  - `~/Library/Preferences` orphan `.plist` files — listed only when a
    matching leftover dir was already flagged (corroborated orphans)
  - `~/Downloads` `.dmg`/`.pkg` installer leftovers — new `Downloads`
    category
- `macclean --dump` — headless scan, one line per target, for scripting and
  scanner verification
- `.app` bundle + `.dmg` distribution — `scripts/package.sh` assembles
  `macclean.app` (icon, `Info.plist`, ad-hoc signature) and packs a dmg with
  an `/Applications` drop link; CI release job builds it on `v*` tags
- Landing page at `docs/index.html` (GitHub Pages)

### Changed

- UI says "Clean items" instead of "Move to Trash" — not every action moves
  a file now
- Dirs managed by a Command target (e.g. `~/Library/Caches/Homebrew` for
  `brew cleanup -s`) are suppressed from file scanners — never listed twice

### Fixed

- Apple system dirs without `com.apple` names (`CloudDocs`, `FileProvider`,
  `Spotlight`, `familycircled`, `mbuseragent`) are no longer flagged as
  leftovers — deleting them could disrupt iCloud and live daemons

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

[Unreleased]: https://github.com/ranb27/macclean/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/ranb27/macclean/releases/tag/v0.2.0
[0.1.0]: https://github.com/ranb27/macclean/releases/tag/v0.1.0
