//! Bindings to the Quill rich-text editor loaded from a CDN in `index.html`,
//! and the subset of its Delta document model we consume.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
pub struct QuillConfig {
    pub theme: &'static str,
    pub modules: QuillModules,
    /// Quill renders read-only content without the editing affordances, but
    /// keeps the same Delta-driven rendering path.
    #[serde(rename = "readOnly")]
    pub read_only: bool,
}

/// Quill's `toolbar` option is tri-state: an array builds a toolbar, `false`
/// suppresses it, and *absent* means "use the default toolbar". Serde maps
/// `None` to `undefined`, which Quill reads as absent — so view mode must send
/// an explicit `false` rather than omitting the key.
#[derive(Serialize)]
#[serde(untagged)]
pub enum Toolbar {
    Names(&'static [&'static str]),
    Disabled(bool),
}

#[derive(Serialize)]
pub struct QuillModules {
    pub toolbar: Toolbar,
}

#[wasm_bindgen]
extern "C" {
    pub type Quill;

    #[wasm_bindgen(constructor)]
    pub fn new(selector: &str, options: &JsValue) -> Quill;

    #[wasm_bindgen(method, js_name = insertText)]
    pub fn insert_text(this: &Quill, index: usize, text: &str, source: &str);

    #[wasm_bindgen(method, js_name = deleteText)]
    pub fn delete_text(this: &Quill, index: usize, length: usize, source: &str);

    #[wasm_bindgen(method, js_name = getContents)]
    pub fn get_contents(this: &Quill) -> JsValue;

    #[wasm_bindgen(method, js_name = setContents)]
    pub fn set_contents(this: &Quill, delta: &JsValue, source: &str);

    #[wasm_bindgen(method, js_name = enable)]
    pub fn enable(this: &Quill, enabled: bool);

    #[wasm_bindgen(method, js_name = formatText)]
    pub fn format_text(
        this: &Quill,
        index: usize,
        length: usize,
        name: &str,
        value: &JsValue,
        source: &str,
    );

    #[wasm_bindgen(method, js_name = removeFormat)]
    pub fn remove_format(this: &Quill, index: usize, length: usize, source: &str);

    #[wasm_bindgen(method, js_name = on)]
    pub fn on(this: &Quill, event: &str, handler: &js_sys::Function);

    #[wasm_bindgen(method, js_name = getModule)]
    pub fn get_module(this: &Quill, name: &str) -> JsValue;
}

/// The subset of Quill's Delta ops we care about: plain-text inserts carrying
/// optional formatting attributes (bold/italic/header/list/link/code).
#[derive(Deserialize)]
pub struct DeltaOp {
    #[serde(default)]
    pub insert: Option<String>,
    #[serde(default)]
    pub attributes: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Deserialize)]
pub struct Delta {
    pub ops: Vec<DeltaOp>,
}
