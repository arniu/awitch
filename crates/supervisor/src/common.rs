use std::path::{Path, PathBuf};

pub(super) fn pid(pid_file: &Path) -> Option<i32> {
    let content = std::fs::read_to_string(pid_file).ok()?;
    let first_line = content.lines().next()?;
    first_line.trim().parse().ok()
}

pub(super) fn latest_log_file(name: &str, log_dir: &Path) -> Option<PathBuf> {
    let prefix = format!("{name}.");
    std::fs::read_dir(log_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_log_file(p, &prefix))
        .max_by_key(|p| {
            std::fs::metadata(p)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH)
        })
}

fn is_log_file(path: &Path, prefix: &str) -> bool {
    let Some(file_name) = path.file_name().and_then(|f| f.to_str()) else {
        return false;
    };

    file_name.starts_with(prefix) && file_name.ends_with(".log")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn latest_log_file_selects_by_recency() {
        let dir = log_test_dir();

        // a flat-only dir still resolves — `<name>.log` is the generic contract
        std::fs::write(dir.join("example.log"), "").unwrap();
        assert_eq!(
            latest_log_file("example", &dir),
            Some(dir.join("example.log"))
        );

        // a stale flat legacy file must not beat a newer rotation file
        touch(&dir.join("example.log"), 1000);
        touch(&dir.join("example.2026-08-27.log"), 100);
        touch(&dir.join("example.2026-08-28.log"), 10);
        touch(&dir.join("server.log"), 1); // other-name file: excluded by prefix
        assert_eq!(
            latest_log_file("example", &dir),
            Some(dir.join("example.2026-08-28.log"))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn log_test_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "supervisor-log-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &PathBuf, age_secs: u64) {
        std::fs::write(path, "").unwrap();
        let old = std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(age_secs))
            .unwrap();
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(old))
            .unwrap();
    }
}
