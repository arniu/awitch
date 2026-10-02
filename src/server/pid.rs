use std::path::{Path, PathBuf};

pub(crate) struct PidFile {
    path: PathBuf,
}

impl PidFile {
    pub(crate) fn write(path: &Path) -> std::io::Result<PidFile> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let contents = format!("{}\n", std::process::id());
        std::fs::write(path, &contents).map_err(|e| {
            std::io::Error::new(e.kind(), format!("pid_file {}: {e}", path.display()))
        })?;

        Ok(PidFile {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
