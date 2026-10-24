use crate::model::{Action, CommandSpec};
use std::path::Path;

/// Trash actions go to ~/.Trash via NSFileManager (recoverable through
/// Finder "Put Back"). Truncate empties the file in place — required
/// for log files still held open by daemons. Command runs a whitelisted
/// cleanup tool's own removal logic instead of touching files.
pub fn execute(path: &Path, action: Action) -> Result<(), String> {
    match action {
        Action::Trash => trash::delete(path).map_err(|e| e.to_string()),
        Action::Truncate => std::fs::File::create(path)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        Action::Command(spec) => run_command(spec),
    }
}

fn run_command(spec: &CommandSpec) -> Result<(), String> {
    let bin = spec
        .resolve_bin()
        .ok_or_else(|| format!("{}: binary not found", spec.display))?;
    let out = std::process::Command::new(&bin)
        .args(spec.args)
        .output()
        .map_err(|e| format!("{}: {e}", spec.display))?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let detail = stderr.trim();
        if detail.is_empty() {
            Err(format!("{} exited with {}", spec.display, out.status))
        } else {
            Err(format!("{}: {}", spec.display, detail))
        }
    }
}
