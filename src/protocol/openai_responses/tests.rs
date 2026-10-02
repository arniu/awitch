use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::protocol::corpus::RESPONSES_STREAMS;
use crate::protocol::test_support::{
    assert_required, frame_of_type, frames, modelled_required, named_frames, set_of, spec_required,
    value_at, without,
};

use super::stream::{Event, Part};
use super::{
    FunctionCallOutput, FunctionToolCall, InputItem, InputMessage, InputTokensDetails, OutputItem,
    OutputMessage, Request, Response, Tool, Usage,
};

const SAMPLE: &str = RESPONSES_STREAMS[0].1;

const CREATED_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/created_required.json");
const COMPLETED_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/completed_required.json");
const INCOMPLETE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/incomplete_required.json");
const ITEM_ADDED_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/output_item_added_required.json");
const ITEM_DONE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/output_item_done_required.json");
const PART_ADDED_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/content_part_added_required.json");
const PART_DONE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/content_part_done_required.json");
const TEXT_DELTA_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/text_delta_required.json");
const TEXT_DONE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/text_done_required.json");
const ARGUMENTS_DELTA_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/arguments_delta_required.json");
const ARGUMENTS_DONE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/arguments_done_required.json");
const OUTPUT_TEXT_CONTENT_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/output_text_content_required.json");
const OUTPUT_MESSAGE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/output_message_required.json");
const FUNCTION_CALL_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/function_call_required.json");
const USAGE_REQUIRED: &str = include_str!("../../../fixtures/openai_responses/usage_required.json");
const USAGE_DETAILS_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/usage_details_required.json");
const RESPONSE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/response_required.json");
const INPUT_MESSAGE_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/input_message_required.json");
const FUNCTION_CALL_OUTPUT_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/function_call_output_required.json");
const FUNCTION_TOOL_REQUIRED: &str =
    include_str!("../../../fixtures/openai_responses/function_tool_required.json");

/// The events the model carries, which the sample covers.
const MODELLED_EVENTS: &[&str] = &[
    "response.created",
    "response.output_item.added",
    "response.content_part.added",
    "response.output_text.delta",
    "response.output_text.done",
    "response.content_part.done",
    "response.output_item.done",
];

/// The frames the sample carries that the model refuses. `response.in_progress`
/// has no variant at all; `response.completed` is modelled but the sample's
/// usage omits the details the published schema requires, so the frame does not
/// decode until they are supplied.
const UNMODELLED_EVENTS: &[&str] = &["response.completed", "response.in_progress"];

/// The spec's required fields the local model does not carry. Every event
/// carries the sequence number, and the text events carry the logprobs.
const UNMODELLED_EVENT_FIELDS: &[&str] = &["sequence_number", "logprobs"];
/// The part the model carries is the text member of the `OutputContent`
/// union, without the logprobs the schema requires of it.
const UNMODELLED_PART_FIELDS: &[&str] = &["logprobs"];
const UNMODELLED_RESPONSE_FIELDS: &[&str] = &[
    "created_at",
    "error",
    "instructions",
    "tools",
    "parallel_tool_calls",
    "metadata",
    "tool_choice",
    "temperature",
    "top_p",
];

/// The sample's terminal frame, with the usage details the schema requires of
/// it and the sample itself omits.
fn completed_with_details() -> Value {
    let mut completed = frame_of_type(SAMPLE, "response.completed");
    completed["response"]["usage"]["input_tokens_details"] =
        json!({"cached_tokens": 0, "cache_write_tokens": 0});
    completed
}

#[test]
fn the_samples_frames_decode_unless_the_model_names_them_a_gap() {
    let mut decoded = BTreeSet::new();
    let mut undecoded = BTreeSet::new();
    for event in frames(SAMPLE) {
        let type_ = event["type"]
            .as_str()
            .expect("a frame carries a type")
            .to_string();
        match serde_json::from_value::<Event>(event) {
            Ok(_) => decoded.insert(type_),
            Err(_) => undecoded.insert(type_),
        };
    }
    assert_eq!(
        decoded,
        set_of(MODELLED_EVENTS),
        "the frames the model carries are the ones it decodes"
    );
    assert_eq!(
        undecoded,
        set_of(UNMODELLED_EVENTS),
        "the frames it refuses are exactly the ones named as gaps"
    );
}

/// A client routes on the `event:` line, so it is the name awitch writes back for
/// each event. The vendored sample carries that name twice, and the model has a
/// name for the event in both places, each written by hand.
#[test]
fn a_frame_carries_the_name_the_model_gives_the_event() {
    let mut checked = BTreeSet::new();
    for (name, frame) in named_frames(SAMPLE) {
        let type_ = frame["type"].as_str().expect("a frame carries its type");
        let Ok(event) = serde_json::from_value::<Event>(frame.clone()) else {
            continue;
        };
        checked.insert(type_.to_string());
        assert_eq!(
            event.event_type(),
            name.unwrap_or_else(|| panic!("{type_}: the frame carries its name in an event line")),
            "{type_}: the name the model gives the event is the one the frame carries"
        );
        assert_eq!(
            event.event_type(),
            type_,
            "{type_}: the payload repeats the name the frame carries, and the model writes that one"
        );
    }
    assert_eq!(
        checked,
        set_of(MODELLED_EVENTS),
        "every event the model carries is one whose name the frames tie down"
    );
}

/// The pinned stream example is documentation rather than a recording: where it
/// disagrees with the schema it sits in, the schema decides. So the example's own
/// terminal frame is deliberately not decodable, and a frame that reports the
/// details is.
#[test]
fn the_samples_completion_omits_the_usage_details_the_schema_requires() {
    let completed = frame_of_type(SAMPLE, "response.completed");
    let usage = value_at(&completed, "/response/usage");
    assert!(
        usage.get("input_tokens_details").is_none(),
        "the sample reports a usage without the details the schema requires of it"
    );
    assert!(
        serde_json::from_value::<Event>(completed.clone()).is_err(),
        "so the terminal frame does not decode"
    );

    assert!(
        serde_json::from_value::<Event>(completed_with_details()).is_ok(),
        "supplying the details the schema requires makes the frame decode"
    );
}

#[test]
fn the_samples_events_require_what_the_schema_requires() {
    let cases = [
        ("response.created", CREATED_REQUIRED),
        ("response.output_item.added", ITEM_ADDED_REQUIRED),
        ("response.content_part.added", PART_ADDED_REQUIRED),
        ("response.output_text.delta", TEXT_DELTA_REQUIRED),
        ("response.output_text.done", TEXT_DONE_REQUIRED),
        ("response.content_part.done", PART_DONE_REQUIRED),
        ("response.output_item.done", ITEM_DONE_REQUIRED),
    ];
    for (type_, list) in cases {
        let required = modelled_required(&spec_required(list), UNMODELLED_EVENT_FIELDS);
        assert_required::<Event>(type_, &frame_of_type(SAMPLE, type_), &required);
    }

    let required = modelled_required(&spec_required(COMPLETED_REQUIRED), UNMODELLED_EVENT_FIELDS);
    assert_required::<Event>("response.completed", &completed_with_details(), &required);
}

#[test]
fn an_event_the_sample_does_not_carry_requires_what_the_schema_requires() {
    let mut incomplete = frame_of_type(SAMPLE, "response.created");
    incomplete["type"] = json!("response.incomplete");
    let required = modelled_required(&spec_required(INCOMPLETE_REQUIRED), UNMODELLED_EVENT_FIELDS);
    assert_required::<Event>("response.incomplete", &incomplete, &required);

    let required = modelled_required(
        &spec_required(ARGUMENTS_DELTA_REQUIRED),
        UNMODELLED_EVENT_FIELDS,
    );
    let delta = json!({
        "type": "response.function_call_arguments.delta",
        "item_id": "i",
        "output_index": 0,
        "delta": "{}",
    });
    assert_required::<Event>("arguments delta", &delta, &required);

    let required = modelled_required(
        &spec_required(ARGUMENTS_DONE_REQUIRED),
        UNMODELLED_EVENT_FIELDS,
    );
    let done = json!({
        "type": "response.function_call_arguments.done",
        "item_id": "i",
        "output_index": 0,
        "arguments": "{}",
    });
    assert_required::<Event>("arguments done", &done, &required);
}

#[test]
fn the_samples_response_requires_what_the_schema_requires() {
    let required = modelled_required(
        &spec_required(RESPONSE_REQUIRED),
        UNMODELLED_RESPONSE_FIELDS,
    );
    assert_required::<Response>(
        "response",
        value_at(&frame_of_type(SAMPLE, "response.created"), "/response"),
        &required,
    );

    let created = frame_of_type(SAMPLE, "response.created");
    let response: Response = serde_json::from_value(value_at(&created, "/response").clone())
        .expect("the response decodes");
    assert!(
        response.incomplete_details.is_none(),
        "the sample's response carries a null, not an absent, incomplete details"
    );

    let completed = completed_with_details();
    assert_required::<Usage>(
        "usage",
        value_at(&completed, "/response/usage"),
        &modelled_required(&spec_required(USAGE_REQUIRED), &["output_tokens_details"]),
    );
    assert_required::<InputTokensDetails>(
        "usage details",
        value_at(&completed, "/response/usage/input_tokens_details"),
        &spec_required(USAGE_DETAILS_REQUIRED),
    );
}

#[test]
fn an_output_item_of_the_sample_requires_what_the_schema_requires() {
    let done = frame_of_type(SAMPLE, "response.output_item.done");
    let item = value_at(&done, "/item");
    assert!(
        matches!(
            serde_json::from_value::<OutputItem>(item.clone()),
            Ok(OutputItem::Message(_))
        ),
        "the union reads a completed message as a message"
    );
    assert_required::<OutputMessage>("message", item, &spec_required(OUTPUT_MESSAGE_REQUIRED));
    assert!(
        serde_json::from_value::<OutputItem>(without(item, "type")).is_err(),
        "the union has no member for an item without a type"
    );

    let part = frame_of_type(SAMPLE, "response.content_part.done");
    assert_required::<Part>(
        "part",
        value_at(&part, "/part"),
        &modelled_required(
            &spec_required(OUTPUT_TEXT_CONTENT_REQUIRED),
            UNMODELLED_PART_FIELDS,
        ),
    );
    assert_required::<OutputMessage>(
        "message in progress",
        value_at(
            &frame_of_type(SAMPLE, "response.output_item.added"),
            "/item",
        ),
        &spec_required(OUTPUT_MESSAGE_REQUIRED),
    );
}

#[test]
fn a_function_call_carries_what_the_schema_requires() {
    let call = json!({
        "type": "function_call",
        "call_id": "call_1",
        "name": "get_weather",
        "arguments": "{}",
    });
    assert_required::<FunctionToolCall>(
        "function call",
        &call,
        &spec_required(FUNCTION_CALL_REQUIRED),
    );
    assert!(
        matches!(
            serde_json::from_value::<OutputItem>(call.clone()),
            Ok(OutputItem::FunctionCall(_))
        ),
        "the union reads a function call as a function call"
    );
    assert!(matches!(
        serde_json::from_value::<OutputItem>(json!({"type": "reasoning", "summary": []})),
        Ok(OutputItem::Unmodelled(_))
    ));
}

#[test]
fn a_request_carries_what_the_schema_requires() {
    assert_required::<Request>("request", &json!({}), &[] as &[&str]);
    assert_required::<Tool>(
        "tool",
        &json!({"type": "function", "name": "n", "parameters": {}}),
        &modelled_required(&spec_required(FUNCTION_TOOL_REQUIRED), &["strict"]),
    );
}

#[test]
fn a_request_input_item_carries_what_its_kind_requires() {
    assert_required::<InputMessage>(
        "message",
        &json!({"role": "user", "content": "hi"}),
        &spec_required(INPUT_MESSAGE_REQUIRED),
    );
    assert_required::<FunctionToolCall>(
        "function call",
        &json!({"type": "function_call", "call_id": "c", "name": "n", "arguments": "{}"}),
        &spec_required(FUNCTION_CALL_REQUIRED),
    );
    assert_required::<FunctionCallOutput>(
        "function call output",
        &json!({"type": "function_call_output", "output": "x"}),
        &spec_required(FUNCTION_CALL_OUTPUT_REQUIRED),
    );
    assert!(
        serde_json::from_value::<InputMessage>(
            json!({"role": "user", "content": "hi", "type": "message"})
        )
        .is_ok(),
        "a message may carry its type"
    );
}

#[test]
fn a_request_input_item_is_told_apart_by_shape() {
    assert!(matches!(
        serde_json::from_value::<InputItem>(json!({"role": "user", "content": "hi"})),
        Ok(InputItem::Message(_))
    ));
    assert!(matches!(
        serde_json::from_value::<InputItem>(
            json!({"role": "user", "content": "hi", "type": "message"})
        ),
        Ok(InputItem::Message(_))
    ));
    assert!(matches!(
        serde_json::from_value::<InputItem>(
            json!({"type": "function_call", "call_id": "c", "name": "n", "arguments": "{}"})
        ),
        Ok(InputItem::FunctionCall(_))
    ));
    assert!(matches!(
        serde_json::from_value::<InputItem>(json!({"type": "function_call_output", "output": "x"})),
        Ok(InputItem::FunctionCallOutput(_))
    ));
    assert!(matches!(
        serde_json::from_value::<InputItem>(json!({"type": "reasoning", "summary": []})),
        Ok(InputItem::Unmodelled(_))
    ));
}

#[test]
fn a_response_writes_the_fields_the_protocol_requires() {
    let response = Response {
        id: "r".into(),
        object: "response".into(),
        status: Some("completed".into()),
        model: "gpt".into(),
        output: vec![OutputItem::Message(OutputMessage {
            id: "m".into(),
            type_: "message".into(),
            role: "assistant".into(),
            content: vec![json!({"type": "output_text", "text": "hi"})],
            status: "completed".into(),
        })],
        incomplete_details: None,
        usage: None,
        conversation: None,
    };
    let value: Value = serde_json::to_value(&response).expect("a response serializes");
    assert!(
        value.get("incomplete_details").is_some_and(Value::is_null),
        "there are always incomplete details, and they are null when there are none"
    );
    assert_eq!(value["output"][0]["type"], "message");
    assert_eq!(value["output"][0]["content"][0]["text"], "hi");
}
