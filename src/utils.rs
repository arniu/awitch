use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub fn random_hex(bytes: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut hex = String::with_capacity(bytes * 2);
    for byte in rand::random_iter::<u8>().take(bytes) {
        hex.push(char::from(HEX[usize::from(byte >> 4)]));
        hex.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }

    hex
}

/// The last four characters of a secret key.
pub fn last4(key: &str) -> String {
    key.chars()
        .skip(key.chars().count().saturating_sub(4))
        .collect()
}

/// Current wall-clock time as Unix seconds.
pub fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Unix seconds as an RFC 3339 timestamp.
pub fn iso8601(unix_secs: i64) -> String {
    chrono::DateTime::from_timestamp(unix_secs, 0)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

/// The UTC date `unix_secs` falls on.
pub fn utc_date(unix_secs: i64) -> chrono::NaiveDate {
    chrono::DateTime::from_timestamp(unix_secs, 0)
        .unwrap_or_default()
        .date_naive()
}

/// The instant the UTC day `date` starts at.
pub fn utc_midnight(date: chrono::NaiveDate) -> i64 {
    date.and_time(chrono::NaiveTime::MIN).and_utc().timestamp()
}

/// Opens `path` and locks it exclusively.
///
/// The lock file is never written, replaced, or unlinked, and the handle must
/// not be cloned — deleting it would split the exclusion across two inodes,
/// and a cloned descriptor would keep the lock alive after this one drops.
pub fn lock_file(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false) // lock file: content is irrelevant, only the flock
        .write(true)
        .open(path)
        .map_err(|e| io::Error::new(e.kind(), format!("cannot open {}: {e}", path.display())))?;
    file.try_lock().map_err(|e| match e {
        TryLockError::WouldBlock => io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("{} is locked by another process", path.display()),
        ),
        TryLockError::Error(e) => {
            io::Error::new(e.kind(), format!("cannot lock {}: {e}", path.display()))
        }
    })?;

    Ok(file)
}

/// Create `dir` if missing, owner-only (0o700) on unix: the umask default
/// (0o755) would let other local users list the secrets it holds. Other
/// platforms keep their own directory defaults.
pub fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

/// Write `bytes` to `path`, whole or not at all: missing parents are created, an
/// existing file keeps its mode, and both the file and its directory are synced
/// before it returns.
pub fn write_file_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic(path, bytes, None)
}

/// [`write_file_atomic`] with the file's mode forced to owner-only (0o600),
/// whatever it held before. The directory is the caller's to keep private — see
/// [`ensure_private_dir`].
pub fn write_private_file_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic(path, bytes, private_perms())
}

fn write_atomic(path: &Path, bytes: &[u8], perms: Option<std::fs::Permissions>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!("{file_name}.{:x}.tmp", rand::random::<u64>()));
    std::fs::write(&tmp, bytes)?;

    // The rename swaps the inode, so the tmp must carry the target's mode —
    // an explicit mode wins over it, and with no target the umask default stands.
    let read_perms = || std::fs::metadata(path).ok().map(|m| m.permissions());
    if let Some(perm) = perms.or_else(read_perms) {
        std::fs::set_permissions(&tmp, perm)?;
    }

    sync_file(&tmp)?;
    std::fs::rename(&tmp, path)?;
    sync_parent(path)
}

#[cfg(unix)]
fn sync_file(path: &Path) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}
#[cfg(not(unix))]
fn sync_file(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn private_perms() -> Option<std::fs::Permissions> {
    Some(std::fs::Permissions::from_mode(0o600))
}
#[cfg(not(unix))]
fn private_perms() -> Option<std::fs::Permissions> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("awitch-fs-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[cfg(unix)]
    fn mode(path: &std::path::Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn lock_is_exclusive_and_released_on_drop() {
        let path = scratch("lock");

        let first = lock_file(&path).unwrap();
        let e = lock_file(&path).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock);

        drop(first);
        let second = lock_file(&path).unwrap();
        drop(second);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_failed_write_leaves_the_target_untouched() {
        let dir = scratch("atomic");
        std::fs::create_dir_all(&dir).unwrap();

        // A directory where the file should go: the rename fails for real.
        let path = dir.join("settings.json");
        std::fs::create_dir(&path).unwrap();

        write_file_atomic(&path, b"new").unwrap_err();

        assert!(path.is_dir(), "the failed rename must not touch the target");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_write_keeps_the_targets_mode_unless_it_names_one() {
        let dir = scratch("atomic-mode");
        std::fs::create_dir_all(&dir).unwrap();

        let plain = dir.join("cfg.json");
        let secret = dir.join("control.token");
        for path in [&plain, &secret] {
            std::fs::write(path, b"old").unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }

        write_file_atomic(&plain, b"new").unwrap();
        write_private_file_atomic(&secret, b"new").unwrap();

        assert_eq!(
            mode(&plain),
            0o644,
            "a plain rewrite must keep the target's mode"
        );
        assert_eq!(
            mode(&secret),
            0o600,
            "an explicit mode must override the target's"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_private_dir_is_owner_only() {
        let dir = scratch("private-dir");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        ensure_private_dir(&dir).unwrap();

        assert_eq!(mode(&dir), 0o700);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
