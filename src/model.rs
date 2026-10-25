use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Safety {
    /// Regenerable data — caches, derived data, logs. Always safe.
    Regenerable,
    /// Probably safe but worth a glance — leftovers, large unknowns.
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Move to ~/.Trash — recoverable via Finder "Put Back".
    Trash,
    /// Truncate file in place — for live log files held open by daemons.
    Truncate,
    /// Run a whitelisted cleanup command — argv is fixed, never free-form.
    Command(&'static CommandSpec),
}

/// A fixed cleanup command offered as a target when its tool is installed.
/// `argv` is a compile-time constant — the command can never be injected.
#[derive(Debug, PartialEq, Eq)]
pub struct CommandSpec {
    /// Absolute paths tried in order — first existing file wins.
    pub bins: &'static [&'static str],
    pub args: &'static [&'static str],
    /// Shown in the target list in place of a filesystem path.
    pub display: &'static str,
    /// Home-relative (or absolute) dirs — offered only if at least one exists.
    pub data_dirs: &'static [&'static str],
    /// Dirs whose sizes are summed as the reclaimable estimate. Dirs listed
    /// here are suppressed from file-based scanners to avoid double-counting.
    pub size_hint: &'static [&'static str],
}

impl CommandSpec {
    /// First candidate binary that exists on this machine.
    pub fn resolve_bin(&self) -> Option<PathBuf> {
        self.bins.iter().map(PathBuf::from).find(|p| p.is_file())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    DevCaches,
    AppCaches,
    Leftovers,
    Downloads,
    Logs,
    /// Paths added by the user's config file (`[[custom]]` entries).
    Custom,
}

impl Category {
    pub fn label(&self) -> &'static str {
        match self {
            Self::DevCaches => "Developer Caches",
            Self::AppCaches => "App Caches",
            Self::Leftovers => "App Leftovers",
            Self::Downloads => "Downloads",
            Self::Logs => "Logs",
            Self::Custom => "Custom",
        }
    }

    pub const ALL: [Category; 6] = [
        Self::DevCaches,
        Self::AppCaches,
        Self::Leftovers,
        Self::Downloads,
        Self::Logs,
        Self::Custom,
    ];
}

#[derive(Debug)]
pub struct CleanTarget {
    pub path: PathBuf,
    pub size: u64,
    pub category: Category,
    pub safety: Safety,
    pub action: Action,
    pub reason: String,
    pub selected: bool,
}

impl CleanTarget {
    pub fn new(
        path: PathBuf,
        size: u64,
        category: Category,
        safety: Safety,
        action: Action,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            selected: matches!(safety, Safety::Regenerable),
            path,
            size,
            category,
            safety,
            action,
            reason: reason.into(),
        }
    }
}

pub fn fmt_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    if bytes == 0 {
        return "0 B".into();
    }
    let mut exp = 0usize;
    let mut val = bytes as f64;
    while val >= 1024.0 && exp < UNITS.len() - 1 {
        val /= 1024.0;
        exp += 1;
    }
    if exp == 0 {
        format!("{} B", bytes)
    } else if val >= 100.0 {
        format!("{:.0} {}", val, UNITS[exp])
    } else {
        format!("{:.1} {}", val, UNITS[exp])
    }
}
