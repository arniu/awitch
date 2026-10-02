use std::collections::BTreeMap;

use serde_json::Value;

use crate::protocol::Error;
use crate::protocol::openai_chat::{self as openai};

use crate::protocol::openai_responses::OpenaiResponses;
use crate::protocol::openai_responses::stream::{Event, Part};
use crate::protocol::openai_responses::{
    FunctionToolCall, IncompleteDetails, InputItem, InputTokensDetails, OutputItem, OutputMessage,
    Request, Response, Usage,
};
use crate::protocol::translate::hub;

use crate::protocol::translate::{StreamTranslate, Translate};

#[derive(Clone)]
pub struct ResponsesToChat;

impl Translate for ResponsesToChat {
    fn transform_request(&self, req: Value) -> Result<Value, Error> {
        let req: Request = serde_json::from_value(req)?;
        let hub = responses_to_chat(req)?;
        serde_json::to_value(&hub).map_err(Error::Json)
    }

    fn transform_response(&self, resp: Value) -> Result<Value, Error> {
        let resp = chat_to_responses(resp)?;
        serde_json::to_value(&resp).map_err(Error::Json)
    }
}

impl StreamTranslate<OpenaiResponses> for ResponsesToChat {
    type State = ResponsesStreamState;

    fn transform_event(&self, state: &mut Self::State, event: &hub::Event) -> Vec<Event> {
        state.on_event(event)
    }
}

fn responses_to_chat(req: Request) -> Result<openai::Request, Error> {
    let mut messages: Vec<openai::Message> = Vec::new();

    if let Some(instructions) = &req.instructions
        && !instructions.trim().is_empty()
    {
        messages.push(system_msg(instructions));
    }

    for item in &req.input {
        match item {
            InputItem::Message(message) => {
                let content = openai::Content::String(content_text(&message.content));
                messages.push(match message.role.as_str() {
                    "developer" | "system" => openai::Message::System { content },
                    "assistant" => openai::Message::Assistant {
                        content: Some(content),
                        tool_calls: None,
                    },
                    _ => openai::Message::User { content },
                });
            }
            InputItem::FunctionCall(call) => {
                messages.push(openai::Message::Assistant {
                    content: None,
                    tool_calls: Some(vec![openai::ToolCall {
                        id: call.call_id.clone(),
                        type_: "function".into(),
                        function: openai::Function {
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                        },
                    }]),
                });
            }
            InputItem::FunctionCallOutput(output) => {
                messages.push(openai::Message::Tool {
                    content: openai::Content::String(content_text(&output.output)),
                    tool_call_id: output.call_id.clone().unwrap_or_default(),
                });
            }
            InputItem::Unmodelled(_) => {}
        }
    }

    Ok(openai::Request {
        model: String::new(),
        messages,
        max_tokens: None,
        temperature: None,
        top_p: None,
        stop: None,
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
                        parameters: Some(t.parameters),
                    },
                })
                .collect()
        }),
        tool_choice: None,
    })
}

enum Status {
    Completed,
    Incomplete { reason: Option<&'static str> },
}

impl Status {
    fn from_finish_reason(finish_reason: &str) -> Status {
        match finish_reason {
            "length" => Status::Incomplete {
                reason: Some("max_output_tokens"),
            },
            "content_filter" => Status::Incomplete {
                reason: Some("content_filter"),
            },
            _ => Status::Completed,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Status::Completed => "completed",
            Status::Incomplete { .. } => "incomplete",
        }
    }

    fn incomplete_details(&self) -> Option<IncompleteDetails> {
        match self {
            Status::Incomplete { reason } => Some(IncompleteDetails {
                reason: reason.map(str::to_string),
            }),
            Status::Completed => None,
        }
    }
}

fn chat_to_responses(resp: Value) -> Result<Response, Error> {
    let resp: openai::Response = serde_json::from_value(resp)?;

    let choice = resp.choices.into_iter().next();
    let status = choice
        .as_ref()
        .map(|choice| Status::from_finish_reason(&choice.finish_reason))
        .unwrap_or(Status::Completed);

    let mut output: Vec<OutputItem> = Vec::new();
    if let Some(choice) = choice {
        if let Some(text) = choice.message.content
            && !text.trim().is_empty()
        {
            output.push(message_item(status.name(), text_content(&text)));
        }
        if let Some(tool_calls) = choice.message.tool_calls {
            for tc in tool_calls {
                let id = format!("fc_{}", output.len());
                output.push(function_call_item(
                    Some(id),
                    &tc.id,
                    &tc.function.name,
                    &tc.function.arguments,
                ));
            }
        }
    }

    Ok(Response {
        id: resp.id,
        object: "response".into(),
        status: Some(status.name().into()),
        model: resp.model,
        output,
        incomplete_details: status.incomplete_details(),
        usage: resp.usage.as_ref().map(map_usage),
        conversation: None,
    })
}

fn message_item(status: &str, content: Vec<Value>) -> OutputItem {
    OutputItem::Message(OutputMessage {
        id: "msg".into(),
        type_: "message".into(),
        role: "assistant".into(),
        content,
        status: status.into(),
    })
}

fn function_call_item(
    id: Option<String>,
    call_id: &str,
    name: &str,
    arguments: &str,
) -> OutputItem {
    OutputItem::FunctionCall(FunctionToolCall {
        type_: "function_call".into(),
        call_id: call_id.into(),
        name: name.into(),
        arguments: arguments.into(),
        id,
    })
}

fn text_part(text: &str) -> Part {
    Part {
        type_: "output_text".into(),
        text: text.to_string(),
        annotations: Vec::new(),
    }
}

fn text_content(text: &str) -> Vec<Value> {
    vec![serde_json::to_value(text_part(text)).expect("a part serializes")]
}

fn map_usage(u: &openai::Usage) -> Usage {
    let details = u.prompt_tokens_details;
    Usage {
        input_tokens: u.prompt_tokens,
        output_tokens: u.completion_tokens,
        total_tokens: u.prompt_tokens + u.completion_tokens,
        input_tokens_details: InputTokensDetails {
            cached_tokens: details.map_or(0, |d| d.cached_tokens),
            cache_write_tokens: details.map_or(0, |d| d.cache_write_tokens),
        },
    }
}

fn system_msg(text: &str) -> openai::Message {
    openai::Message::System {
        content: openai::Content::String(text.to_string()),
    }
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        other => other.to_string(),
    }
}

struct ToolItem {
    output_index: u32,
    call_id: String,
    name: String,
    arguments: String,
}

impl ToolItem {
    fn item_id(&self) -> String {
        format!("fc_{}", self.output_index)
    }
}

#[derive(Default)]
pub struct ResponsesStreamState {
    started: bool,
    done: bool,
    stop_reason: Option<String>,
    id: String,
    model: String,
    usage: Option<Usage>,
    text: String,
    message_index: Option<u32>,
    next_output_index: u32,
    tools: BTreeMap<String, ToolItem>,
}

impl ResponsesStreamState {
    fn allocate_output_index(&mut self) -> u32 {
        let index = self.next_output_index;
        self.next_output_index += 1;
        index
    }

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
                vec![Event::Created {
                    response: Response {
                        id: self.id.clone(),
                        object: "response".into(),
                        status: Some("in_progress".into()),
                        model: self.model.clone(),
                        output: vec![],
                        incomplete_details: None,
                        usage: None,
                        conversation: None,
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
        let index = match self.message_index {
            Some(index) => index,
            None => {
                let index = self.allocate_output_index();
                self.message_index = Some(index);
                out.push(Event::OutputItemAdded {
                    output_index: index,
                    item: message_item("in_progress", Vec::new()),
                });
                out.push(Event::ContentPartAdded {
                    item_id: "msg".into(),
                    output_index: index,
                    content_index: 0,
                    part: Part {
                        type_: "output_text".into(),
                        text: String::new(),
                        annotations: vec![],
                    },
                });
                index
            }
        };
        self.text.push_str(delta);
        out.push(Event::OutputTextDelta {
            item_id: "msg".into(),
            output_index: index,
            content_index: 0,
            delta: delta.to_string(),
        });
        out
    }

    fn on_tool_start(&mut self, call_id: &str, name: &str) -> Vec<Event> {
        let output_index = self.allocate_output_index();
        self.tools.insert(
            call_id.to_string(),
            ToolItem {
                output_index,
                call_id: call_id.to_string(),
                name: name.to_string(),
                arguments: String::new(),
            },
        );
        vec![Event::OutputItemAdded {
            output_index,
            item: function_call_item(Some(format!("fc_{output_index}")), call_id, name, ""),
        }]
    }

    fn on_tool_args(&mut self, call_id: &str, delta: &str) -> Vec<Event> {
        let Some(item) = self.tools.get_mut(call_id) else {
            return Vec::new();
        };
        item.arguments.push_str(delta);
        let item_id = item.item_id();
        let output_index = item.output_index;
        vec![Event::FunctionCallArgumentsDelta {
            item_id,
            output_index,
            delta: delta.to_string(),
        }]
    }

    fn finish(&mut self) -> Vec<Event> {
        if self.done {
            return Vec::new();
        }
        self.done = true;
        let status = self.stop_reason.as_deref().map_or(
            Status::Incomplete { reason: None },
            Status::from_finish_reason,
        );

        let mut tools: Vec<&ToolItem> = self.tools.values().collect();
        tools.sort_by_key(|t| t.output_index);

        let mut done: Vec<(u32, Vec<Event>)> = Vec::new();
        let mut output: Vec<(u32, OutputItem)> = Vec::new();
        if let Some(index) = self.message_index {
            done.push((
                index,
                vec![
                    Event::OutputTextDone {
                        item_id: "msg".into(),
                        output_index: index,
                        content_index: 0,
                        text: self.text.clone(),
                    },
                    Event::ContentPartDone {
                        item_id: "msg".into(),
                        output_index: index,
                        content_index: 0,
                        part: text_part(&self.text),
                    },
                    Event::OutputItemDone {
                        output_index: index,
                        item: message_item(status.name(), text_content(&self.text)),
                    },
                ],
            ));
            output.push((index, message_item(status.name(), text_content(&self.text))));
        }
        for tool in &tools {
            done.push((
                tool.output_index,
                vec![
                    Event::FunctionCallArgumentsDone {
                        item_id: tool.item_id(),
                        output_index: tool.output_index,
                        arguments: tool.arguments.clone(),
                    },
                    Event::OutputItemDone {
                        output_index: tool.output_index,
                        item: function_call_item(
                            Some(tool.item_id()),
                            &tool.call_id,
                            &tool.name,
                            &tool.arguments,
                        ),
                    },
                ],
            ));
            output.push((
                tool.output_index,
                function_call_item(
                    Some(tool.item_id()),
                    &tool.call_id,
                    &tool.name,
                    &tool.arguments,
                ),
            ));
        }

        done.sort_by_key(|(index, _)| *index);
        let mut out = Vec::new();
        for (_, events) in done {
            out.extend(events);
        }

        output.sort_by_key(|(index, _)| *index);
        let output: Vec<OutputItem> = output.into_iter().map(|(_, item)| item).collect();

        let response = Response {
            id: self.id.clone(),
            object: "response".into(),
            status: Some(status.name().into()),
            model: self.model.clone(),
            output,
            incomplete_details: status.incomplete_details(),
            usage: self.usage,
            conversation: None,
        };
        out.push(match status {
            Status::Completed => Event::Completed { response },
            Status::Incomplete { .. } => Event::Incomplete { response },
        });
        out
    }
}
