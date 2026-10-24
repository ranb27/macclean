//! `macclean --mcp` — Model Context Protocol server over stdio.
//!
//! Transport is newline-delimited JSON-RPC 2.0 (MCP stdio). Three tools:
//! `disk_summary`, `scan_targets`, `clean_targets`. The clean tool is
//! whitelist-by-construction: it re-runs a fresh scan and only executes
//! targets whose safety is Regenerable — an AI client cannot make it delete
//! anything the app itself wouldn't pre-tick.

use crate::model::{Action, CleanTarget, Safety, fmt_size};
use crate::{scan, trash};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;

/// Read JSON-RPC lines from stdin, write responses to stdout until EOF.
pub fn run() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => {
                write_line(
                    &mut out,
                    &json!({"jsonrpc": "2.0", "id": Value::Null,
                            "error": {"code": -32700, "message": "parse error"}}),
                )?;
                continue;
            }
        };
        if let Some(resp) = handle(&msg) {
            write_line(&mut out, &resp)?;
        }
    }
    Ok(())
}

fn write_line(out: &mut impl Write, v: &Value) -> std::io::Result<()> {
    writeln!(out, "{v}")?;
    out.flush()
}

/// Dispatch one message. Requests produce a response; notifications produce None.
fn handle(msg: &Value) -> Option<Value> {
    let id = msg.get("id")?;
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let respond = |r: Value| Some(json!({"jsonrpc": "2.0", "id": id, "result": r}));
    match method {
        // Echo the client's requested version — maximum interop.
        "initialize" => {
            let ver = msg
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2024-11-05");
            respond(json!({
                "protocolVersion": ver,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "macclean", "version": env!("CARGO_PKG_VERSION")},
            }))
        }
        "ping" => respond(json!({})),
        "tools/list" => respond(json!({"tools": tool_list()})),
        "tools/call" => respond(call_tool(msg.get("params"))),
        "resources/list" => respond(json!({"resources": []})),
        "prompts/list" => respond(json!({"prompts": []})),
        _ => Some(json!({"jsonrpc": "2.0", "id": id,
                         "error": {"code": -32601, "message": "method not found"}})),
    }
}

fn tool_list() -> Value {
    json!([
        {
            "name": "disk_summary",
            "description": "Free and total disk space on the startup volume. Fast — no scan.",
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "scan_targets",
            "description": "Scan the Mac and list cleanup targets with size, category, safety \
                            (safe = regenerable, review = needs user judgement), and reason.",
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "clean_targets",
            "description": "Clean the given target paths (Trash / truncate / whitelisted command). \
                            Only accepts paths returned by scan_targets whose safety is 'safe' — \
                            anything else is skipped. Always recoverable via the Trash.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Target paths exactly as returned by scan_targets",
                    },
                },
                "required": ["paths"],
            },
        },
    ])
}

fn call_tool(params: Option<&Value>) -> Value {
    let Some(params) = params else {
        return tool_error("missing params");
    };
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments");
    match name {
        "disk_summary" => tool_ok(disk_summary()),
        "scan_targets" => tool_ok(scan_targets()),
        "clean_targets" => match clean_targets(args) {
            Ok(v) => tool_ok(v),
            Err(e) => tool_error(&e),
        },
        _ => tool_error(&format!("unknown tool: {name}")),
    }
}

fn tool_ok(structured: Value) -> Value {
    json!({
        "content": [{"type": "text", "text": structured.to_string()}],
        "structuredContent": structured,
        "isError": false,
    })
}

fn tool_error(msg: &str) -> Value {
    json!({
        "content": [{"type": "text", "text": msg}],
        "isError": true,
    })
}

/// Free/total bytes of the startup volume via statfs("/").
fn disk_summary() -> Value {
    let (total, free) = unsafe {
        let mut s: libc::statfs = std::mem::zeroed();
        if libc::statfs(c"/".as_ptr(), &mut s) != 0 {
            return json!({"error": "statfs failed"});
        }
        (
            s.f_blocks * u64::from(s.f_bsize),
            s.f_bavail * u64::from(s.f_bsize),
        )
    };
    json!({
        "total_bytes": total,
        "free_bytes": free,
        "summary": format!("{} free of {}", fmt_size(free), fmt_size(total)),
    })
}

fn target_json(t: &CleanTarget) -> Value {
    json!({
        "path": t.path.display().to_string(),
        "size_bytes": t.size,
        "size": fmt_size(t.size),
        "category": t.category.label(),
        "safety": match t.safety {
            Safety::Regenerable => "safe",
            Safety::Review => "review",
        },
        "action": match t.action {
            Action::Trash => "trash",
            Action::Truncate => "truncate",
            Action::Command(s) => s.display,
        },
        "reason": t.reason,
        "deletable_via_mcp": matches!(t.safety, Safety::Regenerable),
    })
}

fn scan_targets() -> Value {
    let r = scan::scan_all();
    let total: u64 = r.targets.iter().map(|t| t.size).sum();
    json!({
        "targets": r.targets.iter().map(target_json).collect::<Vec<_>>(),
        "count": r.targets.len(),
        "total_reclaimable_bytes": total,
        "total_reclaimable": fmt_size(total),
        "scan_errors": r.errors,
    })
}

/// Targets the AI is allowed to clean — regenerable only.
fn allowed_set(targets: &[CleanTarget]) -> HashMap<&PathBuf, &CleanTarget> {
    targets
        .iter()
        .filter(|t| matches!(t.safety, Safety::Regenerable))
        .map(|t| (&t.path, t))
        .collect()
}

/// Clean only paths that a fresh scan still reports as regenerable.
/// Anything else — review-level or not a target at all — is skipped.
fn clean_targets(args: Option<&Value>) -> Result<Value, String> {
    let paths: Vec<PathBuf> = args
        .and_then(|a| a.get("paths"))
        .and_then(Value::as_array)
        .ok_or("missing arguments.paths")?
        .iter()
        .filter_map(Value::as_str)
        .map(PathBuf::from)
        .collect();

    let scan = scan::scan_all();
    let allowed = allowed_set(&scan.targets);
    let mut cleaned = Vec::new();
    let mut skipped = Vec::new();
    let mut failed = Vec::new();
    let mut freed: u64 = 0;

    for path in paths {
        let Some(&target) = allowed.get(&path) else {
            skipped.push(format!("{} (not a safe scan target)", path.display()));
            continue;
        };
        match trash::execute(&target.path, target.action) {
            Ok(()) => {
                cleaned.push(target.path.display().to_string());
                freed += target.size;
            }
            Err(e) => failed.push(format!("{}: {e}", target.path.display())),
        }
    }

    Ok(json!({
        "cleaned": cleaned,
        "skipped": skipped,
        "failed": failed,
        "freed": fmt_size(freed),
        "freed_bytes": freed,
        "note": "cleaned items are in ~/.Trash — Put Back restores them",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Category;

    fn req(id: u64, method: &str, params: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
    }

    #[test]
    fn initialize_echoes_client_version() {
        let r = handle(&req(
            1,
            "initialize",
            json!({"protocolVersion": "2025-03-26"}),
        ))
        .unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "macclean");
    }

    #[test]
    fn tools_list_has_three_tools() {
        let r = handle(&req(2, "tools/list", json!({}))).unwrap();
        let tools = r["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);
        assert!(tools.iter().all(|t| t["inputSchema"].is_object()));
    }

    #[test]
    fn unknown_method_errors() {
        let r = handle(&req(3, "nope", json!({}))).unwrap();
        assert_eq!(r["error"]["code"], -32601);
    }

    #[test]
    fn notifications_get_no_response() {
        assert!(
            handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none()
        );
    }

    #[test]
    fn allowed_set_filters_review_targets() {
        let safe = CleanTarget::new(
            PathBuf::from("/tmp/a"),
            100,
            Category::AppCaches,
            Safety::Regenerable,
            Action::Trash,
            "cache",
        );
        let review = CleanTarget::new(
            PathBuf::from("/tmp/b"),
            100,
            Category::Leftovers,
            Safety::Review,
            Action::Trash,
            "leftover",
        );
        let targets = [safe, review];
        let set = allowed_set(&targets);
        let a = PathBuf::from("/tmp/a");
        let b = PathBuf::from("/tmp/b");
        assert!(set.contains_key(&a));
        assert!(!set.contains_key(&b));
    }
}
