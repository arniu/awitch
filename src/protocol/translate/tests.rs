use std::collections::BTreeSet;

use axum::body::Bytes;
use futures_util::stream::StreamExt;
use serde_json::{Value, json};

use crate::protocol::anthropic::Anthropic;
use crate::protocol::corpus::{ANTHROPIC_STREAMS, CHAT_STREAMS, RESPONSES_STREAMS};
use crate::protocol::openai_chat::OpenaiChat;
use crate::protocol::openai_responses::OpenaiResponses;
use crate::protocol::test_support::{frame_of_type, frames, frames_of_type, value_at};
use crate::protocol::translate::anthropic_chat::AnthropicToChat;
use crate::protocol::translate::responses_chat::ResponsesToChat;
use crate::protocol::translate::stream::{
    StreamEnd, StreamOutcome, forward_stream, translate_stream,
};
use crate::protocol::translate::{Pair, StreamTranslate};
use crate::protocol::{Metadata, Protocol, StreamCodec};

const ANTHROPIC_STREAM: &str = include_str!("../../../fixtures/anthropic/basic_response.txt");
const CHAT_STREAM: &str = include_str!("../../../fixtures/openai_chat/text_stream.sse");
const CHAT_TOOL_STREAM: &str = include_str!("../../../fixtures/openai_chat/tool_call.sse");
const CHAT_TOOL_CALLS_STREAM: &str =
    include_str!("../../../fixtures/openai_chat/multiple_tool_calls.sse");
const CHAT_COMPLETION: &str =
    include_str!("../../../fixtures/openai_chat/chat_completion_tool_calls.json");

const TOOL_CALL_ID: &str = "call_c91SqDXlYFuETYv8mUHzz6pp";
const TOOL_ARGUMENTS: &str = r#"{"city":"Edinburgh","country":"UK","units":"c"}"#;

fn completed(ended: StreamEnd) -> Metadata {
    match ended {
        StreamEnd::Completed(metadata) => metadata,
        _ => panic!("the recorded stream completes"),
    }
}

fn chunk_stream(
    text: &str,
    size: usize,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send + Unpin + 'static {
    let chunks: Vec<Bytes> = text
        .as_bytes()
        .chunks(size)
        .map(Bytes::copy_from_slice)
        .collect();
    futures_util::stream::iter(chunks.into_iter().map(Ok::<_, std::io::Error>)).boxed()
}

async fn collect(outcome: StreamOutcome) -> (String, StreamEnd) {
    let chunks: Vec<Bytes> = outcome
        .bytes
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|chunk| chunk.expect("the forwarded stream yields bytes"))
        .collect();
    let bytes = String::from_utf8(chunks.concat()).expect("the forwarded stream is text");
    let ended = outcome
        .ended
        .await
        .expect("the stream reports how it ended");
    (bytes, ended)
}

async fn translate_by<S, X>(source: S, translate: X, text: &str, size: usize) -> (String, StreamEnd)
where
    S: StreamCodec,
    X: StreamTranslate<S>,
{
    let outcome = translate_stream(source, OpenaiChat, translate, chunk_stream(text, size))
        .await
        .expect("the stream builds");
    collect(outcome).await
}

async fn translate<S, X>(source: S, translate: X, text: &str) -> (String, StreamEnd)
where
    S: StreamCodec,
    X: StreamTranslate<S>,
{
    translate_by(source, translate, text, text.len()).await
}

async fn forward_by<S>(source: S, text: &str, size: usize) -> (String, StreamEnd)
where
    S: StreamCodec,
{
    let outcome = forward_stream(source, chunk_stream(text, size))
        .await
        .expect("the stream builds");
    collect(outcome).await
}

async fn forward<S>(source: S, text: &str) -> (String, StreamEnd)
where
    S: StreamCodec,
{
    forward_by(source, text, text.len()).await
}

fn tools(text: &str) -> Vec<Value> {
    frames(text)
        .into_iter()
        .filter(|event| {
            event["type"] == "content_block_start" && event["content_block"]["type"] == "tool_use"
        })
        .collect()
}

fn partial_json(text: &str) -> String {
    let mut arguments = String::new();
    for delta in frames_of_type(text, "content_block_delta") {
        if let Some(fragment) = value_at(&delta, "/delta")
            .get("partial_json")
            .and_then(Value::as_str)
        {
            arguments.push_str(fragment);
        }
    }
    arguments
}

fn chat_text(text: &str) -> String {
    let mut content = String::new();
    for chunk in frames(text) {
        for choice in value_at(&chunk, "/choices")
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(fragment) = choice["delta"]["content"].as_str() {
                content.push_str(fragment);
            }
        }
    }
    content
}

fn responses_text(text: &str) -> String {
    let mut content = String::new();
    for delta in frames_of_type(text, "response.output_text.delta") {
        content.push_str(
            value_at(&delta, "/delta")
                .as_str()
                .expect("a delta is text"),
        );
    }
    content
}

#[tokio::test]
async fn a_forward_stream_forwards_the_recorded_bytes_and_keeps_their_metadata() {
    let (bytes, ended) = forward(Anthropic, ANTHROPIC_STREAM).await;
    assert_eq!(
        bytes, ANTHROPIC_STREAM,
        "the client sees the recorded frames verbatim"
    );
    let metadata = completed(ended);
    assert_eq!(
        (metadata.usage.input_tokens, metadata.usage.output_tokens),
        (11, 6),
        "the frames' counts reach the ledger"
    );

    let (bytes, ended) = forward(OpenaiChat, CHAT_STREAM).await;
    assert_eq!(
        bytes, CHAT_STREAM,
        "the client sees the recorded chunks verbatim"
    );
    let metadata = completed(ended);
    assert_eq!(
        (metadata.usage.input_tokens, metadata.usage.output_tokens),
        (14, 30),
        "the frames' counts reach the ledger"
    );
}

#[tokio::test]
async fn a_forward_stream_forwards_bytes_that_arrive_mid_frame() {
    let (bytes, _) = forward_by(Anthropic, ANTHROPIC_STREAM, 7).await;
    assert_eq!(
        bytes, ANTHROPIC_STREAM,
        "a frame split across reads still reaches the client verbatim"
    );
}

/// Every vendored stream of a protocol, whatever the model makes of its frames:
/// a frame it cannot read is forwarded anyway, and never ends the stream.
async fn every_stream_reaches_the_client_verbatim<S: StreamCodec + Clone>(
    source: S,
    streams: &[(&str, &str)],
) {
    for (name, text) in streams {
        let (bytes, ended) = forward(source.clone(), text).await;
        assert_eq!(bytes, *text, "{name}: the client sees the frames verbatim");
        assert!(
            matches!(ended, StreamEnd::Completed(_)),
            "{name}: a frame the model cannot read does not end the stream"
        );
    }
}

#[tokio::test]
async fn every_anthropic_stream_reaches_the_client_verbatim() {
    every_stream_reaches_the_client_verbatim(Anthropic, ANTHROPIC_STREAMS).await;
}

#[tokio::test]
async fn every_chat_stream_reaches_the_client_verbatim() {
    every_stream_reaches_the_client_verbatim(OpenaiChat, CHAT_STREAMS).await;
}

#[tokio::test]
async fn every_responses_stream_reaches_the_client_verbatim() {
    every_stream_reaches_the_client_verbatim(OpenaiResponses, RESPONSES_STREAMS).await;
}

#[tokio::test]
async fn a_responses_terminal_frame_the_model_cannot_read_leaves_no_usage() {
    let (name, sample) = RESPONSES_STREAMS[0];
    let (bytes, ended) = forward(OpenaiResponses, sample).await;
    assert_eq!(bytes, sample, "{name}: the client sees the frames verbatim");

    let metadata = completed(ended);
    assert_eq!(
        (metadata.usage.input_tokens, metadata.usage.output_tokens),
        (0, 0),
        "the sample's terminal frame does not decode, so its counts never reach the ledger"
    );
}

#[tokio::test]
async fn a_chat_stream_translates_into_an_anthropic_one_that_names_the_same_tool_call() {
    let (bytes, ended) = translate(Anthropic, AnthropicToChat, CHAT_TOOL_STREAM).await;
    completed(ended);

    let blocks = tools(&bytes);
    assert_eq!(blocks.len(), 1, "the upstream names one tool call");
    let block = value_at(&blocks[0], "/content_block");
    assert_eq!(value_at(block, "/id"), TOOL_CALL_ID);
    assert_eq!(value_at(block, "/name"), "GetWeatherArgs");
    assert_eq!(
        serde_json::from_str::<Value>(&partial_json(&bytes)).ok(),
        serde_json::from_str::<Value>(TOOL_ARGUMENTS).ok(),
        "the deltas the client reads spell the arguments the upstream sent"
    );

    let delta = frame_of_type(&bytes, "message_delta");
    assert_eq!(
        value_at(&delta, "/delta/stop_reason"),
        "tool_use",
        "the upstream's tool call finish reason reads as a tool use stop reason"
    );
    assert_eq!(
        frames(&bytes).last().map(|last| last["type"].clone()),
        Some(json!("message_stop")),
        "the translated stream closes the message"
    );
}

/// One chunk of a chat stream, carrying only the fields the translator reads.
fn chat_chunk(delta: Value) -> String {
    format!(
        "data: {}\n\n",
        json!({
            "id": "c",
            "model": "m",
            "choices": [{"index": 0, "delta": delta, "finish_reason": null}],
        })
    )
}

fn chat_finished(reason: &str) -> String {
    format!(
        "data: {}\n\n",
        json!({
            "id": "c",
            "model": "m",
            "choices": [{"index": 0, "delta": {}, "finish_reason": reason}],
        })
    )
}

fn chat_tool_call(index: u32, id: Option<&str>, name: Option<&str>, arguments: &str) -> Value {
    let mut call = json!({"index": index, "function": {"arguments": arguments}});
    if let Some(id) = id {
        call["id"] = json!(id);
        call["type"] = json!("function");
    }
    if let Some(name) = name {
        call["function"]["name"] = json!(name);
    }
    call
}

/// The blocks the client was told about, as `(index, frame)` events, checking as
/// it walks that no frame writes to a block the client was told had stopped and
/// that no block is stopped twice.
fn block_frames(text: &str) -> Vec<(u64, String)> {
    let mut stopped: BTreeSet<u64> = BTreeSet::new();
    let mut seen: Vec<(u64, String)> = Vec::new();
    for event in frames(text) {
        let type_ = event["type"].as_str().unwrap_or_default().to_string();
        if !type_.starts_with("content_block") {
            continue;
        }
        let index = event["index"].as_u64().unwrap_or_default();
        match type_.as_str() {
            "content_block_start" => assert!(
                !stopped.contains(&index),
                "block {index} starts again after the client was told it stopped"
            ),
            "content_block_stop" => {
                assert!(stopped.insert(index), "block {index} stops twice");
            }
            "content_block_delta" => assert!(
                !stopped.contains(&index),
                "a delta writes to block {index}, which the client was told had stopped"
            ),
            _ => {}
        }
        seen.push((index, type_));
    }
    seen
}

#[tokio::test]
async fn a_tool_call_that_interrupts_text_closes_the_text_block_first() {
    let stream = concat!(
        "data: {\"id\":\"c\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_0\",\"type\":\"function\",\"function\":{\"name\":\"f\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let (bytes, ended) = translate(Anthropic, AnthropicToChat, stream).await;
    completed(ended);

    assert_eq!(
        block_frames(&bytes),
        [
            (0, "content_block_start"),
            (0, "content_block_delta"),
            (0, "content_block_stop"),
            (1, "content_block_start"),
            (1, "content_block_delta"),
            (1, "content_block_stop"),
        ]
        .map(|(index, type_)| (index, type_.to_string())),
        "each block the client is told about is opened and closed in turn"
    );
}

/// The `content_block_start` is the only frame the anthropic stream carries a
/// tool use's id and name in: no later frame revises them, and the client sends
/// the id back as the `tool_use_id` of the result it returns. A block announced
/// before the upstream names the call can therefore never carry the identity the
/// upstream sent, and the client could not name the call it answers.
#[tokio::test]
async fn a_tool_call_the_upstream_names_late_still_carries_its_identity() {
    let stream = [
        chat_chunk(json!({"tool_calls": [chat_tool_call(0, None, None, "{\"ci")]})),
        chat_chunk(
            json!({"tool_calls": [chat_tool_call(0, Some("call_1"), Some("f"), "ty\": \"")]}),
        ),
        chat_chunk(json!({"tool_calls": [chat_tool_call(0, None, None, "X\"}")]})),
        chat_finished("tool_calls"),
        "data: [DONE]\n\n".to_string(),
    ]
    .concat();
    let (bytes, ended) = translate(Anthropic, AnthropicToChat, &stream).await;
    completed(ended);

    let blocks = tools(&bytes);
    assert_eq!(blocks.len(), 1, "the upstream names one tool call");
    assert_eq!(
        value_at(&blocks[0], "/content_block/id"),
        "call_1",
        "the client reads the id the upstream sent"
    );
    assert_eq!(
        value_at(&blocks[0], "/content_block/name"),
        "f",
        "the client reads the name the upstream sent"
    );
    assert_eq!(
        partial_json(&bytes),
        "{\"city\": \"X\"}",
        "the arguments the upstream sent reach the client whole, in the order they arrived"
    );
}

/// A tool call the upstream never names cannot be announced: the block would
/// carry an id and a name the upstream never sent, and awitch writes what the
/// upstream sent rather than inventing the identity of a call a client has to
/// answer by id.
#[tokio::test]
async fn a_tool_call_the_upstream_never_identifies_is_not_announced() {
    let stream = [
        chat_chunk(json!({"content": "hi"})),
        chat_chunk(json!({"tool_calls": [chat_tool_call(0, None, None, "{}")]})),
        chat_finished("tool_calls"),
        "data: [DONE]\n\n".to_string(),
    ]
    .concat();
    let (bytes, ended) = translate(Anthropic, AnthropicToChat, &stream).await;
    completed(ended);

    assert!(
        tools(&bytes).is_empty(),
        "no block is announced for a call the upstream never identified"
    );
    assert_eq!(
        block_frames(&bytes),
        [
            (0, "content_block_start"),
            (0, "content_block_delta"),
            (0, "content_block_stop")
        ]
        .map(|(index, type_)| (index, type_.to_string())),
        "the text block is opened, written and closed"
    );
}

/// A chat stream names its tool calls by index and the arguments of one call, so
/// a stream that returns to a call it already finished has no anthropic block to
/// write the late arguments to. Writing them anyway would put a delta after the
/// stop the client already saw; starting a second block would hand the client
/// two blocks under one id.
#[tokio::test]
async fn a_return_to_a_stopped_tool_call_does_not_write_to_its_block() {
    let stream = [
        chat_chunk(json!({"tool_calls": [chat_tool_call(0, Some("call_0"), Some("f0"), "")]})),
        chat_chunk(json!({"tool_calls": [chat_tool_call(1, Some("call_1"), Some("f1"), "")]})),
        chat_chunk(json!({"tool_calls": [chat_tool_call(0, None, None, "{}")]})),
        chat_finished("tool_calls"),
        "data: [DONE]\n\n".to_string(),
    ]
    .concat();
    let (bytes, ended) = translate(Anthropic, AnthropicToChat, &stream).await;
    completed(ended);

    let blocks = tools(&bytes);
    assert_eq!(
        blocks.len(),
        2,
        "each call the upstream named got one block"
    );
    assert_eq!(value_at(&blocks[0], "/content_block/id"), "call_0");
    assert_eq!(value_at(&blocks[1], "/content_block/id"), "call_1");
    assert_eq!(
        block_frames(&bytes),
        [
            (0, "content_block_start"),
            (0, "content_block_stop"),
            (1, "content_block_start"),
            (1, "content_block_stop"),
        ]
        .map(|(index, type_)| (index, type_.to_string())),
        "each block is started and stopped once"
    );
}

#[tokio::test]
async fn a_chat_stream_translates_into_a_responses_one_that_reports_the_same_usage() {
    let (bytes, ended) = translate(OpenaiResponses, ResponsesToChat, CHAT_STREAM).await;
    let metadata = completed(ended);

    assert_eq!(
        responses_text(&bytes),
        chat_text(CHAT_STREAM),
        "the client reads the text the upstream streamed"
    );

    let done = frame_of_type(&bytes, "response.completed");
    assert_eq!(value_at(&done, "/response/usage/input_tokens"), 14);
    assert_eq!(value_at(&done, "/response/usage/output_tokens"), 30);
    assert_eq!(
        (metadata.usage.input_tokens, metadata.usage.output_tokens),
        (14, 30),
        "the translated counts reach the ledger"
    );
}

fn item_at<'a>(output: &'a [Value], event: &Value, what: &str) -> &'a Value {
    let index = value_at(event, "/output_index")
        .as_u64()
        .expect("the event carries its output index");
    assert!(
        (index as usize) < output.len(),
        "{what} names output index {index}, but the response puts {} items there",
        output.len()
    );
    &output[index as usize]
}

/// Every item event of a translated stream has to name the item the terminal
/// response reports at that index — an index a client cannot look up in the
/// response it terminates with is an index it cannot use.
async fn streamed_items_land_where_the_response_puts_them(stream: &str) {
    let (bytes, ended) = translate(OpenaiResponses, ResponsesToChat, stream).await;
    completed(ended);

    let done = frame_of_type(&bytes, "response.completed");
    let output = value_at(&done, "/response/output")
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !output.is_empty(),
        "the response reports the items the stream created"
    );

    let added = frames_of_type(&bytes, "response.output_item.added");
    assert_eq!(
        added.len(),
        output.len(),
        "the stream announces every item the response reports"
    );
    for event in &added {
        let item = item_at(&output, event, "response.output_item.added");
        assert_eq!(
            value_at(event, "/item/id"),
            value_at(item, "/id"),
            "the announced item is the one the response puts at that index"
        );
    }
    for event in frames_of_type(&bytes, "response.output_item.done") {
        assert_eq!(
            value_at(&event, "/item"),
            item_at(&output, &event, "response.output_item.done"),
            "the finished item is the one the response reports"
        );
    }
    for event in frames_of_type(&bytes, "response.function_call_arguments.delta") {
        assert_eq!(
            value_at(&event, "/item_id"),
            value_at(item_at(&output, &event, "an arguments delta"), "/id"),
            "an arguments delta names the item at its index"
        );
    }
}

#[tokio::test]
async fn a_tool_call_streams_into_the_items_the_response_reports() {
    streamed_items_land_where_the_response_puts_them(CHAT_TOOL_STREAM).await;
    streamed_items_land_where_the_response_puts_them(CHAT_TOOL_CALLS_STREAM).await;
}

/// `response.output_item.added` is where a client learns the `call_id` it answers
/// a call with, and the item the response reports at the end carries the same
/// call. An item announced before the upstream named the call would carry an
/// identity the upstream never sent, and the client would answer a call the
/// upstream does not know.
#[tokio::test]
async fn a_tool_item_is_announced_with_the_identity_the_upstream_sent() {
    let stream = [
        chat_chunk(json!({"tool_calls": [chat_tool_call(0, None, None, "{\"ci")]})),
        chat_chunk(
            json!({"tool_calls": [chat_tool_call(0, Some("call_1"), Some("f"), "ty\": \"")]}),
        ),
        chat_chunk(json!({"tool_calls": [chat_tool_call(0, None, None, "X\"}")]})),
        chat_finished("tool_calls"),
        "data: [DONE]\n\n".to_string(),
    ]
    .concat();
    let (bytes, ended) = translate(OpenaiResponses, ResponsesToChat, &stream).await;
    completed(ended);

    let added = frames_of_type(&bytes, "response.output_item.added");
    assert_eq!(added.len(), 1, "the upstream names one tool call");
    assert_eq!(value_at(&added[0], "/item/call_id"), "call_1");
    assert_eq!(value_at(&added[0], "/item/name"), "f");

    let completed = frame_of_type(&bytes, "response.completed");
    let item = value_at(&completed, "/response/output/0");
    assert_eq!(
        value_at(item, "/call_id"),
        value_at(&added[0], "/item/call_id"),
        "the item the response reports carries the call id the stream announced"
    );
    assert_eq!(value_at(item, "/name"), value_at(&added[0], "/item/name"));
    assert_eq!(
        value_at(item, "/arguments"),
        "{\"city\": \"X\"}",
        "the arguments the upstream sent reach the response whole, in the order they arrived"
    );
}

/// The part the terminal response reports is the part the stream finished: a
/// client that assembles its message from the frames and one that reads the
/// terminal response read the same content.
#[tokio::test]
async fn a_terminal_text_part_is_the_one_the_stream_finished() {
    let (bytes, ended) = translate(OpenaiResponses, ResponsesToChat, CHAT_STREAM).await;
    completed(ended);

    assert_eq!(
        value_at(
            &frame_of_type(&bytes, "response.completed"),
            "/response/output/0/content/0"
        ),
        value_at(
            &frame_of_type(&bytes, "response.content_part.done"),
            "/part"
        ),
        "the terminal content is what the part the stream finished carried"
    );
}

#[tokio::test]
async fn text_and_tool_items_keep_the_order_the_stream_opened_them_in() {
    let text_first = concat!(
        "data: {\"id\":\"c\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"f\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    streamed_items_land_where_the_response_puts_them(text_first).await;

    let (bytes, ended) = translate(OpenaiResponses, ResponsesToChat, text_first).await;
    completed(ended);
    let done = frame_of_type(&bytes, "response.completed");
    let output = value_at(&done, "/response/output")
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        output.len(),
        2,
        "the turn said something and then called one tool"
    );
    assert_eq!(value_at(&output[0], "/type"), "message");
    assert_eq!(value_at(&output[1], "/type"), "function_call");
    assert_eq!(
        value_at(&output[1], "/id"),
        "fc_1",
        "a tool item's id is its own index in the response"
    );

    let tool_first = concat!(
        "data: {\"id\":\"c\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"f\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    streamed_items_land_where_the_response_puts_them(tool_first).await;

    let (bytes, ended) = translate(OpenaiResponses, ResponsesToChat, tool_first).await;
    completed(ended);
    let done = frame_of_type(&bytes, "response.completed");
    let output = value_at(&done, "/response/output")
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        value_at(&output[0], "/type"),
        "function_call",
        "the response orders its items the way the stream opened them, \
         not the way the encoder happens to visit them"
    );
    assert_eq!(value_at(&output[0], "/id"), "fc_0");
    assert_eq!(value_at(&output[1], "/type"), "message");
    let done_indices: Vec<u64> = frames_of_type(&bytes, "response.output_item.done")
        .iter()
        .map(|event| event["output_index"].as_u64().unwrap_or_default())
        .collect();
    assert_eq!(
        done_indices,
        [0, 1],
        "an item is finished in the order the response reports it"
    );
}

#[test]
fn a_completion_with_text_and_a_tool_call_numbers_the_items_it_reports() {
    let body = serde_json::to_vec(&json!({
        "id": "chatcmpl-1",
        "model": "m",
        "choices": [{
            "index": 0,
            "finish_reason": "tool_calls",
            "message": {
                "role": "assistant",
                "content": "let me check",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "f", "arguments": "{}"}
                }]
            }
        }]
    }))
    .expect("the upstream body serializes");
    let (_, bytes) = Pair::resolve(Protocol::OpenaiResponses, Protocol::OpenaiChat)
        .serve(Bytes::from(body))
        .expect("the completion serves");
    let body: Value = serde_json::from_slice(&bytes).expect("the translated body is json");
    let output = value_at(&body, "/output")
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        output.len(),
        2,
        "the turn said something and then called one tool"
    );
    assert_eq!(value_at(&output[0], "/type"), "message");
    assert_eq!(value_at(&output[1], "/type"), "function_call");
    assert_eq!(
        value_at(&output[1], "/id"),
        "fc_1",
        "a tool item's id is its own index in the response, not its rank among the tools"
    );
}

#[test]
fn a_completion_the_upstream_cut_short_reports_its_item_as_incomplete() {
    let body = serde_json::to_vec(&json!({
        "id": "chatcmpl-1",
        "model": "m",
        "choices": [{
            "index": 0,
            "finish_reason": "length",
            "message": {"role": "assistant", "content": "a truncated thou"}
        }]
    }))
    .expect("the upstream body serializes");
    let (_, bytes) = Pair::resolve(Protocol::OpenaiResponses, Protocol::OpenaiChat)
        .serve(Bytes::from(body))
        .expect("the completion serves");
    let body: Value = serde_json::from_slice(&bytes).expect("the translated body is json");
    assert_eq!(value_at(&body, "/status"), "incomplete");
    assert_eq!(
        value_at(&body, "/incomplete_details/reason"),
        "max_output_tokens"
    );
    assert_eq!(
        value_at(&body, "/output/0/status"),
        "incomplete",
        "the message it cut short is not a completed one"
    );
}

#[test]
fn a_same_protocol_completion_reaches_the_client_unchanged() {
    let (metadata, bytes) = Pair::resolve(Protocol::OpenaiChat, Protocol::OpenaiChat)
        .serve(Bytes::from_static(CHAT_COMPLETION.as_bytes()))
        .expect("the completion serves");
    assert_eq!(bytes, CHAT_COMPLETION.as_bytes());
    assert_eq!(
        (metadata.usage.input_tokens, metadata.usage.output_tokens),
        (82, 17)
    );
}

#[test]
fn a_completion_translates_with_the_tool_call_the_upstream_returned() {
    let (metadata, bytes) = Pair::resolve(Protocol::Anthropic, Protocol::OpenaiChat)
        .serve(Bytes::from_static(CHAT_COMPLETION.as_bytes()))
        .expect("the completion serves");
    let body: Value = serde_json::from_slice(&bytes).expect("the translated body is json");
    let block = value_at(&body, "/content/0");
    assert_eq!(block["type"], "tool_use");
    assert_eq!(block["id"], "call_abc123");
    assert_eq!(block["name"], "get_current_weather");
    assert_eq!(block["input"], json!({"location": "Boston, MA"}));
    assert_eq!(body["stop_reason"], "tool_use");
    assert_eq!(
        (metadata.usage.input_tokens, metadata.usage.output_tokens),
        (82, 17)
    );

    let (metadata, bytes) = Pair::resolve(Protocol::OpenaiResponses, Protocol::OpenaiChat)
        .serve(Bytes::from_static(CHAT_COMPLETION.as_bytes()))
        .expect("the completion serves");
    let body: Value = serde_json::from_slice(&bytes).expect("the translated body is json");
    let item = value_at(&body, "/output/0");
    assert_eq!(item["type"], "function_call");
    assert_eq!(
        item["id"], "fc_0",
        "a tool item's id is its own index in the response"
    );
    assert_eq!(item["call_id"], "call_abc123");
    assert_eq!(item["name"], "get_current_weather");
    assert_eq!(item["arguments"], "{\n\"location\": \"Boston, MA\"\n}");
    assert_eq!(
        (metadata.usage.input_tokens, metadata.usage.output_tokens),
        (82, 17)
    );
}

#[test]
fn the_pair_table_resolves_every_supported_pair() {
    for protocol in [
        Protocol::Anthropic,
        Protocol::OpenaiChat,
        Protocol::OpenaiResponses,
    ] {
        assert!(matches!(Pair::resolve(protocol, protocol), Pair::Same(_)));
    }
    assert!(matches!(
        Pair::resolve(Protocol::Anthropic, Protocol::OpenaiChat),
        Pair::AnthropicToChat
    ));
    assert!(matches!(
        Pair::resolve(Protocol::OpenaiResponses, Protocol::OpenaiChat),
        Pair::ResponsesToChat
    ));
}
