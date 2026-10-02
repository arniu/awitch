use std::borrow::Cow;
use std::collections::HashSet;

use anyhow::{Context, anyhow, bail};
use serde_json::Value;

use crate::protocol::Protocol;
use crate::provider::{Model, Provider};

fn parse_models(body: &Value) -> anyhow::Result<Vec<Model>> {
    let entries = match body {
        Value::Object(map) => match map.get("data") {
            Some(Value::Array(items)) => items,
            _ => return Err(anyhow!("expected an object with a 'data' array")),
        },
        Value::Array(items) => items,
        _ => {
            return Err(anyhow!(
                "expected an object with a 'data' array or a bare array"
            ));
        }
    };

    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for entry in entries {
        let Some(obj) = entry.as_object() else {
            continue;
        };
        let Some(Value::String(id)) = obj.get("id") else {
            continue;
        };
        let id = id.trim();
        if id.is_empty() || !seen.insert(id.to_string()) {
            continue;
        }
        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string);
        models.push(Model {
            id: id.to_string(),
            name,
            context_window: None,
            reasoning: None,
        });
    }
    Ok(models)
}

/// Fetch one provider's model catalog from its `models_url`.
pub(crate) async fn fetch_models(
    client: &reqwest::Client,
    p: &Provider,
) -> anyhow::Result<Vec<Model>> {
    let url = p
        .models_url()
        .ok_or_else(|| anyhow!("provider '{}' has no models_url configured", p.id))?;

    let resp = client
        .get(&url)
        .bearer_auth(&p.key)
        .send()
        .await
        .context("models request failed")?;
    if !resp.status().is_success() {
        bail!("models endpoint returned {}", resp.status());
    }

    let value: Value = resp.json().await.context("bad models json")?;
    parse_models(&value)
}

/// A provider's account balance from its declared balance_url.
pub(crate) async fn poll_balance(
    client: &reqwest::Client,
    provider: &Provider,
) -> anyhow::Result<money::Money> {
    let url = provider
        .balance_url()
        .ok_or_else(|| anyhow!("provider '{}' has no balance_url configured", provider.id))?;
    let resp = client
        .get(&url)
        .bearer_auth(&provider.key)
        .send()
        .await
        .context("balance request failed")?;
    if !resp.status().is_success() {
        bail!("balance endpoint returned {}", resp.status());
    }
    let json: Value = resp.json().await.context("bad balance json")?;
    let balance = extract_balance(&json)
        .with_context(|| format!("no numeric balance field found in {json}"))?;
    Ok(money::Money {
        amount: balance,
        currency: extract_currency(&json),
    })
}

/// Dig for a numeric balance anywhere in the vendor's response.
fn extract_balance(v: &Value) -> Option<money::Amount> {
    fn walk(v: &Value, preferred: bool) -> Option<money::Amount> {
        match v {
            Value::Object(map) => {
                for (k, val) in map {
                    let is_preferred = matches!(
                        k.as_str(),
                        "total_balance" | "available_balance" | "total_credits"
                    );
                    let key_matches = if preferred {
                        is_preferred
                    } else {
                        is_preferred || k.contains("balance")
                    };
                    if key_matches && let Some(n) = as_amount(val) {
                        return Some(n);
                    }
                }
                for val in map.values() {
                    if let Some(n) = walk(val, preferred) {
                        return Some(n);
                    }
                }
                None
            }
            Value::Array(items) => items.iter().find_map(|i| walk(i, preferred)),
            _ => None,
        }
    }
    walk(v, true).or_else(|| walk(v, false))
}

/// Exact decimal parse of a JSON number or string.
fn as_amount(v: &Value) -> Option<money::Amount> {
    let s = match v {
        Value::Number(n) => Cow::Owned(n.to_string()),
        Value::String(s) => Cow::Borrowed(s),
        _ => return None,
    };

    s.parse().ok()
}

/// Best-effort currency from the vendor's balance response.
fn extract_currency(v: &Value) -> money::Currency {
    fn walk(v: &Value) -> Option<&str> {
        match v {
            Value::Object(map) => {
                for (k, val) in map {
                    if k.eq_ignore_ascii_case("currency")
                        && let Some(c) = val.as_str()
                    {
                        return Some(c);
                    }
                    if let Some(c) = walk(val) {
                        return Some(c);
                    }
                }
                None
            }
            Value::Array(items) => items.iter().find_map(walk),
            _ => None,
        }
    }
    walk(v)
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(money::Currency::Usd)
}

/// The probe targets of a provider: each served protocol's endpoint URL.
fn targets(p: &Provider) -> Vec<(Protocol, String)> {
    p.protocols
        .keys()
        .map(|proto| {
            (
                *proto,
                p.url_for(*proto)
                    .expect("a served protocol resolves to a URL"),
            )
        })
        .collect()
}

/// Probe a provider's served protocols. The blocking client (and its timeout)
/// is built at the CLI seam, never here.
pub(crate) fn probe_endpoints(
    client: &reqwest::blocking::Client,
    p: &Provider,
) -> Vec<(Protocol, String, u16)> {
    let code = |req: reqwest::blocking::RequestBuilder| {
        req.send().ok().map(|r| r.status().as_u16()).unwrap_or(0)
    };
    targets(p)
        .into_iter()
        .map(|(proto, url)| {
            // GET with the key; a GET 404 falls back to a credential-less
            // empty POST (POST-only paths hide behind GET 404).
            let mut status = code(client.get(&url).bearer_auth(&p.key));
            if status == 404 {
                status = code(client.post(&url).json(&serde_json::json!({})));
            }
            (proto, url, status)
        })
        .collect()
}
