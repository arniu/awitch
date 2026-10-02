use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::required_nullable;

/// `stop` is a string, or a list of strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StopSeq {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<StopSeq>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_options: Option<StreamOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_usage: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum Message {
    System {
        content: Content,
    },
    User {
        content: Content,
    },
    Assistant {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<Content>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ToolCall>>,
    },
    Tool {
        content: Content,
        tool_call_id: String,
    },
}

/// `content` is a string, or a list of parts (text / image_url -- keeps
/// text; other parts pass through as-is).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    String(String),
    Parts(Vec<Value>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub function: Function,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    /// OpenAI serializes arguments as a JSON **string**.
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "type")]
    pub type_: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: String,
    pub model: String,
    pub choices: Vec<Choice>,
    #[serde(default)]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub message: ResponseMessage,
    pub finish_reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResponseMessage {
    #[serde(deserialize_with = "required_nullable::deserialize")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct PromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: u32,
    #[serde(default)]
    pub cache_write_tokens: u32,
}

impl From<Usage> for crate::protocol::Usage {
    fn from(u: Usage) -> Self {
        crate::protocol::Usage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        }
    }
}

pub mod stream {
    use serde::{Deserialize, Serialize};

    use crate::protocol::required_nullable;

    /// One OpenAI SSE `data:` line for chat completions streaming.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Chunk {
        pub id: String,
        pub model: String,
        pub choices: Vec<ChunkChoice>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub usage: Option<super::Usage>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ChunkChoice {
        pub index: u32,
        pub delta: Delta,
        #[serde(deserialize_with = "required_nullable::deserialize")]
        pub finish_reason: Option<String>,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct Delta {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub tool_calls: Option<Vec<ToolCallDelta>>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ToolCallDelta {
        pub index: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub function: Option<FunctionDelta>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct FunctionDelta {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub arguments: Option<String>,
    }
}

use http::StatusCode;

#[expect(
    clippy::enum_variant_names,
    reason = "the postfix is part of the serialized error name"
)]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorType {
    InvalidRequestError,
    AuthenticationError,
    RateLimitError,
    ServerError,
}

impl ErrorType {
    pub fn from_status(status: StatusCode) -> Self {
        match status {
            StatusCode::BAD_REQUEST => ErrorType::InvalidRequestError,
            StatusCode::UNAUTHORIZED => ErrorType::AuthenticationError,
            StatusCode::TOO_MANY_REQUESTS => ErrorType::RateLimitError,
            _ => ErrorType::ServerError,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    pub r#type: ErrorType,
    pub message: String,
    pub param: Option<String>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Error {
    pub error: ErrorBody,
}

impl Error {
    pub fn new(status: StatusCode, message: &str) -> Self {
        Self {
            error: ErrorBody {
                r#type: ErrorType::from_status(status),
                message: message.to_string(),
                param: None,
                code: None,
            },
        }
    }
}
