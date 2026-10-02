//! macOS LaunchAgent integration: a per-user LaunchAgent loaded in the
//! `gui/<uid>` domain.

use std::process::Command;

use super::common;
use super::{Action, Error, Service};

/// **launchd** — launchd receives argv as an array, so no shell quoting is
/// involved.
fn plist_program_arguments(program: &str, args: &[String]) -> String {
    let elements = std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(|a| format!("<string>{}</string>", xml_escape(a)))
        .collect::<String>();
    format!("<array>{elements}</array>")
}

/// **plist XML** — element-content escaping: `& < >` (minimum for text;
/// `"`/`'` only in attributes). Escaping `>` also satisfies the `]]>` rule.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// **launchd** — the plist; its settings are pinned by ADR-0010. `KeepAlive`
/// restarts on crash only (`SuccessfulExit=false`), so a clean stop stays
/// stopped.
fn launchd_plist(label: &str, program: &str, args: &[String]) -> String {
    let label = xml_escape(label);
    let program_args = plist_program_arguments(program, args);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
           <key>Label</key><string>{label}</string>\n\
           <key>ProgramArguments</key>\n\
           {program_args}\n\
           <key>RunAtLoad</key><true/>\n\
           <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\n\
           <key>ThrottleInterval</key><integer>10</integer>\n\
         </dict>\n\
         </plist>\n"
    )
}

fn launchd_domain(uid: u32) -> String {
    format!("gui/{uid}")
}

fn launchd_target(domain: &str, label: &str) -> String {
    format!("{domain}/{label}")
}

fn launchctl_bootout_args(target: &str) -> Vec<String> {
    ["bootout", target].iter().map(|a| a.to_string()).collect()
}

fn launchctl_bootstrap_args(domain: &str, plist: &str) -> Vec<String> {
    ["bootstrap", domain, plist]
        .iter()
        .map(|a| a.to_string())
        .collect()
}

fn launchctl_kickstart_args(target: &str) -> Vec<String> {
    ["kickstart", target]
        .iter()
        .map(|a| a.to_string())
        .collect()
}

fn plist_path(service: &Service) -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", service.label))
}

fn domain() -> Result<String, Error> {
    let home = dirs::home_dir().unwrap_or_default();
    let uid = std::fs::metadata(&home).map_or(0, |m| {
        use std::os::unix::fs::MetadataExt;
        m.uid()
    });
    Ok(launchd_domain(uid))
}

fn target(service: &Service) -> Result<String, Error> {
    Ok(launchd_target(&domain()?, &service.label))
}

fn pid_alive(p: i32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(p.to_string())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn running(service: &Service) -> bool {
    common::pid(&service.pid_file).is_some_and(pid_alive)
}

/// Stop by signal and wait for exit — launchd can't stop a `KeepAlive` job
/// cleanly while loaded, so stop goes through the pid.
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

    let _ = Command::new("kill")
        .arg(p.to_string())
        .stderr(std::process::Stdio::null())
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
    let plist = plist_path(service);
    if let Some(parent) = plist.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::create_dir_all(&service.log_dir)?;

    let plist_xml = launchd_plist(&service.label, &service.program, &service.args);
    std::fs::write(&plist, plist_xml).map_err(|source| Error::Write {
        path: plist.clone(),
        source,
    })?;

    // Idempotent: bootout (tolerate "not loaded") then bootstrap.
    let _ = Command::new("launchctl")
        .args(launchctl_bootout_args(&target(service)?))
        .status();
    let status = Command::new("launchctl")
        .args(launchctl_bootstrap_args(
            &domain()?,
            plist.to_str().unwrap_or_default(),
        ))
        .status()?;
    if !status.success() {
        return Err(Error::LaunchctlBootstrap);
    }
    println!("installed {} (login-autostart, running now)", service.label);
    Ok(())
}

fn uninstall(service: &Service) -> Result<(), Error> {
    let _ = Command::new("launchctl")
        .args(launchctl_bootout_args(&target(service)?))
        .status();
    let _ = std::fs::remove_file(plist_path(service));
    println!("uninstalled {}", service.label);
    Ok(())
}

fn start(service: &Service) -> Result<(), Error> {
    if !plist_path(service).exists() {
        return Err(Error::NotInstalled);
    }
    let status = Command::new("launchctl")
        .args(launchctl_kickstart_args(&target(service)?))
        .status()?;
    if !status.success() {
        return Err(Error::LaunchctlKickstart);
    }
    println!("started {}", service.label);
    Ok(())
}

fn status(service: &Service) -> Result<(), Error> {
    println!("installed: {}", plist_path(service).exists());
    println!("running: {}", running(service));
    Ok(())
}

fn logs(service: &Service) -> Result<(), Error> {
    match common::latest_log_file(&service.name, &service.log_dir) {
        Some(path) => {
            println!("log: {}", path.display());
            let _ = Command::new("tail")
                .args(["-n", "50", path.to_str().unwrap_or_default()])
                .status();
        }
        None => println!("no logs yet"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchd_plist_full_text() {
        let plist = launchd_plist(
            "com.example.app",
            "/usr/local/bin/example",
            &["serve".into()],
        );
        assert_eq!(
            plist,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
               <key>Label</key><string>com.example.app</string>\n\
               <key>ProgramArguments</key>\n\
               <array><string>/usr/local/bin/example</string><string>serve</string></array>\n\
               <key>RunAtLoad</key><true/>\n\
               <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\n\
               <key>ThrottleInterval</key><integer>10</integer>\n\
             </dict>\n\
             </plist>\n"
        );
    }

    #[test]
    fn launchd_domain_and_target() {
        assert_eq!(launchd_domain(501), "gui/501");
        assert_eq!(
            launchd_target("gui/501", "com.example.app"),
            "gui/501/com.example.app"
        );
    }

    #[test]
    fn launchctl_argv() {
        let target = "gui/501/com.example.app";
        assert_eq!(launchctl_bootout_args(target), ["bootout", target]);
        assert_eq!(launchctl_kickstart_args(target), ["kickstart", target]);
        assert_eq!(
            launchctl_bootstrap_args(
                "gui/501",
                "/Users/x/Library/LaunchAgents/com.example.app.plist"
            ),
            [
                "bootstrap",
                "gui/501",
                "/Users/x/Library/LaunchAgents/com.example.app.plist"
            ]
        );
    }
}
