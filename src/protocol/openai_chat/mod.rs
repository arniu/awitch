pub(crate) mod types;

#[cfg(test)]
mod tests;

pub use types::*;

use http::StatusCode;
use serde_json::Value;

use crate::protocol::{Spec, StreamCodec};

#[derive(Clone)]
pub struct OpenaiChat;

impl Spec for OpenaiChat {
    fn error(&self, status: StatusCode, message: &str) -> Value {
        serde_json::to_value(types::Error::new(status, message))
            .expect("a modelled error serializes")
    }

    fn extract_usage(&self, response: &Value) -> crate::protocol::Usage {
        crate::protocol::usage_from::<types::Usage>(response)
    }
}

impl StreamCodec for OpenaiChat {
    type Item = types::stream::Chunk;

    fn observe_usage(&self, acc: &mut crate::protocol::Usage, item: &Self::Item) {
        // The stream's usage lands in the final chunk.
        if let Some(u) = &item.usage {
            *acc = (*u).into();
        }
    }
}
