//! The routing subject: the snapshot routing decides on.

use crate::routing::{RoutingInput, RoutingProvider};
use crate::utils::now_unix_secs;

use super::{Command, Ctx};
use crate::db::Result;

pub(in crate::db) struct GetRoutingSnapshot {
    pub app: String,
}

impl Command for GetRoutingSnapshot {
    type Reply = RoutingInput;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        // A read refreshes the slices a write marked stale (ADR-0006).
        cx.routing.refresh(cx.conn)?;

        let pinned = cx.routing.pins.get(&self.app).cloned().unwrap_or_default();
        let providers = cx
            .routing
            .providers
            .values()
            .map(|e| RoutingProvider {
                provider: e.provider.clone(),
                prices: e.prices.clone(),
                metrics: e.metrics.clone(),
            })
            .collect();

        Ok(RoutingInput {
            providers,
            mode: cx.settings.routing_mode,
            pinned_providers: pinned,
            balance_threshold: cx.settings.routing_balance_threshold,
            failure_threshold: cx.settings.routing_failure_threshold,
            cooldown_secs: cx.settings.routing_cooldown_secs,
            created_at: now_unix_secs(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Store;
    use crate::db::test_support::{add_provider, add_provider_with_models, failed_attempt};
    use crate::provider::{Attempt, AttemptOutcome, ProviderPatch};
    use serde_json::json;

    /// A provider write is visible to the next read: the slice is marked stale
    /// and rebuilt (ADR-0006).
    #[tokio::test]
    async fn a_provider_write_is_visible_to_the_next_snapshot() {
        let store = Store::open_in_memory().unwrap();
        add_provider(&store, "deepseek").await;

        let input = store.routing_snapshot("claude").await.unwrap();
        assert_eq!(input.providers.len(), 1);
        // unset mode stays None here — routing defaults it (policy, not storage)
        assert_eq!(input.mode, crate::routing::RoutingMode::Eco);

        let patch: ProviderPatch =
            serde_json::from_value(json!({"base_url": "https://api.deepseek.com/v2"})).unwrap();
        store.update_provider("deepseek", &patch).await.unwrap();
        let input = store.routing_snapshot("claude").await.unwrap();
        assert_eq!(
            input.providers[0].provider.base_url,
            "https://api.deepseek.com/v2"
        );
    }

    /// A balance row updates the provider in place — the next snapshot shows it
    /// without any rebuild.
    #[tokio::test]
    async fn a_balance_write_is_visible_to_the_next_snapshot() {
        let store = Store::open_in_memory().unwrap();
        add_provider_with_models(&store, "p", &["m"]).await;
        let balance = money::Money {
            amount: "5".parse().unwrap(),
            currency: money::Currency::Usd,
        };
        store.insert_balance("p", &balance).await.unwrap();

        let input = store.routing_snapshot("app").await.unwrap();
        assert_eq!(input.providers[0].metrics.balance, Some(balance));
    }

    /// A settings write takes effect on the next read — the mode and the
    /// ejection rule's parameters alike, and not only after a restart.
    #[tokio::test]
    async fn a_settings_write_is_visible_to_the_next_snapshot() {
        let store = Store::open_in_memory().unwrap();
        add_provider_with_models(&store, "p", &["m"]).await;

        store
            .set_settings(&[("routing.mode".into(), "speed".into())])
            .await
            .unwrap();
        let input = store.routing_snapshot("app").await.unwrap();
        assert_eq!(input.mode, crate::routing::RoutingMode::Speed);

        store
            .set_settings(&[("routing.failure_threshold".into(), "1".into())])
            .await
            .unwrap();
        store.enqueue_attempt(failed_attempt("p", crate::utils::now_unix_secs()));

        let input = store.routing_snapshot("app").await.unwrap();
        assert_eq!(input.failure_threshold, 1);
        let metrics = &input.providers[0].metrics;
        assert!(
            crate::routing::ejected(
                metrics,
                input.created_at,
                input.failure_threshold,
                input.cooldown_secs
            ),
            "the new threshold must apply without a restart"
        );

        store
            .set_settings(&[("routing.cooldown_secs".into(), "0".into())])
            .await
            .unwrap();
        let input = store.routing_snapshot("app").await.unwrap();
        assert_eq!(input.cooldown_secs, 0);
    }

    /// A snapshot must not leak across apps: a pin is app-specific.
    #[tokio::test]
    async fn pins_are_per_app_in_the_snapshot() {
        let store = Store::open_in_memory().unwrap();
        add_provider(&store, "mock").await;
        store.insert_pin("claude", "mock").await.unwrap();

        let input = store.routing_snapshot("claude").await.unwrap();
        assert_eq!(input.pinned_providers, vec!["mock".to_string()]);
        // the snapshot is per app: a different app must not inherit claude's pin
        let input = store.routing_snapshot("codex").await.unwrap();
        assert!(
            input.pinned_providers.is_empty(),
            "{:?}",
            input.pinned_providers
        );
    }

    /// A rebuild of the derived state may not change what the snapshot reports:
    /// the write that triggers it is unrelated to the observation.
    #[tokio::test]
    async fn a_rebuild_does_not_change_the_reported_failures() {
        let store = Store::open_in_memory().unwrap();
        add_provider_with_models(&store, "p", &["m"]).await;
        let now = crate::utils::now_unix_secs();
        for (outcome, at) in [
            (AttemptOutcome::Failed, now - 95),
            (AttemptOutcome::Delivered, now - 94),
            (AttemptOutcome::Failed, now - 93),
            (AttemptOutcome::Failed, now - 92),
        ] {
            store.enqueue_attempt(Attempt {
                provider_id: "p".into(),
                outcome,
                latency: 10,
                at,
            });
        }

        let failures =
            |input: crate::routing::RoutingInput| input.providers[0].metrics.consecutive_failures;
        let hot = failures(store.routing_snapshot("app").await.unwrap());
        // a pin write marks the Pins slice; the next snapshot rebuilds it
        // (providers + metrics are not marked, so their derived state is reused)
        store.insert_pin("app", "p").await.unwrap();
        let after_pin = failures(store.routing_snapshot("app").await.unwrap());

        assert_eq!(after_pin, hot);
        assert_eq!(hot, 2);
    }
}
