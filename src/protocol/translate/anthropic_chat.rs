use serde_json::Value;

use crate::protocol::Error;
use crate::protocol::openai_chat::{self as openai};

use crate::protocol::anthropic::Anthropic;
use crate::protocol::anthropic::stream::{Delta, Event, MessageDeltaData, MessageDeltaUsage};
use crate::protocol::anthropic::{
    Content, ContentBlock, ContentBlockParam, Request, Response, Role, ToolChoice, Usage,
};
use crate::protocol::translate::hub;

use crate::protocol::translate::{StreamTranslate, Translate};

#[derive(Clone)]
pub struct AnthropicToChat;

impl Translate for AnthropicToChat {
    fn transform_request(&self, req: Value) -> Result<Value, Error> {
        let req: Request = serde_json::from_value(req)?;
        let hub = anthropic_to_openai(req)?;
        serde_json::to_value(&hub).map_err(Error::Json)
    }

    fn transform_response(&self, resp: Value) -> Result<Value, Error> {
        let resp = openai_to_anthropic(resp)?;
        serde_json::to_value(&resp).map_err(Error::Json)
    }
}

impl StreamTranslate<Anthropic> for AnthropicToChat {
    type State = AnthropicStreamState;

    fn transform_event(&self, state: &mut Self::State, event: &hub::Event) -> Vec<Event> {
        state.on_event(event)
    }
}

fn stop_reason(r: &str) -> String {
    match r {
        "tool_calls" => "tool_use".into(),
        "stop" => "end_turn".into(),
        "length" => "max_tokens".into(),
        "content_filter" => "refusal".into(),
        other => other.to_string(),
    }
}

fn anthropic_to_openai(req: Request) -> Result<openai::Request, Error> {
    if req.thinking.is_some() {
        return Err(Error::ThinkingUnsupported);
    }

    let mut messages: Vec<openai::Message> = Vec::new();

    if let Some(system) = &req.system {
        let text = system.text();
        if !text.trim().is_empty() {
            messages.push(system_msg(text));
        }
    }

    for m in &req.messages {
        match m.role {
            Role::System => {
                let text = match &m.content {
                    Content::Text(text) => text.clone(),
                    Content::Blocks(blocks) => block_text(blocks),
                };
                if !text.trim().is_empty() {
                    messages.push(system_msg(text));
                }
            }
            Role::User => match &m.content {
                Content::Text(t) => messages.push(user_msg(openai::Content::String(t.clone()))),
                Content::Blocks(blocks) => {
                    for block in blocks {
                        match block {
                            ContentBlockParam::Text { text } => {
                                messages.push(user_msg(openai::Content::String(text.clone())))
                            }
                            ContentBlockParam::ToolResult {
                                tool_use_id,
                                content,
                                ..
                            } => messages.push(openai::Message::Tool {
                                content: openai::Content::String(tool_result_text(content)),
                                tool_call_id: tool_use_id.clone(),
                            }),
                            _ => {}
                        }
                    }
                }
            },
            Role::Assistant => match &m.content {
                Content::Text(t) => {
                    messages.push(assistant_msg(Some(t.clone()), None));
                }
                Content::Blocks(blocks) => {
                    let mut text: Option<String> = None;
                    let mut tool_calls: Vec<openai::ToolCall> = Vec::new();
                    for block in blocks {
                        match block {
                            ContentBlockParam::Text { text: t } => {
                                let cur = text.get_or_insert_with(String::new);
                                if !cur.is_empty() {
                                    cur.push('\n');
                                }
                                cur.push_str(t);
                            }
                            ContentBlockParam::ToolUse { id, name, input } => {
                                tool_calls.push(openai::ToolCall {
                                    id: id.clone(),
                                    type_: "function".into(),
                                    function: openai::Function {
                                        name: name.clone(),
                                        arguments: serde_json::to_string(input)
                                            .unwrap_or_else(|_| "{}".into()),
                                    },
                                });
                            }
                            _ => {}
                        }
                    }
                    if text.is_some() || !tool_calls.is_empty() {
                        messages.push(assistant_msg(
                            text,
                            if tool_calls.is_empty() {
                                None
                            } else {
                                Some(tool_calls)
                            },
                        ));
                    }
                }
            },
        }
    }

    Ok(openai::Request {
        model: String::new(),
        messages,
        max_tokens: Some(req.max_tokens),
        temperature: req.temperature,
        top_p: req.top_p,
        stop: req.stop_sequences.map(openai::StopSeq::Multiple),
        stream: req.stream,
        stream_options: (req.stream == Some(true)).then_some(openai::StreamOptions {
            include_usage: Some(true),
        }),
        tools: req.tools.map(|tools| {
            tools
                .into_iter()
                .map(|t| openai::Tool {
                    type_: "function".into(),
                    function: openai::FunctionDef {
                        name: t.name,
                        description: t.description,
                        parameters: Some(t.input_schema),
                    },
                })
                .collect()
        }),
        tool_choice: req.tool_choice.map(translate_tool_choice),
    })
}

fn system_msg(text: String) -> openai::Message {
    openai::Message::System {
        content: openai::Content::String(text),
    }
}

fn block_text(blocks: &[ContentBlockParam]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlockParam::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn user_msg(content: openai::Content) -> openai::Message {
    openai::Message::User { content }
}

fn assistant_msg(
    content: Option<String>,
    tool_calls: Option<Vec<openai::ToolCall>>,
) -> openai::Message {
    openai::Message::Assistant {
        content: content.map(openai::Content::String),
        tool_calls,
    }
}

fn tool_result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|i| i.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

fn translate_tool_choice(c: ToolChoice) -> Value {
    match c {
        ToolChoice::Auto => serde_json::json!("auto"),
        ToolChoice::Any => serde_json::json!("required"),
        ToolChoice::None => serde_json::json!("none"),
        ToolChoice::Tool { name } => serde_json::json!({
            "type": "function",
            "function": { "name": name }
        }),
    }
}

fn openai_to_anthropic(resp: Value) -> Result<Response, Error> {
    let resp: openai::Response = serde_json::from_value(resp)?;

    let mut content: Vec<ContentBlock> = Vec::new();
    let mut stop: Option<String> = None;
    if let Some(choice) = resp.choices.into_iter().next() {
        if let Some(text) = choice.message.content
            && !text.trim().is_empty()
        {
            content.push(ContentBlock::Text { text });
        }
        if let Some(tool_calls) = choice.message.tool_calls {
            for tc in tool_calls {
                let input: Value = serde_json::from_str(&tc.function.arguments)
                    .unwrap_or_else(|_| serde_json::json!({}));
                content.push(ContentBlock::ToolUse {
                    id: tc.id,
                    name: tc.function.name,
                    input,
                });
            }
        }
        stop = Some(stop_reason(&choice.finish_reason));
    }

    let usage = resp.usage.unwrap_or_default();

    Ok(Response {
        type_: "message".into(),
        id: resp.id,
        role: "assistant".into(),
        content,
        model: resp.model,
        stop_reason: stop,
        stop_sequence: None,
        usage: map_usage(&usage),
    })
}

fn map_usage(u: &openai::Usage) -> Usage {
    let details = u.prompt_tokens_details;
    let cache_read = details.map(|d| d.cached_tokens);
    let cache_write = details.map(|d| d.cache_write_tokens);
    Usage {
        input_tokens: u
            .prompt_tokens
            .saturating_sub(cache_read.unwrap_or(0) + cache_write.unwrap_or(0)),
        output_tokens: u.completion_tokens,
        cache_creation_input_tokens: cache_write,
        cache_read_input_tokens: cache_read,
    }
}

#[derive(Default)]
pub struct AnthropicStreamState {
    started: bool,
    done: bool,
    stop_reason: Option<String>,
    id: String,
    model: String,
    usage: Option<Usage>,
    next_block: u32,
    text_open: bool,
    open_tool: Option<(String, u32)>,
}

impl AnthropicStreamState {
    fn on_event(&mut self, event: &hub::Event) -> Vec<Event> {
        if self.done {
            return Vec::new();
        }

        match event {
            hub::Event::Meta { id, model } => {
                if self.started {
                    return Vec::new();
                }
                self.started = true;
                self.id = id.clone();
                self.model = model.clone();
                vec![Event::MessageStart {
                    message: Response {
                        type_: "message".into(),
                        id: self.id.clone(),
                        role: "assistant".into(),
                        content: Vec::new(),
                        model: self.model.clone(),
                        stop_reason: None,
                        stop_sequence: None,
                        usage: Usage::default(),
                    },
                }]
            }
            hub::Event::TextDelta { delta } => self.on_text(delta),
            hub::Event::ToolCallStart { id, name } => self.on_tool_start(id, name),
            hub::Event::ToolCallArgs { id, delta } => self.on_tool_args(id, delta),
            hub::Event::Usage { usage } => {
                self.usage = Some(map_usage(usage));
                Vec::new()
            }
            hub::Event::Finish { reason } => {
                self.stop_reason = reason.clone();
                self.finish()
            }
        }
    }

    fn on_text(&mut self, delta: &str) -> Vec<Event> {
        let mut out = Vec::new();
        self.close_tool(&mut out);
        if !self.text_open {
            self.text_open = true;
            out.push(Event::ContentBlockStart {
                index: self.next_block,
                content_block: ContentBlock::Text {
                    text: String::new(),
                },
            });
            self.next_block += 1;
        }
        out.push(Event::ContentBlockDelta {
            index: self.next_block - 1,
            delta: Delta::TextDelta {
                text: delta.to_string(),
            },
        });
        out
    }

    fn on_tool_start(&mut self, id: &str, name: &str) -> Vec<Event> {
        let mut out = Vec::new();
        self.close_text(&mut out);
        self.close_tool(&mut out);
        let block_index = self.next_block;
        self.next_block += 1;
        self.open_tool = Some((id.to_string(), block_index));
        out.push(Event::ContentBlockStart {
            index: block_index,
            content_block: ContentBlock::ToolUse {
                id: id.to_string(),
                name: name.to_string(),
                input: Value::Object(Default::default()),
            },
        });
        out
    }

    fn on_tool_args(&mut self, id: &str, delta: &str) -> Vec<Event> {
        let mut out = Vec::new();
        if let Some((open_id, block)) = &self.open_tool
            && open_id == id
        {
            out.push(Event::ContentBlockDelta {
                index: *block,
                delta: Delta::InputJsonDelta {
                    partial_json: delta.to_string(),
                },
            });
        }
        out
    }

    fn close_text(&mut self, out: &mut Vec<Event>) {
        if self.text_open {
            self.text_open = false;
            out.push(Event::ContentBlockStop {
                index: self.next_block - 1,
            });
        }
    }

    fn close_tool(&mut self, out: &mut Vec<Event>) {
        if let Some((_, block)) = self.open_tool.take() {
            out.push(Event::ContentBlockStop { index: block });
        }
    }

    fn finish(&mut self) -> Vec<Event> {
        if self.done {
            return Vec::new();
        }
        self.done = true;
        let mut out = Vec::new();
        self.close_text(&mut out);
        self.close_tool(&mut out);
        out.push(Event::MessageDelta {
            delta: MessageDeltaData {
                stop_reason: self.stop_reason.as_deref().map(stop_reason),
                stop_sequence: None,
            },
            usage: MessageDeltaUsage {
                input_tokens: self.usage.map(|u| u.input_tokens),
                output_tokens: self.usage.map_or(0, |u| u.output_tokens),
                cache_creation_input_tokens: self.usage.and_then(|u| u.cache_creation_input_tokens),
                cache_read_input_tokens: self.usage.and_then(|u| u.cache_read_input_tokens),
            },
        });
        out.push(Event::MessageStop);
        out
    }
}
