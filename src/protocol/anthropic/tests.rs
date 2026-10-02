use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::protocol::corpus::ANTHROPIC_STREAMS;
use crate::protocol::test_support::{
    assert_required, frame_of_type, frames, frames_of_type, named_frames, set_of, value_at,
};
use crate::protocol::{Metadata, StreamCodec};

use super::stream::{Delta, Event, MessageDeltaData, MessageDeltaUsage};
use super::{
    Anthropic, ContentBlock, ContentBlockParam, Message, Request, Response, Role, Tool, ToolChoice,
    Usage,
};

const BASIC: &str = include_str!("../../../fixtures/anthropic/basic_response.txt");
const DELTA_OMITTED_USAGE: &str =
    include_str!("../../../fixtures/anthropic/message_delta_omitted_usage_response.txt");
const DELTA_FIELDS: &str =
    include_str!("../../../fixtures/anthropic/message_delta_fields_response.txt");
const TOOL_USE: &str = include_str!("../../../fixtures/anthropic/tool_use_response.txt");
const STREAM_A_REFUSAL: &str = include_str!("../../../fixtures/anthropic/stream-a-refusal.sse");

/// The frames the model does not carry, named by the one thing missing from it:
/// an event with no variant, or a block or delta whose type it does not model.
/// Every one of them is present in the vendored bytes.
const GAPS: &[&str] = &[
    "ping",
    "content_block_start:compaction",
    "content_block_start:fallback",
    "content_block_start:server_tool_use",
    "content_block_start:web_search_tool_result",
    "content_block_delta:citations_delta",
    "content_block_delta:compaction_delta",
    "content_block_delta:signature_delta",
    "content_block_delta:thinking_delta",
];

/// The keys a vendored frame reports that the model does not write back, by frame
/// type and pointer. The frame type is part of the key because a pointer alone is
/// ambiguous — a `message_delta` and a `content_block_delta` both hold their object
/// under `delta`. The frames are the evidence — the anthropic protocol publishes no
/// schema — so the keys the walk finds and the keys this list names have to be the
/// same set. Where the model has a node of its own the walk descends and compares,
/// so a nested field, a new event variant, or a key dropped in one frame and kept
/// in another all fail here whether or not anyone remembered to name them. Below a
/// key the model drops, and in the frames it refuses, there is nothing to compare.
const UNMODELLED: &[(&str, &str, &[&str])] = &[
    (
        "message_delta",
        "",
        &["context_management", "input_transformations"],
    ),
    (
        "message_start",
        "/message",
        &["input_transformations", "stop_details"],
    ),
    (
        "message_start",
        "/message/usage",
        &["cache_creation", "inference_geo", "service_tier"],
    ),
    (
        "content_block_start",
        "/content_block",
        &["caller", "citations"],
    ),
    ("message_delta", "/delta", &["container", "stop_details"]),
    (
        "message_delta",
        "/usage",
        &[
            "fallback_credit",
            "iterations",
            "output_tokens_details",
            "server_tool_use",
        ],
    ),
];

/// What the model does not carry, when it refuses the frame outright.
fn unmodelled(frame: &Value) -> Option<String> {
    if serde_json::from_value::<Event>(frame.clone()).is_ok() {
        return None;
    }
    Some(match frame["type"].as_str()? {
        "content_block_start" => format!(
            "content_block_start:{}",
            value_at(frame, "/content_block/type")
                .as_str()
                .expect("a block carries a type")
        ),
        "content_block_delta" => format!(
            "content_block_delta:{}",
            value_at(frame, "/delta/type")
                .as_str()
                .expect("a delta carries a type")
        ),
        other => other.to_string(),
    })
}

#[test]
fn the_vendored_frames_decode_unless_the_model_names_them_a_gap() {
    let mut found = BTreeSet::new();
    for (_, text) in ANTHROPIC_STREAMS {
        for frame in frames(text) {
            if let Some(label) = unmodelled(&frame) {
                found.insert(label);
            }
        }
    }
    assert_eq!(
        found,
        set_of(GAPS),
        "the frames that do not decode are exactly the shapes the model does not carry"
    );
}

#[test]
fn a_recorded_message_requires_what_the_protocol_requires() {
    let start = frame_of_type(BASIC, "message_start");
    assert_required::<Response>(
        "message_start message",
        value_at(&start, "/message"),
        &[
            "id",
            "type",
            "role",
            "content",
            "model",
            "stop_reason",
            "stop_sequence",
            "usage",
        ],
    );
    assert_required::<Usage>(
        "message_start usage",
        value_at(&start, "/message/usage"),
        &["input_tokens", "output_tokens"],
    );

    let delta = frame_of_type(BASIC, "message_delta");
    assert_required::<MessageDeltaData>(
        "message_delta delta",
        value_at(&delta, "/delta"),
        &["stop_reason", "stop_sequence"],
    );
    assert_required::<MessageDeltaUsage>(
        "message_delta usage",
        value_at(&delta, "/usage"),
        &["output_tokens"],
    );

    for start in frames_of_type(BASIC, "content_block_start") {
        assert_required::<ContentBlock>(
            "content_block_start",
            value_at(&start, "/content_block"),
            &["type", "text"],
        );
        assert_required::<Event>(
            "content_block_start frame",
            &start,
            &["type", "index", "content_block"],
        );
    }
    for delta in frames_of_type(BASIC, "content_block_delta") {
        assert_required::<Delta>(
            "content_block_delta",
            value_at(&delta, "/delta"),
            &["type", "text"],
        );
        assert_required::<Event>(
            "content_block_delta frame",
            &delta,
            &["type", "index", "delta"],
        );
    }
    for stop in frames_of_type(BASIC, "content_block_stop") {
        assert_required::<Event>("content_block_stop frame", &stop, &["type", "index"]);
    }
}

#[test]
fn a_recorded_tool_use_block_requires_what_the_protocol_requires() {
    let start = frames_of_type(TOOL_USE, "content_block_start")
        .into_iter()
        .find(|frame| frame["content_block"]["type"] == "tool_use")
        .expect("the recorded tool use starts a tool_use block");
    assert_required::<ContentBlock>(
        "tool_use block",
        value_at(&start, "/content_block"),
        &["type", "id", "name", "input"],
    );

    let deltas: Vec<Value> = frames_of_type(TOOL_USE, "content_block_delta")
        .into_iter()
        .filter(|frame| frame["delta"]["type"] == "input_json_delta")
        .collect();
    assert!(
        !deltas.is_empty(),
        "the recorded tool use carries json deltas"
    );
    for delta in deltas {
        assert_required::<Delta>(
            "input_json_delta",
            value_at(&delta, "/delta"),
            &["type", "partial_json"],
        );
    }
}

#[test]
fn a_vendor_authored_thinking_block_requires_what_the_protocol_requires() {
    let start = frames_of_type(STREAM_A_REFUSAL, "content_block_start")
        .into_iter()
        .find(|frame| frame["content_block"]["type"] == "thinking")
        .expect("the vendored stream starts a thinking block");
    assert_required::<ContentBlock>(
        "thinking block",
        value_at(&start, "/content_block"),
        &["type", "thinking", "signature"],
    );
}

/// Walks one vendored node against the node the model serializes for it: the model
/// has to write back exactly the frame, value for value, with only the keys this
/// test names as dropped missing. So every key the model writes has to be one the
/// frame reports and carry the value it reports, and every key the frame reports
/// has to be one the model writes — or one that lands in `dropped` for this frame
/// and pointer. Where the model has a node of its own the walk descends and
/// compares its interior the same way.
fn compare(
    frame: &'static str,
    bytes: &Value,
    model: &Value,
    pointer: &str,
    dropped: &mut BTreeMap<(&'static str, String), BTreeSet<String>>,
) {
    let (Some(bytes), Some(model)) = (bytes.as_object(), model.as_object()) else {
        assert_eq!(
            model, bytes,
            "{frame} {pointer}: the model writes {model} where the frame reports {bytes}"
        );
        return;
    };
    for key in model.keys() {
        assert!(
            bytes.contains_key(key),
            "{frame} {pointer}: the model writes {key}, which no vendored frame reports"
        );
    }
    for (key, value) in bytes {
        let Some(model_value) = model.get(key) else {
            dropped
                .entry((frame, pointer.to_string()))
                .or_default()
                .insert(key.clone());
            continue;
        };
        let child = format!("{pointer}/{key}");
        match (value, model_value) {
            (Value::Object(_), Value::Object(_)) => {
                compare(frame, value, model_value, &child, dropped);
            }
            (Value::Array(items), Value::Array(model_items)) => {
                assert_eq!(
                    items.len(),
                    model_items.len(),
                    "{frame} {child}: the model writes {} of the {} items the frame reports",
                    model_items.len(),
                    items.len()
                );
                for (item, model_item) in items.iter().zip(model_items) {
                    match (item, model_item) {
                        (Value::Object(_), Value::Object(_)) => {
                            compare(frame, item, model_item, &format!("{child}/*"), dropped);
                        }
                        (item, model_item) => assert_eq!(
                            model_item, item,
                            "{frame} {child}/*: the model writes {model_item} where the frame reports {item}"
                        ),
                    }
                }
            }
            (value, model_value) => assert_eq!(
                model_value, value,
                "{frame} {child}: the model writes {model_value} where the frame reports {value}"
            ),
        }
    }
}

#[test]
fn a_vendored_object_carries_what_the_model_writes_and_the_fields_named_as_dropped() {
    let mut found: BTreeMap<(&'static str, String), BTreeSet<String>> = BTreeMap::new();
    for (_, text) in ANTHROPIC_STREAMS {
        for (name, frame_value) in named_frames(text) {
            let Ok(event) = serde_json::from_value::<Event>(frame_value.clone()) else {
                continue;
            };
            let type_ = event.event_type();
            assert_eq!(
                Some(type_),
                name,
                "the name the model gives the event is the one the frame carries in its event line"
            );
            assert_eq!(
                type_,
                frame_value["type"]
                    .as_str()
                    .expect("a vendored frame carries its type"),
                "the name the model gives the event is the one the frame carries"
            );
            let model = serde_json::to_value(&event).expect("the event serializes");
            compare(type_, &frame_value, &model, "", &mut found);
        }
    }

    let mut named: BTreeMap<(&'static str, String), BTreeSet<String>> = BTreeMap::new();
    for (frame, pointer, dropped) in UNMODELLED {
        named.insert(
            (frame, (*pointer).to_string()),
            dropped.iter().map(|key| (*key).to_string()).collect(),
        );
    }
    assert_eq!(
        found, named,
        "the keys a vendored frame reports that the model does not write back are exactly the ones \
         this test names as dropped — a name still here for a key the model writes back means the \
         field is modelled now and the name is stale"
    );
}

/// The corpus carries no `text_delta` with an empty `text`, so a serializer that
/// skipped the field would be invisible to the walk above. These are the degenerate
/// values a required field can hold: a field the protocol requires is written
/// whatever its value, and the corpus cannot show that because none of its frames
/// carries an empty one.
#[test]
fn a_frame_the_model_reads_is_written_back_unchanged() {
    for value in [
        json!({"type": "message_stop"}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": ""}}),
        json!({"type": "message_delta", "delta": {"stop_reason": null, "stop_sequence": null}, "usage": {"output_tokens": 0}}),
    ] {
        let event: Event =
            serde_json::from_value(value.clone()).unwrap_or_else(|err| panic!("{value}: {err}"));
        assert_eq!(
            serde_json::to_value(&event).expect("the event serializes"),
            value,
            "the model writes back the frame it read"
        );
    }
}

#[test]
fn a_count_the_upstream_omits_is_not_applicable() {
    let start = frame_of_type(BASIC, "message_start");
    let usage: Usage = serde_json::from_value(value_at(&start, "/message/usage").clone())
        .expect("the recorded usage decodes");
    assert_eq!(usage.input_tokens, 11);
    assert!(
        usage.cache_creation_input_tokens.is_none() && usage.cache_read_input_tokens.is_none(),
        "a cache count the upstream omits means not applicable, not zero"
    );

    let delta = frame_of_type(DELTA_OMITTED_USAGE, "message_delta");
    let usage: MessageDeltaUsage = serde_json::from_value(value_at(&delta, "/usage").clone())
        .expect("the delta usage decodes");
    assert_eq!(usage.output_tokens, 8);
    assert!(
        usage.input_tokens.is_none()
            && usage.cache_creation_input_tokens.is_none()
            && usage.cache_read_input_tokens.is_none(),
        "the frame reports an output count and omits the rest"
    );

    let delta = frame_of_type(DELTA_FIELDS, "message_delta");
    let usage: MessageDeltaUsage = serde_json::from_value(value_at(&delta, "/usage").clone())
        .expect("the delta usage decodes");
    assert_eq!(usage.input_tokens, Some(40));
    assert_eq!(usage.cache_read_input_tokens, Some(7));
    assert_eq!(usage.total_input_tokens(), Some(59));
}

#[test]
fn the_ledger_takes_the_totals_the_frames_report() {
    let start: Event = serde_json::from_value(frame_of_type(DELTA_FIELDS, "message_start"))
        .expect("the vendored message_start decodes");
    let mut metadata = Metadata::default();
    Anthropic.observe_usage(&mut metadata.usage, &start);
    assert_eq!(
        metadata.usage.input_tokens, 40,
        "the frame reports 25 input tokens and 15 cached ones, and the ledger counts the total"
    );

    let delta: Event = serde_json::from_value(frame_of_type(DELTA_FIELDS, "message_delta"))
        .expect("the vendored message_delta decodes");
    Anthropic.observe_usage(&mut metadata.usage, &delta);
    assert_eq!(
        (metadata.usage.input_tokens, metadata.usage.output_tokens),
        (59, 8),
        "the cumulative counts the delta reports overwrite the ones the start seeded"
    );
}

#[test]
fn a_request_requires_what_the_api_reference_lists() {
    assert_required::<Request>(
        "request",
        &json!({"model": "claude", "messages": [], "max_tokens": 16}),
        &["model", "messages", "max_tokens"],
    );
    assert_required::<Message>(
        "message",
        &json!({"role": "user", "content": "hi"}),
        &["role", "content"],
    );
    assert_required::<Tool>(
        "tool",
        &json!({"name": "t", "input_schema": {"type": "object"}}),
        &["name", "input_schema"],
    );
    assert_required::<ContentBlockParam>(
        "text",
        &json!({"type": "text", "text": "hi"}),
        &["type", "text"],
    );
    assert_required::<ContentBlockParam>(
        "tool use",
        &json!({"type": "tool_use", "id": "t", "name": "n", "input": {}}),
        &["type", "id", "name", "input"],
    );
    assert_required::<ContentBlockParam>(
        "tool result",
        &json!({"type": "tool_result", "tool_use_id": "t"}),
        &["type", "tool_use_id"],
    );
    assert_required::<ContentBlockParam>(
        "thinking",
        &json!({"type": "thinking", "thinking": "t", "signature": "s"}),
        &["type", "thinking", "signature"],
    );
}

#[test]
fn a_request_carries_the_sampling_parameters_it_was_sent() {
    let request: Request = serde_json::from_value(json!({
        "model": "claude",
        "messages": [],
        "max_tokens": 16,
        "temperature": 0.0,
        "top_p": 0.5,
    }))
    .expect("the request decodes");
    assert_eq!(request.temperature, Some(0.0));
    assert_eq!(request.top_p, Some(0.5));
}

#[test]
fn a_request_carries_the_system_prompt_it_was_sent() {
    let request: Request = serde_json::from_value(json!({
        "model": "claude",
        "messages": [],
        "max_tokens": 16,
        "system": "be brief",
    }))
    .expect("the request decodes");
    assert_eq!(
        request
            .system
            .and_then(|system| serde_json::to_value(system).ok()),
        Some(json!("be brief")),
        "the system prompt is read under the name the API reference gives it"
    );
}

#[test]
fn the_protocols_own_variants_decode() {
    for role in ["user", "assistant", "system"] {
        assert!(
            serde_json::from_value::<Role>(json!(role)).is_ok(),
            "{role} is a protocol role"
        );
    }
    assert!(serde_json::from_value::<ToolChoice>(json!({"type": "none"})).is_ok());
    assert!(serde_json::from_value::<ToolChoice>(json!({"type": "any"})).is_ok());
}

#[test]
fn a_required_nullable_field_without_a_value_is_written_as_null() {
    let response = Response {
        type_: "message".into(),
        id: "msg_1".into(),
        role: "assistant".into(),
        content: Vec::new(),
        model: "claude".into(),
        stop_reason: None,
        stop_sequence: None,
        usage: Usage::default(),
    };
    let value = serde_json::to_value(&response).expect("the response serializes");
    for field in ["stop_reason", "stop_sequence"] {
        assert!(
            value.get(field).is_some_and(Value::is_null),
            "a response without a {field} carries an explicit null, not nothing"
        );
    }

    let event = Event::MessageDelta {
        delta: MessageDeltaData {
            stop_reason: None,
            stop_sequence: None,
        },
        usage: MessageDeltaUsage {
            output_tokens: 0,
            ..Default::default()
        },
    };
    let value = serde_json::to_value(&event).expect("the event serializes");
    for field in ["stop_reason", "stop_sequence"] {
        assert!(
            value["delta"].get(field).is_some_and(Value::is_null),
            "a message delta without a {field} carries an explicit null"
        );
    }
}

#[test]
fn a_count_the_upstream_omits_is_not_written() {
    let value = serde_json::to_value(Usage::default()).expect("the usage serializes");
    for field in ["cache_creation_input_tokens", "cache_read_input_tokens"] {
        assert!(
            value.get(field).is_none(),
            "no recorded usage carries a null {field}, so the encoder writes no key at all"
        );
    }

    let value = serde_json::to_value(MessageDeltaUsage {
        output_tokens: 0,
        ..Default::default()
    })
    .expect("the delta usage serializes");
    for field in [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
    ] {
        assert!(
            value.get(field).is_none(),
            "no recorded delta usage carries a null {field}, so the encoder writes no key at all"
        );
    }
    assert_eq!(
        value.get("output_tokens"),
        Some(&json!(0)),
        "the count every delta usage reports is always written"
    );
}
