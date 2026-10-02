#![cfg(target_os = "windows")]

use std::fs;
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

/// Removes the scheduled task even if the test panics mid-lifecycle.
struct TaskCleanup(String);

impl Drop for TaskCleanup {
    fn drop(&mut self) {
        let _ = Command::new("schtasks")
            .args(["/delete", "/tn", &self.0, "/f"])
            .status();
    }
}

fn task_exists(name: &str) -> bool {
    Command::new("schtasks")
        .args(["/query", "/tn", name])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn pid_alive(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}")])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

#[test]
fn full_lifecycle_against_real_task_scheduler() {
    let tmp = TempDir::new();
    let name = format!("supervisor-test-{}", std::process::id());
    let _cleanup = TaskCleanup(name.clone());

    let service = Service {
        name: name.clone(),
        label: "com.example.supervisor-test".into(),
        program: "C:\\Windows\\System32\\cmd.exe".into(),
        args: vec!["/c".into(), "exit".into()],
        log_dir: tmp.path().join("logs"),
        pid_file: tmp.path().join("server.pid"),
    };

    // install: a real onlogon task, then /run to start it now.
    supervisor::run(&service, Action::Install).unwrap();
    assert!(
        task_exists(&name),
        "task should be registered after install"
    );

    // status: exercises /query + running (no pid yet).
    supervisor::run(&service, Action::Status).unwrap();

    // a real process standing in for the gateway; stop must kill it.
    let mut child = Command::new("ping")
        .args(["-n", "61", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    fs::write(&service.pid_file, format!("{pid}\n")).unwrap();

    // logs: the backend picks the newest `<name>.*.log` in log_dir.
    fs::create_dir_all(&service.log_dir).unwrap();
    fs::write(service.log_dir.join(format!("{name}.log")), "hello\n").unwrap();
    supervisor::run(&service, Action::Logs).unwrap();

    // stop: real taskkill /f against the recorded pid.
    supervisor::run(&service, Action::Stop).unwrap();
    assert!(!pid_alive(pid), "process {pid} should be dead after stop");
    let _ = child.wait();

    // uninstall: /delete removes the task.
    supervisor::run(&service, Action::Uninstall).unwrap();
    assert!(!task_exists(&name), "task should be gone after uninstall");
}
