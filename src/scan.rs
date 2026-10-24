use crate::config;
use crate::model::{Action, Category, CleanTarget, CommandSpec, Safety};
use rayon::prelude::*;
use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const MIN_SIZE: u64 = 5 * 1024 * 1024; // ignore dirs under 5 MB — noise
/// Orphan .plist files are small by nature — they earn a row by being
/// orphaned, not by size. Still skip the tiniest stubs.
const PLIST_MIN: u64 = 4 * 1024;

/// Paths that are always 100% regenerable package/dev tool caches.
const DEV_CACHE_PATHS: &[(&str, &str)] = &[
    (".npm/_cacache", "npm package cache"),
    (".npm/_npx", "npx package cache"),
    (".bun/install/cache", "bun install cache"),
    (".cache/uv", "uv package cache"),
    (".cache/pip", "pip package cache"),
    ("Library/Caches/pip", "pip package cache"),
    (".pnpm-store", "pnpm store cache"),
    ("go/pkg/mod", "Go module cache"),
    (".gradle/caches", "Gradle cache"),
    (".cargo/registry/cache", "cargo registry cache"),
    (".cargo/git", "cargo git checkouts"),
    (
        "Library/Developer/Xcode/DerivedData",
        "Xcode build artifacts",
    ),
    ("Library/Developer/CoreSimulator/Caches", "simulator caches"),
    (".m2/repository", "Maven repository cache"),
    ("Library/Caches/Yarn", "Yarn cache"),
    (".yarn/cache", "Yarn cache"),
    (".deno", "Deno cache"),
];

/// Big Xcode data that is regenerable in principle but expensive to lose —
/// always Review. Distinct from `xcrun simctl delete unavailable`, which
/// surgically removes only broken devices and is offered separately.
const DEV_REVIEW_PATHS: &[(&str, &str)] = &[
    (
        "Library/Developer/CoreSimulator/Devices",
        "all iOS simulator device data",
    ),
    (
        "Library/Developer/Xcode/Archives",
        "Xcode archives (signed app builds)",
    ),
    (
        "Library/Developer/Xcode/iOS DeviceSupport",
        "device debug symbols (re-downloaded on connect)",
    ),
];

/// Whitelisted cleanup commands. Offered when a candidate binary exists and
/// the tool has data on this machine. size_hint dirs are suppressed from the
/// file scanners so the same bytes are never listed twice.
const COMMAND_TARGETS: &[CommandSpec] = &[
    CommandSpec {
        bins: &["/opt/homebrew/bin/brew", "/usr/local/bin/brew"],
        args: &["cleanup", "-s"],
        display: "brew cleanup -s",
        data_dirs: &["Library/Caches/Homebrew", "/opt/homebrew/Cellar"],
        size_hint: &["Library/Caches/Homebrew"],
    },
    CommandSpec {
        bins: &["/usr/bin/xcrun"],
        args: &["simctl", "delete", "unavailable"],
        display: "xcrun simctl delete unavailable",
        data_dirs: &["Library/Developer/CoreSimulator"],
        size_hint: &[],
    },
];

/// Cache-like subdirs inside Application Support/<app>/ — safe for
/// installed apps only (we never list dirs that are never safe).
const APP_CACHE_SUBDIRS: &[&str] = &[
    "Cache",
    "CachedData",
    "Code Cache",
    "GPUCache",
    "DawnWebGPUCache",
    "DawnGraphiteCache",
    "Service Worker",
    "Crashpad",
    "ShaderCache",
    "CachedExtensions",
    "CachedExtensionVSIXs",
    "logs",
];

/// Directories inside ~/Library that hold per-app data — scanned for
/// orphans whose app bundle no longer exists.
const APP_DATA_DIRS: &[&str] = &[
    "Application Support",
    "Containers",
    "Group Containers",
    "HTTPStorages",
    "Saved Application State",
];

/// Never offer these for deletion even if they look orphaned.
const LEFTOVER_SKIP_PREFIXES: &[&str] = &[
    "com.apple",
    "apple",
    "ns.",
    "group.com.apple",
    "icloud",
    "clouddocs",
    "fileprovider",
    "syncservices",
    "crashreporter",
    "knowledge",
    "callservicesd",
    "caches",
    // Apple system entries that don't use com.apple naming (HTTPStorages,
    // Application Support) — flagging them would break live daemons.
    "spotlight",
    "familycircle",
    "mbuseragent",
];

pub struct ScanResult {
    pub targets: Vec<CleanTarget>,
    pub errors: Vec<String>,
}

/// macOS soft fd limit is 256 — parallel directory walks exhaust it and
/// silently undercount. Raise the soft limit (hard is unlimited) once.
pub fn raise_fd_limit() {
    unsafe {
        let mut rl = std::mem::zeroed::<libc::rlimit>();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rl) == 0 {
            let target = rl.rlim_max.min(65_536);
            if rl.rlim_cur < target {
                rl.rlim_cur = target;
                libc::setrlimit(libc::RLIMIT_NOFILE, &rl);
            }
        }
    }
}

pub fn scan_all() -> ScanResult {
    raise_fd_limit();
    let home = match std::env::var("HOME") {
        Ok(h) => PathBuf::from(h),
        Err(_) => {
            return ScanResult {
                targets: vec![],
                errors: vec!["HOME not set".into()],
            };
        }
    };

    let installed = installed_app_names(&home);
    let mut targets = Vec::new();
    let mut errors = Vec::new();

    targets.extend(scan_devtools(&home));
    targets.extend(scan_logs(&home));

    let (mut cmd_t, covered) = scan_commands(&home);
    targets.append(&mut cmd_t);

    let (mut cache_t, mut cache_e) = scan_app_caches(&home, &installed, &covered);
    targets.append(&mut cache_t);
    errors.append(&mut cache_e);

    let (mut leftover_t, leftover_names) = scan_leftovers(&home, &installed);
    targets.append(&mut leftover_t);
    targets.extend(scan_launch_agents(&home, &installed));
    targets.extend(scan_preferences(&home, &installed, &leftover_names));
    targets.extend(scan_downloads(&home, &installed));

    let (cfg, cfg_err) = config::load();
    targets.retain(|t| !is_excluded(t, &home, &cfg));
    append_custom(&cfg, &mut targets);
    if let Some(e) = cfg_err {
        errors.push(e);
    }

    targets.sort_by_key(|t| std::cmp::Reverse(t.size));
    ScanResult { targets, errors }
}

/// A target is excluded when its path sits inside an excluded dir — or,
/// for command targets, when a dir the command manages is excluded.
/// Excluding `~/Library/Caches/Homebrew` therefore hides `brew cleanup -s`;
/// excluding `~/Library/Developer/CoreSimulator` hides `simctl delete`.
fn is_excluded(t: &CleanTarget, home: &Path, cfg: &config::Config) -> bool {
    if cfg.excludes(&t.path) {
        return true;
    }
    if let Action::Command(spec) = t.action {
        return spec
            .data_dirs
            .iter()
            .any(|d| cfg.excludes(&resolve_dir(home, d)));
    }
    false
}

/// `[[custom]]` entries from the config file. Exclusions still win, and a
/// path a scanner already lists is never duplicated.
fn append_custom(cfg: &config::Config, targets: &mut Vec<CleanTarget>) {
    for c in &cfg.custom {
        if cfg.excludes(&c.path) || !c.path.exists() || targets.iter().any(|t| t.path == c.path) {
            continue;
        }
        targets.push(CleanTarget::new(
            c.path.clone(),
            path_size(&c.path),
            Category::Custom,
            c.safety,
            Action::Trash,
            c.reason.clone(),
        ));
    }
}

// ---------- dev caches ----------

fn scan_devtools(home: &Path) -> Vec<CleanTarget> {
    let caches = DEV_CACHE_PATHS
        .par_iter()
        .filter_map(|(rel, reason)| {
            let path = home.join(rel);
            path.is_dir().then(|| {
                sized(
                    path,
                    Category::DevCaches,
                    Safety::Regenerable,
                    Action::Trash,
                    *reason,
                )
            })
        })
        .flatten();
    let review = DEV_REVIEW_PATHS
        .par_iter()
        .filter_map(|(rel, reason)| {
            let path = home.join(rel);
            path.is_dir().then(|| {
                sized(
                    path,
                    Category::DevCaches,
                    Safety::Review,
                    Action::Trash,
                    *reason,
                )
            })
        })
        .flatten();
    caches.chain(review).collect()
}

// ---------- command targets ----------

/// Home-relative entries joined to home; absolute entries used as-is.
fn resolve_dir(home: &Path, dir: &str) -> PathBuf {
    if dir.starts_with('/') {
        PathBuf::from(dir)
    } else {
        home.join(dir)
    }
}

/// Whitelisted cleanup commands, offered when the tool is installed and has
/// data here. Returns the targets plus the dirs a command already manages —
/// file scanners skip those so bytes are never listed twice.
fn scan_commands(home: &Path) -> (Vec<CleanTarget>, HashSet<PathBuf>) {
    let mut targets = Vec::new();
    let mut covered = HashSet::new();
    for spec in COMMAND_TARGETS {
        let Some(bin) = spec.resolve_bin() else {
            continue;
        };
        if !spec.data_dirs.iter().any(|d| resolve_dir(home, d).is_dir()) {
            continue;
        }
        for hint in spec.size_hint {
            let dir = resolve_dir(home, hint);
            if dir.is_dir() {
                covered.insert(dir);
            }
        }
        let size: u64 = spec
            .size_hint
            .iter()
            .map(|d| path_size(&resolve_dir(home, d)))
            .sum();
        targets.push(CleanTarget::new(
            bin,
            size,
            Category::DevCaches,
            Safety::Regenerable,
            Action::Command(spec),
            format!("runs `{}`", spec.display),
        ));
    }
    (targets, covered)
}

// ---------- app caches ----------

fn scan_app_caches(
    home: &Path,
    installed: &HashSet<String>,
    covered: &HashSet<PathBuf>,
) -> (Vec<CleanTarget>, Vec<String>) {
    let mut candidates: Vec<(PathBuf, String)> = Vec::new();

    // ~/Library/Caches/<entry> — one candidate per top-level entry.
    if let Ok(entries) = std::fs::read_dir(home.join("Library/Caches")) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name.starts_with("com.apple") {
                continue; // TCC-protected system caches — can't touch anyway
            }
            if covered.contains(&e.path()) {
                continue; // a Command target already manages this dir
            }
            candidates.push((e.path(), "app cache".into()));
        }
    }

    // ~/Library/Application Support/<app>/<cache subdir> — installed apps only.
    if let Ok(entries) = std::fs::read_dir(home.join("Library/Application Support")) {
        for e in entries.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let dir_name = e.file_name().to_string_lossy().to_string();
            if dir_name.starts_with('.') || dir_name.starts_with("com.apple") {
                continue; // TCC-protected — can't trash anyway
            }
            if !is_installed(&dir_name, installed) {
                continue; // orphans handled by leftover scanner
            }
            for sub in APP_CACHE_SUBDIRS {
                let cache_dir = e.path().join(sub);
                if cache_dir.is_dir() {
                    candidates.push((cache_dir, format!("{} cache", dir_name)));
                }
            }
        }
    }

    let targets = candidates
        .par_iter()
        .filter_map(|(p, r)| {
            sized(
                p.clone(),
                Category::AppCaches,
                Safety::Regenerable,
                Action::Trash,
                r.clone(),
            )
        })
        .collect();
    (targets, vec![])
}

// ---------- leftovers ----------

/// Leftover dirs plus their normalized names — the prefs scanner corroborates
/// against this set so a lone system .plist is never flagged on its own.
fn scan_leftovers(home: &Path, installed: &HashSet<String>) -> (Vec<CleanTarget>, HashSet<String>) {
    let mut candidates = Vec::new();
    let mut names = HashSet::new();

    for dir in APP_DATA_DIRS {
        if let Ok(entries) = std::fs::read_dir(home.join("Library").join(dir)) {
            for e in entries.flatten() {
                if !e.path().is_dir() {
                    continue;
                }
                let name = e.file_name().to_string_lossy().to_string();
                if skip_leftover_name(&name) {
                    continue;
                }
                if !is_installed(&name, installed) {
                    names.insert(normalize(&name));
                    candidates.push((e.path(), name));
                }
            }
        }
    }

    let targets = candidates
        .par_iter()
        .filter_map(|(p, name)| {
            sized(
                p.clone(),
                Category::Leftovers,
                Safety::Review,
                Action::Trash,
                format!("no installed app matches \"{}\"", name),
            )
        })
        .collect();
    (targets, names)
}

fn skip_leftover_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    name.starts_with('.') || LEFTOVER_SKIP_PREFIXES.iter().any(|p| lower.starts_with(p))
}

// ---------- launch agents ----------

/// ~/Library/LaunchAgents/*.plist whose owning app is gone. Matching is
/// deliberately conservative: a label match OR a still-on-disk program
/// binary means installed — breaking a live agent is worse than missing an
/// orphan.
fn scan_launch_agents(home: &Path, installed: &HashSet<String>) -> Vec<CleanTarget> {
    let mut candidates = Vec::new();
    if let Ok(entries) = std::fs::read_dir(home.join("Library/LaunchAgents")) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "plist") {
                continue;
            }
            let dict = plist::from_file::<_, plist::Value>(&p)
                .ok()
                .and_then(|v| v.as_dictionary().cloned());
            let label = dict
                .as_ref()
                .and_then(|d| d.get("Label"))
                .and_then(|v| v.as_string())
                .map(String::from)
                .or_else(|| p.file_stem().map(|s| s.to_string_lossy().into_owned()));
            let Some(label) = label else { continue };
            if skip_leftover_name(&label)
                || is_installed(&label, installed)
                || agent_program_exists(dict.as_ref())
            {
                continue;
            }
            candidates.push((p, label));
        }
    }
    candidates
        .par_iter()
        .filter_map(|(p, label)| {
            sized_min(
                p.clone(),
                0,
                Category::Leftovers,
                Safety::Review,
                Action::Trash,
                format!("launch agent \"{label}\" — no matching app"),
            )
        })
        .collect()
}

/// The program an agent runs. If that binary still exists on disk the owning
/// software is installed even when its label doesn't name-match the .app —
/// e.g. vendor labels like com.google.Keystone.
fn agent_program_exists(dict: Option<&plist::Dictionary>) -> bool {
    let Some(dict) = dict else { return false };
    let prog = dict.get("Program").and_then(|v| v.as_string()).or_else(|| {
        dict.get("ProgramArguments")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_string())
    });
    prog.is_some_and(|p| p.starts_with('/') && Path::new(p).exists())
}

// ---------- preference plists ----------

/// ~/Library/Preferences/*.plist that corroborate a flagged leftover dir —
/// a plist alone is never enough: plenty of system prefs (org.cups.*,
/// sharedfilelistd) don't start with com.apple and would be false positives.
fn scan_preferences(
    home: &Path,
    installed: &HashSet<String>,
    leftover_names: &HashSet<String>,
) -> Vec<CleanTarget> {
    let mut candidates = Vec::new();
    if let Ok(entries) = std::fs::read_dir(home.join("Library/Preferences")) {
        for e in entries.flatten() {
            let p = e.path();
            if !p.is_file() || p.extension().is_none_or(|x| x != "plist") {
                continue; // ByHost/ and other subdirs are system-managed
            }
            let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
                continue;
            };
            if skip_leftover_name(&stem) || is_installed(&stem, installed) {
                continue;
            }
            let n = normalize(&stem);
            let corroborated = n.len() >= 5
                && leftover_names
                    .iter()
                    .any(|l| l.len() >= 5 && (n.contains(l.as_str()) || l.contains(&n)));
            if !corroborated {
                continue;
            }
            candidates.push((p, stem));
        }
    }
    candidates
        .par_iter()
        .filter_map(|(p, stem)| {
            sized_min(
                p.clone(),
                PLIST_MIN,
                Category::Leftovers,
                Safety::Review,
                Action::Trash,
                format!("no installed app matches \"{stem}\""),
            )
        })
        .collect()
}

// ---------- downloads ----------

/// Installer images left in ~/Downloads. Always Review — we flag, user
/// judges whether the app was installed (reason hints when it was).
fn scan_downloads(home: &Path, installed: &HashSet<String>) -> Vec<CleanTarget> {
    let mut candidates = Vec::new();
    if let Ok(entries) = std::fs::read_dir(home.join("Downloads")) {
        for e in entries.flatten() {
            let p = e.path();
            let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase());
            if !p.is_file() || !matches!(ext.as_deref(), Some("dmg" | "pkg")) {
                continue;
            }
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let reason = if is_installed(&stem, installed) {
                "installer for an already-installed app"
            } else {
                "downloaded installer"
            };
            candidates.push((p, reason));
        }
    }
    candidates
        .par_iter()
        .filter_map(|(p, reason)| {
            sized(
                p.clone(),
                Category::Downloads,
                Safety::Review,
                Action::Trash,
                *reason,
            )
        })
        .collect()
}

// ---------- logs ----------

fn scan_logs(home: &Path) -> Vec<CleanTarget> {
    let mut targets = Vec::new();

    // ~/Library/Logs/<entry>
    if let Ok(entries) = std::fs::read_dir(home.join("Library/Logs")) {
        for e in entries.flatten() {
            if let Some(t) = sized(
                e.path(),
                Category::Logs,
                Safety::Regenerable,
                Action::Trash,
                "log files",
            ) {
                targets.push(t);
            }
        }
    }

    // Large live log files (homebrew services etc.) — truncate, not trash:
    // daemons keep writing to the moved file so trashing frees nothing.
    for base in ["/opt/homebrew/var/log", "/usr/local/var/log"] {
        if let Ok(entries) = std::fs::read_dir(base) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "log")
                    && let Some(t) = sized(
                        p,
                        Category::Logs,
                        Safety::Review,
                        Action::Truncate,
                        "service log (truncated in place)",
                    )
                {
                    targets.push(t);
                }
            }
        }
    }

    targets
}

// ---------- installed app matching ----------

fn installed_app_names(home: &Path) -> HashSet<String> {
    let mut names = HashSet::new();
    let dirs = [
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
        home.join("Applications"),
    ];

    for dir in dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "app") {
                    collect_app_names(&p, &mut names);
                }
            }
        }
    }
    names
}

fn collect_app_names(app: &Path, names: &mut HashSet<String>) {
    let mut raw_names: Vec<String> = Vec::new();
    if let Some(stem) = app.file_stem() {
        raw_names.push(stem.to_string_lossy().into_owned());
    }
    let plist_path = app.join("Contents/Info.plist");
    if let Ok(value) = plist::from_file::<_, plist::Value>(&plist_path)
        && let Some(dict) = value.as_dictionary()
    {
        for key in ["CFBundleName", "CFBundleDisplayName", "CFBundleExecutable"] {
            if let Some(s) = dict.get(key).and_then(|v| v.as_string()) {
                raw_names.push(s.to_string());
            }
        }
        if let Some(id) = dict.get("CFBundleIdentifier").and_then(|v| v.as_string())
            && let Some(last) = id.rsplit('.').next()
        {
            raw_names.push(last.to_string());
        }
    }
    for raw in raw_names {
        names.insert(normalize(&raw));
        // Also index each word ≥5 chars so "Brave Browser" covers a
        // "BraveSoftware" data dir.
        for word in raw.split(|c: char| !c.is_alphanumeric()) {
            let w = normalize(word);
            if w.len() >= 5 {
                names.insert(w);
            }
        }
    }
}

/// A Library dir "belongs" to an installed app when normalized names
/// overlap — exact match at any length, or ≥5-char containment both ways.
fn is_installed(dir_name: &str, installed: &HashSet<String>) -> bool {
    // Check the dir name and its bundle-id segments (e.g. "com.openai.atlas").
    let mut names = vec![dir_name.to_string()];
    if dir_name.contains('.') {
        names.extend(dir_name.split('.').map(|s| s.to_string()));
        if let Some(last) = dir_name
            .strip_suffix(".savedState")
            .or(dir_name.rsplit('.').next())
        {
            names.push(last.to_string());
        }
    }

    for name in &names {
        let n = normalize(name);
        // Exact match first — safe even for short names like "zed".
        if installed.contains(&n) {
            return true;
        }
        // Substring either way: "Google" dir ↔ "googlechrome" app.
        // Require 5+ chars on both sides — shorter names cause false
        // positives, e.g. "codex" containing "code" (VS Code).
        if n.len() >= 5
            && installed
                .iter()
                .any(|i| i.len() >= 5 && (n.contains(i.as_str()) || i.contains(&n)))
        {
            return true;
        }
    }
    false
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

// ---------- sizing ----------

fn sized(
    path: PathBuf,
    category: Category,
    safety: Safety,
    action: Action,
    reason: impl Into<String>,
) -> Option<CleanTarget> {
    sized_min(path, MIN_SIZE, category, safety, action, reason)
}

/// `sized` with a custom floor — orphan plists/agents are small by nature
/// and earn a row by being orphaned, not by size.
fn sized_min(
    path: PathBuf,
    min_size: u64,
    category: Category,
    safety: Safety,
    action: Action,
    reason: impl Into<String>,
) -> Option<CleanTarget> {
    let size = path_size(&path);
    (size >= min_size).then(|| CleanTarget::new(path, size, category, safety, action, reason))
}

fn path_size(path: &Path) -> u64 {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return 0,
    };
    if meta.is_file() {
        return meta.blocks() * 512;
    }
    // Serial walk — callers already parallelize via rayon. jwalk spawns a
    // thread pool per call; nesting it inside par_iter exhausts threads and
    // made dirs silently report 0 bytes.
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.metadata().map(|m| m.blocks() * 512).unwrap_or(0))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_beats_length_guard() {
        let installed: HashSet<String> = ["zed", "code", "googlechrome"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(is_installed("Zed", &installed));
        assert!(is_installed("Code", &installed));
        assert!(is_installed("Google", &installed));
    }

    #[test]
    fn codex_must_not_match_vscode() {
        // "codex" contains "code" — containment alone is too loose.
        let installed: HashSet<String> = ["code", "visualstudiocode", "vscode"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(!is_installed("Codex", &installed));
    }

    #[test]
    fn bundle_id_segments_match() {
        let installed: HashSet<String> = ["figma"].into_iter().map(String::from).collect();
        assert!(is_installed("com.figma.Desktop", &installed));
    }

    #[test]
    fn apple_dirs_are_skipped() {
        assert!(skip_leftover_name("com.apple.Spotlight"));
        assert!(skip_leftover_name(".hidden"));
        assert!(!skip_leftover_name("Codex"));
    }

    #[test]
    fn launch_agent_labels_match_via_segments() {
        let installed: HashSet<String> =
            ["docker", "figma"].into_iter().map(String::from).collect();
        assert!(is_installed("com.docker.helper", &installed));
        assert!(is_installed("com.figma.Desktop.agent", &installed));
        assert!(!is_installed("com.uninstalled.tool", &installed));
    }

    #[test]
    fn pref_stems_match_installed_apps() {
        let installed: HashSet<String> = ["brave", "googlechrome"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(is_installed("com.brave.Browser", &installed));
        assert!(is_installed("com.google.Chrome", &installed));
        assert!(!is_installed("com.gone.App", &installed));
    }

    #[test]
    fn command_specs_are_sane() {
        for spec in COMMAND_TARGETS {
            // No PATH lookups — a bare name could resolve to anything.
            assert!(spec.bins.iter().all(|b| b.starts_with('/')));
            assert!(!spec.args.is_empty());
            // size_hint dirs must be a subset of data_dirs so "covered"
            // never hides a dir the command doesn't relate to.
            for hint in spec.size_hint {
                assert!(spec.data_dirs.contains(hint));
            }
        }
    }

    #[test]
    fn resolve_dir_handles_relative_and_absolute() {
        let home = Path::new("/Users/x");
        assert_eq!(
            resolve_dir(home, "Library/Caches"),
            home.join("Library/Caches")
        );
        assert_eq!(
            resolve_dir(home, "/opt/homebrew"),
            PathBuf::from("/opt/homebrew")
        );
    }
}
