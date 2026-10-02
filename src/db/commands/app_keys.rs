//! The app-key subject: the key → app registry, and the cache auth reads from.

use super::{Command, Ctx};
use crate::db::AppKey;
use crate::db::Result;
use crate::db::sql::{
    app_by_key, delete_app_key, get_app_key, insert_app_key, keys_by_app, list_app_keys,
};

pub(in crate::db) struct AppByKey {
    pub key: String,
}

impl Command for AppByKey {
    type Reply = Option<String>;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        app_for_key(&self.key, cx)
    }
}

pub(in crate::db) struct ListAppKeys;

impl Command for ListAppKeys {
    type Reply = Vec<AppKey>;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        list_app_keys(cx.conn)
    }
}

pub(in crate::db) struct KeysByApp {
    pub app: String,
}

impl Command for KeysByApp {
    type Reply = Vec<AppKey>;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        keys_by_app(cx.conn, &self.app)
    }
}

pub(in crate::db) struct GetAppKey {
    pub id: String,
}

impl Command for GetAppKey {
    type Reply = Option<AppKey>;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        get_app_key(cx.conn, &self.id)
    }
}

pub(in crate::db) struct DeleteAppKey {
    pub id: String,
}

impl Command for DeleteAppKey {
    type Reply = bool;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        // Auth is a memory read, so the cache must lose exactly the key this
        // removes — an older credential of the same app stays live.
        let cached = get_app_key(cx.conn, &self.id)
            .ok()
            .flatten()
            .map(|row| row.key);
        let removed = delete_app_key(cx.conn, &self.id)?;
        if removed && let Some(key) = cached {
            cx.keys.remove(&key);
        }

        Ok(removed)
    }
}

pub(in crate::db) struct InsertAppKey {
    pub app: String,
    pub key: String,
}

impl Command for InsertAppKey {
    type Reply = AppKey;

    fn run(self, cx: &mut Ctx<'_>) -> Result<Self::Reply> {
        let row = insert_app_key(cx.conn, &self.app, &self.key)?;
        cx.keys.insert(self.key, self.app);
        Ok(row)
    }
}

/// key → app, memory-first: the database is the truth, the cache the read path.
fn app_for_key(key: &str, cx: &mut Ctx<'_>) -> Result<Option<String>> {
    if let Some(app) = cx.keys.get(key) {
        return Ok(Some(app.clone()));
    }

    match app_by_key(cx.conn, key)? {
        Some(app) => {
            cx.keys.insert(key.to_string(), app.clone());
            Ok(Some(app))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Store;

    /// Revoking one key is selective and idempotent: the app's other credentials
    /// keep working, and the same id revokes once.
    #[tokio::test]
    async fn revoking_one_key_is_selective_and_idempotent() {
        let store = Store::open_in_memory().unwrap();
        store.insert_app_key("claude", "sk-a").await.unwrap();
        let b = store.insert_app_key("claude", "sk-b").await.unwrap();

        assert!(store.delete_app_key(&b.id).await.unwrap());
        assert_eq!(store.app_by_key("sk-b").await.unwrap(), None);
        assert_eq!(
            store.app_by_key("sk-a").await.unwrap().as_deref(),
            Some("claude")
        );
        assert!(!store.delete_app_key(&b.id).await.unwrap());
    }
}
