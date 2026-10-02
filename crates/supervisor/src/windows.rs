//! Windows logon-triggered autostart: a Task Scheduler `onlogon` task running
//! as the user — not a Windows Service (which would run as LocalSystem and
//! resolve the user's home against the wrong profile).

use std::process::Command;

use super::common;
use super::{Action, Error, Service};

/// **CommandLineToArgvW** — one token (matches Rust std's `append_arg`): N
/// backslashes before `"` → 2N+1, trailing ones doubled. Quote iff empty /
/// whitespace / `"` / trailing `\` — a superset of std (std quotes only
/// empty/space/tab); empty → `""`.
fn cmdline_token(s: &str) -> String {
    let needs_quote =
        s.is_empty() || s.chars().any(char::is_whitespace) || s.contains('"') || s.ends_with('\\');
    let mut out = String::new();
    if needs_quote {
        out.push('"');
    }
    let mut backslashes = 0usize;
    for c in s.chars() {
        if c == '\\' {
            backslashes += 1;
        } else {
            if c == '"' {
                // N+1 extra backslashes → 2N+1 before an internal quote.
                out.push_str(&"\\".repeat(backslashes + 1));
            }
            backslashes = 0;
        }
        out.push(c);
    }
    if needs_quote {
        // N extra backslashes → 2N before the closing quote.
        out.push_str(&"\\".repeat(backslashes));
        out.push('"');
    }
    out
}

fn cmdline_command(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(cmdline_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn schtasks_create_args(name: &str, run: &str) -> Vec<String> {
    ["/create", "/tn", name, "/tr", run, "/sc", "onlogon", "/f"]
        .iter()
        .map(|a| a.to_string())
        .collect()
}

fn schtasks_run_args(name: &str) -> Vec<String> {
    ["/run", "/tn", name]
        .iter()
        .map(|a| a.to_string())
        .collect()
}

fn schtasks_delete_args(name: &str) -> Vec<String> {
    ["/delete", "/tn", name, "/f"]
        .iter()
        .map(|a| a.to_string())
        .collect()
}

fn schtasks_query_args(name: &str) -> Vec<String> {
    ["/query", "/tn", name]
        .iter()
        .map(|a| a.to_string())
        .collect()
}

fn pid_alive(p: i32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {p}")])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&p.to_string()))
        .unwrap_or(false)
}

fn running(service: &Service) -> bool {
    common::pid(&service.pid_file).is_some_and(pid_alive)
}

/// Stop by signal and wait for exit — a Task Scheduler task has no clean
/// stop, so stop goes through the pid.
fn stop_by_signal(service: &Service) {
    let Some(p) = common::pid(&service.pid_file) else {
        println!("not running");
        return;
    };
    // Guard against a stale pid_file whose PID has been recycled.
    if !pid_alive(p) {
        println!("not running");
        return;
    }

    let _ = Command::new("taskkill")
        .args(["/f", "/pid", &p.to_string()])
        .status();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline && pid_alive(p) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    if pid_alive(p) {
        println!("still stopping (pid {p})");
    } else {
        println!("stopped (pid {p})");
    }
}

pub fn run(service: &Service, action: Action) -> Result<(), Error> {
    match action {
        Action::Install => install(service),
        Action::Uninstall => uninstall(service),
        Action::Start => start(service),
        Action::Stop => {
            stop_by_signal(service);
            Ok(())
        }
        Action::Restart => {
            stop_by_signal(service);
            start(service)
        }
        Action::Status => status(service),
        Action::Logs => logs(service),
    }
}

fn install(service: &Service) -> Result<(), Error> {
    // The /tr value is parsed by CommandLineToArgvW when schtasks runs the
    // task — quote every token (program included) so a space-bearing profile
    // path round-trips. The /tn name stays an argv-array arg (no shell).
    let run = cmdline_command(&service.program, &service.args);
    let status = Command::new("schtasks")
        .args(schtasks_create_args(&service.name, &run))
        .status()?;
    if !status.success() {
        return Err(Error::SchtasksCreate);
    }
    // `onlogon` fires only at next logon — start it now too.
    let _ = Command::new("schtasks")
        .args(schtasks_run_args(&service.name))
        .status();
    println!("installed {} (logon-autostart, running now)", service.name);
    Ok(())
}

fn uninstall(service: &Service) -> Result<(), Error> {
    let _ = Command::new("schtasks")
        .args(schtasks_delete_args(&service.name))
        .status();
    println!("uninstalled {}", service.name);
    Ok(())
}

fn start(service: &Service) -> Result<(), Error> {
    let status = Command::new("schtasks")
        .args(schtasks_run_args(&service.name))
        .status()?;
    if !status.success() {
        return Err(Error::SchtasksRun);
    }
    println!("started {}", service.name);
    Ok(())
}

fn status(service: &Service) -> Result<(), Error> {
    let installed = Command::new("schtasks")
        .args(schtasks_query_args(&service.name))
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    println!("installed: {installed}");
    println!("running: {}", running(service));
    Ok(())
}

fn logs(service: &Service) -> Result<(), Error> {
    match common::latest_log_file(&service.name, &service.log_dir) {
        Some(path) => println!("log: {}", path.display()),
        None => println!("no logs yet"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schtasks_argv() {
        let run = "\"C:\\x\\example.exe\" serve";
        assert_eq!(
            schtasks_create_args("example", run),
            [
                "/create", "/tn", "example", "/tr", run, "/sc", "onlogon", "/f"
            ]
        );
        assert_eq!(schtasks_run_args("example"), ["/run", "/tn", "example"]);
        assert_eq!(
            schtasks_delete_args("example"),
            ["/delete", "/tn", "example", "/f"]
        );
        assert_eq!(schtasks_query_args("example"), ["/query", "/tn", "example"]);
    }

    #[test]
    fn cmdline_token_quotes_when_needed() {
        assert_eq!(cmdline_token("serve"), "serve");
        assert_eq!(cmdline_token(""), "\"\"");
        assert_eq!(
            cmdline_token("C:\\Program Files\\x.exe"),
            "\"C:\\Program Files\\x.exe\""
        );
        assert_eq!(cmdline_token("trailing\\"), "\"trailing\\\\\"");
        assert_eq!(cmdline_token("a\"b"), "\"a\\\"b\"");
    }

    #[test]
    fn cmdline_command_joins_tokens() {
        assert_eq!(
            cmdline_command("C:\\Program Files\\x.exe", &["serve".into(), "run".into()]),
            "\"C:\\Program Files\\x.exe\" serve run"
        );
    }
}
