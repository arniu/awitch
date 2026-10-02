//! The providers subject: the pool, its models and its prices.

use crate::provider::{Model, Provider, ProviderNew, ProviderPatch};

use super::{Command, Ctx};
use crate::db::Result;
use crate::db::cache::Stale;
use crate::db::sql::{
    delete_provider, get_provider, insert_provider, list_providers, set_provider_models,
    update_provider,
};

pub(in crate::db) struct GetProvider {
    pub id: String,
}

impl Command for GetProvider {
    type Reply = Option<Provider>;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        get_provider(cx.conn, &self.id)
    }
}

pub(in crate::db) struct ListProviders;

impl Command for ListProviders {
    type Reply = Vec<Provider>;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        list_providers(cx.conn)
    }
}

pub(in crate::db) struct InsertProvider {
    pub new: Box<ProviderNew>,
}

impl Command for InsertProvider {
    type Reply = Option<Provider>;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        let inserted = insert_provider(cx.conn, *self.new)?;
        if inserted.is_some() {
            cx.routing.mark(Stale::Providers);
        }
        Ok(inserted)
    }
}

pub(in crate::db) struct UpdateProvider {
    pub id: String,
    pub patch: Box<ProviderPatch>,
}

impl Command for UpdateProvider {
    type Reply = Option<Provider>;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        let updated = update_provider(cx.conn, &self.id, &self.patch)?;
        if updated.is_some() {
            cx.routing.mark(Stale::Providers);
        }
        Ok(updated)
    }
}

pub(in crate::db) struct SetProviderModels {
    pub id: String,
    pub models: Vec<Model>,
}

impl Command for SetProviderModels {
    type Reply = ();

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        set_provider_models(cx.conn, &self.id, self.models)?;
        cx.routing.mark(Stale::Providers);
        Ok(())
    }
}

pub(in crate::db) struct DeleteProvider {
    pub id: String,
}

impl Command for DeleteProvider {
    type Reply = bool;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        let removed = delete_provider(cx.conn, &self.id)?;
        if removed {
            cx.routing.mark(Stale::Providers);
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Store;
    use crate::db::test_support::{add_provider_with_models, model};
    use crate::provider::ProviderPatch;
    use serde_json::json;

    /// Catalog sync writes go through the store's atomic models-only update
    /// (ADR-0008): the write re-reads the current record inside the actor, so a
    /// provider edited between the sync's list read and its write keeps that
    /// edit — only `.models` is replaced.
    #[tokio::test]
    async fn set_provider_models_keeps_a_concurrent_edit() {
        let store = Store::open_in_memory().unwrap();
        add_provider_with_models(&store, "p", &["a"]).await;
        // an edit lands after the sync read the provider …
        let patch: ProviderPatch =
            serde_json::from_value(json!({"key": "k2", "name": "Renamed"})).unwrap();
        store.update_provider("p", &patch).await.unwrap();
        // … then the sync writes its parsed catalog
        store
            .set_provider_models("p", vec![model("b")])
            .await
            .unwrap();

        let p = store.get_provider("p").await.unwrap().unwrap();
        assert_eq!(p.key, "k2");
        assert_eq!(p.name.as_deref(), Some("Renamed"));
        assert_eq!(
            p.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["b"]
        );
    }
    /// The opposite order: a catalog sync lands between the edit's read and its
    /// write, and its models survive — the merge re-reads the record, so neither
    /// write rolls the other back.
    #[tokio::test]
    async fn update_provider_merges_onto_models_written_in_between() {
        let store = Store::open_in_memory().unwrap();
        add_provider_with_models(&store, "p", &["a"]).await;
        store
            .set_provider_models("p", vec![model("b")])
            .await
            .unwrap();

        let patch: ProviderPatch =
            serde_json::from_value(json!({"key": "k2", "name": "Renamed"})).unwrap();
        let updated = store.update_provider("p", &patch).await.unwrap().unwrap();
        assert_eq!(updated.key, "k2");
        assert_eq!(updated.name.as_deref(), Some("Renamed"));

        let p = store.get_provider("p").await.unwrap().unwrap();
        assert_eq!(p.key, "k2");
        assert_eq!(
            p.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["b"]
        );
    }
    /// Two edits in sequence merge field-wise: the second never clobbers the
    /// first's change.
    #[tokio::test]
    async fn update_provider_keeps_an_earlier_edits_change() {
        let store = Store::open_in_memory().unwrap();
        add_provider_with_models(&store, "p", &["a"]).await;

        let key_patch: ProviderPatch = serde_json::from_value(json!({"key": "k2"})).unwrap();
        store.update_provider("p", &key_patch).await.unwrap();
        let name_patch: ProviderPatch = serde_json::from_value(json!({"name": "Renamed"})).unwrap();
        store.update_provider("p", &name_patch).await.unwrap();

        let p = store.get_provider("p").await.unwrap().unwrap();
        assert_eq!(p.key, "k2");
        assert_eq!(p.name.as_deref(), Some("Renamed"));
    }
    #[tokio::test]
    async fn update_provider_on_a_missing_id_is_none() {
        let store = Store::open_in_memory().unwrap();
        let patch: ProviderPatch = serde_json::from_value(json!({"key": "k"})).unwrap();
        assert!(
            store
                .update_provider("nope", &patch)
                .await
                .unwrap()
                .is_none()
        );
    }
    /// The atomic update never resurrects a provider deleted since the sync's
    /// read, writes nothing when the pull is unchanged, and never wipes a
    /// populated catalog on an empty pull.
    #[tokio::test]
    async fn set_provider_models_never_resurrects_nor_wipes() {
        let store = Store::open_in_memory().unwrap();

        // deleted between read and write → no resurrect
        add_provider_with_models(&store, "d", &["a"]).await;
        store.delete_provider("d").await.unwrap();
        assert!(
            store
                .set_provider_models("d", vec![model("x")])
                .await
                .is_err()
        );
        assert!(store.list_providers().await.unwrap().is_empty());

        // equal pull → no write; empty pull over a populated catalog → kept
        add_provider_with_models(&store, "e", &["a"]).await;
        store
            .set_provider_models("e", vec![model("a")])
            .await
            .unwrap();
        store.set_provider_models("e", vec![]).await.unwrap();
        let e = store.get_provider("e").await.unwrap().unwrap();
        assert!(e.models.is_empty());

        // empty → populated populates
        add_provider_with_models(&store, "f", &[]).await;
        store
            .set_provider_models("f", vec![model("g")])
            .await
            .unwrap();
        let f = store.get_provider("f").await.unwrap().unwrap();
        assert_eq!(
            f.models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["g"]
        );
    }
}
