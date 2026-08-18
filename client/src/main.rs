mod doc;
mod marks;
mod protocol;
mod quill;

use crate::doc::Doc;
use crate::marks::{json_from_scalar, MARK_NAMES};
use crate::protocol::{UserState, WsMessage, DOC_KEY_KEYWORDS, DOC_KEY_TITLE, DOC_KEY_VERSION};
use crate::quill::{Delta, Quill, QuillConfig, QuillModules, Toolbar};
use automerge::{patches::PatchAction, AutoCommit, ObjId};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use web_sys::{console::log_1, CloseEvent, ErrorEvent, MessageEvent, WebSocket};
use yew::prelude::*;

/// Reconnect backoff bounds, in milliseconds.
const RECONNECT_BASE_MS: u32 = 500;
const RECONNECT_MAX_MS: u32 = 15_000;

/// The editor is mounted in both modes; the only difference is whether it is
/// editable and carries a toolbar. Holding the `Quill` handle inside the mode
/// makes "in edit mode but no editor" unrepresentable.
enum Mode {
    View(Option<Quill>),
    Edit(Quill),
}

impl Mode {
    fn editor(&self) -> Option<&Quill> {
        match self {
            Mode::Edit(q) => Some(q),
            Mode::View(q) => q.as_ref(),
        }
    }
}

struct App {
    doc: Doc,
    mode: Mode,
    ws: Option<WebSocket>,
    users: HashMap<String, UserState>,
    my_id: Option<String>,
    reconnect_delay_ms: u32,
    /// Which mode the user has selected. `mode` lags this by one render, since
    /// the editor can only be mounted after Yew has produced `#body-editor`.
    want_edit: bool,
}

enum Msg {
    WsMessage(WsMessage),
    WsOpened,
    WsClosed,
    WsError(String),
    Reconnect,
    UpdateField(&'static str, String),
    SetEditMode(bool),
    QuillTextChanged,
}

fn ws_url() -> &'static str {
    let local = web_sys::window()
        .and_then(|w| w.location().hostname().ok())
        .map(|h| h == "localhost" || h == "127.0.0.1")
        .unwrap_or(false);
    if local {
        "ws://localhost:8787/ws"
    } else {
        "wss://colab-editor-rs.dynamicflash.workers.dev/ws"
    }
}

impl Component for App {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let mut app = Self {
            // Start EMPTY - the server is the source of truth.
            doc: Doc::new(),
            mode: Mode::View(None),
            ws: None,
            users: HashMap::new(),
            my_id: None,
            reconnect_delay_ms: RECONNECT_BASE_MS,
            want_edit: false,
        };
        app.connect(ctx);
        app
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::WsMessage(ws_msg) => self.handle_ws_message(ws_msg),
            Msg::WsOpened => {
                log_1(&"[WS] Connected".into());
                self.reconnect_delay_ms = RECONNECT_BASE_MS;
                true
            }
            Msg::WsClosed => {
                log_1(&"[WS] Disconnected".into());
                self.ws = None;
                self.users.clear();
                self.schedule_reconnect(ctx);
                true
            }
            Msg::WsError(err) => {
                log_1(&format!("[WS] Error: {}", err).into());
                false
            }
            Msg::Reconnect => {
                if self.ws.is_none() {
                    log_1(&"[WS] Reconnecting…".into());
                    self.connect(ctx);
                }
                false
            }
            Msg::UpdateField(key, value) => {
                if !self.doc.set_field(key, value) {
                    return false;
                }
                self.broadcast_doc();
                true
            }
            Msg::SetEditMode(edit) => {
                if edit == self.want_edit {
                    return false;
                }
                if !edit {
                    // Sync any trailing edit before tearing the editor down.
                    self.sync_body_from_editor();
                }
                self.teardown_quill();
                self.want_edit = edit;
                self.set_editing(edit);
                true
            }
            Msg::QuillTextChanged => {
                self.sync_body_from_editor();
                false
            }
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, _first_render: bool) {
        // Mount Quill only once the target DOM node actually exists. `want_edit`
        // is what the last SetEditMode asked for; the mode still holds the old
        // editor (or none) until we mount the matching one here.
        if let Mode::View(None) = self.mode {
            self.mount_quill(ctx, self.want_edit);
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let title = self.doc.get_str(DOC_KEY_TITLE);
        let keywords = self.doc.get_str(DOC_KEY_KEYWORDS);
        let version = self.doc.get_u64(DOC_KEY_VERSION);
        let edit = self.want_edit;

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
                            html! { <span class={class}>{ &user.user_id }</span> }
                        })}
                    </div>
                </header>

                <div class="mode-switch-row">
                    <fieldset class="mode-switch">
                        <label>
                            <input
                                type="checkbox"
                                role="switch"
                                checked={edit}
                                onclick={ctx.link().callback(|e: MouseEvent| {
                                    let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                    Msg::SetEditMode(input.checked())
                                })}
                            />
                            { if edit { "Edit mode" } else { "View mode" } }
                        </label>
                    </fieldset>
                    <span class="version-pill">{ format!("v{}", version) }</span>
                </div>

                <article class={classes!("content-card", if edit { "edit-mode" } else { "view-mode" })}>
                    if edit {
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
                    } else {
                        <h2 class="view-title">{ title }</h2>
                        if !keywords.trim().is_empty() {
                            <div class="keyword-chips">
                                { for keywords.split(',').map(|kw| kw.trim()).filter(|kw| !kw.is_empty()).map(|kw| {
                                    html! { <span class="keyword-chip">{ kw }</span> }
                                })}
                            </div>
                        }
                        <div id="body-editor" class="quill-editor quill-readonly"></div>
                    }
                </article>
            </main>
        }
    }
}

impl App {
    // --- connection ---------------------------------------------------

    fn connect(&mut self, ctx: &Context<Self>) {
        let Some(websocket) = WebSocket::new(ws_url()).ok() else {
            log_1(&"[WS] Failed to create WebSocket".into());
            self.schedule_reconnect(ctx);
            return;
        };
        let link = ctx.link().clone();

        let link_msg = link.clone();
        let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
            let Some(txt) = e.data().as_string() else {
                return;
            };
            match serde_json::from_str::<WsMessage>(&txt) {
                Ok(msg) => link_msg.send_message(Msg::WsMessage(msg)),
                Err(e) => log_1(&format!("[WS] Failed to parse message: {:?}", e).into()),
            }
        }) as Box<dyn FnMut(MessageEvent)>);
        websocket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        let link_open = link.clone();
        let onopen = Closure::wrap(Box::new(move |_| {
            link_open.send_message(Msg::WsOpened);
        }) as Box<dyn FnMut(JsValue)>);
        websocket.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();

        let link_close = link.clone();
        let onclose = Closure::wrap(Box::new(move |_: CloseEvent| {
            link_close.send_message(Msg::WsClosed);
        }) as Box<dyn FnMut(CloseEvent)>);
        websocket.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();

        let link_error = link.clone();
        let onerror = Closure::wrap(Box::new(move |e: ErrorEvent| {
            link_error.send_message(Msg::WsError(e.message()));
        }) as Box<dyn FnMut(ErrorEvent)>);
        websocket.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();

        self.ws = Some(websocket);
    }

    /// Retry with exponential backoff. Without this a single network blip
    /// leaves the client permanently disconnected while the UI still claims
    /// to be connecting.
    fn schedule_reconnect(&mut self, ctx: &Context<Self>) {
        let delay = self.reconnect_delay_ms;
        self.reconnect_delay_ms = (delay.saturating_mul(2)).min(RECONNECT_MAX_MS);

        let link = ctx.link().clone();
        let cb = Closure::once_into_js(move || link.send_message(Msg::Reconnect));
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.unchecked_ref(),
                delay as i32,
            );
        }
    }

    fn handle_ws_message(&mut self, msg: WsMessage) -> bool {
        match msg {
            WsMessage::Init {
                user_id,
                snapshot,
                users,
            } => {
                log_1(&format!("[WS] Init! My ID: {}", user_id).into());
                self.my_id = Some(user_id.clone());

                if let Some(data) = snapshot {
                    match AutoCommit::load(&data) {
                        Ok(doc) => self.load_initial_doc(doc),
                        Err(_) => log_1(&"[WS] Failed to load snapshot".into()),
                    }
                }

                self.users.clear();
                for user in users {
                    if user.online {
                        self.users.insert(user.user_id.clone(), user);
                    }
                }
                self.users.insert(
                    user_id.clone(),
                    UserState {
                        user_id,
                        online: true,
                        editing: self.want_edit,
                    },
                );
                true
            }
            WsMessage::Content(data) => match AutoCommit::load(&data) {
                Ok(remote) => self.merge_incoming_doc(remote),
                Err(_) => {
                    log_1(&"[WS] Failed to load remote content".into());
                    false
                }
            },
            WsMessage::UserState(user_state) => self.handle_user_state(user_state),
        }
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

    // --- document sync ------------------------------------------------

    /// Adopt the server's snapshot wholesale. Unlike a merge there is no
    /// shared history to diff against, so the editor is reloaded from scratch
    /// rather than patched incrementally.
    fn load_initial_doc(&mut self, incoming: AutoCommit) {
        self.doc.replace_with(incoming);
        self.reload_editor_contents();
    }

    /// Merge a peer's document and apply only the body's changed span to the
    /// live editor, preserving cursor position by construction instead of
    /// replacing editor content wholesale.
    fn merge_incoming_doc(&mut self, mut incoming: AutoCommit) -> bool {
        let heads_before = self.doc.get_heads();
        if let Err(e) = self.doc.merge(&mut incoming) {
            log_1(&format!("[WS] Merge failed: {:?}", e).into());
            return false;
        }
        let heads_after = self.doc.get_heads();

        let Some(body_obj) = self.doc.body_obj() else {
            return true;
        };
        if self.mode.editor().is_none() {
            return true;
        }

        let patches = self.doc.diff(&heads_before, &heads_after);
        let mut marks_changed = false;
        if let Some(editor) = self.mode.editor() {
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
        true
    }

    /// Diff the editor's current text against the CRDT body and record only
    /// the changed span as Automerge ops (via a Myers diff), instead of
    /// replacing the whole field. Also resyncs formatting marks. Called on
    /// every real user keystroke (text edit or toolbar formatting change).
    fn sync_body_from_editor(&mut self) {
        let Some(editor) = self.mode.editor() else {
            return;
        };
        let delta: Delta =
            serde_wasm_bindgen::from_value(editor.get_contents()).unwrap_or(Delta { ops: vec![] });
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

        let text_changed = text != self.doc.get_body();
        let body_obj = self.doc.body_obj_or_create();
        // Automerge's mark()/unmark() always record ops, even when they have
        // no net effect (same as put() — see set_field's equality guard), so
        // rewrite_marks must only run when formatting has actually changed,
        // not merely on every call (e.g. entering/leaving edit mode with no
        // edits would otherwise still bump the version).
        let marks_changed =
            self.doc
                .marks_differ_from_delta(&body_obj, &delta, text.chars().count());

        if !text_changed && !marks_changed {
            return;
        }
        if text_changed {
            self.doc.update_text(&body_obj, &text);
        }
        if marks_changed {
            self.doc.rewrite_marks(&body_obj, &delta);
        }
        self.doc.bump_version();
        self.broadcast_doc();
    }

    /// Re-derive the editor's formatting from the CRDT's current mark spans.
    /// Ranges only (no content insert/delete), so cursor position is unaffected.
    fn resync_marks_to_editor(&mut self, body_obj: &ObjId) {
        let len = self.doc.body_len(body_obj);
        let marks = self.doc.marks(body_obj);
        let Some(editor) = self.mode.editor() else {
            return;
        };
        if len > 0 {
            editor.remove_format(0, len, "silent");
        }
        for mark in marks {
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

    /// Replace the editor's whole contents from the CRDT. Only for the paths
    /// where incremental patching isn't possible (initial snapshot, mount).
    fn reload_editor_contents(&mut self) {
        let body = self.doc.get_body();
        let body_obj = self.doc.body_obj();
        let Some(editor) = self.mode.editor() else {
            return;
        };
        editor.set_contents(&JsValue::NULL, "silent");
        if !body.is_empty() {
            editor.insert_text(0, &body, "silent");
        }
        if let Some(body_obj) = body_obj {
            self.resync_marks_to_editor(&body_obj);
        }
    }

    // --- presence -----------------------------------------------------

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

    /// TODO: this sends the entire document — including its full history — on
    /// every keystroke, so cost grows without bound as the document is edited.
    /// Automerge offers `save_incremental()` and a sync protocol
    /// (`automerge::sync::State`) for exactly this. Switching requires a
    /// matching change in the relay, which currently *replaces* its stored
    /// snapshot with whatever a client sends (`worker/src/index.ts`); it would
    /// need to either accumulate incremental chunks or run Automerge itself to
    /// merge. See "Known limitations" in worker/README.md.
    fn broadcast_doc(&mut self) {
        let data = self.doc.save();
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

    // --- editor lifecycle ---------------------------------------------

    /// Quill's `snow` theme, given an array `toolbar` config, creates its own
    /// `.ql-toolbar` element as a *sibling before* the container we mounted
    /// it in — outside anything Yew's virtual DOM diffing tracks. Removing
    /// only `#body-editor` (which Yew handles for us when the mode branch
    /// unmounts) leaves that toolbar behind, so it must be torn down
    /// explicitly here or it accumulates on every Edit/View toggle.
    fn teardown_quill(&mut self) {
        let editor = match std::mem::replace(&mut self.mode, Mode::View(None)) {
            Mode::Edit(q) => Some(q),
            Mode::View(q) => q,
        };
        let Some(editor) = editor else { return };
        let toolbar_module = editor.get_module("toolbar");
        if let Ok(container) = js_sys::Reflect::get(&toolbar_module, &"container".into()) {
            if let Some(el) = container.dyn_ref::<web_sys::Element>() {
                el.remove();
            }
        }
    }

    /// Mount Quill into `#body-editor`. View mode reuses the same editor,
    /// disabled and without a toolbar, rather than hand-rendering HTML from
    /// marks — that keeps one rendering path and avoids reconstructing
    /// block-level formatting (headers, lists) by hand.
    fn mount_quill(&mut self, ctx: &Context<Self>, editable: bool) {
        let config = QuillConfig {
            theme: "snow",
            modules: QuillModules {
                // Derived from MARK_NAMES so the toolbar and the marks we
                // persist can never drift apart. View mode gets no toolbar.
                toolbar: if editable {
                    Toolbar::Names(MARK_NAMES)
                } else {
                    Toolbar::Disabled(false)
                },
            },
            read_only: !editable,
        };
        let Ok(options) = serde_wasm_bindgen::to_value(&config) else {
            log_1(&"[quill] Failed to serialize config".into());
            return;
        };
        let editor = Quill::new("#body-editor", &options);
        if !editable {
            editor.enable(false);
        }

        self.mode = if editable {
            Mode::Edit(editor)
        } else {
            Mode::View(Some(editor))
        };

        // Quill starts empty; load the current CRDT body and its formatting
        // without firing text-change (source "silent"), so it isn't mistaken
        // for a user edit.
        self.reload_editor_contents();

        if !editable {
            return;
        }
        let Some(editor) = self.mode.editor() else {
            return;
        };
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
