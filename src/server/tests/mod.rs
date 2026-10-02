//! Integration tests: the real routers behind a live listener (ADR-0010), so
//! the routing/auth/handler wiring is the test surface. The data plane is not
//! covered here — what it does is protocol behavior, and a mock upstream
//! authored by the same hand as the assertions cannot vouch for that.

mod control_plane;

use super::*;
use crate::protocol::Protocol;
use axum::Router;
use money::Currency;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Seed one template-free provider (production `add_provider` path) with the
/// shared test shape: key `mock-key`, name = the id.
async fn seed_template_free(
    store: &Store,
    id: &str,
    base_url: &str,
    protocols: std::collections::BTreeMap<Protocol, crate::provider::Endpoint>,
) {
    store
        .insert_provider(crate::provider::ProviderNew {
            id: Some(id.into()),
            template_id: None,
            key: "mock-key".into(),
            name: Some(id.into()),
            base_url: base_url.into(),
            models_url: None,
            balance_url: None,
            protocols,
        })
        .await
        .unwrap()
        .expect("fresh provider id");
}

/// Seed one more provider of the same test shape, models and balance included.
pub(super) async fn add_mock_provider(store: &Store, id: &str, base_url: &str) {
    let protocols = std::collections::BTreeMap::from([(
        Protocol::OpenaiChat,
        crate::provider::Endpoint::Canonical,
    )]);
    seed_template_free(store, id, base_url, protocols).await;
    store
        .set_provider_models(
            id,
            vec![serde_json::from_value(json!({"id": "m"})).unwrap()],
        )
        .await
        .unwrap();
    store
        .insert_balance(
            id,
            &money::Money {
                amount: money::Amount::new(10, 0),
                currency: Currency::Usd,
            },
        )
        .await
        .unwrap();
}

pub(super) async fn store_with_provider(base_url: &str) -> Store {
    let store = Store::open_in_memory().unwrap();
    add_mock_provider(&store, "mock", base_url).await;
    store
}

pub(super) fn dummy_task_tx() -> tokio::sync::mpsc::Sender<crate::tasks::TaskEvent> {
    tokio::sync::mpsc::channel(1).0
}

pub(super) async fn serve(app: Router) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{addr}"), handle)
}
