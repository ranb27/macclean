use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

struct TempHome(PathBuf);

impl TempHome {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "macclean-scheduled-test-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn scheduled_scan(home: Option<&Path>) -> Output {
    let cwd = TempHome::new();
    let mut command = Command::new(env!("CARGO_BIN_EXE_macclean"));
    command.arg("--scheduled-scan").current_dir(&cwd.0);
    match home {
        Some(home) => {
            command.env("HOME", home);
        }
        None => {
            command.env_remove("HOME");
        }
    }
    command.output().unwrap()
}

fn assert_report_failure(output: Output) {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("scheduled scan:"),
        "{:?}",
        output
    );
}

#[test]
fn rejects_missing_empty_and_invalid_home() {
    let home = TempHome::new();
    assert_report_failure(scheduled_scan(None));
    assert_report_failure(scheduled_scan(Some(Path::new(""))));
    assert_report_failure(scheduled_scan(Some(Path::new("/dev/null"))));
    assert_report_failure(scheduled_scan(Some(&home.0.join("missing"))));
    assert_report_failure(scheduled_scan(Some(Path::new("relative"))));
}

#[test]
fn fails_if_report_directory_cannot_be_created() {
    let home = TempHome::new();
    std::fs::write(home.0.join("Library"), b"not a directory").unwrap();
    let output = scheduled_scan(Some(&home.0));
    assert!(String::from_utf8_lossy(&output.stderr).contains("create"));
    assert_report_failure(output);
}

#[test]
fn fails_if_report_cannot_be_written() {
    let home = TempHome::new();
    let report = home
        .0
        .join("Library/Application Support/macclean/last-scan.json");
    std::fs::create_dir_all(&report).unwrap();
    let output = scheduled_scan(Some(&home.0));
    assert!(String::from_utf8_lossy(&output.stderr).contains("write"));
    assert_report_failure(output);
}

#[cfg(target_os = "macos")]
#[test]
fn saves_report_and_warns_when_notification_cannot_start() {
    let home = TempHome::new();
    let keep = home.0.join("keep.txt");
    std::fs::write(&keep, b"do not delete").unwrap();

    // Block osascript so the test never posts a real notification.
    let output = Command::new("/usr/bin/sandbox-exec")
        .args([
            "-p",
            "(version 1) (allow default) (deny process-exec (literal \"/usr/bin/osascript\"))",
            env!("CARGO_BIN_EXE_macclean"),
            "--scheduled-scan",
        ])
        .env("HOME", &home.0)
        .current_dir(&home.0)
        .output()
        .unwrap();

    assert!(output.status.success(), "{:?}", output);
    assert!(String::from_utf8_lossy(&output.stderr).contains("notification failed"));
    let report = home
        .0
        .join("Library/Application Support/macclean/last-scan.json");
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap();
    assert!(report["at"].as_str().is_some_and(|at| !at.is_empty()));
    assert!(report["reclaimable_bytes"].as_u64().is_some());
    let expected = format!(
        "{} targets · {} reclaimable",
        report["targets"].as_u64().unwrap(),
        report["reclaimable"].as_str().unwrap()
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), expected);
    assert_eq!(std::fs::read(&keep).unwrap(), b"do not delete");
}
