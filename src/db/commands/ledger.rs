//! The ledger subject: the accounting rows and the usage summary.

use crate::ledger::{BucketWidth, LedgerNew, UsageSummary};

use super::{Command, Ctx};
use crate::db::Result;
use crate::db::sql::{
    insert_ledger, provider_by_conversation, provider_by_response, prune_ledger, usage_window,
};

pub(in crate::db) struct InsertLedger {
    pub row: LedgerNew,
}

impl Command for InsertLedger {
    type Reply = ();

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        // A hot-path row: best-effort, logged when the write fails.
        if let Err(e) = insert_ledger(cx.conn, &self.row) {
            tracing::error!("ledger insert failed: {e}");
        }

        Ok(())
    }
}

pub(in crate::db) struct GetUsageWindow {
    pub start: i64,
    pub end: i64,
    pub bucket_width: BucketWidth,
    pub app: Option<String>,
    pub provider_id: Option<String>,
}

impl Command for GetUsageWindow {
    type Reply = Vec<UsageSummary>;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        usage_window(
            cx.conn,
            self.start,
            self.end,
            self.bucket_width,
            self.app.as_deref(),
            self.provider_id.as_deref(),
        )
    }
}

pub(in crate::db) struct PruneLedger;

impl Command for PruneLedger {
    type Reply = ();

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        prune_ledger(cx.conn, cx.settings.ledger_retention_secs)
    }
}

pub(in crate::db) struct ProviderByResponse {
    pub id: String,
}

impl Command for ProviderByResponse {
    type Reply = Option<String>;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        provider_by_response(cx.conn, &self.id)
    }
}

pub(in crate::db) struct ProviderByConversation {
    pub id: String,
}

impl Command for ProviderByConversation {
    type Reply = Option<String>;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        provider_by_conversation(cx.conn, &self.id)
    }
}
