pub(crate) mod types;

#[cfg(test)]
mod tests;

pub use types::*;

use http::StatusCode;
use serde_json::Value;

use crate::protocol::{Spec, StreamCodec};

#[derive(Clone)]
pub struct Anthropic;

impl Spec for Anthropic {
    fn auth_headers(&self, key: &str) -> Vec<(String, String)> {
        vec![
            ("x-api-key".into(), key.into()),
            ("anthropic-version".into(), "2023-06-01".into()),
        ]
    }

    fn error(&self, status: StatusCode, message: &str) -> Value {
        serde_json::to_value(types::Error::new(status, message))
            .expect("a modelled error serializes")
    }

    fn extract_usage(&self, response: &Value) -> crate::protocol::Usage {
        crate::protocol::usage_from::<types::Usage>(response)
    }
}

impl StreamCodec for Anthropic {
    type Item = types::stream::Event;

    fn event_name(&self, item: &Self::Item) -> Option<&'static str> {
        Some(item.event_type())
    }

    fn observe_usage(&self, acc: &mut crate::protocol::Usage, item: &Self::Item) {
        // message_start seeds input_tokens; message_delta carries the cumulative
        // authoritative counts, overwriting each field it reports. Anthropic's
        // input_tokens excludes the cache counts, which total input includes.
        match item {
            types::stream::Event::MessageStart { message } => {
                acc.input_tokens = message.usage.total_input_tokens();
            }

            types::stream::Event::MessageDelta { usage, .. } => {
                if let Some(input) = usage.total_input_tokens() {
                    acc.input_tokens = input;
                }

                acc.output_tokens = usage.output_tokens;
            }
            _ => {}
        }
    }
}
