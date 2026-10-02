#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use supervisor::{Action, Service};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "supervisor-flow-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A stub for a supervisor-facing command: it appends its argv to the
/// `SUPERVISOR_STUB_LOG` env var and exits 0.
fn write_stub(bin_dir: &Path, name: &str) {
    let script = "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$SUPERVISOR_STUB_LOG\"\nexit 0\n";
    let path = bin_dir.join(name);
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn stub_lines() -> Vec<String> {
    fs::read_to_string(std::env::var("SUPERVISOR_STUB_LOG").unwrap())
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn full_lifecycle_against_fake_systemd() {
    let tmp = TempDir::new();
    let home = tmp.path().join("home");
    let bin = tmp.path().join("bin");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&bin).unwrap();

    let stub_log = tmp.path().join("stub.log");
    fs::write(&stub_log, "").unwrap();

    // Only this one test runs in this process, so no concurrent readers of
    // these vars; children inherit the redirected env.
    unsafe {
        std::env::set_var("HOME", home.as_os_str());
        std::env::set_var(
            "PATH",
            format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        std::env::set_var("SUPERVISOR_STUB_LOG", stub_log.as_os_str());
    }

    write_stub(&bin, "systemctl");
    write_stub(&bin, "journalctl");

    let service = Service {
        name: "supervisor-test".into(),
        label: "com.example.supervisor-test".into(),
        program: "/bin/sleep".into(),
        args: vec!["60".into()],
        log_dir: tmp.path().join("logs"),
        pid_file: tmp.path().join("server.pid"),
    };

    let unit = home
        .join(".config")
        .join("systemd")
        .join("user")
        .join("supervisor-test.service");

    // install: writes the unit, then daemon-reload + enable --now.
    supervisor::run(&service, Action::Install).unwrap();
    let unit_text = fs::read_to_string(&unit).unwrap();
    assert!(unit_text.contains("Description=supervisor-test"));
    assert!(unit_text.contains("ExecStart=/bin/sleep 60"));
    assert!(unit_text.contains("Restart=on-failure"));

    let lines = stub_lines();
    assert!(lines.contains(&"--user daemon-reload".to_string()));
    assert!(lines.contains(&"--user enable --now supervisor-test.service".to_string()));

    // start / restart / stop route through systemctl verbs.
    supervisor::run(&service, Action::Start).unwrap();
    assert!(stub_lines().contains(&"--user start supervisor-test.service".to_string()));

    supervisor::run(&service, Action::Restart).unwrap();
    assert!(stub_lines().contains(&"--user restart supervisor-test.service".to_string()));

    supervisor::run(&service, Action::Stop).unwrap();
    assert!(stub_lines().contains(&"--user stop supervisor-test.service".to_string()));

    // logs route through journalctl, scoped to the one unit.
    supervisor::run(&service, Action::Logs).unwrap();
    assert!(
        stub_lines().contains(&"--user -u supervisor-test.service -n 50 --no-pager".to_string())
    );

    // uninstall: disable --now, remove the unit, daemon-reload again.
    supervisor::run(&service, Action::Uninstall).unwrap();
    assert!(!unit.exists());
    assert!(stub_lines().contains(&"--user disable --now supervisor-test.service".to_string()));
    let reloads = stub_lines()
        .iter()
        .filter(|l| *l == "--user daemon-reload")
        .count();
    assert_eq!(reloads, 2);
}
