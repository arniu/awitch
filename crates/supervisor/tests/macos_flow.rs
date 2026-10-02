#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

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
fn full_lifecycle_against_fake_launchd() {
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

    write_stub(&bin, "launchctl");
    write_stub(&bin, "tail");

    let label = "com.example.supervisor-test";
    let service = Service {
        name: "supervisor-test".into(),
        label: label.into(),
        program: "/bin/sleep".into(),
        args: vec!["60".into()],
        log_dir: tmp.path().join("logs"),
        pid_file: tmp.path().join("server.pid"),
    };

    let uid = fs::metadata(&home).unwrap().uid();
    let plist = home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{label}.plist"));

    // install: writes the plist, then bootout (tolerated) + bootstrap.
    supervisor::run(&service, Action::Install).unwrap();
    let plist_text = fs::read_to_string(&plist).unwrap();
    assert!(plist_text.contains("<string>com.example.supervisor-test</string>"));
    assert!(plist_text.contains("<string>/bin/sleep</string>"));
    assert!(plist_text.contains("<string>60</string>"));

    let lines = stub_lines();
    assert!(lines.contains(&format!("bootout gui/{uid}/{label}")));
    assert!(lines.contains(&format!("bootstrap gui/{uid} {}", plist.display())));

    // start: kickstart against the installed target.
    supervisor::run(&service, Action::Start).unwrap();
    assert!(stub_lines().contains(&format!("kickstart gui/{uid}/{label}")));

    // a detached process standing in for the real gateway, its pid recorded
    // where the gateway would write it.
    // The detached process must not inherit the capture pipe — it would
    // keep it open and block `.output()` until it exits.
    let out = Command::new("sh")
        .args(["-c", "sleep 60 </dev/null >/dev/null 2>&1 & echo $!"])
        .output()
        .unwrap();
    let pid: i32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
    fs::write(&service.pid_file, format!("{pid}\n")).unwrap();

    // logs: the backend picks the newest `<name>.*.log` and tails it.
    let log_file = service.log_dir.join("supervisor-test.log");
    fs::write(&log_file, "hello\n").unwrap();
    supervisor::run(&service, Action::Logs).unwrap();
    assert!(stub_lines().contains(&format!("-n 50 {}", log_file.display())));

    // stop: signal the recorded pid and wait for it to exit.
    supervisor::run(&service, Action::Stop).unwrap();
    let alive = Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    assert!(!alive, "detached process {pid} should be dead after stop");

    // uninstall: bootout again + remove the plist.
    supervisor::run(&service, Action::Uninstall).unwrap();
    assert!(!plist.exists());
    let bootouts = stub_lines()
        .iter()
        .filter(|l| l.starts_with("bootout "))
        .count();
    assert_eq!(bootouts, 2);
}
