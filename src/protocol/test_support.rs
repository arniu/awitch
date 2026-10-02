use std::collections::BTreeSet;

use serde::de::DeserializeOwned;
use serde_json::Value;

pub const DONE: &str = "[DONE]";

pub fn set_of(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| item.to_string()).collect()
}

pub fn data_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|data| data.trim().to_string())
        .filter(|data| !data.is_empty())
        .collect()
}

pub fn frames(text: &str) -> Vec<Value> {
    named_frames(text)
        .into_iter()
        .map(|(_, frame)| frame)
        .collect()
}

/// Each frame's payload, paired with the name the frame carries in its `event:`
/// line, where it carries one, which is where a client reads it.
pub fn named_frames(text: &str) -> Vec<(Option<&str>, Value)> {
    let mut frames = Vec::new();
    let mut name = None;
    for line in text.lines() {
        if let Some(event) = line.strip_prefix("event:") {
            name = Some(event.trim());
        } else if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if !data.is_empty() && data != DONE {
                frames.push((
                    name,
                    serde_json::from_str(data).expect("a vendored frame is a json payload"),
                ));
            }
        }
    }
    frames
}

pub fn frames_of_type(text: &str, type_: &str) -> Vec<Value> {
    frames(text)
        .into_iter()
        .filter(|frame| frame["type"] == type_)
        .collect()
}

pub fn frame_of_type(text: &str, type_: &str) -> Value {
    let frames = frames_of_type(text, type_);
    let [frame] = frames.as_slice() else {
        panic!(
            "the vendored fixture carries one {type_} frame, not {}",
            frames.len()
        );
    };
    frame.clone()
}

pub fn value_at<'a>(value: &'a Value, pointer: &str) -> &'a Value {
    value
        .pointer(pointer)
        .unwrap_or_else(|| panic!("the value carries nothing at {pointer}"))
}

pub fn modelled_required(required: &[String], unmodelled: &[&str]) -> Vec<String> {
    required
        .iter()
        .filter(|field| !unmodelled.contains(&field.as_str()))
        .cloned()
        .collect()
}

pub fn spec_required(text: &str) -> Vec<String> {
    let required: Vec<String> =
        serde_json::from_str(text).expect("an extracted required list is a json list");
    assert!(
        !required.is_empty(),
        "an extracted required list is never empty"
    );
    required
}

pub fn without(value: &Value, field: &str) -> Value {
    let mut value = value.clone();
    let removed = value
        .as_object_mut()
        .expect("a mutation base is an object")
        .remove(field);
    assert!(
        removed.is_some(),
        "{field} is not in the mutation base, so its requiredness is untested"
    );
    value
}

pub fn assert_required<T: DeserializeOwned>(
    label: &str,
    value: &Value,
    fields: &[impl AsRef<str>],
) {
    assert!(
        serde_json::from_value::<T>(value.clone()).is_ok(),
        "{label}: the whole object decodes"
    );
    for field in fields {
        let field = field.as_ref();
        assert!(
            serde_json::from_value::<T>(without(value, field)).is_err(),
            "{label}: {field} is required"
        );
    }
}
