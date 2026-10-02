//! systemd user-unit integration: a per-user unit in `~/.config/systemd/user/`.

use std::process::Command;

use super::{Action, Error, Service};

/// **systemd** — a unit string value: `%`→`%%` (unknown `%` is a load error
/// since v236) and `\`→`\\` (C-escapes apply to all settings). `$` is
/// untouched — no env expansion outside `Exec*`.
fn systemd_value(s: &str) -> String {
    s.replace('%', "%%").replace('\\', "\\\\")
}

/// **systemd** — one `ExecStart=` token: `%`→`%%`, `$`→`$$`, then quote iff
/// empty or containing whitespace / `"` / `'` / `\` (escaping `"` and `\`
/// inside; `'` is literal there). A bare token's lone `;` is escaped so it
/// isn't a command separator.
fn systemd_exec_arg(s: &str) -> String {
    let mut out = s.replace('%', "%%").replace('$', "$$");
    let needs_quote = s.is_empty()
        || s.chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '\\'));
    if needs_quote {
        out = out.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{out}\"")
    } else {
        out.replace(';', "\\;")
    }
}

fn systemd_exec_line(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(systemd_exec_arg)
        .collect::<Vec<_>>()
        .join(" ")
}

/// **systemd** — the unit file; its restart and autostart settings are
/// pinned by ADR-0010.
fn systemd_unit(name: &str, program: &str, args: &[String]) -> String {
    let description = systemd_value(name);
    let exec = systemd_exec_line(program, args);
    format!(
        "[Unit]\n\
         Description={description}\n\
         \n\
         [Service]\n\
         Type=exec\n\
         ExecStart={exec}\n\
         Restart=on-failure\n\
         RestartSec=2\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

fn unit_file_name(name: &str) -> String {
    format!("{name}.service")
}

fn systemctl_args(tail: &[&str]) -> Vec<String> {
    std::iter::once("--user".to_string())
        .chain(tail.iter().map(|a| a.to_string()))
        .collect()
}

fn journalctl_args(unit: &str) -> Vec<String> {
    ["--user", "-u", unit, "-n", "50", "--no-pager"]
        .iter()
        .map(|a| a.to_string())
        .collect()
}

fn unit_name(service: &Service) -> String {
    unit_file_name(&service.name)
}

fn unit_path(service: &Service) -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config")
        .join("systemd")
        .join("user")
        .join(unit_name(service))
}

fn systemctl(args: &[String]) -> Result<(), Error> {
    let status = Command::new("systemctl").args(args).status()?;
    if !status.success() {
        return Err(Error::Systemctl {
            args: args.join(" "),
        });
    }
    Ok(())
}

pub fn run(service: &Service, action: Action) -> Result<(), Error> {
    match action {
        Action::Install => install(service),
        Action::Uninstall => uninstall(service),
        Action::Start => start(service),
        Action::Stop => {
            let _ = systemctl(&systemctl_args(&["stop", &unit_name(service)]));
            Ok(())
        }
        Action::Restart => {
            let _ = systemctl(&systemctl_args(&["restart", &unit_name(service)]));
            Ok(())
        }
        Action::Status => status(service),
        Action::Logs => logs(service),
    }
}

fn install(service: &Service) -> Result<(), Error> {
    let path = unit_path(service);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let unit = systemd_unit(&service.name, &service.program, &service.args);
    std::fs::write(&path, unit).map_err(|source| Error::Write { path, source })?;

    systemctl(&systemctl_args(&["daemon-reload"]))?;
    systemctl(&systemctl_args(&["enable", "--now", &unit_name(service)]))?;
    println!(
        "installed {} (login-autostart, running now)",
        unit_name(service)
    );
    Ok(())
}

fn uninstall(service: &Service) -> Result<(), Error> {
    let _ = systemctl(&systemctl_args(&["disable", "--now", &unit_name(service)]));
    let _ = std::fs::remove_file(unit_path(service));
    let _ = systemctl(&systemctl_args(&["daemon-reload"]));
    println!("uninstalled {}", unit_name(service));
    Ok(())
}

fn start(service: &Service) -> Result<(), Error> {
    if !unit_path(service).exists() {
        return Err(Error::NotInstalled);
    }
    systemctl(&systemctl_args(&["start", &unit_name(service)]))?;
    println!("started {}", unit_name(service));
    Ok(())
}

fn status(service: &Service) -> Result<(), Error> {
    println!("installed: {}", unit_path(service).exists());
    let active = Command::new("systemctl")
        .args(systemctl_args(&["is-active", &unit_name(service)]))
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
        .unwrap_or(false);
    println!("running: {active}");
    Ok(())
}

fn logs(service: &Service) -> Result<(), Error> {
    let _ = Command::new("journalctl")
        .args(journalctl_args(&unit_name(service)))
        .status();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemd_unit_full_text() {
        let unit = systemd_unit("example", "/home/x/example", &["serve".into()]);
        assert_eq!(
            unit,
            "[Unit]\n\
             Description=example\n\
             \n\
             [Service]\n\
             Type=exec\n\
             ExecStart=/home/x/example serve\n\
             Restart=on-failure\n\
             RestartSec=2\n\
             \n\
             [Install]\n\
             WantedBy=default.target\n"
        );
    }

    #[test]
    fn unit_file_name_suffixes_service() {
        assert_eq!(unit_file_name("example"), "example.service");
    }

    #[test]
    fn systemctl_argv_puts_the_user_flag_first() {
        assert_eq!(
            systemctl_args(&["daemon-reload"]),
            ["--user", "daemon-reload"]
        );
        assert_eq!(
            systemctl_args(&["enable", "--now", "example.service"]),
            ["--user", "enable", "--now", "example.service"]
        );
        assert_eq!(
            systemctl_args(&["is-active", "example.service"]),
            ["--user", "is-active", "example.service"]
        );
    }

    #[test]
    fn journalctl_argv_targets_one_unit() {
        assert_eq!(
            journalctl_args("example.service"),
            ["--user", "-u", "example.service", "-n", "50", "--no-pager"]
        );
    }
}
