//! Generic OS supervisor integration: install a command as a per-user
//! login-session service.

#![deny(missing_docs)]

use std::path::PathBuf;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod common;

/// A service definition: the command to run and where its runtime files live.
pub struct Service {
    /// Human name — the systemd unit / Windows task name (e.g. "example");
    /// on macOS it also names the log file.
    pub name: String,
    /// macOS reverse-DNS label (e.g. "com.example.app") — the plist name/Label;
    /// ignored on Linux/Windows.
    pub label: String,
    /// The executable to run.
    pub program: String,
    /// Arguments passed to the program.
    pub args: Vec<String>,
    /// Log directory (macOS/Windows write a log file; Linux logs to the
    /// journal and ignores this).
    pub log_dir: PathBuf,
    /// PID file the program writes — used by `Stop`/`Status` where the
    /// supervisor has no clean stop (launchd, Windows Task Scheduler);
    /// Linux stops via systemctl and ignores it.
    pub pid_file: PathBuf,
}

/// An operation on a supervised service.
#[derive(Debug, Clone, Copy)]
pub enum Action {
    /// Register the service, enable login-autostart, and start it now.
    Install,
    /// Stop, disable, and remove the service.
    Uninstall,
    /// Start the service.
    Start,
    /// Stop the service.
    Stop,
    /// Stop, then start the service.
    Restart,
    /// Report whether the service is installed and running.
    Status,
    /// Show the service's recent logs.
    Logs,
}

/// An error from a supervisor operation.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The current platform has no supervisor backend.
    #[error("unsupported platform")]
    UnsupportedPlatform,

    /// `start` was called before `install`.
    #[error("not installed — run 'install' first")]
    NotInstalled,

    /// `launchctl bootstrap` reported failure.
    #[error("launchctl bootstrap failed")]
    LaunchctlBootstrap,

    /// `launchctl kickstart` reported failure.
    #[error("launchctl kickstart failed")]
    LaunchctlKickstart,

    /// `systemctl` reported failure.
    #[error("systemctl {args} failed")]
    Systemctl {
        /// The space-joined arguments passed to `systemctl`.
        args: String,
    },

    /// `schtasks /create` reported failure.
    #[error("schtasks /create failed")]
    SchtasksCreate,

    /// `schtasks /run` reported failure (usually: not installed).
    #[error("schtasks /run failed (not installed?)")]
    SchtasksRun,

    /// A file the backend needed to write could not be written.
    #[error("write {path}: {source}")]
    Write {
        /// The file that failed to write.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// Any other I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Run one [`Action`] against the supervisor for the current platform.
pub fn run(service: &Service, action: Action) -> Result<(), Error> {
    #[cfg(target_os = "macos")]
    {
        return macos::run(service, action);
    }
    #[cfg(target_os = "linux")]
    {
        return linux::run(service, action);
    }
    #[cfg(target_os = "windows")]
    {
        return windows::run(service, action);
    }
    #[expect(unreachable_code)]
    Err(Error::UnsupportedPlatform)
}
