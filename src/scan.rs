use crate::model::{Action, Category, CleanTarget, Safety};
use rayon::prelude::*;
use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const MIN_SIZE: u64 = 5 * 1024 * 1024; // ignore dirs under 5 MB — noise

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
    ("Library/Developer/Xcode/DerivedData", "Xcode build artifacts"),
    (
        "Library/Developer/CoreSimulator/Caches",
        "simulator caches",
    ),
    (".m2/repository", "Maven repository cache"),
    ("Library/Caches/Yarn", "Yarn cache"),
    (".yarn/cache", "Yarn cache"),
    (".deno", "Deno cache"),
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
    "com.apple", "apple", "ns.", "group.com.apple", "icloud", "syncservices",
    "crashreporter", "knowledge", "callservicesd", "caches",
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
        Err(_) => return ScanResult { targets: vec![], errors: vec!["HOME not set".into()] },
    };

    let installed = installed_app_names(&home);
    let mut targets = Vec::new();
    let mut errors = Vec::new();

    targets.extend(scan_devtools(&home));
    targets.extend(scan_logs(&home));

    let (mut cache_t, mut cache_e) = scan_app_caches(&home, &installed);
    targets.append(&mut cache_t);
    errors.append(&mut cache_e);

    targets.extend(scan_leftovers(&home, &installed));

    targets.sort_by(|a, b| b.size.cmp(&a.size));
    ScanResult { targets, errors }
}

// ---------- dev caches ----------

fn scan_devtools(home: &Path) -> Vec<CleanTarget> {
    DEV_CACHE_PATHS
        .par_iter()
        .filter_map(|(rel, reason)| {
            let path = home.join(rel);
            path.is_dir().then(|| {
                sized(path, Category::DevCaches, Safety::Regenerable, Action::Trash, *reason)
            })
        })
        .flatten()
        .collect()
}

// ---------- app caches ----------

fn scan_app_caches(home: &Path, installed: &HashSet<String>) -> (Vec<CleanTarget>, Vec<String>) {
    let mut candidates: Vec<(PathBuf, String)> = Vec::new();

    // ~/Library/Caches/<entry> — one candidate per top-level entry.
    if let Ok(entries) = std::fs::read_dir(home.join("Library/Caches")) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name.starts_with("com.apple") {
                continue; // TCC-protected system caches — can't touch anyway
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
            sized(p.clone(), Category::AppCaches, Safety::Regenerable, Action::Trash, r.clone())
        })
        .collect();
    (targets, vec![])
}

// ---------- leftovers ----------

fn scan_leftovers(home: &Path, installed: &HashSet<String>) -> Vec<CleanTarget> {
    let mut candidates = Vec::new();

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
                    candidates.push((e.path(), name));
                }
            }
        }
    }

    candidates
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
        .collect()
}

fn skip_leftover_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    name.starts_with('.')
        || LEFTOVER_SKIP_PREFIXES.iter().any(|p| lower.starts_with(p))
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
                if p.extension().is_some_and(|x| x == "log") {
                    if let Some(t) = sized(
                        p,
                        Category::Logs,
                        Safety::Review,
                        Action::Truncate,
                        "service log (truncated in place)",
                    ) {
                        targets.push(t);
                    }
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
    if let Ok(value) = plist::from_file::<_, plist::Value>(&plist_path) {
        if let Some(dict) = value.as_dictionary() {
            for key in ["CFBundleName", "CFBundleDisplayName", "CFBundleExecutable"] {
                if let Some(s) = dict.get(key).and_then(|v| v.as_string()) {
                    raw_names.push(s.to_string());
                }
            }
            if let Some(id) = dict.get("CFBundleIdentifier").and_then(|v| v.as_string()) {
                if let Some(last) = id.rsplit('.').next() {
                    raw_names.push(last.to_string());
                }
            }
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
/// overlap — either direction, requiring at least 4 chars to avoid
/// false matches on entries like `go` or `Code`.
fn is_installed(dir_name: &str, installed: &HashSet<String>) -> bool {
    // Check the dir name and its bundle-id segments (e.g. "com.openai.atlas").
    let mut names = vec![dir_name.to_string()];
    if dir_name.contains('.') {
        names.extend(dir_name.split('.').map(|s| s.to_string()));
        if let Some(last) = dir_name.strip_suffix(".savedState").or(dir_name.rsplit('.').next()) {
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
    let size = path_size(&path);
    (size >= MIN_SIZE).then(|| CleanTarget::new(path, size, category, safety, action, reason))
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
        .map(|e| {
            e.metadata()
                .map(|m| m.blocks() * 512)
                .unwrap_or(0)
        })
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
}
