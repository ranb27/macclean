//! One-click wiring of `macclean --mcp` into the user's AI clients.
//! Each client is either a config file we can merge into (JSON/TOML),
//! a CLI command (`claude mcp add`), or copy-only (web-managed clients).
//! Writers preserve existing config contents — we only touch our own key.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// How we decide the client is installed on this machine.
pub enum Detect {
    /// Home-relative dirs — detected if any exists.
    Dirs(&'static [&'static str]),
    /// Binary on PATH or at a well-known absolute path.
    Bin(&'static [&'static str]),
    /// Can't detect — always offer the manual path.
    None,
}

/// How a client gets wired.
pub enum Method {
    /// `{"mcpServers": {"macclean": {command, args}}}` merged into JSON.
    McpJson(&'static str),
    /// Zed's `context_servers` shape in settings.json.
    ZedJson(&'static str),
    /// `[mcp_servers.macclean]` in Codex's config.toml.
    CodexToml(&'static str),
    /// `bin` + args_prefix + exe + `--mcp` argv run directly.
    Cli {
        bin: &'static str,
        args_prefix: &'static [&'static str],
    },
    /// No file to write — user pastes the snippet themselves.
    Manual,
}

pub struct Client {
    pub name: &'static str,
    pub detect: Detect,
    pub method: Method,
    /// Shown after a successful connect — usually "restart the app".
    pub note: &'static str,
}

pub const CLIENTS: &[Client] = &[
    Client {
        name: "Claude Desktop",
        detect: Detect::Dirs(&["Library/Application Support/Claude"]),
        method: Method::McpJson("Library/Application Support/Claude/claude_desktop_config.json"),
        note: "restart Claude Desktop to pick it up",
    },
    Client {
        name: "Claude Code",
        detect: Detect::Bin(&[
            "claude",
            "/opt/homebrew/bin/claude",
            "/usr/local/bin/claude",
        ]),
        method: Method::Cli {
            bin: "claude",
            args_prefix: &["mcp", "add", "macclean", "-s", "user", "--"],
        },
        note: "registered at user scope — available in every project",
    },
    Client {
        name: "Cursor",
        detect: Detect::Dirs(&[".cursor"]),
        method: Method::McpJson(".cursor/mcp.json"),
        note: "restart Cursor to pick it up",
    },
    Client {
        name: "Windsurf",
        detect: Detect::Dirs(&[".codeium/windsurf"]),
        method: Method::McpJson(".codeium/windsurf/mcp_config.json"),
        note: "restart Windsurf to pick it up",
    },
    Client {
        name: "Codex CLI",
        detect: Detect::Dirs(&[".codex"]),
        method: Method::CodexToml(".codex/config.toml"),
        note: "new codex sessions pick it up",
    },
    Client {
        name: "Gemini CLI",
        detect: Detect::Dirs(&[".gemini"]),
        method: Method::McpJson(".gemini/settings.json"),
        note: "new gemini sessions pick it up",
    },
    Client {
        name: "Zed",
        detect: Detect::Dirs(&[".config/zed"]),
        method: Method::ZedJson(".config/zed/settings.json"),
        note: "restart Zed to pick it up",
    },
    Client {
        name: "Devin",
        detect: Detect::Dirs(&[".config/devin"]),
        method: Method::Manual,
        note: "MCP servers are added in your Devin org settings",
    },
    Client {
        name: "Other MCP client",
        detect: Detect::None,
        method: Method::Manual,
        note: "paste into your client's mcpServers config",
    },
];

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// This running binary — correct whether launched from .app or target/.
pub fn exe_path() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "macclean".into())
}

/// What the user's client config should contain for us.
pub fn snippet() -> String {
    format!(
        "\"macclean\": {{\"command\": \"{}\", \"args\": [\"--mcp\"]}}",
        exe_path()
    )
}

pub fn is_detected(c: &Client) -> bool {
    match &c.detect {
        Detect::Dirs(dirs) => dirs.iter().any(|d| home().join(d).exists()),
        Detect::Bin(bins) => bins.iter().any(|b| {
            if b.contains('/') {
                Path::new(b).is_file()
            } else {
                which_on_path(b)
            }
        }),
        Detect::None => false,
    }
}

/// PATH lookups — GUI apps get a minimal PATH, so check the usual
/// install spots on top of whatever PATH provides.
fn which_on_path(bin: &str) -> bool {
    let mut candidates: Vec<PathBuf> = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(|d| Path::new(d).join(bin))
        .collect();
    for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        candidates.push(Path::new(d).join(bin));
    }
    candidates.push(home().join(".local/bin").join(bin));
    candidates.iter().any(|p| p.is_file())
}

fn which(bin: &str) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(|d| Path::new(d).join(bin))
        .collect();
    for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        candidates.push(Path::new(d).join(bin));
    }
    candidates.push(home().join(".local/bin").join(bin));
    candidates.into_iter().find(|p| p.is_file())
}

/// Connect one client. Returns a human-readable status line.
pub fn connect(c: &Client) -> Result<String, String> {
    let exe = exe_path();
    match &c.method {
        Method::McpJson(rel) => {
            let path = home().join(rel);
            merge_json(&path, "mcpServers", |m| {
                m.insert(
                    "macclean".into(),
                    json!({"command": exe, "args": ["--mcp"]}),
                );
            })?;
            Ok(format!("wrote {}", path.display()))
        }
        Method::ZedJson(rel) => {
            let path = home().join(rel);
            merge_json(&path, "context_servers", |m| {
                m.insert(
                    "macclean".into(),
                    json!({"command": {"path": exe, "args": ["--mcp"]}}),
                );
            })?;
            Ok(format!("wrote {}", path.display()))
        }
        Method::CodexToml(rel) => {
            let path = home().join(rel);
            merge_toml(&path, &exe)?;
            Ok(format!("wrote {}", path.display()))
        }
        Method::Cli { bin, args_prefix } => {
            let bin_path = which(bin).ok_or_else(|| format!("`{bin}` not found"))?;
            let mut args: Vec<String> = args_prefix.iter().map(|s| s.to_string()).collect();
            args.push(exe);
            args.push("--mcp".into());
            let out = std::process::Command::new(&bin_path)
                .args(&args)
                .output()
                .map_err(|e| e.to_string())?;
            if out.status.success() {
                Ok(format!("registered via `{}`", bin_path.display()))
            } else {
                Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
            }
        }
        Method::Manual => Err("no config file — copy the snippet below".into()),
    }
}

/// Merge `top_key.macclean = …` into a JSON file, preserving everything
/// else. Creates the file (and parent dirs) if missing.
fn merge_json(
    path: &Path,
    top_key: &str,
    edit: impl FnOnce(&mut serde_json::Map<String, Value>),
) -> Result<(), String> {
    let mut root: Value = match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => serde_json::from_str(&text)
            .map_err(|e| format!("{}: invalid JSON ({e}) — edit it manually", path.display()))?,
        _ => json!({}),
    };
    if !root.is_object() {
        return Err(format!(
            "{}: not a JSON object — edit manually",
            path.display()
        ));
    }
    let servers = root
        .as_object_mut()
        .unwrap()
        .entry(top_key)
        .or_insert_with(|| json!({}));
    if !servers.is_object() {
        return Err(format!("{}: `{top_key}` isn't an object", path.display()));
    }
    edit(servers.as_object_mut().unwrap());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&root).unwrap() + "\n")
        .map_err(|e| e.to_string())
}

/// `[mcp_servers.macclean]` block in config.toml, preserving the rest.
fn merge_toml(path: &Path, exe: &str) -> Result<(), String> {
    let mut root: toml::Table = match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => text
            .parse::<toml::Table>()
            .map_err(|e| format!("{}: invalid TOML ({e}) — edit manually", path.display()))?,
        _ => toml::Table::new(),
    };
    let servers = root
        .entry("mcp_servers")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let Some(table) = servers.as_table_mut() else {
        return Err(format!("{}: `mcp_servers` isn't a table", path.display()));
    };
    table.insert(
        "macclean".into(),
        toml::Value::Table(toml::Table::from_iter([
            ("command".into(), toml::Value::String(exe.into())),
            (
                "args".into(),
                toml::Value::Array(vec![toml::Value::String("--mcp".into())]),
            ),
        ])),
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        toml::to_string_pretty(&root).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("macclean-conn-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn json_merge_creates_and_preserves() {
        let path = tempdir("json").join("cfg/config.json");
        merge_json(&path, "mcpServers", |m| {
            m.insert(
                "macclean".into(),
                json!({"command": "/x", "args": ["--mcp"]}),
            );
        })
        .unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["macclean"]["command"], "/x");

        // second write into a file that already has other keys
        std::fs::write(&path, r#"{"other": 1, "mcpServers": {"keep": {}}}"#).unwrap();
        merge_json(&path, "mcpServers", |m| {
            m.insert(
                "macclean".into(),
                json!({"command": "/y", "args": ["--mcp"]}),
            );
        })
        .unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["other"], 1);
        assert!(v["mcpServers"]["keep"].is_object());
        assert_eq!(v["mcpServers"]["macclean"]["command"], "/y");
    }

    #[test]
    fn toml_merge_creates_section() {
        let path = tempdir("toml").join("config.toml");
        std::fs::write(&path, "model = \"o4\"\n").unwrap();
        merge_toml(&path, "/x").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[mcp_servers.macclean]") || text.contains("mcp_servers"));
        let v: toml::Table = text.parse().unwrap();
        assert_eq!(
            v["mcp_servers"]["macclean"]["command"].as_str().unwrap(),
            "/x"
        );
        assert_eq!(v["model"].as_str().unwrap(), "o4");
    }
}
