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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    DevCaches,
    AppCaches,
    Leftovers,
    Logs,
}

impl Category {
    pub fn label(&self) -> &'static str {
        match self {
            Self::DevCaches => "Developer Caches",
            Self::AppCaches => "App Caches",
            Self::Leftovers => "App Leftovers",
            Self::Logs => "Logs",
        }
    }

    pub const ALL: [Category; 4] = [
        Self::DevCaches,
        Self::AppCaches,
        Self::Leftovers,
        Self::Logs,
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
