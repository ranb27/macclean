//! User config — custom scan paths and exclusions.
//!
//! Read from `~/Library/Application Support/macclean/config.toml`
//! (`MACCLEAN_CONFIG` overrides the location). Every key is optional — a
//! missing or empty file scans exactly like having no config at all.
//!
//! ```toml
//! exclude = ["~/Library/Caches/keep-me", "/opt/big-cache"]
//!
//! [[custom]]
//! path = "~/work/scratch"
//! safety = "review"          # "safe" marks it regenerable — review is the default
//! reason = "work scratch cache"
//! ```

use crate::model::Safety;
use std::path::{Path, PathBuf};

pub struct Config {
    /// Targets under any of these paths are never offered. Exclusions win
    /// over custom paths — "never touch" beats "scan this".
    pub exclude: Vec<PathBuf>,
    /// Extra dirs the user wants offered for reclamation.
    pub custom: Vec<CustomTarget>,
}

pub struct CustomTarget {
    pub path: PathBuf,
    pub safety: Safety,
    pub reason: String,
}

impl Config {
    /// Exact path or any descendant of an excluded dir.
    pub fn excludes(&self, path: &Path) -> bool {
        self.exclude.iter().any(|e| path.starts_with(e))
    }
}

/// `MACCLEAN_CONFIG` first (tests/overrides), else the Application Support
/// location — macclean's own dir is never scan-flagged because the app is
/// installed.
pub fn path() -> PathBuf {
    if let Ok(p) = std::env::var("MACCLEAN_CONFIG")
        && !p.is_empty()
    {
        return PathBuf::from(p);
    }
    home().join("Library/Application Support/macclean/config.toml")
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// `~` → home, `~/x` → home/x, bare relative → home-relative, `/abs` as-is.
fn expand(home: &Path, raw: &str) -> PathBuf {
    if raw == "~" {
        home.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else if raw.starts_with('/') {
        PathBuf::from(raw)
    } else {
        home.join(raw)
    }
}

/// Loaded config plus an optional human-readable problem (bad TOML,
/// unknown keys). Problems never abort the scan — they surface as scan
/// errors so the user sees them in the app, `--dump`, and MCP alike.
pub fn load() -> (Config, Option<String>) {
    let path = path();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            return (
                Config {
                    exclude: vec![],
                    custom: vec![],
                },
                None,
            );
        }
    };
    match parse(&text, &home()) {
        Ok(cfg) => (cfg, None),
        Err(e) => (
            Config {
                exclude: vec![],
                custom: vec![],
            },
            Some(format!("{}: {e}", path.display())),
        ),
    }
}

fn parse(text: &str, home: &Path) -> Result<Config, String> {
    let root: toml::Table = text.parse().map_err(|e| format!("invalid TOML ({e})"))?;
    let mut cfg = Config {
        exclude: vec![],
        custom: vec![],
    };

    if let Some(v) = root.get("exclude") {
        let list = v
            .as_array()
            .ok_or("`exclude` must be a list of path strings")?;
        for (i, item) in list.iter().enumerate() {
            let s = item
                .as_str()
                .ok_or(format!("`exclude[{i}]` must be a string"))?;
            cfg.exclude.push(expand(home, s));
        }
    }

    if let Some(v) = root.get("custom") {
        let list = v
            .as_array()
            .ok_or("`custom` must be a list of tables — use [[custom]]")?;
        for (i, item) in list.iter().enumerate() {
            let t = item
                .as_table()
                .ok_or(format!("`custom[{i}]` must be a table"))?;
            let raw_path = t
                .get("path")
                .and_then(toml::Value::as_str)
                .ok_or(format!("`custom[{i}].path` is required"))?;
            let safety = match t.get("safety").and_then(toml::Value::as_str) {
                None | Some("review") => Safety::Review,
                Some("safe") | Some("regenerable") => Safety::Regenerable,
                Some(other) => {
                    return Err(format!(
                        "`custom[{i}].safety` must be \"safe\" or \"review\", got \"{other}\""
                    ));
                }
            };
            let reason = t
                .get("reason")
                .and_then(toml::Value::as_str)
                .unwrap_or("custom path")
                .to_string();
            cfg.custom.push(CustomTarget {
                path: expand(home, raw_path),
                safety,
                reason,
            });
        }
    }

    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/Users/test")
    }

    #[test]
    fn empty_config_is_default() {
        let cfg = parse("", &home()).unwrap();
        assert!(cfg.exclude.is_empty() && cfg.custom.is_empty());
    }

    #[test]
    fn paths_expand_home_and_tilde() {
        let cfg = parse(
            r#"exclude = ["~/keep", "rel/dir", "/abs/dir", "~"]"#,
            &home(),
        )
        .unwrap();
        assert_eq!(cfg.exclude[0], home().join("keep"));
        assert_eq!(cfg.exclude[1], home().join("rel/dir"));
        assert_eq!(cfg.exclude[2], PathBuf::from("/abs/dir"));
        assert_eq!(cfg.exclude[3], home());
    }

    #[test]
    fn exclusion_matches_descendants() {
        let cfg = parse(r#"exclude = ["~/Library/Caches"]"#, &home()).unwrap();
        assert!(cfg.excludes(&home().join("Library/Caches/foo/bar")));
        assert!(cfg.excludes(&home().join("Library/Caches")));
        assert!(!cfg.excludes(&home().join("Library/Caches-2")));
        assert!(!cfg.excludes(&home().join("Downloads")));
    }

    #[test]
    fn custom_defaults_to_review() {
        let cfg = parse(
            r#"
            [[custom]]
            path = "~/scratch"
            "#,
            &home(),
        )
        .unwrap();
        assert_eq!(cfg.custom.len(), 1);
        assert_eq!(cfg.custom[0].safety, Safety::Review);
        assert_eq!(cfg.custom[0].reason, "custom path");
    }

    #[test]
    fn custom_safe_marks_regenerable() {
        let cfg = parse(
            r#"
            [[custom]]
            path = "/tmp/cache"
            safety = "safe"
            reason = "my cache"
            "#,
            &home(),
        )
        .unwrap();
        assert_eq!(cfg.custom[0].safety, Safety::Regenerable);
        assert_eq!(cfg.custom[0].reason, "my cache");
    }

    #[test]
    fn bad_safety_is_an_error_not_a_crash() {
        assert!(
            parse(
                "[[custom]]\npath = \"~/x\"\nsafety = \"definitely\"\n",
                &home()
            )
            .is_err()
        );
    }

    #[test]
    fn bad_toml_is_an_error_not_a_crash() {
        assert!(parse("[[[not toml", &home()).is_err());
        assert!(parse("exclude = 42", &home()).is_err());
        assert!(parse("custom = [\"x\"]", &home()).is_err());
        assert!(parse("[[custom]]\nreason = \"no path\"", &home()).is_err());
    }
}
