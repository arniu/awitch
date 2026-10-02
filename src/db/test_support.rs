//! Fixtures the store's tests share: one way to seed a provider or a row.

use std::collections::BTreeMap;

use rusqlite::Connection;

use crate::ledger::LedgerNew;
use crate::protocol::Protocol;
use crate::protocol::Usage;
use crate::provider::{Attempt, AttemptOutcome, Endpoint, Model, ProviderNew};

use super::Store;
use super::sql::insert_provider;

/// The minimal provider record the tests start from.
pub(in crate::db) fn provider(id: &str) -> ProviderNew {
    ProviderNew {
        id: Some(id.into()),
        template_id: None,
        key: "k".into(),
        name: None,
        base_url: "https://seed.example.com".into(),
        models_url: None,
        balance_url: None,
        protocols: BTreeMap::from([(Protocol::OpenaiChat, Endpoint::Canonical)]),
    }
}

/// The same provider as a row — for tests that drive the storage layer.
pub(in crate::db) fn seed_provider(conn: &Connection, id: &str) {
    insert_provider(conn, provider(id))
        .unwrap()
        .expect("fresh provider");
}

/// The same provider through the store — for tests that drive the actor.
pub(in crate::db) async fn add_provider(store: &Store, id: &str) {
    store
        .insert_provider(provider(id))
        .await
        .unwrap()
        .expect("fresh provider");
}

/// A provider with a model catalog.
pub(in crate::db) async fn add_provider_with_models(store: &Store, id: &str, models: &[&str]) {
    add_provider(store, id).await;
    if !models.is_empty() {
        store
            .set_provider_models(id, models.iter().map(|id| model(id)).collect())
            .await
            .unwrap();
    }
}

/// A failed observation at the given time.
pub(in crate::db) fn failed_attempt(provider_id: &str, at: i64) -> Attempt {
    Attempt {
        provider_id: provider_id.into(),
        outcome: AttemptOutcome::Failed,
        latency: 0,
        at,
    }
}

pub(in crate::db) fn model(id: &str) -> Model {
    serde_json::from_value(serde_json::json!({"id": id})).unwrap()
}

/// A ledger row carrying the fields its table requires.
pub(in crate::db) fn ledger_row(app: &str, provider_id: &str) -> LedgerNew {
    LedgerNew {
        app: app.into(),
        provider_id: provider_id.into(),
        response_id: None,
        conversation_id: None,
        requested_protocol: Protocol::OpenaiChat,
        requested_model: "m".into(),
        served_protocol: Protocol::OpenaiChat,
        served_model: "m".into(),
        usage: Usage {
            input_tokens: 1,
            output_tokens: 1,
        },
        price: None,
    }
}
