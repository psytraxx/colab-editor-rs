//! Formatting marks: the bridge between Quill's Delta `attributes` and
//! Automerge's `Mark` spans.

use crate::quill::Delta;
use wasm_bindgen::prelude::*;

/// Formatting attribute names we track as Automerge marks. This is the single
/// source of truth: the Quill toolbar in `init_quill` is derived from it, and
/// every mark read/write filters against it.
pub const MARK_NAMES: &[&str] = &[
    "bold",
    "italic",
    "underline",
    "header",
    "list",
    "link",
    "code",
];

pub fn scalar_from_json(value: &serde_json::Value) -> Option<automerge::ScalarValue> {
    match value {
        serde_json::Value::Bool(b) => Some((*b).into()),
        serde_json::Value::String(s) => Some(s.clone().into()),
        serde_json::Value::Number(n) => Some(n.to_string().into()),
        _ => None,
    }
}

pub fn json_from_scalar(value: &automerge::ScalarValue) -> JsValue {
    match value {
        automerge::ScalarValue::Boolean(b) => JsValue::from_bool(*b),
        automerge::ScalarValue::Str(s) => JsValue::from_str(s),
        other => JsValue::from_str(&other.to_string()),
    }
}

/// A single formatting span: `(name, value, start, end)` over character offsets.
pub type Span = (String, automerge::ScalarValue, usize, usize);

/// Walk a Quill Delta and collect the formatting spans it implies, in
/// character offsets. Shared by the "did formatting change?" comparison and
/// the "write formatting to the CRDT" path, which would otherwise duplicate
/// this traversal verbatim.
pub fn delta_spans(delta: &Delta) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut pos = 0usize;
    for op in &delta.ops {
        let Some(text) = &op.insert else { continue };
        let span_len = text.chars().count();
        if let Some(attrs) = &op.attributes {
            for (name, value) in attrs {
                if !MARK_NAMES.contains(&name.as_str()) {
                    continue;
                }
                if let Some(scalar) = scalar_from_json(value) {
                    spans.push((name.clone(), scalar, pos, pos + span_len));
                }
            }
        }
        pos += span_len;
    }
    spans
}

/// Builds a per-character "active mark set" over `[0, len)` from a list of
/// spans, so formatting can be compared positionally instead of by raw (and
/// possibly differently-split) spans.
pub fn mark_fingerprint(len: usize, spans: &[Span]) -> Vec<Vec<(&str, String)>> {
    let mut fp = vec![Vec::new(); len];
    for (name, value, start, end) in spans {
        let entry = (name.as_str(), value.to_string());
        for slot in fp.iter_mut().take((*end).min(len)).skip(*start) {
            slot.push(entry.clone());
        }
    }
    for slot in &mut fp {
        slot.sort();
    }
    fp
}
