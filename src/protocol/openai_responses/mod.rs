pub(crate) mod types;

#[cfg(test)]
mod tests;

pub use types::*;

use http::StatusCode;
use serde_json::Value;

use crate::protocol::{Continuation, Spec, StreamCodec};

#[derive(Clone)]
pub struct OpenaiResponses;

impl Spec for OpenaiResponses {
    fn error(&self, status: StatusCode, message: &str) -> Value {
        serde_json::to_value(crate::protocol::openai_chat::Error::new(status, message))
            .expect("a modelled error serializes")
    }

    fn extract_usage(&self, response: &Value) -> crate::protocol::Usage {
        crate::protocol::usage_from::<types::Usage>(response)
    }

    fn extract_response_id(&self, response: &Value) -> Option<String> {
        response
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    fn extract_conversation_id(&self, response: &Value) -> Option<String> {
        response
            .get("conversation")
            .and_then(|c| c.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    fn continuation(&self, request: &Value) -> Option<Continuation> {
        if let Some(id) = request.get("previous_response_id").and_then(Value::as_str) {
            return Some(Continuation::Response(id.to_string()));
        }
        let conversation = request.get("conversation").filter(|v| !v.is_null())?;
        let id = conversation
            .as_str()
            .or_else(|| conversation.get("id").and_then(Value::as_str))?;
        Some(Continuation::Conversation(id.to_string()))
    }
}

impl StreamCodec for OpenaiResponses {
    type Item = types::stream::Event;

    fn event_name(&self, item: &Self::Item) -> Option<&'static str> {
        Some(item.event_type())
    }

    fn observe_usage(&self, acc: &mut crate::protocol::Usage, item: &Self::Item) {
        match item {
            types::stream::Event::Completed { response }
            | types::stream::Event::Incomplete { response } => {
                *acc = response.usage.unwrap_or_default().into();
            }
            _ => {}
        }
    }

    fn observe_response_id(&self, acc: &mut Option<String>, item: &Self::Item) {
        if let types::stream::Event::Created { response } = item {
            *acc = Some(response.id.clone());
        }
    }

    fn observe_conversation_id(&self, acc: &mut Option<String>, item: &Self::Item) {
        if let types::stream::Event::Created { response } = item {
            *acc = response.conversation.as_ref().map(|c| c.id.clone());
        }
    }
}
