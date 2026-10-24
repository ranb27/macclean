//! Scheduled scans — a LaunchAgent that runs `macclean --scheduled-scan`
//! daily or weekly. The scheduled run only *reports*: it writes a summary
//! the app can show and posts a macOS notification. It never deletes —
//! consistent with dry-run-by-design.

use serde_json::json;
use std::path::PathBuf;

/// How often the LaunchAgent fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frequency {
    Daily,
    Weekly,
}

impl Frequency {
    /// `StartInterval` seconds for the plist.
    fn interval(self) -> u64 {
        match self {
            Self::Daily => 86_400,
            Self::Weekly => 604_800,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
        }
    }
}

const AGENT_LABEL: &str = "com.macclean.scan";

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

/// Where the LaunchAgent plist lives — same directory macclean already
/// scans for orphans (its own agent always matches the installed app).
pub fn agent_path() -> PathBuf {
    home().join(format!("Library/LaunchAgents/{AGENT_LABEL}.plist"))
}

/// Latest scheduled-scan summary, written by `--scheduled-scan`.
pub fn last_scan_path() -> PathBuf {
    home().join("Library/Application Support/macclean/last-scan.json")
}

/// Currently installed frequency, read back from the plist — launchd is
/// the source of truth, never a shadow copy in the config file.
pub fn current() -> Option<Frequency> {
    let dict = plist::from_file::<_, plist::Value>(&agent_path())
        .ok()?
        .into_dictionary()?;
    match dict
        .get("StartInterval")
        .and_then(|v| v.as_unsigned_integer())
    {
        Some(86_400) => Some(Frequency::Daily),
        Some(604_800) => Some(Frequency::Weekly),
        _ => None,
    }
}

/// Read the summary of the last scheduled run, if one completed.
pub fn last_scan() -> Option<String> {
    let text = std::fs::read_to_string(last_scan_path()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let at = v["at"].as_str()?;
    let n = v["targets"].as_u64()?;
    let size = v["reclaimable"].as_str()?;
    Some(format!("{n} targets · {size} — {at}"))
}

/// Install/replace the agent for `freq`, or remove it for `None`.
/// Returns a human-readable status line for the UI.
pub fn set(freq: Option<Frequency>) -> Result<String, String> {
    match freq {
        Some(f) => install(f),
        None => uninstall(),
    }
}

fn install(freq: Frequency) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let agent = agent_path();
    if let Some(parent) = agent.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let log = home().join("Library/Logs/macclean-scheduled.log");
    let plist = dict_plist(
        exe.to_string_lossy().as_ref(),
        freq.interval(),
        &log.to_string_lossy(),
    );
    std::fs::write(&agent, plist).map_err(|e| e.to_string())?;

    // Replace whatever was loaded before: bootout is fine to fail when
    // nothing was registered yet, then bootstrap the fresh plist.
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    let _ = launchctl(&["bootout", &domain, &agent.to_string_lossy()]);
    launchctl(&["bootstrap", &domain, &agent.to_string_lossy()])
        .or_else(|_| launchctl(&["load", "-w", &agent.to_string_lossy()]))
        .map_err(|e| format!("wrote {} but {e}", agent.display()))?;
    Ok(format!("{} scan scheduled", freq.label()))
}

fn uninstall() -> Result<String, String> {
    let agent = agent_path();
    if !agent.exists() {
        return Ok("no scheduled scan installed".into());
    }
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    let _ = launchctl(&["bootout", &domain, &agent.to_string_lossy()])
        .or_else(|_| launchctl(&["unload", "-w", &agent.to_string_lossy()]));
    std::fs::remove_file(&agent).map_err(|e| e.to_string())?;
    Ok("scheduled scan removed".into())
}

/// Launchctl with fixed argv — same whitelist discipline as `trash.rs`.
fn launchctl(args: &[&str]) -> Result<(), String> {
    let out = std::process::Command::new("/bin/launchctl")
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The plist, written by hand — seven lines, no plist-writer needed.
fn dict_plist(exe: &str, interval: u64, log: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{AGENT_LABEL}</string>
    <key>ProgramArguments</key>
    <array><string>{exe}</string><string>--scheduled-scan</string></array>
    <key>StartInterval</key><integer>{interval}</integer>
    <key>StandardOutPath</key><string>{log}</string>
    <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#
    )
}

/// `macclean --scheduled-scan` — headless scan, record the summary,
/// notify. Deletion stays a manual choice made in the app.
pub fn run_scheduled() -> Result<String, String> {
    let r = crate::scan::scan_all();
    let total: u64 = r.targets.iter().map(|t| t.size).sum();
    let summary = format!(
        "{} targets · {} reclaimable",
        r.targets.len(),
        crate::model::fmt_size(total)
    );

    if let Some(parent) = last_scan_path().parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let at = {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // local wall time via `date` — no chrono dependency
        std::process::Command::new("/bin/date")
            .args(["-r", &secs.to_string(), "+%Y-%m-%d %H:%M"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("epoch {secs}"))
    };
    let _ = std::fs::write(
        last_scan_path(),
        format!(
            "{}\n",
            json!({"at": at, "targets": r.targets.len(),
                   "reclaimable": crate::model::fmt_size(total),
                   "reclaimable_bytes": total})
        ),
    );

    // Notification via osascript — ships with macOS, no dependency.
    let _ = std::process::Command::new("/usr/bin/osascript")
        .args([
            "-e",
            &format!(
                "display notification \"{summary}\" with title \"macclean\" subtitle \"Scheduled scan\""
            ),
        ])
        .output();

    Ok(summary)
}
