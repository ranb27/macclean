use crate::model::Action;
use std::path::Path;

/// Trash actions go to ~/.Trash via NSFileManager (recoverable through
/// Finder "Put Back"). Truncate empties the file in place — required
/// for log files still held open by daemons.
pub fn remove(path: &Path, action: Action) -> Result<(), String> {
    match action {
        Action::Trash => trash::delete(path).map_err(|e| e.to_string()),
        Action::Truncate => std::fs::File::create(path)
            .map(|_| ())
            .map_err(|e| e.to_string()),
    }
}
