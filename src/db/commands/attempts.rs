//! The attempts subject: one forward attempt's observation, and its effect on
//! the provider's derived metrics.

use super::{Command, Ctx};
use crate::db::Result;
use crate::db::cache::observe;
use crate::db::sql::{insert_attempt, prune_attempts};
use crate::provider::Attempt;

pub(in crate::db) struct InsertAttempt {
    pub row: Attempt,
}

impl Command for InsertAttempt {
    type Reply = ();

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        // A hot-path row: the write is best-effort (a failure is logged and the
        // observation still counts), and it updates the provider in place.
        if let Err(e) = insert_attempt(cx.conn, &self.row) {
            tracing::error!("attempt insert failed: {e}");
        }
        if let Some(entry) = cx.routing.ensure_entry(cx.conn, &self.row.provider_id) {
            observe(entry, &self.row);
        }

        Ok(())
    }
}

pub(in crate::db) struct PruneAttempts;

impl Command for PruneAttempts {
    type Reply = ();

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        prune_attempts(cx.conn, cx.settings.attempts_max_per_provider)
    }
}
