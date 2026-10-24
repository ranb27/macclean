//! One-click wiring of `macclean --mcp` into the user's AI clients.
//! Each client is either a config file we can merge into (JSON/TOML),
//! a CLI command (`claude mcp add`), or copy-only (web-managed clients).
//! Writers preserve existing config contents — we only touch our own key.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// How we decide the client is installed on this machine — detected if
/// any dir exists or any binary resolves (PATH + well-known locations).
pub struct Detect {
    pub dirs: &'static [&'static str],
    pub bins: &'static [&'static str],
}

/// How a client gets wired.
pub enum Method {
    /// `{"mcpServers": {"macclean": {command, args}}}` merged into JSON.
    McpJson(&'static str),
    /// `[mcp_servers.macclean]` in Codex's config.toml.
    CodexToml(&'static str),
    /// Claude covers both surfaces: Desktop's config file plus the
    /// `claude` CLI for Claude Code — we wire whichever are present.
    Claude,
    /// No file to write — user pastes the snippet themselves.
    Manual,
}

pub struct Client {
    pub name: &'static str,
    /// Secondary line under the name in the Connect AI window.
    pub sub: &'static str,
    pub detect: Detect,
    pub method: Method,
    /// Shown after a successful connect — usually "restart the app".
    pub note: &'static str,
}

/// Big-players list only — anyone else uses the generic snippet.
pub const CLIENTS: &[Client] = &[
    Client {
        name: "Claude",
        sub: "Anthropic — Desktop app & Code CLI",
        detect: Detect {
            dirs: &["Library/Application Support/Claude"],
            bins: &["claude"],
        },
        method: Method::Claude,
        note: "restart the app to pick it up",
    },
    Client {
        name: "OpenAI Codex",
        sub: "OpenAI — Codex CLI",
        detect: Detect {
            dirs: &[".codex"],
            bins: &["codex"],
        },
        method: Method::CodexToml(".codex/config.toml"),
        note: "new codex sessions pick it up",
    },
    Client {
        name: "Google Gemini",
        sub: "Google — Gemini CLI",
        detect: Detect {
            dirs: &[".gemini"],
            bins: &["gemini"],
        },
        method: Method::McpJson(".gemini/settings.json"),
        note: "new gemini sessions pick it up",
    },
    Client {
        name: "Devin",
        sub: "Cognition — org settings",
        detect: Detect {
            dirs: &[".config/devin"],
            bins: &[],
        },
        method: Method::Manual,
        note: "MCP servers are added in your Devin org settings",
    },
    Client {
        name: "Other MCP client",
        sub: "Custom MCP clients",
        detect: Detect {
            dirs: &[],
            bins: &[],
        },
        method: Method::Manual,
        note: "paste into your client's mcpServers config",
    },
];

const CLAUDE_DESKTOP_DIR: &str = "Library/Application Support/Claude";
const CLAUDE_DESKTOP_CFG: &str = "Library/Application Support/Claude/claude_desktop_config.json";
const CLAUDE_CODE_CFG: &str = ".claude.json";

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

/// Where the wiring lands — shown in the expanded row.
pub fn config_hint(c: &Client) -> String {
    match &c.method {
        Method::McpJson(rel) | Method::CodexToml(rel) => format!("~/{rel}"),
        Method::Claude => "~/Library/…/claude_desktop_config.json + `claude mcp add`".into(),
        Method::Manual => "paste the snippet into your client's config".into(),
    }
}

/// Installed on this machine?
pub fn is_detected(c: &Client) -> bool {
    c.detect.dirs.iter().any(|d| home().join(d).exists())
        || c.detect.bins.iter().any(|b| which(b).is_some())
}

/// Already wired — the config already names a `macclean` server.
pub fn is_wired(c: &Client) -> bool {
    match &c.method {
        Method::McpJson(rel) => json_has_macclean(&home().join(rel)),
        Method::CodexToml(rel) => toml_has_macclean(&home().join(rel)),
        Method::Claude => {
            json_has_macclean(&home().join(CLAUDE_DESKTOP_CFG))
                || json_has_macclean(&home().join(CLAUDE_CODE_CFG))
        }
        Method::Manual => false,
    }
}

/// PATH lookups — GUI apps get a minimal PATH, so check the usual
/// install spots on top of whatever PATH provides.
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

fn json_has_macclean(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|v| v["mcpServers"]["macclean"].is_object())
        .unwrap_or(false)
}

fn toml_has_macclean(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|v| {
            v.get("mcp_servers")?
                .get("macclean")?
                .as_table()
                .map(|_| true)
        })
        .unwrap_or(false)
}

/// Connect one client. Returns a human-readable status line.
pub fn connect(c: &Client) -> Result<String, String> {
    let exe = exe_path();
    match &c.method {
        Method::McpJson(rel) => {
            let path = home().join(rel);
            merge_json(&path, |m| {
                m.insert(
                    "macclean".into(),
                    json!({"command": exe, "args": ["--mcp"]}),
                );
            })?;
            Ok(format!("wrote {}", path.display()))
        }
        Method::CodexToml(rel) => {
            let path = home().join(rel);
            merge_toml(&path, &exe)?;
            Ok(format!("wrote {}", path.display()))
        }
        Method::Claude => {
            let mut done = Vec::new();
            if home().join(CLAUDE_DESKTOP_DIR).is_dir() {
                let path = home().join(CLAUDE_DESKTOP_CFG);
                merge_json(&path, |m| {
                    m.insert(
                        "macclean".into(),
                        json!({"command": exe, "args": ["--mcp"]}),
                    );
                })?;
                done.push("Desktop");
            }
            if which("claude").is_some() {
                claude_cli_add(&exe)?;
                done.push("Code");
            }
            if done.is_empty() {
                Err("no Claude app found".into())
            } else {
                Ok(format!("wired Claude {}", done.join(" + ")))
            }
        }
        Method::Manual => Err("no config file — copy the snippet below".into()),
    }
}

/// Remove the macclean entry — the inverse of `connect`.
pub fn disconnect(c: &Client) -> Result<String, String> {
    match &c.method {
        Method::McpJson(rel) => {
            let path = home().join(rel);
            remove_json_key(&path)?;
            Ok(format!("removed from {}", path.display()))
        }
        Method::CodexToml(rel) => {
            let path = home().join(rel);
            remove_toml_key(&path)?;
            Ok(format!("removed from {}", path.display()))
        }
        Method::Claude => {
            let mut done = Vec::new();
            let path = home().join(CLAUDE_DESKTOP_CFG);
            if json_has_macclean(&path) {
                remove_json_key(&path)?;
                done.push("Desktop");
            }
            let code_cfg = home().join(CLAUDE_CODE_CFG);
            if json_has_macclean(&code_cfg) {
                if which("claude").is_some() {
                    claude_cli_remove()?;
                } else {
                    // CLI gone but the config remains — edit it directly.
                    remove_json_key(&code_cfg)?;
                }
                done.push("Code");
            }
            if done.is_empty() {
                Err("macclean isn't in any Claude config".into())
            } else {
                Ok(format!("removed from Claude {}", done.join(" + ")))
            }
        }
        Method::Manual => Err("nothing to disconnect".into()),
    }
}

fn claude_cli_remove() -> Result<(), String> {
    let bin = which("claude").unwrap();
    // entries we add are user-scope; try that first, then a bare remove.
    for args in [
        &["mcp", "remove", "macclean", "-s", "user"][..],
        &["mcp", "remove", "macclean"][..],
    ] {
        if let Ok(out) = std::process::Command::new(&bin).args(args).output()
            && out.status.success()
        {
            return Ok(());
        }
    }
    Err("`claude mcp remove` failed".into())
}

fn remove_json_key(path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut root: Value = serde_json::from_str(&text).map_err(|e| format!("invalid JSON: {e}"))?;
    let Some(servers) = root.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return Err("no mcpServers in config".into());
    };
    servers.remove("macclean");
    std::fs::write(path, serde_json::to_string_pretty(&root).unwrap() + "\n")
        .map_err(|e| e.to_string())
}

fn remove_toml_key(path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut root: toml::Table = text.parse().map_err(|e| format!("invalid TOML: {e}"))?;
    if let Some(t) = root
        .get_mut("mcp_servers")
        .and_then(toml::Value::as_table_mut)
    {
        t.remove("macclean");
    }
    std::fs::write(
        path,
        toml::to_string_pretty(&root).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn claude_cli_add(exe: &str) -> Result<(), String> {
    let bin = which("claude").unwrap();
    let out = std::process::Command::new(bin)
        .args(["mcp", "add", "macclean", "-s", "user", "--", exe, "--mcp"])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// End-to-end check: spawn `macclean --mcp`, handshake, list tools.
/// stdin closes after the requests → server exits on EOF.
pub fn test_server() -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = Command::new(exe)
        .arg("--mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut w) = child.stdin.take() {
        let init = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{{}},\"clientInfo\":{{\"name\":\"macclean-selftest\",\"version\":\"{}\"}}}}}}",
            env!("CARGO_PKG_VERSION")
        );
        let _ = writeln!(w, "{init}");
        let _ = writeln!(
            w,
            "{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}}"
        );
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("server didn't exit: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    if text.contains("\"name\":\"macclean\"") && text.contains("\"clean_targets\"") {
        Ok("server handshake ok — 3 tools advertised".into())
    } else {
        Err(format!(
            "unexpected response: {}",
            text.lines().next().unwrap_or("(empty)")
        ))
    }
}

/// Merge `mcpServers.macclean = …` into a JSON file, preserving everything
/// else. Creates the file (and parent dirs) if missing.
fn merge_json(
    path: &Path,
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
        .entry("mcpServers")
        .or_insert_with(|| json!({}));
    if !servers.is_object() {
        return Err(format!("{}: `mcpServers` isn't an object", path.display()));
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
        merge_json(&path, |m| {
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
        merge_json(&path, |m| {
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
        let v: toml::Table = text.parse().unwrap();
        assert_eq!(
            v["mcp_servers"]["macclean"]["command"].as_str().unwrap(),
            "/x"
        );
        assert_eq!(v["model"].as_str().unwrap(), "o4");
    }

    #[test]
    fn disconnect_round_trip() {
        let path = tempdir("rt").join("cfg.json");
        merge_json(&path, |m| {
            m.insert(
                "macclean".into(),
                json!({"command": "/x", "args": ["--mcp"]}),
            );
            m.insert("other".into(), json!({"command": "/y"}));
        })
        .unwrap();
        assert!(json_has_macclean(&path));
        remove_json_key(&path).unwrap();
        assert!(!json_has_macclean(&path));
        // sibling server survives the removal
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "/y");
    }

    #[test]
    fn wired_detection_reads_config() {
        let dir = tempdir("wired");
        let path = dir.join("s.json");
        assert!(!json_has_macclean(&path));
        std::fs::write(&path, r#"{"mcpServers":{"macclean":{"command":"/x"}}}"#).unwrap();
        assert!(json_has_macclean(&path));
    }
}
