mod apps;
mod doc;
mod record;
mod schema;
mod value_spec;

#[cfg(test)]
mod tests;

use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use doc::{Doc, Format};
use record::{Edit, FileEdits};
use schema::{App, File};
use value_spec::Field;

pub use value_spec::Target;

/// The app specs awitch offers.
pub fn apps() -> impl Iterator<Item = &'static App> + Clone {
    apps::supported()
}

pub fn app_by_name(name: &str) -> Option<&'static App> {
    apps::supported().find(|app| app.name == name)
}

/// Revert a recorded edit in a doc:
/// - restore `old_value` when the doc still holds `value`;
/// - delete the key path when there was no prior value.
fn revert(doc: &mut Doc, edit: &Edit) -> anyhow::Result<bool> {
    if doc.get(&edit.key_path) != Some(&edit.value) {
        return Ok(false);
    }

    let Some(prev) = &edit.old_value else {
        return Ok(doc.remove(&edit.key_path));
    };

    doc.set(&edit.key_path, prev.clone())?;
    Ok(true)
}

/// A file's doc — an absent file is an empty doc in its name's format.
fn read_doc(path: &Path) -> anyhow::Result<Doc> {
    let format =
        Format::from_path(path).ok_or_else(|| anyhow!("unknown format: {}", path.display()))?;
    if !path.exists() {
        return Ok(format.empty_doc());
    }

    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    format
        .parse(&text)
        .map_err(|e| anyhow!("{}: {e}", path.display()))
}

/// Write a doc's content to `path` in its own format, atomically.
fn write_doc(path: &Path, doc: &Doc) -> anyhow::Result<()> {
    let text = doc
        .to_text()
        .map_err(|e| anyhow!("{}: {e}", path.display()))?;
    crate::utils::write_file_atomic(path, text.as_bytes())?;
    Ok(())
}

/// Coverage is per file: every schema file has a bucket covering all of its
/// key paths (file paths are unique per app).
fn covered(schema: &[File], buckets: &[FileEdits]) -> bool {
    schema.iter().all(|file| {
        let Some(bucket) = buckets.iter().find(|f| f.path == file.path) else {
            return false;
        };
        file.patches
            .iter()
            .all(|ks| bucket.edits.iter().any(|e| e.key_path == ks.key_path))
    })
}

struct RecordFile {
    path: PathBuf,
}

impl RecordFile {
    fn new(path: PathBuf) -> RecordFile {
        RecordFile { path }
    }

    /// The lock file sits next to the record under the same base.
    fn lock(&self) -> io::Result<std::fs::File> {
        self.ensure_dir()?;
        crate::utils::lock_file(&self.path.with_extension("lock"))
    }

    /// The record's raw text: no record reads as None.
    fn read(&self) -> io::Result<Option<String>> {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => Ok(Some(s)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Persist files as a verified record; an empty record writes nothing.
    fn write(&self, files: &[FileEdits]) -> io::Result<()> {
        if files.is_empty() {
            return Ok(());
        }

        let s = record::encode(files)
            .map_err(|e| io::Error::other(format!("{}: {e}", self.path.display())))?;
        // Every writer locks first; lock() already ensured the parent 0700.
        crate::utils::write_file_atomic(&self.path, s.as_bytes())
    }

    fn clear(&self) -> io::Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn ensure_dir(&self) -> io::Result<()> {
        let Some(parent) = self.path.parent() else {
            return Ok(());
        };

        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;

        Ok(())
    }
}

/// Read back the target the current docs point at.
fn read_target(files: &[File], docs: &[Doc]) -> Option<Target> {
    let mut url: Option<String> = None;
    let mut key: Option<String> = None;
    for (file, doc) in files.iter().zip(docs) {
        for ks in file.patches {
            let Some(value) = doc.get(ks.key_path) else {
                continue;
            };

            if url.is_none()
                && let Some(found) = Field::Url.read(value, &ks.value)
            {
                url = found.as_str().filter(|s| !s.is_empty()).map(str::to_string);
            }
            if key.is_none()
                && let Some(found) = Field::Key.read(value, &ks.value)
            {
                key = found.as_str().filter(|s| !s.is_empty()).map(str::to_string);
            }
        }
    }

    url.zip(key).map(|(url, key)| Target { url, key })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub state: State,
    pub target: Option<Target>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Unpointed,
    Unrecoverable,
    Divergent,
    Normal,
}

impl State {
    pub fn as_str(&self) -> &'static str {
        match self {
            State::Unpointed => "unpointed",
            State::Unrecoverable => "unrecoverable",
            State::Divergent => "divergent",
            State::Normal => "normal",
        }
    }

    pub fn is_pointed(self) -> bool {
        !matches!(self, State::Unpointed)
    }
}

pub struct Instance {
    app: &'static App,
    home: PathBuf,
    record_file: RecordFile,
    env_override: Option<String>,
}

impl App {
    pub fn bind(&'static self, config_dir: &Path) -> Instance {
        Instance {
            app: self,
            home: dirs::home_dir().unwrap_or_default(),
            record_file: RecordFile::new(
                config_dir
                    .join("point")
                    .join(self.name)
                    .with_extension("toml"),
            ),
            env_override: self
                .base_dir
                .env_override
                .and_then(|e| std::env::var(e).ok()),
        }
    }
}

impl Instance {
    pub fn is_installed(&self) -> bool {
        self.app.files.iter().all(|f| self.file_path(f).exists())
    }

    fn plan(&self, target: &Target) -> anyhow::Result<Vec<FileEdits>> {
        let mut out = Vec::new();
        for file in self.app.files {
            let doc = read_doc(&self.file_path(file))?;
            let mut edits = Vec::new();
            for ks in file.patches {
                edits.push(Edit {
                    key_path: ks.key_path.into(),
                    value: ks.value.render(target)?,
                    old_value: doc.get(ks.key_path).cloned(),
                });
            }

            out.push(FileEdits {
                path: file.path.into(),
                edits,
            });
        }

        Ok(out)
    }

    fn apply(&self, planned: &[FileEdits]) -> anyhow::Result<()> {
        let mut docs: Vec<(PathBuf, Doc, Vec<&Edit>)> = Vec::new();
        for file in self.app.files {
            let path = self.file_path(file);
            let doc = read_doc(&path)?;
            let bucket = planned.iter().find(|f| f.path == file.path);

            let mut edits = Vec::new();
            for ks in file.patches {
                // Never a miss: plan covers every schema key path of every file.
                if let Some(edit) =
                    bucket.and_then(|b| b.edits.iter().find(|e| e.key_path == ks.key_path))
                {
                    // Guard: still at the plan-time baseline — an edit slipped in.
                    if doc.get(ks.key_path) != edit.old_value.as_ref() {
                        return Err(anyhow!(
                            "'{}' changed since point planned it — aborting",
                            edit.key_path
                        ));
                    }

                    edits.push(edit);
                }
            }

            docs.push((path, doc, edits));
        }

        for (path, mut doc, edits) in docs {
            for edit in edits {
                doc.set(&edit.key_path, edit.value.clone())?;
            }

            write_doc(&path, &doc)?;
        }

        Ok(())
    }

    fn restore(&self, edit_logs: &[FileEdits]) -> anyhow::Result<()> {
        for file in self.app.files {
            let path = self.file_path(file);
            let mut doc = read_doc(&path)?;
            let bucket = edit_logs.iter().find(|f| f.path == file.path);
            let mut changed = false;
            for ks in file.patches {
                if let Some(edit) =
                    bucket.and_then(|b| b.edits.iter().find(|e| e.key_path == ks.key_path))
                {
                    changed |= revert(&mut doc, edit)?;
                }
            }

            for path in file.trim_empty {
                changed |= doc.trim_empty(path);
            }
            if changed {
                write_doc(&path, &doc)?;
            }
        }

        Ok(())
    }

    fn expand(&self, raw: &str) -> PathBuf {
        match raw.strip_prefix("~/") {
            Some(rest) => self.home.join(rest),
            None => PathBuf::from(raw),
        }
    }

    pub(super) fn file_path(&self, file: &File) -> PathBuf {
        if file.path.starts_with("~/") || file.path.starts_with('/') {
            self.expand(file.path)
        } else {
            self.root_dir().join(file.path)
        }
    }

    fn root_dir(&self) -> PathBuf {
        let base = self
            .env_override
            .as_deref()
            .unwrap_or(self.app.base_dir.default);
        self.expand(base)
    }

    /// The record files a point/undo may replay: none when no record stands,
    /// an error when the one that does can't be trusted for a restore.
    fn existing_record(&self) -> anyhow::Result<Option<Vec<FileEdits>>> {
        let Some(text) = self.record_file.read()? else {
            return Ok(None);
        };

        let Some(buckets) = record::decode(&text) else {
            return Err(anyhow!(
                "pointing record is corrupt — run 'awitch reset' to recover"
            ));
        };

        if !covered(self.app.files, &buckets) {
            return Err(anyhow!(
                "record predates this app's schema — run 'awitch reset' to recover"
            ));
        }

        Ok(Some(buckets))
    }

    pub fn check(&self) -> anyhow::Result<Report> {
        // The record's own read error stops before any doc is touched.
        let record = self.record_file.read()?;

        // Every doc is read before any verdict — a read error is an error,
        // not a state; an absent file reads as an empty doc.
        let mut file_docs = Vec::with_capacity(self.app.files.len());
        for file in self.app.files {
            file_docs.push(read_doc(&self.file_path(file))?);
        }

        let installed = self.is_installed();

        // What the current docs point at is read back from C alone, whatever
        // the record says — so a reset's residue stays visible (spec §reset).
        let target = if installed {
            read_target(self.app.files, &file_docs)
        } else {
            None
        };

        let Some(text) = record else {
            return Ok(Report {
                state: State::Unpointed,
                target,
            });
        };

        let Some(buckets) = record::decode(&text) else {
            return Ok(Report {
                state: State::Unrecoverable,
                target,
            });
        };
        // Incomplete coverage or nothing installed to point at leaves the
        // record unusable until a fresh point lands (spec §3).
        if !covered(self.app.files, &buckets) || !installed {
            return Ok(Report {
                state: State::Unrecoverable,
                target,
            });
        }

        // Diverged = a schema value moved from what its edit wrote;
        // bucket entries whose key paths the schema dropped are ignored.
        let holds = self.app.files.iter().zip(&file_docs).all(|(file, doc)| {
            let Some(bucket) = buckets.iter().find(|f| f.path == file.path) else {
                return false;
            };
            file.patches.iter().all(|ks| {
                bucket
                    .edits
                    .iter()
                    .find(|e| e.key_path == ks.key_path)
                    .is_some_and(|e| doc.get(ks.key_path) == Some(&e.value))
            })
        });

        let state = if holds {
            State::Normal
        } else {
            State::Divergent
        };

        Ok(Report { state, target })
    }

    pub fn point(&self, target: &Target) -> anyhow::Result<()> {
        let _lock = self.record_file.lock()?;
        let old = self.existing_record()?.unwrap_or_default();
        if !old.is_empty() {
            self.restore(&old)?;
            let _ = self.record_file.clear();
        }

        let planned = self.plan(target)?;
        self.record_file
            .write(&planned)
            .map_err(|e| anyhow!("record failed ({e}); nothing written"))?;
        self.apply(&planned).map_err(|e| {
            anyhow!("apply failed ({e}); record kept — rerun 'awitch point' to converge")
        })
    }

    /// Undo a point: true = undone, false = nothing to undo.
    pub fn undo(&self) -> anyhow::Result<bool> {
        let _lock = self.record_file.lock()?;
        let Some(old) = self.existing_record()? else {
            return Ok(false);
        };

        self.restore(&old)?;
        self.record_file.clear()?;

        Ok(true)
    }

    /// Clear a broken record: true = cleared, false = nothing to reset.
    pub fn reset(&self) -> anyhow::Result<bool> {
        let _lock = self.record_file.lock()?;
        let Some(text) = self.record_file.read()? else {
            return Ok(false);
        };
        let usable = record::decode(&text).is_some_and(|files| covered(self.app.files, &files));
        if usable {
            return Ok(false);
        }

        self.record_file.clear()?;

        Ok(true)
    }
}

#[cfg(test)]
impl Instance {
    pub(super) fn with_home(mut self, home: impl Into<PathBuf>) -> Instance {
        self.home = home.into();
        self.env_override = None;
        self
    }

    pub(super) fn with_env_override(mut self, value: impl Into<PathBuf>) -> Instance {
        self.env_override = Some(value.into().to_string_lossy().into_owned());
        self
    }
}
