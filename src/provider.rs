use std::collections::BTreeMap;

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::protocol::Protocol;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("provider id must not be empty")]
    EmptyProviderId,
    #[error("provider '{id}': base_url must not be empty")]
    EmptyBaseUrl { id: String },
    #[error("provider '{id}': base_url must be an http(s) URL, got {url:?}")]
    NonHttpBaseUrl { id: String, url: String },
    #[error("provider '{id}': {protocol} endpoint path {path:?} must start with '/'")]
    BadEndpointPath {
        id: String,
        protocol: Protocol,
        path: String,
    },
    #[error("unknown provider template '{id}'")]
    UnknownTemplate { id: String },
    #[error("provider '{id}' diverged from its template")]
    DivergedFromTemplate { id: String },
}

/// A pin: an app is pinned to a provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Pin {
    pub(crate) app: String,
    pub(crate) provider_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Path(String),
    Canonical,
}

impl Endpoint {
    pub fn resolve(&self, protocol: Protocol) -> &str {
        match self {
            Endpoint::Canonical => canonical_path(protocol),
            Endpoint::Path(path) => path,
        }
    }
}

fn canonical_path(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Anthropic => "/v1/messages",
        Protocol::OpenaiChat => "/v1/chat/completions",
        Protocol::OpenaiResponses => "/v1/responses",
    }
}

impl Serialize for Endpoint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Endpoint::Path(path) => serializer.serialize_str(path),
            Endpoint::Canonical => serializer.serialize_map(Some(0))?.end(),
        }
    }
}

impl<'de> Deserialize<'de> for Endpoint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Endpoint;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "an endpoint path string or an empty table")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(Endpoint::Path(value.to_string()))
            }

            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(Endpoint::Path(value))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                if map
                    .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                    .is_some()
                {
                    Err(serde::de::Error::custom("expected an empty table"))
                } else {
                    Ok(Endpoint::Canonical)
                }
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderPatch {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub models_url: Option<String>,
    #[serde(default)]
    pub balance_url: Option<String>,
    #[serde(default)]
    pub protocols: Option<BTreeMap<Protocol, Endpoint>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub reasoning: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    #[serde(default)]
    pub template_id: Option<String>,
    pub key: String,
    #[serde(default)]
    pub name: Option<String>,
    pub base_url: String,
    #[serde(default)]
    pub models_url: Option<String>,
    #[serde(default)]
    pub balance_url: Option<String>,
    #[serde(default)]
    pub protocols: BTreeMap<Protocol, Endpoint>,
    #[serde(default)]
    pub models: Vec<Model>,
}

/// A provider to create.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderNew {
    pub id: Option<String>,
    pub template_id: Option<String>,
    pub key: String,
    pub name: Option<String>,
    pub base_url: String,
    pub models_url: Option<String>,
    pub balance_url: Option<String>,
    pub protocols: BTreeMap<Protocol, Endpoint>,
}

impl ProviderNew {
    pub(crate) fn from_template(tid: &str, key: &str) -> Result<Self, Error> {
        let template = builtin_template(tid).ok_or_else(|| Error::UnknownTemplate {
            id: tid.to_string(),
        })?;

        Ok(Self {
            id: None,
            template_id: Some(tid.to_string()),
            key: key.to_string(),
            name: template.name.clone(),
            base_url: template.base_url.clone(),
            models_url: template.models_url.clone(),
            balance_url: template.balance_url.clone(),
            protocols: template.protocols.clone(),
        })
    }

    pub(crate) fn with_id(mut self, id: Option<String>) -> Self {
        self.id = id;
        self
    }
}

impl Provider {
    /// The URL a served protocol is reached at — `None` when the provider
    /// does not serve the protocol: an absent family never fabricates a URL.
    pub(crate) fn url_for(&self, protocol: Protocol) -> Option<String> {
        self.protocols.get(&protocol).map(|endpoint| {
            let path = endpoint.resolve(protocol);
            join_url(&self.base_url, path)
        })
    }

    /// The absolute account-balance endpoint: the authored `balance_url`
    /// resolved against `base_url`.
    pub(crate) fn balance_url(&self) -> Option<String> {
        self.balance_url
            .as_deref()
            .map(|path| join_url(&self.base_url, path))
    }

    /// The absolute model-list endpoint: the authored `models_url` resolved
    /// against `base_url`.
    pub(crate) fn models_url(&self) -> Option<String> {
        self.models_url
            .as_deref()
            .map(|path| join_url(&self.base_url, path))
    }

    /// A record's structural well-formedness.
    pub fn validate(&self) -> Result<(), Error> {
        if self.id.trim().is_empty() {
            return Err(Error::EmptyProviderId);
        }

        if self.base_url.trim().is_empty() {
            return Err(Error::EmptyBaseUrl {
                id: self.id.clone(),
            });
        }

        if !(self.base_url.starts_with("http://") || self.base_url.starts_with("https://")) {
            return Err(Error::NonHttpBaseUrl {
                id: self.id.clone(),
                url: self.base_url.clone(),
            });
        }

        for (protocol, endpoint) in &self.protocols {
            let path = endpoint.resolve(*protocol);

            if !path.starts_with('/') {
                return Err(Error::BadEndpointPath {
                    id: self.id.clone(),
                    protocol: *protocol,
                    path: path.to_string(),
                });
            }
        }

        // template write gate: a template-backed record stays a pure snapshot
        if let Some(current) = current_snapshot(self)
            && current != *self
        {
            return Err(Error::DivergedFromTemplate {
                id: self.id.clone(),
            });
        }

        Ok(())
    }

    pub fn apply_patch(&mut self, patch: &ProviderPatch) -> Result<(), Error> {
        if let Some(k) = &patch.key {
            self.key = k.clone();
        }

        if let Some(n) = &patch.name {
            self.name = Some(n.clone());
        }

        if let Some(u) = &patch.base_url {
            self.base_url = u.clone();
        }

        if let Some(u) = &patch.models_url {
            self.models_url = Some(u.clone());
        }

        if let Some(u) = &patch.balance_url {
            self.balance_url = Some(u.clone());
        }

        if let Some(protocols) = &patch.protocols {
            self.protocols = protocols.clone();
        }

        Ok(())
    }
}

/// Resolve an endpoint path against `base_url` by standard URL resolution.
fn join_url(base: &str, path: &str) -> String {
    Url::parse(base)
        .and_then(|base| base.join(path))
        .expect("a validated http(s) base resolves any path")
        .to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ProviderMetrics {
    pub balance: Option<money::Money>,
    pub consecutive_failures: u32,
    pub p50_latency: Option<u32>,
    /// When the newest observation was taken — the cooldown is judged against
    /// it (ADR-0009). Zero = never observed.
    pub last_attempt_at: i64,
}

/// How one forward attempt ended, attributed. Only `Failed` (the provider's
/// fault) counts against the cooldown streak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    Delivered,
    Failed,
    Aborted,
}

impl AttemptOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            AttemptOutcome::Delivered => "delivered",
            AttemptOutcome::Failed => "failed",
            AttemptOutcome::Aborted => "aborted",
        }
    }

    /// The inverse of `as_str` — `None` for a value the gateway never writes.
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Delivered, Self::Failed, Self::Aborted]
            .into_iter()
            .find(|outcome| outcome.as_str() == s)
    }
}

/// One forward attempt's terminal observation — the provider-status signal.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Attempt {
    pub(crate) provider_id: String,
    pub(crate) outcome: AttemptOutcome,
    pub(crate) latency: u64,
    pub(crate) at: i64,
}

// ---- provider templates -----------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderTemplate {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub base_url: String,
    #[serde(default)]
    pub models_url: Option<String>,
    #[serde(default)]
    pub balance_url: Option<String>,
    /// Served protocols — required.
    pub protocols: BTreeMap<Protocol, Endpoint>,
}

impl ProviderTemplate {
    /// Parse a template's file text — a built-in file or a local definition.
    pub(crate) fn parse(raw: &str, template_id: &str) -> Result<ProviderTemplate, toml::de::Error> {
        let mut template: ProviderTemplate = toml::from_str(raw)?;
        template.id = template_id.to_string();
        Ok(template)
    }
}

pub(crate) fn snapshot(template: &ProviderTemplate, provider_id: &str, key: &str) -> Provider {
    Provider {
        id: provider_id.to_string(),
        template_id: Some(template.id.clone()),
        key: key.to_string(),
        name: template.name.clone(),
        base_url: template.base_url.clone(),
        models_url: template.models_url.clone(),
        balance_url: template.balance_url.clone(),
        protocols: template.protocols.clone(),
        models: Vec::new(),
    }
}

pub(crate) fn current_snapshot(provider: &Provider) -> Option<Provider> {
    let template = builtin_template(provider.template_id.as_deref()?)?;
    let mut current = snapshot(&template, &provider.id, &provider.key);
    current.models = provider.models.clone();
    Some(current)
}

/// Every built-in template id, in file order.
pub(crate) fn builtin_template_ids() -> impl Iterator<Item = &'static str> {
    BUILTIN_TEMPLATE_FILES.iter().map(|(id, _)| *id)
}

/// Every built-in template, in file order.
pub(crate) fn builtin_templates() -> impl Iterator<Item = ProviderTemplate> {
    builtin_template_ids().filter_map(builtin_template)
}

/// Resolve a built-in template by its id.
pub(crate) fn builtin_template(id: &str) -> Option<ProviderTemplate> {
    builtin_template_file(id)
        .map(|raw| ProviderTemplate::parse(raw, id).expect("built-in template must parse"))
}

/// A built-in template file's raw text, for `provider template <id>` export.
pub(crate) fn builtin_template_file(id: &str) -> Option<&'static str> {
    BUILTIN_TEMPLATE_FILES
        .iter()
        .find(|(name, _)| *name == id)
        .map(|(_, raw)| *raw)
}

pub(crate) fn is_builtin_template(id: &str) -> bool {
    builtin_template_file(id).is_some()
}

// ---- template file internals ----------------------------------------------

macro_rules! builtin_template {
    ($id:literal) => {
        (
            $id,
            include_str!(concat!("../assets/provider_templates/", $id, ".toml")),
        )
    };
}

const BUILTIN_TEMPLATE_FILES: &[(&str, &str)] = &[
    builtin_template!("anthropic"),
    builtin_template!("deepseek"),
    builtin_template!("meta"),
    builtin_template!("moonshotai"),
    builtin_template!("moonshotai-coding"),
    builtin_template!("openai"),
    builtin_template!("openrouter"),
    builtin_template!("qwen"),
    builtin_template!("siliconflow"),
    builtin_template!("siliconflow-intl"),
    builtin_template!("x-ai"),
    builtin_template!("xiaomi"),
    builtin_template!("xiaomi-token-plan"),
    builtin_template!("zhipu"),
    builtin_template!("zhipu-intl"),
];
