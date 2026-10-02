//! The balance subject: a provider's account snapshot, kept as an observation.

use super::{Command, Ctx};
use crate::db::Result;
use crate::db::sql::insert_balance;

pub(in crate::db) struct InsertBalance {
    pub provider_id: String,
    pub balance: money::Money,
}

impl Command for InsertBalance {
    type Reply = ();

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        insert_balance(cx.conn, &self.provider_id, &self.balance)?;

        // An observation, not a shape change: the provider's balance is
        // replaced in place, nothing is rebuilt.
        if let Some(entry) = cx.routing.ensure_entry(cx.conn, &self.provider_id) {
            entry.metrics.balance = Some(self.balance);
        }

        Ok(())
    }
}
