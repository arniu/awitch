use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::required_nullable;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<System>,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Value>,
}

/// `system` is a string, or a list of text blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum System {
    Text(String),
    Blocks(Vec<TextBlock>),
}

impl System {
    /// The concatenated system text (text blocks only).
    pub fn text(&self) -> String {
        match self {
            System::Text(s) => s.clone(),
            System::Blocks(blocks) => blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextBlock {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Content,
}

/// `content` is a string, or a list of content blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Blocks(Vec<ContentBlockParam>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlockParam {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: Value,
        #[serde(default)]
        is_error: Option<bool>,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    Any,
    None,
    Tool { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    #[serde(rename = "type")]
    pub type_: String,
    pub id: String,
    pub role: String,
    pub content: Vec<ContentBlock>,
    pub model: String,
    #[serde(deserialize_with = "required_nullable::deserialize")]
    pub stop_reason: Option<String>,
    #[serde(deserialize_with = "required_nullable::deserialize")]
    pub stop_sequence: Option<String>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
}

impl Usage {
    pub fn total_input_tokens(&self) -> u32 {
        self.input_tokens
            + self.cache_creation_input_tokens.unwrap_or(0)
            + self.cache_read_input_tokens.unwrap_or(0)
    }
}

impl From<Usage> for crate::protocol::Usage {
    fn from(u: Usage) -> Self {
        crate::protocol::Usage {
            input_tokens: u.total_input_tokens(),
            output_tokens: u.output_tokens,
        }
    }
}

pub mod stream {
    use serde::{Deserialize, Serialize};

    use super::{ContentBlock, Response};
    use crate::protocol::required_nullable;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum Event {
        MessageStart {
            message: Response,
        },
        ContentBlockStart {
            index: u32,
            content_block: ContentBlock,
        },
        ContentBlockDelta {
            index: u32,
            delta: Delta,
        },
        ContentBlockStop {
            index: u32,
        },
        MessageDelta {
            delta: MessageDeltaData,
            usage: MessageDeltaUsage,
        },
        MessageStop,
    }

    impl Event {
        pub fn event_type(&self) -> &'static str {
            match self {
                Event::MessageStart { .. } => "message_start",
                Event::ContentBlockStart { .. } => "content_block_start",
                Event::ContentBlockDelta { .. } => "content_block_delta",
                Event::ContentBlockStop { .. } => "content_block_stop",
                Event::MessageDelta { .. } => "message_delta",
                Event::MessageStop => "message_stop",
            }
        }
    }

    #[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
    pub struct MessageDeltaUsage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub input_tokens: Option<u32>,
        pub output_tokens: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub cache_creation_input_tokens: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub cache_read_input_tokens: Option<u32>,
    }

    impl MessageDeltaUsage {
        pub fn total_input_tokens(&self) -> Option<u32> {
            self.input_tokens.map(|input| {
                input
                    + self.cache_creation_input_tokens.unwrap_or(0)
                    + self.cache_read_input_tokens.unwrap_or(0)
            })
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum Delta {
        TextDelta { text: String },
        InputJsonDelta { partial_json: String },
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MessageDeltaData {
        #[serde(deserialize_with = "required_nullable::deserialize")]
        pub stop_reason: Option<String>,
        #[serde(deserialize_with = "required_nullable::deserialize")]
        pub stop_sequence: Option<String>,
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
    PermissionError,
    NotFoundError,
    RateLimitError,
    ApiError,
    OverloadedError,
}

impl ErrorType {
    pub fn from_status(status: StatusCode) -> Self {
        match status {
            StatusCode::BAD_REQUEST => ErrorType::InvalidRequestError,
            StatusCode::UNAUTHORIZED => ErrorType::AuthenticationError,
            StatusCode::FORBIDDEN => ErrorType::PermissionError,
            StatusCode::NOT_FOUND => ErrorType::NotFoundError,
            StatusCode::TOO_MANY_REQUESTS => ErrorType::RateLimitError,
            _ if status.as_u16() == 529 => ErrorType::OverloadedError,
            _ => ErrorType::ApiError,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    pub r#type: ErrorType,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Error {
    pub r#type: &'static str,
    pub error: ErrorBody,
}

impl Error {
    pub fn new(status: StatusCode, message: &str) -> Self {
        Self {
            r#type: "error",
            error: ErrorBody {
                r#type: ErrorType::from_status(status),
                message: message.to_string(),
            },
        }
    }
}
