//! The control-plane API types.
//!
//! Request bodies take `#[serde(deny_unknown_fields)]`, response types do not.
//! A caller-built body is either right or a bug — a typo in an optional field,
//! or a field belonging to another resource — and a silently dropped key hides
//! it, so it must fail loudly. A response has to tolerate fields added later.

use std::collections::BTreeMap;

use money::Money;
use serde::{Deserialize, Serialize};

use crate::ledger::BucketWidth;
use crate::protocol::Protocol;
use crate::provider::{Endpoint, Provider, ProviderMetrics, ProviderNew};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct List<T> {
    pub data: Vec<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health<'a> {
    pub version: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderView {
    #[serde(flatten)]
    pub provider: Provider,
    pub metrics: ProviderMetrics,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageQuery {
    /// An instant inside the page to report, unix seconds.
    pub from: i64,
    pub bucket_width: BucketWidth,
}

/// One page of usage: the buckets covering `[start, end)`, oldest first.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageReport {
    pub start: i64,
    pub end: i64,
    pub bucket_width: BucketWidth,
    pub buckets: Vec<UsageBucket>,
}

/// One time bucket of the page; empty buckets are reported too.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageBucket {
    pub start: i64,
    pub end: i64,
    pub rows: Vec<UsageRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRow {
    pub app: String,
    pub provider_id: String,
    /// The provider's display name, when it has one.
    pub provider_name: Option<String>,
    pub served_model: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost: Money,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderAdd {
    #[serde(default)]
    pub id: Option<String>,
    pub key: String,
    #[serde(default)]
    pub name: Option<String>,
    pub base_url: String,
    #[serde(default)]
    pub models_url: Option<String>,
    #[serde(default)]
    pub balance_url: Option<String>,
    pub protocols: BTreeMap<Protocol, Endpoint>,
}

impl From<ProviderAdd> for ProviderNew {
    fn from(add: ProviderAdd) -> Self {
        Self {
            id: add.id,
            template_id: None,
            key: add.key,
            name: add.name,
            base_url: add.base_url,
            models_url: add.models_url,
            balance_url: add.balance_url,
            protocols: add.protocols,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateInstantiate {
    #[serde(default)]
    pub id: Option<String>,
    pub key: String,
}

/// The filters an app-key listing takes.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppKeysQuery {
    /// Only this app's keys.
    #[serde(default)]
    pub app: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppKeyIssue {
    pub app: String,
}

/// The create response: the only time `key` is returned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppKeyIssued {
    pub id: String,
    pub app: String,
    pub key: String,
    pub created_at: String,
    pub last4: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppKeyView {
    pub id: String,
    pub app: String,
    pub created_at: String,
    pub last4: String,
}
