use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::required_nullable;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input: Vec<InputItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

/// `EasyInputMessage` -- the request's `input` message item; the schema requires
/// `role` and `content` and leaves its `type` optional.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputMessage {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    pub role: String,
    pub content: Value,
}

/// `FunctionToolCall`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionToolCall {
    #[serde(rename = "type")]
    pub type_: String,
    pub call_id: String,
    pub name: String,
    pub arguments: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// `FunctionCallOutputItemParam`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallOutput {
    #[serde(rename = "type")]
    pub type_: String,
    pub output: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
}

/// A member of the `Item` union that awitch does not model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnmodelledItem {
    #[serde(rename = "type")]
    pub type_: String,
}

/// An item of the request's `input`. The protocol discriminates the union by
/// `type`; a message may omit it, so the members are told apart by shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InputItem {
    Message(InputMessage),
    FunctionCall(FunctionToolCall),
    FunctionCallOutput(FunctionCallOutput),
    Unmodelled(UnmodelledItem),
}

/// `OutputMessage`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub role: String,
    pub content: Vec<Value>,
    pub status: String,
}

/// An item of the response's `output`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutputItem {
    Message(OutputMessage),
    FunctionCall(FunctionToolCall),
    Unmodelled(UnmodelledItem),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub type_: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: String,
    pub object: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub model: String,
    pub output: Vec<OutputItem>,
    #[serde(deserialize_with = "required_nullable::deserialize")]
    pub incomplete_details: Option<IncompleteDetails>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<Conversation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncompleteDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub input_tokens_details: InputTokensDetails,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct InputTokensDetails {
    pub cached_tokens: u32,
    pub cache_write_tokens: u32,
}

impl From<Usage> for crate::protocol::Usage {
    fn from(u: Usage) -> Self {
        crate::protocol::Usage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
        }
    }
}

pub mod stream {
    use serde::{Deserialize, Serialize};

    use super::{OutputItem, Response};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type")]
    pub enum Event {
        #[serde(rename = "response.created")]
        Created { response: Response },
        #[serde(rename = "response.output_item.added")]
        OutputItemAdded { output_index: u32, item: OutputItem },
        #[serde(rename = "response.content_part.added")]
        ContentPartAdded {
            item_id: String,
            output_index: u32,
            content_index: u32,
            part: Part,
        },
        #[serde(rename = "response.output_text.delta")]
        OutputTextDelta {
            item_id: String,
            output_index: u32,
            content_index: u32,
            delta: String,
        },
        #[serde(rename = "response.output_text.done")]
        OutputTextDone {
            item_id: String,
            output_index: u32,
            content_index: u32,
            text: String,
        },
        #[serde(rename = "response.content_part.done")]
        ContentPartDone {
            item_id: String,
            output_index: u32,
            content_index: u32,
            part: Part,
        },
        #[serde(rename = "response.output_item.done")]
        OutputItemDone { output_index: u32, item: OutputItem },
        #[serde(rename = "response.function_call_arguments.delta")]
        FunctionCallArgumentsDelta {
            item_id: String,
            output_index: u32,
            delta: String,
        },
        #[serde(rename = "response.function_call_arguments.done")]
        FunctionCallArgumentsDone {
            item_id: String,
            output_index: u32,
            arguments: String,
        },
        #[serde(rename = "response.completed")]
        Completed { response: Response },
        #[serde(rename = "response.incomplete")]
        Incomplete { response: Response },
    }

    impl Event {
        pub fn event_type(&self) -> &'static str {
            match self {
                Event::Created { .. } => "response.created",
                Event::OutputItemAdded { .. } => "response.output_item.added",
                Event::ContentPartAdded { .. } => "response.content_part.added",
                Event::OutputTextDelta { .. } => "response.output_text.delta",
                Event::OutputTextDone { .. } => "response.output_text.done",
                Event::ContentPartDone { .. } => "response.content_part.done",
                Event::OutputItemDone { .. } => "response.output_item.done",
                Event::FunctionCallArgumentsDelta { .. } => {
                    "response.function_call_arguments.delta"
                }
                Event::FunctionCallArgumentsDone { .. } => "response.function_call_arguments.done",
                Event::Completed { .. } => "response.completed",
                Event::Incomplete { .. } => "response.incomplete",
            }
        }
    }

    /// `OutputTextContent`
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Part {
        #[serde(rename = "type")]
        pub type_: String,
        pub text: String,
        pub annotations: Vec<serde_json::Value>,
    }
}
