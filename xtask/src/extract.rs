use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::manifest::Kind;

pub fn document(bytes: &[u8], rel: &str) -> Result<Value> {
    serde_json::from_slice(bytes)
        .with_context(|| format!("{rel}: the fetched document is not json"))
}

pub fn pointer<'a>(document: &'a Value, pointer: &str, rel: &str) -> Result<&'a Value> {
    document
        .pointer(pointer)
        .with_context(|| format!("{rel}: the document carries nothing at {pointer}"))
}

pub fn render(value: &Value, rel: &str, kind: Kind) -> Result<Vec<u8>> {
    match kind {
        Kind::SpecExample => {
            let Some(text) = value.as_str() else {
                bail!("{rel}: the extracted example is not text");
            };
            if text.trim().is_empty() {
                bail!("{rel}: the extracted example is empty");
            }
            Ok(text.as_bytes().to_vec())
        }
        Kind::SpecSchema => {
            let required = required_list(value, rel)?;
            let mut text = serde_json::to_string_pretty(&required)
                .with_context(|| format!("{rel}: the required list does not render"))?;
            text.push('\n');
            Ok(text.into_bytes())
        }
        Kind::Recorded | Kind::Synthetic => {
            bail!("{rel}: a stream fixture is vendored whole, not extracted")
        }
    }
}

pub fn required_list(value: &Value, rel: &str) -> Result<Vec<String>> {
    let Some(items) = value.as_array() else {
        bail!("{rel}: the extracted required list is not a list");
    };
    let mut required = Vec::new();
    for item in items {
        let Some(name) = item.as_str() else {
            bail!("{rel}: the extracted required list holds a non-string");
        };
        required.push(name.to_string());
    }
    if required.is_empty() {
        bail!("{rel}: the extracted required list is empty");
    }
    Ok(required)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Value {
        document(text.as_bytes(), "test").expect("the sample is json")
    }

    #[test]
    fn a_pointer_walks_fields_and_items() {
        let document =
            parse(r#"{"a": {"b": ["one", "two"]}, "paths": {"/x": {"post": {"ok": true}}}}"#);
        assert_eq!(
            pointer(&document, "/a/b/1", "test").expect("the pointer resolves"),
            &Value::from("two")
        );
        assert_eq!(
            pointer(&document, "/paths/~1x/post/ok", "test").expect("the pointer resolves"),
            &Value::from(true)
        );
        assert!(pointer(&document, "/a/missing", "test").is_err());
    }

    #[test]
    fn a_required_list_is_a_list_of_names() {
        let document = parse(r#"{"required": ["choices", "id", "model"]}"#);
        let value = pointer(&document, "/required", "test").expect("the pointer resolves");
        assert_eq!(
            required_list(value, "test").expect("the list renders"),
            ["choices", "id", "model"]
        );
        assert!(required_list(&parse(r#"{"required": []}"#), "test").is_err());
        assert!(required_list(&parse(r#"{"required": "id"}"#), "test").is_err());
    }

    #[test]
    fn an_example_renders_verbatim() {
        let document = parse(r#"{"response": "data: {\"a\": 1}\n"}"#);
        let value = pointer(&document, "/response", "test").expect("the pointer resolves");
        let bytes = render(value, "test", Kind::SpecExample).expect("the example renders");
        assert_eq!(bytes, b"data: {\"a\": 1}\n");
    }

    #[test]
    fn a_stream_fixture_is_never_extracted() {
        let document = parse(r#"{"required": ["id"]}"#);
        let value = pointer(&document, "/required", "test").expect("the pointer resolves");
        assert!(render(value, "test", Kind::Recorded).is_err());
    }
}
