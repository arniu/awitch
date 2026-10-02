use std::collections::{BTreeMap, HashMap};

use rusqlite::{Connection, OptionalExtension, params};

use crate::ledger::{BucketWidth, LedgerNew, UsageSummary};
use crate::pricing::Pricing;
use crate::provider::{Attempt, AttemptOutcome, Model, Provider, ProviderNew, ProviderPatch};
use crate::utils::now_unix_secs;

use super::{AppKey, Result};
use crate::provider::Pin;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS providers (
    id TEXT PRIMARY KEY,
    template_id TEXT,
    record TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_keys (
    id TEXT PRIMARY KEY,
    app TEXT NOT NULL,
    key TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE TABLE IF NOT EXISTS pins (
    app TEXT NOT NULL,
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    PRIMARY KEY (app, provider_id)
);

CREATE TABLE IF NOT EXISTS balance_snapshots (
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    amount INTEGER NOT NULL,
    currency TEXT NOT NULL DEFAULT 'USD',
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE INDEX IF NOT EXISTS idx_balance_provider_id_ts
    ON balance_snapshots(provider_id, created_at DESC);

CREATE TABLE IF NOT EXISTS prices (
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    model_id TEXT NOT NULL,
    pricing TEXT NOT NULL,
    PRIMARY KEY (provider_id, model_id)
);

CREATE TABLE IF NOT EXISTS attempts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    latency INTEGER NOT NULL,
    outcome TEXT NOT NULL,
    at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_attempts_provider_at
    ON attempts(provider_id, at DESC, id DESC);

CREATE TABLE IF NOT EXISTS ledger (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    app TEXT NOT NULL,
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    response_id TEXT,
    conversation_id TEXT,
    requested_protocol TEXT NOT NULL,
    requested_model TEXT NOT NULL,
    served_protocol TEXT NOT NULL,
    served_model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    input_price INTEGER,
    output_price INTEGER,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    cost INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_ledger_response_id
    ON ledger(response_id) WHERE response_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_ledger_conversation_id
    ON ledger(conversation_id) WHERE conversation_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_ledger_provider_id_created_at
    ON ledger(provider_id, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_ledger_created_at
    ON ledger(created_at);
";

// ---- helpers ----

fn write_count(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn read_count(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

fn placeholders(ids: &[&str]) -> String {
    ids.iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect::<Vec<_>>()
        .join(",")
}

fn to_params<'a>(ids: &'a [&'a str]) -> Vec<&'a dyn rusqlite::types::ToSql> {
    ids.iter()
        .map(|id| id as &'a dyn rusqlite::types::ToSql)
        .collect()
}

fn map_storage_error(e: rusqlite::Error) -> crate::db::Error {
    if e.sqlite_extended_error_code() == Some(rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY) {
        crate::db::Error::ReferencedRowMissing
    } else {
        e.into()
    }
}

// ---- type impls ----

impl rusqlite::types::FromSql for crate::protocol::Protocol {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        value
            .as_str()?
            .parse::<Self>()
            .map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(e)))
    }
}

impl rusqlite::types::ToSql for crate::protocol::Protocol {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.to_string()),
        ))
    }
}

// ---- schema ----

pub(super) fn init_schema(conn: &Connection) -> Result<()> {
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    let _ = conn.pragma_update(None, "foreign_keys", "ON");
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

// ---- settings ----

pub(super) fn stored_settings(conn: &Connection) -> Result<HashMap<String, String>> {
    let stored: HashMap<String, String> = conn
        .prepare("SELECT key, value FROM settings")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, rusqlite::Error>>()?;
    Ok(stored)
}

pub(super) fn set_settings(conn: &Connection, entries: &[(String, String)]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for (key, value) in entries {
        tx.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
    }
    tx.commit()?;

    Ok(())
}

// ---- providers ----

pub(super) fn get_provider(conn: &Connection, id: &str) -> Result<Option<Provider>> {
    conn.prepare("SELECT record FROM providers WHERE id = ?1")?
        .query_row(params![id], |r| r.get::<_, String>(0))
        .optional()?
        .map(|json| serde_json::from_str(&json).map_err(Into::into))
        .transpose()
}

pub(super) fn list_providers(conn: &Connection) -> Result<Vec<Provider>> {
    conn.prepare("SELECT record FROM providers")?
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|row| serde_json::from_str(&row?).map_err(Into::into))
        .collect()
}

fn insert_provider_row(conn: &Connection, p: &Provider) -> Result<bool> {
    p.validate()?;

    let record = serde_json::to_string(p)?;
    let inserted: Option<String> = conn
        .query_row(
            "INSERT INTO providers (id, template_id, record) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO NOTHING
             RETURNING id",
            params![p.id, p.template_id, record],
            |row| row.get(0),
        )
        .optional()?;

    Ok(inserted.is_some())
}

fn update_provider_row(conn: &Connection, p: &Provider) -> Result<()> {
    p.validate()?;

    let record = serde_json::to_string(p)?;
    conn.execute(
        "UPDATE providers SET template_id = ?2, record = ?3 WHERE id = ?1",
        params![p.id, p.template_id, record],
    )?;

    Ok(())
}

pub(super) fn insert_provider(conn: &Connection, new: ProviderNew) -> Result<Option<Provider>> {
    let id = match new.id {
        Some(id) => id,
        None => {
            let prefix = new
                .template_id
                .as_deref()
                .map_or_else(|| "p".to_string(), |t| format!("{t}-"));
            next_numbered_id(conn, &prefix)?
        }
    };

    let provider = Provider {
        id,
        template_id: new.template_id,
        key: new.key,
        name: new.name,
        base_url: new.base_url,
        models_url: new.models_url,
        balance_url: new.balance_url,
        protocols: new.protocols,
        models: Vec::new(),
    };

    Ok(insert_provider_row(conn, &provider)?.then_some(provider))
}

pub(super) fn update_provider(
    conn: &Connection,
    id: &str,
    patch: &ProviderPatch,
) -> Result<Option<Provider>> {
    let Some(mut p) = get_provider(conn, id)? else {
        return Ok(None);
    };

    p.apply_patch(patch)?;
    update_provider_row(conn, &p)?;
    Ok(Some(p))
}

pub(super) fn set_provider_models(conn: &Connection, id: &str, models: Vec<Model>) -> Result<()> {
    let Some(mut p) = get_provider(conn, id)? else {
        return Err(crate::db::Error::ReferencedRowMissing);
    };

    if models.as_slice() == p.models {
        return Ok(());
    }

    p.models = models;
    update_provider_row(conn, &p)
}

pub(super) fn delete_provider(conn: &Connection, id: &str) -> Result<bool> {
    let n = conn.execute("DELETE FROM providers WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

fn next_numbered_id(conn: &Connection, prefix: &str) -> Result<String> {
    let max: u32 = conn
        .prepare("SELECT id FROM providers WHERE id LIKE ?1")?
        .query_map(params![format!("{prefix}%")], |r| r.get(0))?
        .filter_map(|r| {
            r.ok()
                .and_then(|id: String| id.strip_prefix(prefix)?.parse().ok())
        })
        .max()
        .unwrap_or(0);
    Ok(format!("{prefix}{}", max + 1))
}

pub(super) fn refresh_stale_template_backed(conn: &Connection) -> Result<()> {
    for p in list_providers(conn)? {
        if let Some(current) = crate::provider::current_snapshot(&p)
            && current != p
        {
            update_provider_row(conn, &current)?;
        }
    }

    Ok(())
}

pub(super) fn get_provider_prices(
    conn: &Connection,
    provider_id: &str,
) -> Result<BTreeMap<String, Pricing>> {
    let mut stmt =
        conn.prepare_cached("SELECT model_id, pricing FROM prices WHERE provider_id = ?1")?;
    stmt.query_and_then([provider_id], |r| {
        let model_id: String = r.get(0)?;
        let pricing: Pricing = serde_json::from_str(&r.get::<_, String>(1)?).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
        })?;

        Ok((model_id, pricing))
    })?
    .collect()
}

// ---- balance ----

pub(super) fn batch_latest_balances(
    conn: &Connection,
    ids: &[&str],
) -> Result<HashMap<String, money::Money>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let ph = placeholders(ids);
    let sql = format!(
        "SELECT b.provider_id, b.amount, b.currency
         FROM balance_snapshots b
         INNER JOIN (
             SELECT provider_id, MAX(created_at) AS max_ts
             FROM balance_snapshots
             GROUP BY provider_id
         ) latest ON b.provider_id = latest.provider_id AND b.created_at = latest.max_ts
         WHERE b.provider_id IN ({ph})"
    );
    let mut stmt = conn.prepare(&sql)?;
    let params = to_params(ids);
    stmt.query_map(params.as_slice(), |r| {
        Ok((
            r.get::<_, String>(0)?,
            money::Money {
                amount: r.get(1)?,
                currency: r.get(2)?,
            },
        ))
    })?
    .collect::<std::result::Result<_, _>>()
    .map_err(Into::into)
}

pub(super) fn insert_balance(
    conn: &Connection,
    provider_id: &str,
    balance: &money::Money,
) -> Result<()> {
    conn.execute(
        "INSERT INTO balance_snapshots
            (provider_id, amount, currency)
         VALUES (?1, ?2, ?3)",
        params![provider_id, balance.amount, balance.currency,],
    )
    .map_err(map_storage_error)?;

    Ok(())
}

// ---- pins ----

fn pin(row: &rusqlite::Row<'_>) -> rusqlite::Result<Pin> {
    Ok(Pin {
        app: row.get(0)?,
        provider_id: row.get(1)?,
    })
}

pub(super) fn has_pin(conn: &Connection, app: &str, provider_id: &str) -> Result<bool> {
    conn.prepare("SELECT 1 FROM pins WHERE app = ?1 AND provider_id = ?2")?
        .query_row(params![app, provider_id], |_| Ok(()))
        .optional()
        .map(|row| row.is_some())
        .map_err(Into::into)
}

pub(super) fn insert_pin(conn: &Connection, app: &str, provider_id: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO pins (app, provider_id) VALUES (?1, ?2)",
        params![app, provider_id],
    )
    .map_err(map_storage_error)?;

    Ok(())
}

pub(super) fn delete_pin(conn: &Connection, app: &str, provider_id: &str) -> Result<bool> {
    let n = conn.execute(
        "DELETE FROM pins WHERE app = ?1 AND provider_id = ?2",
        params![app, provider_id],
    )?;
    Ok(n > 0)
}

pub(super) fn clear_pins(conn: &Connection, app: &str) -> Result<()> {
    conn.execute("DELETE FROM pins WHERE app = ?1", params![app])?;
    Ok(())
}

pub(super) fn clear_all_pins(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM pins", [])?;
    Ok(())
}

pub(super) fn list_pins(conn: &Connection) -> Result<Vec<Pin>> {
    conn.prepare("SELECT app, provider_id FROM pins ORDER BY app, rowid")?
        .query_map([], pin)?
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}

pub(super) fn pins_by_app(conn: &Connection, app: &str) -> Result<Vec<Pin>> {
    conn.prepare("SELECT app, provider_id FROM pins WHERE app = ?1 ORDER BY rowid")?
        .query_map(params![app], pin)?
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}

pub(super) fn pins_by_provider(conn: &Connection, provider_id: &str) -> Result<Vec<Pin>> {
    conn.prepare("SELECT app, provider_id FROM pins WHERE provider_id = ?1 ORDER BY rowid")?
        .query_map(params![provider_id], pin)?
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}

// ---- app keys ----

fn app_key(row: &rusqlite::Row<'_>) -> rusqlite::Result<AppKey> {
    Ok(AppKey {
        id: row.get(0)?,
        app: row.get(1)?,
        key: row.get(2)?,
        created_at: row.get(3)?,
    })
}

pub(super) fn app_by_key(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.prepare("SELECT app FROM app_keys WHERE key = ?1")?
        .query_row(params![key], |r| r.get::<_, String>(0))
        .optional()
        .map_err(Into::into)
}

pub(super) fn list_app_keys(conn: &Connection) -> Result<Vec<AppKey>> {
    conn.prepare("SELECT id, app, key, created_at FROM app_keys ORDER BY created_at, id")?
        .query([])?
        .mapped(app_key)
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}

pub(super) fn keys_by_app(conn: &Connection, app: &str) -> Result<Vec<AppKey>> {
    conn.prepare(
        "SELECT id, app, key, created_at FROM app_keys WHERE app = ?1 ORDER BY created_at, id",
    )?
    .query(params![app])?
    .mapped(app_key)
    .collect::<std::result::Result<_, _>>()
    .map_err(Into::into)
}

pub(super) fn get_app_key(conn: &Connection, id: &str) -> Result<Option<AppKey>> {
    conn.prepare("SELECT id, app, key, created_at FROM app_keys WHERE id = ?1")?
        .query_row(params![id], app_key)
        .optional()
        .map_err(Into::into)
}

pub(super) fn insert_app_key(conn: &Connection, app: &str, key: &str) -> Result<AppKey> {
    let id = format!("ak_{}", crate::utils::random_hex(8));
    conn.query_row(
        "INSERT INTO app_keys (id, app, key) VALUES (?1, ?2, ?3) RETURNING id, app, key, created_at",
        params![id, app, key],
        app_key,
    )
    .map_err(Into::into)
}

pub(super) fn delete_app_key(conn: &Connection, id: &str) -> Result<bool> {
    let n = conn.execute("DELETE FROM app_keys WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

// ---- response id ----

pub(super) fn provider_by_response(conn: &Connection, id: &str) -> Result<Option<String>> {
    conn.prepare("SELECT provider_id FROM ledger WHERE response_id = ?1 ORDER BY id DESC LIMIT 1")?
        .query_row(params![id], |r| r.get::<_, String>(0))
        .optional()
        .map_err(Into::into)
}

pub(super) fn provider_by_conversation(conn: &Connection, id: &str) -> Result<Option<String>> {
    conn.prepare(
        "SELECT provider_id FROM ledger WHERE conversation_id = ?1 ORDER BY id DESC LIMIT 1",
    )?
    .query_row(params![id], |r| r.get::<_, String>(0))
    .optional()
    .map_err(Into::into)
}

// ---- ledger ----

pub(super) fn insert_ledger(conn: &Connection, r: &LedgerNew) -> Result<()> {
    let cost = r.cost();

    conn.execute(
        "INSERT INTO ledger
            (app, provider_id, response_id, conversation_id, requested_protocol,
             requested_model, served_protocol, served_model, input_tokens, output_tokens,
             input_price, output_price, cost)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            r.app,
            r.provider_id,
            r.response_id,
            r.conversation_id,
            r.requested_protocol,
            r.requested_model,
            r.served_protocol,
            r.served_model,
            write_count(u64::from(r.usage.input_tokens)),
            write_count(u64::from(r.usage.output_tokens)),
            r.price.map(|p| p.input),
            r.price.map(|p| p.output),
            cost,
        ],
    )?;
    Ok(())
}

/// Ledger rows in `[start, end)`, folded per `(bucket_start, app, provider_id,
/// model)` into buckets `bucket_width` wide.
pub(super) fn usage_window(
    conn: &Connection,
    start: i64,
    end: i64,
    bucket_width: BucketWidth,
    app: Option<&str>,
    provider_id: Option<&str>,
) -> Result<Vec<UsageSummary>> {
    type UsageKey = (i64, String, String, Option<String>, String);
    type UsageAgg = (u64, u64, u64, money::Amount);

    let mut stmt = conn.prepare(
        "SELECT l.created_at, l.app, l.provider_id, json_extract(p.record, '$.name'),
                l.served_model, l.input_tokens, l.output_tokens, l.cost
         FROM ledger l
         LEFT JOIN providers p ON p.id = l.provider_id
         WHERE l.created_at >= ?1 AND l.created_at < ?2
           AND (?3 IS NULL OR l.app = ?3)
           AND (?4 IS NULL OR l.provider_id = ?4)",
    )?;

    let rows = stmt.query_map(params![start, end, app, provider_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
            read_count(r.get(5)?),
            read_count(r.get(6)?),
            r.get::<_, money::Amount>(7)?,
        ))
    })?;

    let mut agg: HashMap<UsageKey, UsageAgg> = HashMap::new();
    for row in rows {
        let (created_at, app, provider_id, name, model, input_tokens, output_tokens, cost) = row?;
        let start = bucket_width.start(created_at);
        let entry = agg
            .entry((start, app, provider_id, name, model))
            .or_default();
        entry.0 += 1;
        entry.1 += input_tokens;
        entry.2 += output_tokens;
        entry.3 = entry.3.checked_add(cost).unwrap_or(money::Amount::ZERO);
    }

    let mut out: Vec<UsageSummary> = agg
        .into_iter()
        .map(
            |(
                (bucket_start, app, provider_id, provider_name, served_model),
                (requests, input_tokens, output_tokens, cost),
            )| {
                UsageSummary {
                    bucket_start,
                    app,
                    provider_id,
                    provider_name,
                    served_model,
                    requests,
                    input_tokens,
                    output_tokens,
                    cost,
                }
            },
        )
        .collect();

    // Oldest bucket first, then a total order: the aggregation map has no
    // stable iteration order, and these rows are returned as they are.
    out.sort_by(|a, b| {
        a.bucket_start.cmp(&b.bucket_start).then_with(|| {
            (&a.app, &a.provider_id, &a.served_model).cmp(&(
                &b.app,
                &b.provider_id,
                &b.served_model,
            ))
        })
    });

    Ok(out)
}

pub(super) fn prune_ledger(conn: &Connection, retention_secs: i64) -> Result<()> {
    let cutoff = now_unix_secs() - retention_secs;
    conn.execute("DELETE FROM ledger WHERE created_at <= ?1", params![cutoff])?;
    Ok(())
}

// ---- attempts ----

/// Every retained attempt, oldest first within each provider — what the actor
/// replays into derived provider metrics at load.
pub(super) fn attempts_for_load(conn: &Connection) -> Result<Vec<Attempt>> {
    conn.prepare(
        "SELECT provider_id, latency, outcome, at FROM attempts
         ORDER BY provider_id, at, id",
    )?
    .query_map([], |r| {
        let stored: String = r.get(2)?;
        let outcome = AttemptOutcome::parse(&stored).ok_or_else(|| {
            rusqlite::Error::InvalidColumnType(2, "outcome".into(), rusqlite::types::Type::Text)
        })?;
        Ok(Attempt {
            provider_id: r.get(0)?,
            latency: read_count(r.get(1)?),
            outcome,
            at: r.get(3)?,
        })
    })?
    .collect::<std::result::Result<_, _>>()
    .map_err(Into::into)
}

pub(super) fn insert_attempt(conn: &Connection, a: &Attempt) -> Result<()> {
    conn.execute(
        "INSERT INTO attempts (provider_id, latency, outcome, at)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            a.provider_id,
            write_count(a.latency),
            a.outcome.as_str(),
            a.at
        ],
    )?;
    Ok(())
}

pub(super) fn prune_attempts(conn: &Connection, keep: u32) -> Result<()> {
    let providers: Vec<String> = conn
        .prepare("SELECT DISTINCT provider_id FROM attempts")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, rusqlite::Error>>()?;
    for pid in providers {
        conn.execute(
            "DELETE FROM attempts WHERE provider_id = ?1 AND id NOT IN
                (SELECT id FROM attempts WHERE provider_id = ?1
                 ORDER BY at DESC, id DESC LIMIT ?2)",
            params![pid, i64::from(keep)],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::seed_provider;
    use crate::protocol::Protocol;
    use crate::provider::{Model, Provider, ProviderPatch};
    use rusqlite::Connection;
    use std::collections::BTreeMap;

    fn patch(key: Option<&str>, name: Option<&str>, base_url: Option<&str>) -> ProviderPatch {
        ProviderPatch {
            key: key.map(str::to_string),
            name: name.map(str::to_string),
            base_url: base_url.map(str::to_string),
            models_url: None,
            balance_url: None,
            protocols: None,
        }
    }

    const DEEPSEEK_JSON: &str = r#"{
        "id": "deepseek",
        "name": "DeepSeek",
        "protocols": {
            "anthropic": "/anthropic",
            "openai_chat": {},
            "openai_responses": {}
        },
        "base_url": "https://api.deepseek.com",
        "key": "sk-test",
        "models": [
            {"id": "deepseek-chat"}
        ],
        "balance_url": "https://api.deepseek.com/user/balance"
    }"#;

    fn attempt(provider_id: &str, outcome: AttemptOutcome, latency: u64, at: i64) -> Attempt {
        Attempt {
            provider_id: provider_id.into(),
            outcome,
            latency,
            at,
        }
    }

    #[test]
    fn update_provider_merges_onto_the_current_record() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let p: Provider = serde_json::from_str(DEEPSEEK_JSON).unwrap();
        insert_provider_row(&conn, &p).unwrap();

        set_provider_models(
            &conn,
            "deepseek",
            vec![Model {
                id: "deepseek-v4".into(),
                name: None,
                context_window: None,
                reasoning: None,
            }],
        )
        .unwrap();
        let updated = update_provider(
            &conn,
            "deepseek",
            &patch(Some("sk-new"), Some("Renamed"), None),
        )
        .unwrap()
        .unwrap();
        assert_eq!(updated.key, "sk-new");
        assert_eq!(updated.name.as_deref(), Some("Renamed"));
        assert_eq!(updated.models.len(), 1);

        let again = update_provider(
            &conn,
            "deepseek",
            &patch(None, None, Some("https://api.deepseek.com/v2")),
        )
        .unwrap()
        .unwrap();
        assert_eq!(again.name.as_deref(), Some("Renamed"));
        assert_eq!(again.base_url, "https://api.deepseek.com/v2");
        assert_eq!(again.models.len(), 1);

        assert!(
            update_provider(&conn, "nope", &patch(Some("k"), None, None))
                .unwrap()
                .is_none()
        );
        let err = update_provider(&conn, "deepseek", &patch(None, None, Some(""))).unwrap_err();
        assert!(matches!(
            err,
            crate::db::Error::InvalidRecord(crate::provider::Error::EmptyBaseUrl { .. })
        ));
    }

    #[test]
    fn add_rejects_an_existing_id() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let p: Provider = serde_json::from_str(DEEPSEEK_JSON).unwrap();
        insert_provider_row(&conn, &p).unwrap();

        let duplicate = insert_provider(
            &conn,
            ProviderNew {
                id: Some("deepseek".into()),
                template_id: None,
                key: "k2".into(),
                name: None,
                base_url: "https://x.example.com".into(),
                models_url: None,
                balance_url: None,
                protocols: BTreeMap::new(),
            },
        )
        .unwrap();
        assert!(duplicate.is_none());
        assert_eq!(
            get_provider(&conn, "deepseek").unwrap().unwrap().key,
            "sk-test"
        );

        let err2 = insert_provider(
            &conn,
            crate::provider::ProviderNew::from_template("openai", "k3")
                .unwrap()
                .with_id(Some("deepseek".into())),
        )
        .unwrap();
        assert!(err2.is_none());
    }

    #[test]
    fn add_auto_numbers_by_source_and_preserves_fields() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();

        let template_free = |base: &str| ProviderNew {
            id: None,
            template_id: None,
            key: "k".into(),
            name: None,
            base_url: base.into(),
            models_url: None,
            balance_url: None,
            protocols: BTreeMap::new(),
        };
        let a = insert_provider(&conn, template_free("https://a.example.com"))
            .unwrap()
            .unwrap();
        assert_eq!(a.id, "p1");
        assert_eq!(a.base_url, "https://a.example.com");
        assert_eq!(a.key, "k");
        assert!(a.models.is_empty());
        let b = insert_provider(&conn, template_free("https://b.example.com"))
            .unwrap()
            .unwrap();
        assert_eq!(b.id, "p2");

        let t = insert_provider(
            &conn,
            crate::provider::ProviderNew::from_template("deepseek", "sk").unwrap(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(t.id, "deepseek-1");
        assert_eq!(t.template_id.as_deref(), Some("deepseek"));
        assert_eq!(t.key, "sk");
        assert!(t.models.is_empty());
    }

    #[test]
    fn add_rejects_an_explicit_empty_id() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let err = insert_provider(
            &conn,
            ProviderNew {
                id: Some(String::new()),
                template_id: None,
                key: "k".into(),
                name: None,
                base_url: "https://x.example.com".into(),
                models_url: None,
                balance_url: None,
                protocols: BTreeMap::new(),
            },
        )
        .unwrap_err();
        assert!(matches!(
            err,
            crate::db::Error::InvalidRecord(crate::provider::Error::EmptyProviderId)
        ));
    }

    #[test]
    fn prune_attempts_keeps_only_the_newest_per_provider() {
        let conn = Connection::open_in_memory().unwrap();

        init_schema(&conn).unwrap();
        seed_provider(&conn, "deepseek");
        let now = now_unix_secs();
        let keep = 10;
        for i in 0..(keep + 5) {
            insert_attempt(
                &conn,
                &attempt("deepseek", AttemptOutcome::Delivered, 10, now - (i as i64)),
            )
            .unwrap();
        }
        prune_attempts(&conn, keep).unwrap();
        let kept: i64 = conn
            .query_row("SELECT COUNT(*) FROM attempts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, i64::from(keep));
    }

    #[test]
    fn usage_window_orders_rows_deterministically() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        for id in ["p1", "p2"] {
            seed_provider(&conn, id);
        }
        let now = now_unix_secs();
        // equal cost, inserted out of order: the tie must break on the key
        for (app, provider, model) in [("b", "p1", "m"), ("a", "p2", "m"), ("a", "p1", "m")] {
            insert_ledger(
                &conn,
                &LedgerNew {
                    app: app.into(),
                    provider_id: provider.into(),
                    response_id: None,
                    conversation_id: None,
                    requested_protocol: Protocol::OpenaiChat,
                    requested_model: model.into(),
                    served_protocol: Protocol::OpenaiChat,
                    served_model: model.into(),
                    usage: crate::protocol::Usage {
                        input_tokens: 1,
                        output_tokens: 1,
                    },
                    price: Some(crate::pricing::Price {
                        input: "1".parse().unwrap(),
                        output: "1".parse().unwrap(),
                    }),
                },
            )
            .unwrap();
        }

        let keys: Vec<(String, String, String)> =
            usage_window(&conn, now - 60, now + 60, BucketWidth::Day, None, None)
                .unwrap()
                .into_iter()
                .map(|r| (r.app, r.provider_id, r.served_model))
                .collect();
        assert_eq!(
            keys,
            vec![
                ("a".to_string(), "p1".to_string(), "m".to_string()),
                ("a".to_string(), "p2".to_string(), "m".to_string()),
                ("b".to_string(), "p1".to_string(), "m".to_string()),
            ]
        );
    }
}
