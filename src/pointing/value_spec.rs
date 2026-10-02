//! A managed key's write template and the fields it carries.
//!
//! Two entities live here, split by direction:
//!
//! - [`Field`] is a target field (the gateway url / the app key) — the thing
//!   that is *written into* and *read back* from a doc. It knows its template
//!   mark (`{url}` / `{key}`), its value in a [`Target`] (whose fields mirror
//!   the [`Field`] variants), and how to read itself back from a current value.
//! - [`ValueSpec`] is how a managed key is *written*: which of a few fixed
//!   templates the installed value takes (a whole field as scalar, a fixed
//!   pointer, or a JSON entry whose string fields carry field marks).
//!   Writing only — it renders, never reads.
//!
//! Forward (`ValueSpec::render`): substitute each field's mark with the
//! field's value in the target and parse the JSON.
//!
//! Reverse (`Field::read`): a config doc holds no marker saying which piece
//! is the url and which is the key — only the write template records where
//! each field sits. Read-back is therefore template-first: swap the field's
//! mark for a unique sentinel, parse the template as JSON, walk to the
//! sentinel leaf to get its key path, then take that same path in the live
//! value. `render` put the field exactly there, so the paths agree — and
//! indexing the *live* value means later manual edits surface too.
//!
//! A mark must occupy a complete quoted JSON string (a whole string value,
//! never part of a larger literal or a bare key): forward so the filled text
//! parses, reverse so the sentinel lands as a plain leaf value.

use anyhow::anyhow;
use serde_json::Value;

/// What a point points at — the two fields an agent config is pointed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub url: String,
    pub key: String,
}

/// A target field — what is written into and read back from the doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The gateway url.
    Url,
    /// The app key.
    Key,
}

impl Field {
    pub const ALL: [Field; 2] = [Field::Url, Field::Key];

    /// The field's template mark, e.g. `{url}`.
    fn mark(self) -> &'static str {
        match self {
            Field::Url => "{url}",
            Field::Key => "{key}",
        }
    }

    /// The field's value in a target.
    fn value(self, target: &Target) -> &str {
        match self {
            Field::Url => &target.url,
            Field::Key => &target.key,
        }
    }

    /// Read the field back from a value written with `spec`: the whole value
    /// for a whole-field key, the marked field of an entry, nothing otherwise.
    pub fn read<'v>(self, value: &'v Value, spec: &ValueSpec) -> Option<&'v Value> {
        match spec {
            ValueSpec::Entry(t) => entry_value_at(value, t, self.mark()),
            ValueSpec::Url if self == Field::Url => Some(value),
            ValueSpec::Key if self == Field::Key => Some(value),
            _ => None,
        }
    }
}

/// How a managed key's value is written — a fixed template, never a reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSpec {
    /// The whole value is the gateway url (a url-bearing key).
    Url,
    /// The whole value is the app key (a key-bearing key).
    Key,
    /// The whole value is a fixed string — the provider pointer.
    Pointer(&'static str),
    /// The value is a provider entry object; its string fields may carry
    /// field marks (see `Field::mark`).
    Entry(&'static str),
}

impl ValueSpec {
    /// Render the value to write for a target.
    pub fn render(&self, target: &Target) -> anyhow::Result<Value> {
        match self {
            ValueSpec::Url => Ok(Value::String(Field::Url.value(target).to_string())),
            ValueSpec::Key => Ok(Value::String(Field::Key.value(target).to_string())),
            ValueSpec::Pointer(p) => Ok(Value::String(p.to_string())),
            ValueSpec::Entry(t) => fill_entry(t, target),
        }
    }
}

fn quoted(s: &str) -> String {
    format!("\"{s}\"")
}

/// Fill an entry template's field marks with the target's values and parse it.
fn fill_entry(template: &str, target: &Target) -> anyhow::Result<Value> {
    let mut filled = template.to_string();
    for field in Field::ALL {
        let value = serde_json::to_string(field.value(target))?;
        filled = filled.replace(&quoted(field.mark()), &value);
    }

    serde_json::from_str(&filled).map_err(|e| anyhow!("bad entry after fill: {e}"))
}

/// Index into a rendered value at the spot its entry template gave `mark`.
fn entry_value_at<'a>(value: &'a Value, template: &str, mark: &str) -> Option<&'a Value> {
    let path = mark_path(template, mark)?;
    let mut cur = value;
    for seg in &path {
        cur = cur.get(seg.as_str())?;
    }
    Some(cur)
}

/// Where a template's quoted `mark` sits, as a key path.
///
/// The mark is a whole quoted string value (e.g. `"{url}"` in
/// `{"baseUrl":"{url}"}`). Swapping it for a unique sentinel keeps the text
/// valid JSON, so parsing succeeds and walking to the sentinel leaf yields
/// the key path — the same path any value rendered from this template
/// carries the field at.
fn mark_path(template: &str, mark: &str) -> Option<Vec<String>> {
    let needle = quoted(mark);
    let sentinel = format!("__AWITCH_{}__", &mark[1..mark.len() - 1]);
    let filled = template.replace(&needle, &quoted(&sentinel));
    let v: Value = serde_json::from_str(&filled).ok()?;
    walk_to(&v, &sentinel)
}

/// The key path from the root to the leaf equal to `needle`, if any — built
/// leaf-up: each enclosing object key is prepended on the way out.
fn walk_to(v: &Value, needle: &str) -> Option<Vec<String>> {
    match v {
        Value::String(s) if s == needle => Some(Vec::new()),
        Value::Object(m) => m.iter().find_map(|(k, val)| {
            walk_to(val, needle).map(|mut p| {
                p.insert(0, k.clone());
                p
            })
        }),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> Target {
        Target {
            url: "http://127.0.0.1:10689".into(),
            key: "tok".into(),
        }
    }

    #[test]
    fn mark_path_nested_and_flat() {
        let opencode =
            r#"{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"{url}"},"models":{}}"#;
        assert_eq!(
            mark_path(opencode, Field::Url.mark()),
            Some(vec!["options".into(), "baseURL".into()])
        );
        let pi = r#"{"baseUrl":"{url}","api":"openai-completions","apiKey":"{key}"}"#;
        assert_eq!(
            mark_path(pi, Field::Url.mark()),
            Some(vec!["baseUrl".into()])
        );
        assert_eq!(
            mark_path(pi, Field::Key.mark()),
            Some(vec!["apiKey".into()])
        );
        // A field the template does not carry.
        assert_eq!(mark_path(opencode, Field::Key.mark()), None);
    }

    #[test]
    fn fill_entry_and_read_back_round_trip() {
        let t = r#"{"baseUrl":"{url}","api":"openai-completions","apiKey":"{key}"}"#;
        let v = fill_entry(t, &target()).unwrap();
        let want_url: Value = Value::String("http://127.0.0.1:10689".into());
        let want_tok: Value = Value::String("tok".into());
        assert_eq!(entry_value_at(&v, t, Field::Url.mark()), Some(&want_url));
        assert_eq!(entry_value_at(&v, t, Field::Key.mark()), Some(&want_tok));
        assert_eq!(v["api"], "openai-completions");
    }

    #[test]
    fn fill_entry_rejects_malformed() {
        assert!(fill_entry(r#"{"base_url": "{url}""#, &target()).is_err());
    }

    #[test]
    fn render_url_key_pointer_and_entry() {
        assert_eq!(
            ValueSpec::Url.render(&target()).unwrap(),
            Value::String("http://127.0.0.1:10689".into())
        );
        assert_eq!(
            ValueSpec::Key.render(&target()).unwrap(),
            Value::String("tok".into())
        );
        assert_eq!(
            ValueSpec::Pointer("awitch").render(&target()).unwrap(),
            Value::String("awitch".into())
        );
        let t = ValueSpec::Entry(r#"{"baseUrl":"{url}","apiKey":"{key}"}"#);
        assert_eq!(
            t.render(&target()).unwrap(),
            serde_json::json!({"baseUrl":"http://127.0.0.1:10689","apiKey":"tok"})
        );
    }

    #[test]
    fn field_read_back_locates_url_and_key() {
        let url = Value::String("http://127.0.0.1:10689".into());
        let spec = ValueSpec::Url;
        assert_eq!(Field::Url.read(&url, &spec), Some(&url));
        assert_eq!(Field::Key.read(&url, &spec), None);

        let key = Value::String("tok".into());
        assert_eq!(Field::Key.read(&key, &ValueSpec::Key), Some(&key));
        assert_eq!(Field::Url.read(&key, &ValueSpec::Key), None);
        assert_eq!(Field::Url.read(&url, &ValueSpec::Pointer("awitch")), None);

        let entry = ValueSpec::Entry(r#"{"baseUrl":"{url}","apiKey":"{key}"}"#);
        let v = entry.render(&target()).unwrap();
        assert_eq!(
            Field::Key.read(&v, &entry).and_then(Value::as_str),
            Some("tok")
        );
        assert_eq!(
            Field::Url.read(&v, &entry).and_then(Value::as_str),
            Some("http://127.0.0.1:10689")
        );
    }
}
