//! The cc-switch source adapter.
//!
//! Source location: a SQLite db, by default `~/.cc-switch/cc-switch.db`.
//!
//! Data schema: a `providers` table — `id`, `app_type`, `name`,
//! `settings_config`. `app_type` names the app whose settings payload the row
//! carries; `settings_config` is that payload, shaped per app — Claude's `env`,
//! Codex's `config` TOML, Hermes's flat fields, OpenCode's `npm` package plus
//! `options`. Rows for an app whose payload shape is not implemented are
//! reported with the read, never silent.

use std::path::Path;

use anyhow::{Context, bail};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Map, Value};

use super::{Provider as SourceProvider, Source};
use crate::protocol::Protocol;

struct ParsedRow {
    vendor: String,
    base_url: String,
    key: String,
    protocol: Protocol,
}

/// Read the cc-switch pool: `db` if given, else the default
/// `~/.cc-switch/cc-switch.db` (missing there is fatal — pass --db). Returns
/// one `Source` per distinct vendor, each with providers grouped by
/// (base_url, key). Per-row parse failures are logged, never silent.
pub(crate) fn read(db: Option<&Path>) -> anyhow::Result<Vec<Source>> {
    let path = match db {
        Some(path) => path.to_path_buf(),
        None => {
            let path = dirs::home_dir()
                .unwrap_or_default()
                .join(".cc-switch/cc-switch.db");
            if !path.exists() {
                bail!("source db not found at {} (pass --db)", path.display());
            }
            path
        }
    };
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open source db {}", path.display()))?;
    conn.pragma_update(None, "query_only", true)
        .with_context(|| "source db read-only guard")?;

    let mut stmt = conn
        .prepare("SELECT id, app_type, name, settings_config FROM providers")
        .with_context(|| "source db: query providers")?;
    let iter = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;

    // (vendor_name, base_url, key, protocol) — one per parsed row.
    let mut parsed: Vec<ParsedRow> = Vec::new();
    for item in iter {
        let (id, app_type, name, raw) = item.with_context(|| "source db: read provider row")?;
        match parse_row(&app_type, &name, &raw) {
            Ok(row) => parsed.push(row),
            Err(reason) => tracing::warn!("cc-switch row {id}/{app_type}: {reason:#}"),
        }
    }

    // Group by (base_url, key), merging protocols, preserving first-seen order.
    let mut groups: Vec<(String, String, String, Vec<Protocol>)> = Vec::new();
    for row in &parsed {
        match groups
            .iter_mut()
            .find(|g| g.1 == row.base_url && g.2 == row.key)
        {
            Some(g) => {
                if !g.3.contains(&row.protocol) {
                    g.3.push(row.protocol);
                }
            }
            None => groups.push((
                row.vendor.clone(),
                row.base_url.clone(),
                row.key.clone(),
                vec![row.protocol],
            )),
        }
    }

    // Group providers by vendor, preserving first-seen order.
    let mut sources: Vec<Source> = Vec::new();
    for (vendor, base_url, key, protocols) in groups {
        let sp = SourceProvider {
            base_url,
            key,
            protocols,
        };
        match sources
            .iter_mut()
            .find(|s: &&mut Source| s.vendor == vendor)
        {
            Some(s) => s.providers.push(sp),
            None => sources.push(Source {
                vendor,
                providers: vec![sp],
            }),
        }
    }

    Ok(sources)
}

fn parse_row(app_type: &str, name: &str, raw: &str) -> anyhow::Result<ParsedRow> {
    let v: Value = serde_json::from_str(raw).context("settings payload is not JSON")?;
    let (base_url, key, protocol) = match app_type {
        "claude" => claude(&v)?,
        "codex" => codex(&v)?,
        "hermes" => hermes(&v)?,
        "opencode" => opencode(&v)?,
        other => bail!("unsupported app shape: {other}"),
    };
    Ok(ParsedRow {
        vendor: name.to_string(),
        base_url,
        key,
        protocol,
    })
}

/// Claude Code row: the settings payload's `env` block (app-pointing/claude.md) —
/// base URL + bearer token. The model-alias env keys are not read: the model
/// catalog is runtime data the gateway syncs, never carried from the source.
fn claude(v: &Value) -> anyhow::Result<(String, String, Protocol)> {
    let env = v
        .get("env")
        .and_then(Value::as_object)
        .context("no env block")?;
    let base_url = field(env, "ANTHROPIC_BASE_URL")?;
    let key = env
        .get("ANTHROPIC_AUTH_TOKEN")
        .and_then(Value::as_str)
        .or_else(|| env.get("ANTHROPIC_API_KEY").and_then(Value::as_str))
        .context("no ANTHROPIC_AUTH_TOKEN / ANTHROPIC_API_KEY")?
        .to_string();
    Ok((base_url, key, Protocol::Anthropic))
}

/// Codex row: `auth` key + the current `[model_providers.<id>]` TOML section
/// (base URL, protocol).
fn codex(v: &Value) -> anyhow::Result<(String, String, Protocol)> {
    let auth = v
        .get("auth")
        .and_then(Value::as_object)
        .context("no auth block")?;
    let key = field(auth, "OPENAI_API_KEY")?;
    let cfg_raw = v
        .get("config")
        .and_then(Value::as_str)
        .context("no config")?;
    let cfg: toml::Value = toml::from_str(cfg_raw).context("config is not TOML")?;
    let provider = cfg
        .get("model_provider")
        .and_then(toml::Value::as_str)
        .context("no model_provider")?;
    let section = cfg
        .get("model_providers")
        .and_then(toml::Value::as_table)
        .and_then(|t| t.get(provider))
        .and_then(toml::Value::as_table)
        .with_context(|| format!("no [model_providers.{provider}]"))?;
    let base_url = section
        .get("base_url")
        .and_then(toml::Value::as_str)
        .context("no base_url")?
        .to_string();
    let protocol = match section.get("wire_api").and_then(toml::Value::as_str) {
        Some("responses") => Protocol::OpenaiResponses,
        Some("chat") => Protocol::OpenaiChat,
        Some(other) => bail!("unknown wire_api {other:?}"),
        None => bail!("no wire_api"),
    };
    Ok((base_url, key, protocol))
}

/// Hermes row: a flat payload (base URL, key, `api_mode`).
fn hermes(v: &Value) -> anyhow::Result<(String, String, Protocol)> {
    let base_url = v
        .get("base_url")
        .and_then(Value::as_str)
        .context("no base_url")?
        .to_string();
    let key = v
        .get("api_key")
        .and_then(Value::as_str)
        .context("no api_key")?
        .to_string();
    let protocol = match v.get("api_mode").and_then(Value::as_str) {
        Some("chat_completions") => Protocol::OpenaiChat,
        Some("responses") => Protocol::OpenaiResponses,
        Some("anthropic") | Some("messages") => Protocol::Anthropic,
        Some(other) => bail!("unknown api_mode {other:?}"),
        None => bail!("no api_mode"),
    };
    Ok((base_url, key, protocol))
}

/// OpenCode row: the SDK package (`npm`) picks the protocol; the
/// endpoint + key live in `options`.
fn opencode(v: &Value) -> anyhow::Result<(String, String, Protocol)> {
    let protocol = match v.get("npm").and_then(Value::as_str) {
        Some("@ai-sdk/openai-compatible") => Protocol::OpenaiChat,
        Some("@ai-sdk/openai") => Protocol::OpenaiResponses,
        Some("@ai-sdk/anthropic") => Protocol::Anthropic,
        Some(other) => bail!("unknown npm package {other:?}"),
        None => bail!("no npm package"),
    };
    let options = v
        .get("options")
        .and_then(Value::as_object)
        .context("no options block")?;
    let base_url = field(options, "baseURL")?;
    let key = field(options, "apiKey")?;
    Ok((base_url, key, protocol))
}

fn field(obj: &Map<String, Value>, key: &str) -> anyhow::Result<String> {
    obj.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .with_context(|| format!("no {key}"))
}
