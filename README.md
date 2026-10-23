# macclean

A safe, fast macOS disk cleaner written in Rust. Finds reclaimable disk space and
deletes it through the Trash — never `rm`, always recoverable.

![egui](https://img.shields.io/badge/GUI-egui-blue) ![platform](https://img.shields.io/badge/platform-macOS-lightgrey) ![license](https://img.shields.io/badge/license-MIT-green) [![CI](https://github.com/ranb27/macclean/actions/workflows/ci.yml/badge.svg)](https://github.com/ranb27/macclean/actions/workflows/ci.yml)

<!-- TODO: add screenshot — take one with ⌘⇧4 then Space and click the window,
     save as assets/screenshot.png and uncomment the line below -->
<!-- ![macclean](assets/screenshot.png) -->

## Why another cleaner?

Existing tools either delete aggressively (risking your data) or hide their rules
behind a paywall. macclean is built around one principle: **only offer what can be
regenerated, and make deletion reversible.**

- **Move to Trash, not `rm`** — every deletion goes through `NSFileManager`, so
  Finder's *Put Back* restores it.
- **Safety levels** — `safe` targets are 100% regenerable (caches, derived data,
  logs). `review` targets need your judgement (data of uninstalled apps).
- **Never touches** documents, session data, cookies, `IndexedDB`, `LocalStorage`,
  keychains, or anything under `/System`.
- **Dry-run by design** — nothing is deleted until you tick boxes and press the
  button.

## What it scans

| Category | Examples |
|---|---|
| Developer caches | `npm`, `bun`, `pnpm`, `yarn`, `uv`, `pip`, Go modules, Cargo, Gradle, Maven, Deno, Xcode `DerivedData` |
| App caches | `~/Library/Caches/*`, `Application Support/<app>/{Cache, Code Cache, GPUCache, Service Worker, …}` — installed apps only |
| App leftovers | Data dirs whose owning `.app` no longer exists (bundle-ID matching across `Application Support`, `Containers`, `Group Containers`, `HTTPStorages`, `Saved Application State`) |
| Logs | `~/Library/Logs/*`, Homebrew service logs (truncated in place so daemons keep their file handle) |

The leftover detector is the interesting part: it reads `Info.plist` of every
installed `.app` and flags Library entries that match no bundle ID or app name —
the same orphans CleanMyMac charges you to find.

## Install & run

**Prebuilt binary** (Apple Silicon):

```sh
curl -L -o macclean.zip https://github.com/ranb27/macclean/releases/download/v0.1.0/macclean-v0.1.0-macos-arm64.zip
unzip macclean.zip && ./macclean
```

Unsigned — on first run right-click → Open, or `xattr -d com.apple.quarantine macclean`.

**From source:**

```sh
git clone https://github.com/ranb27/macclean
cd macclean
cargo run --release
```

Requires macOS + a Rust toolchain (`rustup` or `brew install rust`).

## How it works

```
scan (background thread)
  ├── DEV_CACHE_PATHS    fixed list of known-regenerable tool caches
  ├── app caches         ~/Library/Caches + cache subdirs of installed apps
  ├── leftovers          Library entries matching no installed bundle ID
  └── logs               user + service logs
        │
        ▼
  CleanTarget { path, size, safety, action }
        │
        ▼
  you select ──► Trash (NSFileManager) or Truncate for live log files
```

Notable implementation details:

- Directory sizes use `st_blocks × 512` (matches `du`, handles sparse files).
- Matching is exact-first, then bundle-ID segment + 5-char containment — tuned
  against real collisions like `codex` vs VS Code's `code` and `steam` inside
  `msteams`.
- Serial `walkdir` for sizing; parallelism comes from rayon at the target level
  (a nested thread pool per walk silently exhausted threads — dirs reported 0 B).
- Soft fd limit is raised at startup; macOS defaults to 256 which parallel walks
  blow through.

## Roadmap

- [ ] LaunchAgent / LaunchDaemon leftover detection (needs sudo for system files)
- [ ] Simulator & Xcode device cleanup via `simctl`
- [ ] Homebrew `--prune` integration
- [ ] Scheduled scan + menu-bar tray mode
- [ ] Config file for custom paths / exclusions

## Contributing

Issues and PRs welcome — especially additions to `DEV_CACHE_PATHS` and
`APP_CACHE_SUBDIRS` for tools we don't cover yet. Rules for new scanners:

1. If a file can be regenerated, it's `safe`. If it can't, it's `review`.
   If deleting it could log someone out or lose state, don't list it at all.
2. Explain the rule in the `reason` field — users should learn why it's safe.

## License

[MIT](LICENSE)
