use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::protocol::corpus::CHAT_STREAMS;
use crate::protocol::test_support::{
    DONE, assert_required, data_lines, frames, modelled_required, set_of, spec_required, value_at,
};

use super::stream::{Chunk, ChunkChoice, ToolCallDelta};
use super::{
    Choice, Function, FunctionDef, Message, Request, Response, ResponseMessage, Tool, Usage,
};

const COMPLETION: &str =
    include_str!("../../../fixtures/openai_chat/chat_completion_tool_calls.json");

const CHUNK_REQUIRED: &str = include_str!("../../../fixtures/openai_chat/chat_chunk_required.json");
const CHUNK_CHOICE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_chat/chat_chunk_choice_required.json");
const COMPLETION_REQUIRED: &str =
    include_str!("../../../fixtures/openai_chat/chat_completion_required.json");
const CHOICE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_chat/chat_choice_required.json");
const USAGE_REQUIRED: &str = include_str!("../../../fixtures/openai_chat/chat_usage_required.json");
const REQUEST_REQUIRED: &str =
    include_str!("../../../fixtures/openai_chat/chat_request_required.json");
const SYSTEM_MESSAGE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_chat/chat_system_message_required.json");
const USER_MESSAGE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_chat/chat_user_message_required.json");
const ASSISTANT_MESSAGE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_chat/chat_assistant_message_required.json");
const TOOL_MESSAGE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_chat/chat_tool_message_required.json");
const TOOL_REQUIRED: &str = include_str!("../../../fixtures/openai_chat/chat_tool_required.json");
const FUNCTION_OBJECT_REQUIRED: &str =
    include_str!("../../../fixtures/openai_chat/chat_function_object_required.json");
const TOOL_CALL_REQUIRED: &str =
    include_str!("../../../fixtures/openai_chat/chat_tool_call_required.json");
const TOOL_CALL_FUNCTION_REQUIRED: &str =
    include_str!("../../../fixtures/openai_chat/chat_tool_call_function_required.json");
const TOOL_CALL_CHUNK_REQUIRED: &str =
    include_str!("../../../fixtures/openai_chat/chat_tool_call_chunk_required.json");
const RESPONSE_MESSAGE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_chat/chat_response_message_required.json");

/// The spec's required fields the local model does not carry.
const UNMODELLED_BODY_FIELDS: &[&str] = &["created", "object"];
/// A streamed choice carries the index; the logprobs are the only field of it
/// the model does not carry.
const UNMODELLED_CHUNK_CHOICE_FIELDS: &[&str] = &["logprobs"];
/// A complete choice carries neither.
const UNMODELLED_CHOICE_FIELDS: &[&str] = &["index", "logprobs"];
/// The model writes the content but not the role or the refusal beside it.
const UNMODELLED_MESSAGE_FIELDS: &[&str] = &["role", "refusal"];

#[test]
fn the_recorded_chunks_decode_and_the_stream_ends_on_the_sentinel() {
    for (name, text) in CHAT_STREAMS {
        for line in data_lines(text) {
            if line == DONE {
                continue;
            }
            assert!(
                serde_json::from_str::<Chunk>(&line).is_ok(),
                "{name}: the chunk {line} does not decode"
            );
        }
        assert!(
            data_lines(text).iter().any(|line| line == DONE),
            "{name}: the stream ends on the done sentinel"
        );
    }
}

#[test]
fn a_recorded_chunk_requires_what_the_spec_requires() {
    let chunk_fields = modelled_required(&spec_required(CHUNK_REQUIRED), UNMODELLED_BODY_FIELDS);
    let choice_fields = modelled_required(
        &spec_required(CHUNK_CHOICE_REQUIRED),
        UNMODELLED_CHUNK_CHOICE_FIELDS,
    );
    let usage_fields = modelled_required(&spec_required(USAGE_REQUIRED), &[]);

    for (name, text) in CHAT_STREAMS {
        for chunk in frames(text) {
            assert_required::<Chunk>(name, &chunk, &chunk_fields);
            for choice in value_at(&chunk, "/choices")
                .as_array()
                .into_iter()
                .flatten()
            {
                assert_required::<ChunkChoice>(name, choice, &choice_fields);
            }
            if chunk.get("usage").is_some_and(|usage| !usage.is_null()) {
                assert_required::<Usage>(name, value_at(&chunk, "/usage"), &usage_fields);
            }
        }
    }
}

#[test]
fn a_choice_reports_a_finish_reason_and_an_explicit_null_content() {
    let required = modelled_required(&spec_required(COMPLETION_REQUIRED), UNMODELLED_BODY_FIELDS);
    let completion: Value = serde_json::from_str(COMPLETION).expect("the vendored body is json");
    assert_required::<Response>("completion", &completion, &required);

    let choice = value_at(&completion, "/choices/0");
    assert_required::<Choice>(
        "choice",
        choice,
        &modelled_required(&spec_required(CHOICE_REQUIRED), UNMODELLED_CHOICE_FIELDS),
    );

    let message = value_at(choice, "/message");
    let decoded: ResponseMessage =
        serde_json::from_value(message.clone()).expect("the message decodes");
    assert!(
        decoded.content.is_none(),
        "a tool call carries an explicit null content, not an absent one"
    );
    assert!(decoded.tool_calls.is_some_and(|calls| calls.len() == 1));
    assert_required::<ResponseMessage>(
        "message",
        message,
        &modelled_required(
            &spec_required(RESPONSE_MESSAGE_REQUIRED),
            UNMODELLED_MESSAGE_FIELDS,
        ),
    );
}

#[test]
fn a_recorded_tool_call_delta_carries_the_id_and_arguments_the_spec_requires() {
    let (_, text) = CHAT_STREAMS
        .iter()
        .find(|(name, _)| *name == "openai_chat/tool_call.sse")
        .expect("the tool call stream is vendored");
    let call_fields = modelled_required(&spec_required(TOOL_CALL_CHUNK_REQUIRED), &[]);
    let mut ids = BTreeSet::new();
    let mut arguments = String::new();
    for chunk in frames(text) {
        for choice in value_at(&chunk, "/choices")
            .as_array()
            .into_iter()
            .flatten()
        {
            for call in value_at(choice, "/delta")
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                assert_required::<ToolCallDelta>("tool call delta", call, &call_fields);
                if let Some(id) = call["id"].as_str() {
                    ids.insert(id.to_string());
                }
                if let Some(fragment) = call["function"]["arguments"].as_str() {
                    arguments.push_str(fragment);
                }
            }
        }
    }
    assert_eq!(
        ids,
        set_of(&["call_c91SqDXlYFuETYv8mUHzz6pp"]),
        "the stream names the tool call once, so a reader can tell its fragments apart"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&arguments).ok(),
        Some(json!({"city": "Edinburgh", "country": "UK", "units": "c"})),
        "the fragments the stream deltas spell a whole argument object"
    );
}

#[test]
fn a_request_requires_what_the_spec_requires() {
    assert_required::<Request>(
        "request",
        &json!({"model": "gpt", "messages": []}),
        &spec_required(REQUEST_REQUIRED),
    );
    assert_required::<Tool>(
        "tool",
        &json!({"type": "function", "function": {"name": "n", "parameters": {}}}),
        &spec_required(TOOL_REQUIRED),
    );
    assert_required::<FunctionDef>(
        "function",
        &json!({"name": "n", "parameters": {}}),
        &spec_required(FUNCTION_OBJECT_REQUIRED),
    );
    assert!(
        serde_json::from_value::<FunctionDef>(json!({"name": "n"})).is_ok(),
        "omitting the parameters defines a function with an empty parameter list"
    );
    assert_required::<Message>(
        "system",
        &json!({"role": "system", "content": "s"}),
        &spec_required(SYSTEM_MESSAGE_REQUIRED),
    );
    assert_required::<Message>(
        "user",
        &json!({"role": "user", "content": "hi"}),
        &spec_required(USER_MESSAGE_REQUIRED),
    );
    assert_required::<Message>(
        "assistant",
        &json!({"role": "assistant"}),
        &spec_required(ASSISTANT_MESSAGE_REQUIRED),
    );
    assert_required::<Message>(
        "tool",
        &json!({"role": "tool", "content": "t", "tool_call_id": "c"}),
        &spec_required(TOOL_MESSAGE_REQUIRED),
    );
    assert_required::<super::ToolCall>(
        "tool call",
        &json!({"id": "c", "type": "function", "function": {"name": "n", "arguments": "{}"}}),
        &spec_required(TOOL_CALL_REQUIRED),
    );
    assert_required::<Function>(
        "function",
        &json!({"name": "n", "arguments": "{}"}),
        &spec_required(TOOL_CALL_FUNCTION_REQUIRED),
    );
    assert!(
        serde_json::from_value::<Message>(json!({"role": "assistant", "content": null})).is_ok(),
        "an assistant message may carry null content"
    );
    assert!(
        serde_json::from_value::<Message>(json!({"role": "user", "content": null})).is_err(),
        "a user message may not"
    );
}

#[test]
fn a_response_message_carries_an_explicit_null_content() {
    let message = ResponseMessage {
        content: None,
        tool_calls: None,
    };
    let value = serde_json::to_value(&message).expect("the message serializes");
    assert!(
        value.get("content").is_some_and(Value::is_null),
        "a response message without content writes an explicit null"
    );
}

#[test]
fn a_non_streaming_finish_reason_is_never_null() {
    assert!(
        serde_json::from_value::<Choice>(
            json!({"message": {"content": null}, "finish_reason": null})
        )
        .is_err(),
        "the protocol reports a finish reason on every complete choice"
    );
}
