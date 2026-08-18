use automerge::{
    patches::PatchAction, transaction::Transactable, AutoCommit, ObjId, ObjType, ReadDoc,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use web_sys::{console::log_1, CloseEvent, ErrorEvent, MessageEvent, WebSocket};
use yew::prelude::*;

// Document field keys
const DOC_KEY_TITLE: &str = "title";
const DOC_KEY_BODY: &str = "body";
const DOC_KEY_KEYWORDS: &str = "keywords";
const DOC_KEY_VERSION: &str = "version";

// WebSocket message types
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
enum WsMessage {
    Init {
        user_id: String,
        snapshot: Option<Vec<u8>>,
        users: Vec<UserState>,
    },
    Content(Vec<u8>),
    UserState(UserState),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct UserState {
    user_id: String,
    online: bool,
    editing: bool,
}

#[derive(Serialize)]
struct QuillConfig {
    theme: &'static str,
    modules: QuillModules,
}

#[derive(Serialize)]
struct QuillModules {
    toolbar: &'static [&'static str],
}

#[wasm_bindgen]
extern "C" {
    type Quill;

    #[wasm_bindgen(constructor)]
    fn new(selector: &str, options: &JsValue) -> Quill;

    #[wasm_bindgen(method, js_name = insertText)]
    fn insert_text(this: &Quill, index: usize, text: &str, source: &str);

    #[wasm_bindgen(method, js_name = deleteText)]
    fn delete_text(this: &Quill, index: usize, length: usize, source: &str);

    #[wasm_bindgen(method, js_name = getText)]
    fn get_text(this: &Quill) -> String;

    #[wasm_bindgen(method, js_name = getContents)]
    fn get_contents(this: &Quill) -> JsValue;

    #[wasm_bindgen(method, js_name = formatText)]
    fn format_text(
        this: &Quill,
        index: usize,
        length: usize,
        name: &str,
        value: &JsValue,
        source: &str,
    );

    #[wasm_bindgen(method, js_name = removeFormat)]
    fn remove_format(this: &Quill, index: usize, length: usize, source: &str);

    #[wasm_bindgen(method, js_name = on)]
    fn on(this: &Quill, event: &str, handler: &js_sys::Function);

    #[wasm_bindgen(method, js_name = getModule)]
    fn get_module(this: &Quill, name: &str) -> JsValue;
}

/// The subset of Quill's Delta ops we care about: plain-text inserts carrying
/// optional formatting attributes (bold/italic/header/list/link/code).
#[derive(Deserialize)]
struct DeltaOp {
    #[serde(default)]
    insert: Option<String>,
    #[serde(default)]
    attributes: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Deserialize)]
struct Delta {
    ops: Vec<DeltaOp>,
}

/// Formatting attribute names we track as Automerge marks, matching the Quill
/// toolbar configured in `init_quill`.
const MARK_NAMES: &[&str] = &[
    "bold",
    "italic",
    "underline",
    "header",
    "list",
    "link",
    "code",
];

fn scalar_from_json(value: &serde_json::Value) -> Option<automerge::ScalarValue> {
    match value {
        serde_json::Value::Bool(b) => Some((*b).into()),
        serde_json::Value::String(s) => Some(s.clone().into()),
        serde_json::Value::Number(n) => Some(n.to_string().into()),
        _ => None,
    }
}

fn json_from_scalar(value: &automerge::ScalarValue) -> JsValue {
    match value {
        automerge::ScalarValue::Boolean(b) => JsValue::from_bool(*b),
        automerge::ScalarValue::Str(s) => JsValue::from_str(s),
        other => JsValue::from_str(&other.to_string()),
    }
}

/// Escapes text for safe use as both HTML content and (quoted) attribute
/// values, since `render_body_html` uses it for both.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Builds a per-character "active mark set" over `[0, len)` from a list of
/// `(name, value, start, end)` spans, so formatting can be compared
/// positionally instead of by raw (and possibly differently-split) spans.
fn mark_fingerprint(
    len: usize,
    spans: &[(String, String, usize, usize)],
) -> Vec<Vec<(String, String)>> {
    let mut fp = vec![Vec::new(); len];
    for (name, value, start, end) in spans {
        for slot in fp.iter_mut().take((*end).min(len)).skip(*start) {
            slot.push((name.clone(), value.clone()));
        }
    }
    for slot in &mut fp {
        slot.sort();
    }
    fp
}

struct App {
    doc: AutoCommit,
    mode: Mode,
    ws: Option<WebSocket>,
    editor: Option<Quill>,
    users: HashMap<String, UserState>,
    my_id: Option<String>,
}

#[derive(PartialEq, Clone)]
enum Mode {
    View,
    Edit,
}

enum Msg {
    WsMessage(WsMessage),
    WsConnected,
    WsClosed,
    WsError(String),
    UpdateField(&'static str, String),
    SetMode(Mode),
    QuillTextChanged,
}

impl Component for App {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        // Initialize EMPTY document - server is source of truth
        let doc = AutoCommit::new();

        // Connect to WebSocket server
        // Automatically detect environment
        let ws_url = if web_sys::window()
            .and_then(|w| w.location().hostname().ok())
            .map(|h| h == "localhost" || h == "127.0.0.1")
            .unwrap_or(false)
        {
            "ws://localhost:8787/ws"
        } else {
            "wss://colab-editor-rs.dynamicflash.workers.dev/ws"
        };

        let ws = WebSocket::new(ws_url).ok();

        if let Some(ref websocket) = ws {
            let link = ctx.link().clone();

            // Setup onmessage
            let link_msg = link.clone();
            let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
                if let Some(txt) = e.data().as_string() {
                    // Log abbreviated message to avoid console spam with large snapshots
                    let log_txt = if txt.chars().count() > 100 {
                        let head: String = txt.chars().take(100).collect();
                        format!("{}...", head)
                    } else {
                        txt.clone()
                    };
                    log_1(&format!("[WS] Received: {}", log_txt).into());

                    match serde_json::from_str::<WsMessage>(&txt) {
                        Ok(msg) => {
                            link_msg.send_message(Msg::WsMessage(msg));
                        }
                        Err(e) => {
                            log_1(&format!("[WS] Failed to parse message: {:?}", e).into());
                        }
                    }
                }
            }) as Box<dyn FnMut(MessageEvent)>);
            websocket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            onmessage.forget();

            // Setup onopen
            let link_open = link.clone();
            let onopen = Closure::wrap(Box::new(move |_| {
                log_1(&"[WS] Connected!".into());
                link_open.send_message(Msg::WsConnected);
            }) as Box<dyn FnMut(JsValue)>);
            websocket.set_onopen(Some(onopen.as_ref().unchecked_ref()));
            onopen.forget();

            // Setup onclose
            let link_close = link.clone();
            let onclose = Closure::wrap(Box::new(move |_: CloseEvent| {
                log_1(&"[WS] Disconnected".into());
                link_close.send_message(Msg::WsClosed);
            }) as Box<dyn FnMut(CloseEvent)>);
            websocket.set_onclose(Some(onclose.as_ref().unchecked_ref()));
            onclose.forget();

            // Setup onerror
            let link_error = link.clone();
            let onerror = Closure::wrap(Box::new(move |e: ErrorEvent| {
                log_1(&format!("[WS] Error: {:?}", e.message()).into());
                link_error.send_message(Msg::WsError(e.message()));
            }) as Box<dyn FnMut(ErrorEvent)>);
            websocket.set_onerror(Some(onerror.as_ref().unchecked_ref()));
            onerror.forget();
        } else {
            log_1(&"[WS] Failed to create WebSocket".into());
        }

        Self {
            doc,
            mode: Mode::View,
            ws,
            editor: None,
            users: HashMap::new(),
            my_id: None,
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::WsMessage(ws_msg) => {
                match ws_msg {
                    WsMessage::Init {
                        user_id,
                        snapshot,
                        users,
                    } => {
                        log_1(&format!("[WS] Init! My ID: {}", user_id).into());
                        self.my_id = Some(user_id.clone());

                        // Load snapshot if present
                        if let Some(data) = snapshot {
                            match AutoCommit::load(&data) {
                                Ok(doc) => {
                                    self.apply_incoming_doc(doc, true);
                                }
                                Err(_) => log_1(&"[WS] Failed to load snapshot".into()),
                            }
                        }

                        // Populate users
                        self.users.clear();
                        for user in users {
                            if user.online {
                                self.users.insert(user.user_id.clone(), user);
                            }
                        }

                        // Add self to users list
                        self.users.insert(
                            user_id.clone(),
                            UserState {
                                user_id,
                                online: true,
                                editing: false,
                            },
                        );

                        true
                    }
                    WsMessage::Content(data) => {
                        log_1(
                            &format!("[WS] Received Content update, {} bytes", data.len()).into(),
                        );
                        match AutoCommit::load(&data) {
                            Ok(remote_doc) => self.apply_incoming_doc(remote_doc, false),
                            Err(_) => {
                                log_1(&"[WS] Failed to load remote content".into());
                                false
                            }
                        }
                    }
                    WsMessage::UserState(user_state) => self.handle_user_state(user_state),
                }
            }
            Msg::WsConnected => {
                log_1(&"[WS] WebSocket connected!".into());
                true
            }
            Msg::WsClosed => {
                log_1(&"[WS] WebSocket closed".into());
                self.ws = None;
                true
            }
            Msg::WsError(err) => {
                log_1(&format!("[WS] WebSocket error: {}", err).into());
                false
            }
            Msg::UpdateField(key, value) => self.set_field(key, value),
            Msg::SetMode(new_mode) => {
                if new_mode == self.mode {
                    return false;
                }
                match new_mode {
                    Mode::Edit => self.set_editing(true),
                    Mode::View => {
                        // Sync any trailing edit before tearing the editor down.
                        self.sync_body_from_editor();
                        self.teardown_quill();
                        self.set_editing(false);
                    }
                }
                self.mode = new_mode;
                true
            }
            Msg::QuillTextChanged => {
                self.sync_body_from_editor();
                false
            }
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, _first_render: bool) {
        // Initialize Quill only once the edit-mode DOM node actually exists.
        if self.mode == Mode::Edit && self.editor.is_none() {
            self.init_quill(ctx);
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let title = self.get_str(DOC_KEY_TITLE);
        let keywords = self.get_str(DOC_KEY_KEYWORDS);
        let body_html = self.render_body_html();
        let version = self.get_u64(DOC_KEY_VERSION);

        html! {
            <main class="container">
                <header class="app-header">
                    <div class="connection-status">
                        if self.ws.is_some() {
                            <mark>{ format!("Connected — {} user(s)", self.users.len()) }</mark>
                        } else {
                            <span>{"Connecting to server…"}</span>
                        }
                    </div>

                    <div class="online-users">
                        { for self.users.values().map(|user| {
                            let class = if user.editing {
                                "user-badge editing"
                            } else {
                                "user-badge inactive"
                            };
                            html! {
                                <span class={class}>
                                    { &user.user_id }
                                </span>
                            }
                        })}
                    </div>
                </header>

                <div class="mode-switch-row">
                    <fieldset class="mode-switch">
                        <label>
                            <input
                                type="checkbox"
                                role="switch"
                                checked={self.mode == Mode::Edit}
                                onclick={ctx.link().callback(|e: MouseEvent| {
                                    let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                    Msg::SetMode(if input.checked() { Mode::Edit } else { Mode::View })
                                })}
                            />
                            { if self.mode == Mode::Edit { "Edit mode" } else { "View mode" } }
                        </label>
                    </fieldset>
                    <span class="version-pill">{ format!("v{}", version) }</span>
                </div>

                if self.mode == Mode::View {
                    <article class="content-card view-mode">
                        <h2 class="view-title">{ title }</h2>
                        if !keywords.trim().is_empty() {
                            <div class="keyword-chips">
                                { for keywords.split(',').map(|kw| kw.trim()).filter(|kw| !kw.is_empty()).map(|kw| {
                                    html! { <span class="keyword-chip">{ kw }</span> }
                                })}
                            </div>
                        }
                        <div class="body-content" style="white-space: pre-wrap;">{Html::from_html_unchecked(body_html.into())}</div>
                    </article>
                } else {
                    <article class="content-card edit-mode">
                        <div class="field">
                            <label>{ "Title" }</label>
                            <input
                                key="title"
                                type="text"
                                value={title}
                                oninput={ctx.link().callback(|e: InputEvent| {
                                    let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                    Msg::UpdateField(DOC_KEY_TITLE, input.value())
                                })}
                            />
                        </div>

                        <div class="field">
                            <label>{ "Keywords" }</label>
                            <input
                                key="keywords"
                                type="text"
                                value={keywords}
                                oninput={ctx.link().callback(|e: InputEvent| {
                                    let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                    Msg::UpdateField(DOC_KEY_KEYWORDS, input.value())
                                })}
                            />
                        </div>

                        <div class="field">
                            <label>{ "Body" }</label>
                            <div id="body-editor" class="quill-editor"></div>
                        </div>
                    </article>
                }
            </main>
        }
    }
}

impl App {
    fn get_scalar(&self, key: &str) -> Option<automerge::ScalarValue> {
        match self.doc.get(automerge::ROOT, key).ok().flatten()?.0 {
            automerge::Value::Scalar(s) => Some(s.into_owned()),
            automerge::Value::Object(_) => None,
        }
    }

    fn get_str(&self, key: &str) -> String {
        match self.get_scalar(key) {
            Some(automerge::ScalarValue::Str(s)) => s.to_string(),
            _ => String::new(),
        }
    }

    fn get_u64(&self, key: &str) -> u64 {
        match self.get_scalar(key) {
            Some(automerge::ScalarValue::Uint(u)) => u,
            Some(automerge::ScalarValue::Int(i)) => i as u64,
            Some(automerge::ScalarValue::F64(f)) => f as u64,
            _ => 0,
        }
    }

    /// The body field is stored as an Automerge `Text` CRDT object (not a plain
    /// scalar string), so that local/remote edits can be applied as minimal
    /// character-level splices instead of whole-document replacement.
    fn body_obj(&mut self) -> ObjId {
        if let Ok(Some((automerge::Value::Object(ObjType::Text), id))) =
            self.doc.get(automerge::ROOT, DOC_KEY_BODY)
        {
            return id;
        }
        self.doc
            .put_object(automerge::ROOT, DOC_KEY_BODY, ObjType::Text)
            .unwrap()
    }

    fn body_obj_ref(&self) -> Option<ObjId> {
        match self.doc.get(automerge::ROOT, DOC_KEY_BODY) {
            Ok(Some((automerge::Value::Object(ObjType::Text), id))) => Some(id),
            _ => None,
        }
    }

    fn get_body(&self) -> String {
        match self.body_obj_ref() {
            Some(id) => self.doc.text(&id).unwrap_or_default(),
            None => String::new(),
        }
    }

    /// Render the body as HTML for the read-only View mode, reconstructing
    /// inline formatting (bold/italic/underline/code/link) from Automerge
    /// marks. Header/list are block-level in Quill's model (attached to the
    /// line-ending character, not a text span) and are not reconstructed
    /// here — View mode shows their text content without that structure.
    fn render_body_html(&self) -> String {
        let Some(body_obj) = self.body_obj_ref() else {
            return String::new();
        };
        let text: Vec<char> = self
            .doc
            .text(&body_obj)
            .unwrap_or_default()
            .chars()
            .collect();
        let marks: Vec<automerge::marks::Mark> = self
            .doc
            .marks(&body_obj)
            .unwrap_or_default()
            .into_iter()
            .filter(|m| matches!(m.name(), "bold" | "italic" | "underline" | "code" | "link"))
            .collect();

        if marks.is_empty() {
            return html_escape(&text.iter().collect::<String>());
        }

        let mut cuts: Vec<usize> = vec![0, text.len()];
        for m in &marks {
            cuts.push(m.start.min(text.len()));
            cuts.push(m.end.min(text.len()));
        }
        cuts.sort_unstable();
        cuts.dedup();

        let mut out = String::new();
        for w in cuts.windows(2) {
            let (a, b) = (w[0], w[1]);
            if a >= b {
                continue;
            }
            let active: Vec<&automerge::marks::Mark> = marks
                .iter()
                .filter(|m| m.start <= a && m.end >= b)
                .collect();
            let escaped = html_escape(&text[a..b].iter().collect::<String>());

            let mut open = String::new();
            let mut close = String::new();
            for name in ["link", "bold", "italic", "underline", "code"] {
                let Some(m) = active.iter().find(|m| m.name() == name) else {
                    continue;
                };
                match name {
                    "link" => {
                        let href = match m.value() {
                            automerge::ScalarValue::Str(s) => s.to_string(),
                            _ => String::new(),
                        };
                        open.push_str(&format!("<a href=\"{}\">", html_escape(&href)));
                        close.insert_str(0, "</a>");
                    }
                    "bold" => {
                        open.push_str("<strong>");
                        close.insert_str(0, "</strong>");
                    }
                    "italic" => {
                        open.push_str("<em>");
                        close.insert_str(0, "</em>");
                    }
                    "underline" => {
                        open.push_str("<u>");
                        close.insert_str(0, "</u>");
                    }
                    "code" => {
                        open.push_str("<code>");
                        close.insert_str(0, "</code>");
                    }
                    _ => {}
                }
            }
            out.push_str(&open);
            out.push_str(&escaped);
            out.push_str(&close);
        }
        out
    }

    fn handle_user_state(&mut self, user_state: UserState) -> bool {
        // We manage our own presence locally; ignore echoes of it from the server.
        if self.my_id.as_ref() == Some(&user_state.user_id) {
            return false;
        }
        if user_state.online {
            self.users.insert(user_state.user_id.clone(), user_state);
        } else {
            self.users.remove(&user_state.user_id);
        }
        true
    }

    fn bump_version(&mut self) {
        let version = self.get_u64(DOC_KEY_VERSION);
        self.doc
            .put(automerge::ROOT, DOC_KEY_VERSION, version + 1)
            .unwrap();
    }

    /// Set a document field, bump the version, and broadcast the new snapshot.
    /// Returns whether the field actually changed.
    fn set_field(&mut self, key: &'static str, value: String) -> bool {
        if self.get_str(key) == value {
            return false;
        }
        self.doc.put(automerge::ROOT, key, value).unwrap();
        self.bump_version();
        self.broadcast_doc();
        true
    }

    /// Diff the editor's current text against the CRDT body and record only
    /// the changed span as Automerge ops (via a Myers diff), instead of
    /// replacing the whole field. Also resyncs formatting marks. Called on
    /// every real user keystroke (text edit or toolbar formatting change).
    fn sync_body_from_editor(&mut self) {
        let Some(editor) = &self.editor else { return };
        let delta_value = editor.get_contents();
        let delta: Delta =
            serde_wasm_bindgen::from_value(delta_value).unwrap_or(Delta { ops: vec![] });
        // Quill's document model always ends in a structural "\n" (its
        // implicit final block), which `getContents()` includes as literal
        // text. Strip exactly one trailing newline before treating this as
        // the stored body — otherwise it accumulates by one "\n" on every
        // sync, since Quill adds its own trailing "\n" again on next load
        // without ever deduping existing ones.
        let raw_text: String = delta
            .ops
            .iter()
            .filter_map(|op| op.insert.as_deref())
            .collect();
        let text = raw_text.strip_suffix('\n').unwrap_or(&raw_text).to_string();

        let text_changed = text != self.get_body();
        let body_obj = self.body_obj();
        // Automerge's mark()/unmark() always record ops, even when they have
        // no net effect (same as put() — see set_field's equality guard), so
        // rewrite_marks must only run when formatting has actually changed,
        // not merely on every call (e.g. entering/leaving edit mode with no
        // edits would otherwise still bump the version).
        let marks_changed = self.marks_differ_from_delta(&body_obj, &delta, text.chars().count());

        if !text_changed && !marks_changed {
            return;
        }
        if text_changed {
            self.doc.update_text(&body_obj, &text).unwrap();
        }
        if marks_changed {
            self.rewrite_marks(&body_obj, &delta);
        }
        self.bump_version();
        self.broadcast_doc();
    }

    /// Compares the formatting implied by a Quill Delta against the CRDT's
    /// current marks, per character rather than by raw span boundaries —
    /// Quill may split a run of identically-formatted text into several
    /// Delta ops that don't line up with Automerge's coalesced mark spans,
    /// so comparing spans directly would report spurious differences.
    fn marks_differ_from_delta(&self, body_obj: &ObjId, delta: &Delta, text_len: usize) -> bool {
        let mut desired_spans = Vec::new();
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
                        desired_spans.push((name.clone(), scalar.to_string(), pos, pos + span_len));
                    }
                }
            }
            pos += span_len;
        }

        let current_spans: Vec<_> = self
            .doc
            .marks(body_obj)
            .unwrap_or_default()
            .into_iter()
            .filter(|m| MARK_NAMES.contains(&m.name()))
            .map(|m| (m.name().to_string(), m.value().to_string(), m.start, m.end))
            .collect();

        mark_fingerprint(text_len, &desired_spans) != mark_fingerprint(text_len, &current_spans)
    }

    /// Replace all formatting marks on the body text with the spans implied
    /// by a Quill Delta's `attributes`. Marks don't change text length or
    /// position, so this never disturbs cursor placement.
    fn rewrite_marks(&mut self, body_obj: &ObjId, delta: &Delta) {
        let len = self
            .doc
            .text(body_obj)
            .map(|t| t.chars().count())
            .unwrap_or(0);
        for name in MARK_NAMES {
            let _ = self
                .doc
                .unmark(body_obj, name, 0, len, automerge::marks::ExpandMark::Both);
        }

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
                        let mark =
                            automerge::marks::Mark::new(name.clone(), scalar, pos, pos + span_len);
                        let _ = self
                            .doc
                            .mark(body_obj, mark, automerge::marks::ExpandMark::None);
                    }
                }
            }
            pos += span_len;
        }
    }

    /// Merge or replace an incoming document and apply only the body's
    /// changed span to the live editor (if mounted), preserving cursor
    /// position by construction instead of replacing editor content wholesale.
    fn apply_incoming_doc(&mut self, mut incoming: AutoCommit, replace: bool) -> bool {
        let heads_before = self.doc.get_heads();
        if replace {
            self.doc = incoming;
        } else if let Err(e) = self.doc.merge(&mut incoming) {
            log_1(&format!("[WS] Merge failed: {:?}", e).into());
            return false;
        }
        let heads_after = self.doc.get_heads();

        if self.editor.is_some() {
            let body_obj = self.body_obj();
            let mut marks_changed = false;
            let patches = self.doc.diff(&heads_before, &heads_after);
            {
                let editor = self.editor.as_ref().unwrap();
                for patch in &patches {
                    if patch.obj != body_obj {
                        continue;
                    }
                    match &patch.action {
                        PatchAction::SpliceText { index, value, .. } => {
                            editor.insert_text(*index, &value.make_string(), "silent");
                        }
                        PatchAction::DeleteSeq { index, length } => {
                            editor.delete_text(*index, *length, "silent");
                        }
                        PatchAction::Mark { .. } => marks_changed = true,
                        _ => {}
                    }
                }
            }
            if marks_changed {
                self.resync_marks_to_editor(&body_obj);
            }
        }
        true
    }

    /// Re-derive the editor's formatting from the CRDT's current mark spans.
    /// Ranges only (no content insert/delete), so cursor position is unaffected.
    fn resync_marks_to_editor(&mut self, body_obj: &ObjId) {
        let Some(editor) = &self.editor else { return };
        let len = self
            .doc
            .text(body_obj)
            .map(|t| t.chars().count())
            .unwrap_or(0);
        if len > 0 {
            editor.remove_format(0, len, "silent");
        }
        for mark in self.doc.marks(body_obj).unwrap_or_default() {
            if !MARK_NAMES.contains(&mark.name.as_str()) {
                continue;
            }
            let value = json_from_scalar(mark.value());
            editor.format_text(
                mark.start,
                mark.end - mark.start,
                mark.name(),
                &value,
                "silent",
            );
        }
    }

    fn set_editing(&mut self, editing: bool) {
        self.broadcast_my_state(editing);
        if let Some(my_id) = &self.my_id {
            if let Some(user) = self.users.get_mut(my_id) {
                user.editing = editing;
            }
        }
    }

    fn send_ws(&self, msg: &WsMessage) {
        match &self.ws {
            Some(ws) => {
                if let Ok(json) = serde_json::to_string(msg) {
                    let _ = ws.send_with_str(&json);
                }
            }
            None => log_1(&"[WS] No WebSocket connection!".into()),
        }
    }

    fn broadcast_doc(&mut self) {
        let data = self.doc.save();
        log_1(&format!("[WS] Sending Content update, {} bytes", data.len()).into());
        self.send_ws(&WsMessage::Content(data));
    }

    fn broadcast_my_state(&self, editing: bool) {
        if let Some(my_id) = &self.my_id {
            self.send_ws(&WsMessage::UserState(UserState {
                user_id: my_id.clone(),
                online: true,
                editing,
            }));
        }
    }

    /// Quill's `snow` theme, given an array `toolbar` config, creates its own
    /// `.ql-toolbar` element as a *sibling before* the container we mounted
    /// it in — outside anything Yew's virtual DOM diffing tracks. Removing
    /// only `#body-editor` (which Yew handles for us when the edit-mode
    /// branch unmounts) leaves that toolbar behind, so it must be torn down
    /// explicitly here or it accumulates on every Edit/View toggle.
    fn teardown_quill(&mut self) {
        let Some(editor) = self.editor.take() else {
            return;
        };
        let toolbar_module = editor.get_module("toolbar");
        if let Ok(container) = js_sys::Reflect::get(&toolbar_module, &"container".into()) {
            if let Some(el) = container.dyn_ref::<web_sys::Element>() {
                el.remove();
            }
        }
    }

    fn init_quill(&mut self, ctx: &Context<Self>) {
        if self.editor.is_some() {
            return;
        }

        let config = QuillConfig {
            theme: "snow",
            modules: QuillModules {
                toolbar: &[
                    "bold",
                    "italic",
                    "underline",
                    "header",
                    "list",
                    "link",
                    "code",
                ],
            },
        };
        let options: JsValue = serde_wasm_bindgen::to_value(&config).unwrap();
        let editor = Quill::new("#body-editor", &options);

        // Quill starts empty; load the current CRDT body and its formatting
        // without firing text-change (source "silent"), so it isn't mistaken
        // for a user edit.
        let body = self.get_body();
        if !body.is_empty() {
            editor.insert_text(0, &body, "silent");
        }
        self.editor = Some(editor);
        let body_obj = self.body_obj();
        self.resync_marks_to_editor(&body_obj);
        let editor = self.editor.as_ref().unwrap();

        let link = ctx.link().clone();
        let on_text_change = Closure::wrap(Box::new(
            move |_delta: JsValue, _old: JsValue, source: JsValue| {
                // Programmatic ("silent") updates never reach this handler, so
                // any event here is guaranteed to be real user input.
                if source.as_string().as_deref() == Some("user") {
                    link.send_message(Msg::QuillTextChanged);
                }
            },
        ) as Box<dyn FnMut(JsValue, JsValue, JsValue)>);
        editor.on("text-change", on_text_change.as_ref().unchecked_ref());
        on_text_change.forget();
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
