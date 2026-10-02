use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rusqlite::Connection;
use tokio::sync::{mpsc, oneshot};

use crate::ledger::{BucketWidth, LedgerNew, UsageSummary};
use crate::provider::{Attempt, Model, Provider, ProviderNew, ProviderPatch};
use crate::routing::RoutingInput;
use crate::settings::Settings;

use super::actor::{
    ActorState, Ask, CHANNEL_BOUND, Command, Envelope, REPLY_TIMEOUT, Shutdown, Tell, actor,
};
use super::commands::{app_keys, attempts, balance, ledger, pins, providers, routing, settings};
use super::sql::{init_schema, refresh_stale_template_backed};
use super::{AppKey, Error, Result};
use crate::provider::Pin;

/// A handle to the store actor (ADR-0006). Cheaply cloneable; clones share the
/// same mailbox and the same actor task.
#[derive(Clone)]
pub struct Store {
    tx: mpsc::Sender<Box<dyn Envelope>>,
    /// Hot-path rows the queue refused (ADR-0006: dropped with a log). Counted,
    /// so the policy is observable instead of only inferable from logs.
    dropped: Arc<AtomicU64>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        Self::start(conn)
    }

    #[cfg_attr(not(test), expect(dead_code))]
    pub fn open_in_memory() -> Result<Store> {
        Self::start(Connection::open_in_memory()?)
    }

    fn start(conn: Connection) -> Result<Store> {
        init_schema(&conn)?;
        refresh_stale_template_backed(&conn)?;
        let state = ActorState::load(conn)?;
        let (tx, rx) = mpsc::channel(CHANNEL_BOUND);
        tokio::spawn(actor(rx, state));
        Ok(Store {
            tx,
            dropped: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Stop the actor after draining queued writes (ADR-0006: clean shutdown
    /// flushes pending writes). Idempotent.
    pub async fn shutdown(self) {
        let _ = self.tx.send(Box::new(Shutdown)).await;
        // The actor exits after draining the queue; the receiver drops with it,
        // and `closed` resolves then. Timeout guards against a hung DB.
        let _ = tokio::time::timeout(Duration::from_secs(5), self.tx.closed()).await;
    }

    /// Request-response over the mailbox. Fail-closed when the actor is gone;
    /// a hung DB (not a slow one) trips the reply timeout.
    async fn ask<C: Command>(&self, command: C) -> Result<C::Reply> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Box::new(Ask { command, reply: tx }))
            .await
            .map_err(|_| Error::Unavailable)?;
        tokio::time::timeout(REPLY_TIMEOUT, rx)
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Unavailable)?
    }

    /// Fire-and-forget. The queue bounds backpressure, it is not capacity: a
    /// row that cannot be queued is dropped with a log (ADR-0006) — re-queuing
    /// it would break the submission order the store relies on.
    fn tell<C: Command<Reply = ()>>(&self, label: &'static str, command: C) {
        match self.tx.try_send(Box::new(Tell { label, command })) {
            Ok(()) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                tracing::warn!("store queue full: {label} dropped");
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                tracing::warn!("store actor stopped: {label} dropped");
            }
        }
    }

    // ---- routing --------------------------------------------------------

    pub async fn routing_snapshot(&self, app: &str) -> Result<RoutingInput> {
        self.ask(routing::GetRoutingSnapshot { app: app.into() })
            .await
    }

    // ---- providers ------------------------------------------------------

    pub async fn insert_provider(&self, new: ProviderNew) -> Result<Option<Provider>> {
        self.ask(providers::InsertProvider { new: Box::new(new) })
            .await
    }

    pub async fn update_provider(
        &self,
        id: &str,
        patch: &ProviderPatch,
    ) -> Result<Option<Provider>> {
        self.ask(providers::UpdateProvider {
            id: id.into(),
            patch: Box::new(patch.clone()),
        })
        .await
    }

    /// Atomically replace a provider's models.
    pub async fn set_provider_models(&self, id: &str, models: Vec<Model>) -> Result<()> {
        self.ask(providers::SetProviderModels {
            id: id.into(),
            models,
        })
        .await
    }

    pub async fn get_provider(&self, id: &str) -> Result<Option<Provider>> {
        self.ask(providers::GetProvider { id: id.into() }).await
    }

    pub async fn list_providers(&self) -> Result<Vec<Provider>> {
        self.ask(providers::ListProviders).await
    }

    /// Returns true if a provider was removed.
    pub async fn delete_provider(&self, id: &str) -> Result<bool> {
        self.ask(providers::DeleteProvider { id: id.into() }).await
    }

    // ---- settings -------------------------------------------------------
    /// Write several settings in one transaction (a PATCH is all-or-nothing).
    pub async fn set_settings(&self, entries: &[(String, String)]) -> Result<()> {
        self.ask(settings::SetSettings {
            entries: entries.to_vec(),
        })
        .await
    }

    pub async fn get_settings(&self) -> Result<Settings> {
        self.ask(settings::GetSettings).await
    }

    // ---- pins -----------------------------------------------------------

    /// Add a pinned candidate provider for an app (idempotent; 1+ allowed).
    pub async fn insert_pin(&self, app: &str, provider_id: &str) -> Result<()> {
        self.ask(pins::InsertPin {
            app: app.into(),
            provider_id: provider_id.into(),
        })
        .await
    }

    pub async fn delete_pin(&self, app: &str, provider_id: &str) -> Result<bool> {
        self.ask(pins::DeletePin {
            app: app.into(),
            provider_id: provider_id.into(),
        })
        .await
    }

    /// Clear every candidate of one app (pin → auto routing).
    pub async fn clear_pins(&self, app: &str) -> Result<()> {
        self.ask(pins::ClearPins { app: app.into() }).await
    }

    /// Clear every pin for every app.
    pub async fn clear_all_pins(&self) -> Result<()> {
        self.ask(pins::ClearAllPins).await
    }

    pub async fn list_pins(&self) -> Result<Vec<Pin>> {
        self.ask(pins::ListPins).await
    }

    pub async fn has_pin(&self, app: &str, provider_id: &str) -> Result<bool> {
        self.ask(pins::HasPin {
            app: app.into(),
            provider_id: provider_id.into(),
        })
        .await
    }

    /// Pin write order.
    pub async fn pins_by_app(&self, app: &str) -> Result<Vec<Pin>> {
        self.ask(pins::PinsByApp { app: app.into() }).await
    }

    /// Pin write order.
    pub async fn pins_by_provider(&self, provider_id: &str) -> Result<Vec<Pin>> {
        self.ask(pins::PinsByProvider {
            provider_id: provider_id.into(),
        })
        .await
    }

    // ---- balance --------------------------------------------------------
    pub async fn insert_balance(&self, provider_id: &str, balance: &money::Money) -> Result<()> {
        self.ask(balance::InsertBalance {
            provider_id: provider_id.into(),
            balance: *balance,
        })
        .await
    }

    // ---- ledger ---------------------------------------------------------

    pub fn enqueue_ledger(&self, row: LedgerNew) {
        self.tell("ledger row", ledger::InsertLedger { row });
    }

    /// Ledger rows in `[start, end)`, folded into buckets `bucket_width` wide.
    pub async fn usage_window(
        &self,
        start: i64,
        end: i64,
        bucket_width: BucketWidth,
        app: Option<&str>,
        provider_id: Option<&str>,
    ) -> Result<Vec<UsageSummary>> {
        self.ask(ledger::GetUsageWindow {
            start,
            end,
            bucket_width,
            app: app.map(str::to_string),
            provider_id: provider_id.map(str::to_string),
        })
        .await
    }

    pub async fn prune_ledger(&self) -> Result<()> {
        self.ask(ledger::PruneLedger).await
    }

    // ---- attempts -------------------------------------------------------

    pub fn enqueue_attempt(&self, row: Attempt) {
        self.tell("attempt row", attempts::InsertAttempt { row });
    }

    pub async fn prune_attempts(&self) -> Result<()> {
        self.ask(attempts::PruneAttempts).await
    }

    // ---- app keys -------------------------------------------------------

    pub async fn insert_app_key(&self, app: &str, key: &str) -> Result<AppKey> {
        self.ask(app_keys::InsertAppKey {
            app: app.into(),
            key: key.into(),
        })
        .await
    }

    pub async fn app_by_key(&self, key: &str) -> Result<Option<String>> {
        self.ask(app_keys::AppByKey { key: key.into() }).await
    }

    pub async fn list_app_keys(&self) -> Result<Vec<AppKey>> {
        self.ask(app_keys::ListAppKeys).await
    }

    pub async fn keys_by_app(&self, app: &str) -> Result<Vec<AppKey>> {
        self.ask(app_keys::KeysByApp { app: app.into() }).await
    }

    pub async fn get_app_key(&self, id: &str) -> Result<Option<AppKey>> {
        self.ask(app_keys::GetAppKey { id: id.into() }).await
    }

    /// Returns true if the key was removed.
    pub async fn delete_app_key(&self, id: &str) -> Result<bool> {
        self.ask(app_keys::DeleteAppKey { id: id.into() }).await
    }

    // ---- response id ----------------------------------------------------

    pub async fn provider_by_response(&self, id: &str) -> Result<Option<String>> {
        self.ask(ledger::ProviderByResponse { id: id.into() }).await
    }

    pub async fn provider_by_conversation(&self, id: &str) -> Result<Option<String>> {
        self.ask(ledger::ProviderByConversation { id: id.into() })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::{ledger_row, provider};
    use crate::provider::AttemptOutcome;

    /// The queue bounds backpressure, it is not capacity: an overflow row is
    /// dropped — counted, not deferred behind the queue (ADR-0006).
    #[tokio::test]
    async fn overflow_rows_are_dropped_not_deferred() {
        let (tx, mut rx) = mpsc::channel(CHANNEL_BOUND);
        let store = Store {
            tx,
            dropped: Arc::new(AtomicU64::new(0)),
        };

        for i in 0..CHANNEL_BOUND + 1 {
            store.enqueue_attempt(Attempt {
                provider_id: format!("p{i}"),
                outcome: AttemptOutcome::Failed,
                latency: 0,
                at: 0,
            });
        }

        assert_eq!(
            store.dropped.load(Ordering::Relaxed),
            1,
            "the overflow row was not counted"
        );

        let mut queued = 0;
        while rx.try_recv().is_ok() {
            queued += 1;
        }
        assert_eq!(queued, CHANNEL_BOUND);

        // draining the queue must not uncover a deferred row
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(rx.try_recv().is_err(), "the overflow row came back");
    }
    /// Every ask gets its own answer: 64 requests in flight, each one asking for
    /// the id it seeded, and every reply carries that id back.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_asks_keep_their_own_replies() {
        let store = Store::open_in_memory().unwrap();
        for i in 0..64 {
            store
                .insert_provider(provider(&format!("p{i}")))
                .await
                .unwrap();
        }

        let mut asks = Vec::new();
        for i in 0..64 {
            let store = store.clone();
            asks.push(tokio::spawn(async move {
                let id = format!("p{i}");
                for _ in 0..100 {
                    let got = store.get_provider(&id).await.unwrap().unwrap();
                    assert_eq!(got.id, id);
                }
            }));
        }
        for ask in asks {
            ask.await.unwrap();
        }
    }

    /// `Shutdown` is a pill: rows queued before it land, and asks after it fail
    /// closed instead of hanging.
    #[tokio::test]
    async fn shutdown_drains_what_was_queued_before_it() {
        let path = std::env::temp_dir().join(format!("awitch-drain-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let store = Store::open(&path).unwrap();
        store.insert_provider(provider("p")).await.unwrap();

        // queued, not yet handled, when the pill arrives
        store.enqueue_ledger(ledger_row("app", "p"));
        store.clone().shutdown().await;

        // the row landed: a fresh handle on the same file sees it
        let reopened = Store::open(&path).unwrap();
        let until = crate::utils::now_unix_secs() + 60;
        assert_eq!(
            reopened
                .usage_window(0, until, BucketWidth::Day, None, None)
                .await
                .unwrap()
                .len(),
            1
        );
        reopened.shutdown().await;
        let _ = std::fs::remove_file(&path);

        // and a request after the pill fails closed — not a ten-second wait
        let after = tokio::time::timeout(Duration::from_millis(200), store.get_provider("p")).await;
        assert!(matches!(after, Ok(Err(Error::Unavailable))), "{after:?}");
    }
}
