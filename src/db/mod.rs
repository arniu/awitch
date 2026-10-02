//! The gateway's state and its one writer (ADR-0006).
//!
//! One actor task owns the SQLite connection; every access is a mailbox item.
//! Access is serialized: a write is applied in submission order, and a read
//! observes every write submitted before it.
//!
//! A command touches only what `commands::Ctx` exposes, never the actor.
//!
//! The database is the only truth: the actor's in-memory state is derived,
//! rebuildable, and never authoritative. Reads are served from that state — no
//! TTL, and SQLite is queried only for the slices a write marked stale: a write
//! declares the slice it changed, and the next read rebuilds it, so repeated
//! writes cost one rebuild. Observations have no slice — they update the
//! affected provider in place. A rebuild failure surfaces on the read that
//! needed it, and the slice stays marked for the next one.
//!
//! Hot-path rows are best-effort: a response may return before they land, and a
//! row that cannot be written is dropped with a log.

mod actor;
mod cache;
mod commands;
mod sql;
mod store;

#[cfg(test)]
pub(in crate::db) mod test_support;

pub use store::Store;

/// The app-key registry row. `key` is the credential itself; `created_at` is
/// unix seconds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AppKey {
    pub(crate) id: String,
    pub(crate) app: String,
    pub(crate) key: String,
    pub(crate) created_at: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("storage: {0}")]
    Storage(#[from] rusqlite::Error),
    #[error("serialization: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("storage unavailable")]
    Unavailable,
    #[error("storage timeout")]
    Timeout,
    #[error("referenced row does not exist")]
    ReferencedRowMissing,
    #[error("invalid record: {0}")]
    InvalidRecord(#[from] crate::provider::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
