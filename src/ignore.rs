//! UI-managed ignore list — paths the user never wants offered again.
//!
//! Distinct from `config.toml`'s `exclude`: that file is hand-edited user
//! policy, while this file is machine-owned state the app writes when you
//! click "ignore" on a row. Both are consulted at scan time, so a hidden
//! path stays hidden in the GUI, `--dump`, and MCP alike — `clean_targets`
//! can never act on an ignored path because the scan never returns it.
//!
//! Stored as a plain JSON array of absolute path strings at
//! `~/.config/macclean/ignore.json`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn path() -> PathBuf {
    home().join(".config/macclean/ignore.json")
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// Missing or malformed file means "nothing ignored" — never a fatal error.
pub fn load() -> HashSet<PathBuf> {
    let Ok(text) = std::fs::read_to_string(path()) else {
        return HashSet::new();
    };
    parse(&text)
}

pub fn save(ignored: &HashSet<PathBuf>) -> Result<(), String> {
    let path = path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
    }
    std::fs::write(&path, render(ignored)).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Exact path or any descendant of an ignored dir — same rule as config
/// exclusions, so "ignore" behaves identically to `exclude`.
pub fn contains(ignored: &HashSet<PathBuf>, path: &Path) -> bool {
    ignored.iter().any(|e| path.starts_with(e))
}

fn parse(text: &str) -> HashSet<PathBuf> {
    serde_json::from_str::<Vec<String>>(text)
        .map(|l| l.into_iter().map(PathBuf::from).collect())
        .unwrap_or_default()
}

fn render(ignored: &HashSet<PathBuf>) -> String {
    let mut list: Vec<_> = ignored.iter().map(|p| p.display().to_string()).collect();
    list.sort();
    format!(
        "{}\n",
        serde_json::to_string_pretty(&list).unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tolerates_garbage() {
        assert!(parse("not json").is_empty());
        assert!(parse("{}").is_empty());
        assert!(parse("[42]").is_empty());
        assert_eq!(parse(r#"["/tmp/a"]"#).len(), 1);
    }

    #[test]
    fn render_roundtrips_sorted() {
        let set: HashSet<PathBuf> = ["/b/x", "/a/y"].iter().map(PathBuf::from).collect();
        let parsed = parse(&render(&set));
        assert_eq!(parsed, set);
    }

    #[test]
    fn contains_matches_descendants() {
        let set: HashSet<PathBuf> = ["/Users/x/Library/Caches/Foo"]
            .iter()
            .map(PathBuf::from)
            .collect();
        assert!(contains(&set, Path::new("/Users/x/Library/Caches/Foo")));
        assert!(contains(
            &set,
            Path::new("/Users/x/Library/Caches/Foo/deep")
        ));
        assert!(!contains(&set, Path::new("/Users/x/Library/Caches/Foo-2")));
        assert!(!contains(&set, Path::new("/tmp")));
    }
}
