pub(crate) mod anthropic;
pub(crate) mod error;
pub(crate) mod openai_chat;
pub(crate) mod openai_responses;
pub(crate) mod required_nullable;
pub(crate) mod translate;

#[cfg(test)]
pub(crate) mod corpus;
#[cfg(test)]
pub(crate) mod test_support;

use http::StatusCode;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use error::Error;

#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Metadata {
    pub usage: Usage,
    pub response_id: Option<String>,
    pub conversation_id: Option<String>,
}

pub enum Continuation {
    Response(String),
    Conversation(String),
}

// ── Spec trait ──────────────────────────────────────────

pub trait Spec: Send + Sync + 'static {
    fn auth_headers(&self, key: &str) -> Vec<(String, String)> {
        vec![("authorization".into(), format!("Bearer {key}"))]
    }

    fn error(&self, status: StatusCode, message: &str) -> Value;

    fn extract_error_message(&self, error: &Value) -> Option<String> {
        error
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    fn continuation(&self, _request: &Value) -> Option<Continuation> {
        None
    }

    fn extract_usage(&self, response: &Value) -> Usage;

    fn extract_response_id(&self, _response: &Value) -> Option<String> {
        None
    }

    fn extract_conversation_id(&self, _response: &Value) -> Option<String> {
        None
    }
}

// ── StreamCodec trait ────────────────────────────────────

pub trait StreamCodec: Spec + Clone {
    type Item: Clone + Send + Serialize + DeserializeOwned + 'static;

    fn event_name(&self, _item: &Self::Item) -> Option<&'static str> {
        None
    }

    fn encode_item(&self, item: &Self::Item) -> (Option<&str>, String) {
        (
            self.event_name(item),
            serde_json::to_string(item).expect("a modelled item serializes"),
        )
    }

    fn decode_item(&self, event: Option<&str>, data: &[u8]) -> Option<Self::Item> {
        serde_json::from_slice(data)
            .map_err(|error| tracing::trace!(event = ?event, error = %error, "cannot decode frame"))
            .ok()
    }

    fn observe_usage(&self, acc: &mut Usage, item: &Self::Item);

    fn observe_response_id(&self, _acc: &mut Option<String>, _item: &Self::Item) {}

    fn observe_conversation_id(&self, _acc: &mut Option<String>, _item: &Self::Item) {}

    fn observe(&self, meta: &mut Metadata, item: &Self::Item) {
        self.observe_usage(&mut meta.usage, item);
        self.observe_response_id(&mut meta.response_id, item);
        self.observe_conversation_id(&mut meta.conversation_id, item);
    }
}

// ── Protocol ─────────────────────────────────────────────

pub type ProtocolSpec = dyn Spec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Anthropic,
    OpenaiChat,
    OpenaiResponses,
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Protocol::Anthropic => "anthropic",
            Protocol::OpenaiChat => "openai_chat",
            Protocol::OpenaiResponses => "openai_responses",
        })
    }
}

impl std::str::FromStr for Protocol {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "anthropic" => Ok(Protocol::Anthropic),
            "openai_chat" => Ok(Protocol::OpenaiChat),
            "openai_responses" => Ok(Protocol::OpenaiResponses),
            other => Err(Error::UnknownProtocol {
                name: other.to_string(),
            }),
        }
    }
}

impl Protocol {
    pub fn spec(&self) -> &'static ProtocolSpec {
        match self {
            Protocol::Anthropic => &anthropic::Anthropic,
            Protocol::OpenaiChat => &openai_chat::OpenaiChat,
            Protocol::OpenaiResponses => &openai_responses::OpenaiResponses,
        }
    }
}

pub(crate) fn usage_from<T>(response: &Value) -> Usage
where
    T: DeserializeOwned + Default + Into<Usage>,
{
    serde_json::from_value::<T>(response.get("usage").cloned().unwrap_or_default())
        .unwrap_or_default()
        .into()
}
