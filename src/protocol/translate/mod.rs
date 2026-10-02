pub mod anthropic_chat;
pub mod hub;
pub mod responses_chat;
pub mod stream;

#[cfg(test)]
mod tests;

use axum::body::Bytes;
use futures_util::stream::Stream;
use serde_json::Value;

use crate::protocol::anthropic::Anthropic;
use crate::protocol::openai_chat::OpenaiChat;
use crate::protocol::openai_responses::OpenaiResponses;
use crate::protocol::{Error, Metadata, Protocol, StreamCodec};

pub trait Translate: Send + Sync + 'static {
    fn transform_request(&self, req: Value) -> Result<Value, Error>;
    fn transform_response(&self, resp: Value) -> Result<Value, Error>;
}

pub trait StreamTranslate<S: StreamCodec>: Translate + Clone {
    type State: Default + Send + 'static;

    fn transform_event(&self, state: &mut Self::State, event: &hub::Event) -> Vec<S::Item>;
}

pub(crate) enum Pair {
    Same(Protocol),
    AnthropicToChat,
    ResponsesToChat,
}

impl Pair {
    pub(crate) fn resolve(requested: Protocol, served: Protocol) -> Pair {
        match (requested, served) {
            (a, b) if a == b => Pair::Same(a),
            (Protocol::Anthropic, Protocol::OpenaiChat) => Pair::AnthropicToChat,
            (Protocol::OpenaiResponses, Protocol::OpenaiChat) => Pair::ResponsesToChat,
            _ => unreachable!("unsupported protocol pair: {requested} → {served}"),
        }
    }

    fn requested(&self) -> Protocol {
        match self {
            Pair::Same(p) => *p,
            Pair::AnthropicToChat => Protocol::Anthropic,
            Pair::ResponsesToChat => Protocol::OpenaiResponses,
        }
    }

    fn served(&self) -> Protocol {
        match self {
            Pair::Same(p) => *p,
            Pair::AnthropicToChat | Pair::ResponsesToChat => Protocol::OpenaiChat,
        }
    }

    pub(crate) fn transform_request(&self, req: Value) -> Result<Value, Error> {
        match self {
            Pair::Same(_) => Ok(req),
            Pair::AnthropicToChat => anthropic_chat::AnthropicToChat.transform_request(req),
            Pair::ResponsesToChat => responses_chat::ResponsesToChat.transform_request(req),
        }
    }

    fn transform_response(&self, resp: Value) -> Result<Value, Error> {
        match self {
            Pair::Same(_) => Ok(resp),
            Pair::AnthropicToChat => anthropic_chat::AnthropicToChat.transform_response(resp),
            Pair::ResponsesToChat => responses_chat::ResponsesToChat.transform_response(resp),
        }
    }

    pub(crate) fn serve(&self, upstream: Bytes) -> Result<(Metadata, Bytes), Error> {
        let served_body: Value = serde_json::from_slice(&upstream)?;
        let usage = self.served().spec().extract_usage(&served_body);
        let client = self.transform_response(served_body)?;
        let metadata = Metadata {
            usage,
            response_id: self.requested().spec().extract_response_id(&client),
            conversation_id: self.requested().spec().extract_conversation_id(&client),
        };
        let bytes = match self {
            Pair::Same(_) => upstream,
            _ => Bytes::from(serde_json::to_vec(&client)?),
        };
        Ok((metadata, bytes))
    }

    pub(crate) async fn serve_stream<E>(
        &self,
        raw_stream: impl Stream<Item = Result<Bytes, E>> + Send + Unpin + 'static,
    ) -> Result<stream::StreamOutcome, E>
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        match self {
            Pair::Same(Protocol::Anthropic) => stream::forward_stream(Anthropic, raw_stream).await,
            Pair::Same(Protocol::OpenaiChat) => {
                stream::forward_stream(OpenaiChat, raw_stream).await
            }
            Pair::Same(Protocol::OpenaiResponses) => {
                stream::forward_stream(OpenaiResponses, raw_stream).await
            }
            Pair::AnthropicToChat => {
                stream::translate_stream(
                    Anthropic,
                    OpenaiChat,
                    anthropic_chat::AnthropicToChat,
                    raw_stream,
                )
                .await
            }
            Pair::ResponsesToChat => {
                stream::translate_stream(
                    OpenaiResponses,
                    OpenaiChat,
                    responses_chat::ResponsesToChat,
                    raw_stream,
                )
                .await
            }
        }
    }
}
