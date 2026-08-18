use automerge::{
    transaction::Transactable,
    AutoCommit, ReadDoc,
};
use web_sys::{console::log_1, WebSocket, MessageEvent, CloseEvent, ErrorEvent};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use yew::prelude::*;
use serde::{Deserialize, Serialize};

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
struct TinyMceConfig {
    selector: &'static str,
    inline: bool,
    menubar: bool,
    plugins: &'static str,
    toolbar: &'static str,
    block_formats: &'static str,
    license_key: &'static str,
}

#[wasm_bindgen]
extern "C" {
    // TinyMCE
    #[wasm_bindgen(js_namespace = tinymce)]
    fn init(options: &JsValue);

    #[wasm_bindgen(js_namespace = tinymce)]
    fn get(id: &str) -> Option<TinyMCEEditor>;

    #[wasm_bindgen(js_namespace = tinymce)]
    fn remove(selector: &str);

    type TinyMCEEditor;

    #[wasm_bindgen(method, js_name = getContent)]
    fn get_content(this: &TinyMCEEditor) -> String;

    #[wasm_bindgen(method, js_name = setContent)]
    fn set_content(this: &TinyMCEEditor, content: &str);

    #[wasm_bindgen(method, js_name = hasFocus)]
    fn has_focus(this: &TinyMCEEditor) -> bool;

    #[wasm_bindgen(method, getter)]
    fn selection(this: &TinyMCEEditor) -> TinyMCESelection;

    type TinyMCESelection;

    #[wasm_bindgen(method, js_name = getBookmark)]
    fn get_bookmark(this: &TinyMCESelection, bookmark_type: i32) -> JsValue;

    #[wasm_bindgen(method, js_name = moveToBookmark)]
    fn move_to_bookmark(this: &TinyMCESelection, bookmark: &JsValue);
}

struct App {
    doc: AutoCommit,
    mode: Mode,
    ws: Option<WebSocket>,
    tinymce_initialized: bool,
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
    ToggleMode,
    SyncBodyFromTinyMCE,
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
            tinymce_initialized: false,
            users: HashMap::new(),
            my_id: None,
        }
    }

    fn update(&mut self, _ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::WsMessage(ws_msg) => {
                match ws_msg {
                    WsMessage::Init { user_id, snapshot, users } => {
                        log_1(&format!("[WS] Init! My ID: {}", user_id).into());
                        self.my_id = Some(user_id.clone());

                        // Load snapshot if present
                        if let Some(data) = snapshot {
                            match AutoCommit::load(&data) {
                                Ok(doc) => { self.apply_incoming_doc(doc, true); }
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
                        self.users.insert(user_id.clone(), UserState {
                            user_id,
                            online: true,
                            editing: false,
                        });

                        true
                    }
                    WsMessage::Content(data) => {
                        log_1(&format!("[WS] Received Content update, {} bytes", data.len()).into());
                        match AutoCommit::load(&data) {
                            Ok(remote_doc) => self.apply_incoming_doc(remote_doc, false),
                            Err(_) => {
                                log_1(&"[WS] Failed to load remote content".into());
                                false
                            }
                        }
                    }
                    WsMessage::UserState(user_state) => {
                        self.handle_user_state(user_state)
                    }
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
            Msg::UpdateField(key, value) => {
                self.set_field(key, value)
            }
            Msg::ToggleMode => {
                self.mode = match self.mode {
                    Mode::View => {
                        self.set_editing(true);
                        Mode::Edit
                    }
                    Mode::Edit => {
                        // Sync body from TinyMCE before tearing it down
                        if self.tinymce_initialized {
                            self.sync_body_from_tinymce();
                            remove("#body-editor");
                            self.tinymce_initialized = false;
                        }
                        self.set_editing(false);
                        Mode::View
                    }
                };
                true
            }
            Msg::SyncBodyFromTinyMCE => {
                self.sync_body_from_tinymce();
                false
            }
        }
    }

    fn rendered(&mut self, ctx: &Context<Self>, _first_render: bool) {
        // Initialize TinyMCE only once the edit-mode DOM node actually exists.
        if self.mode == Mode::Edit && !self.tinymce_initialized {
            self.init_tinymce(ctx);
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let title = self.get_str(DOC_KEY_TITLE);
        let keywords = self.get_str(DOC_KEY_KEYWORDS);
        let body = self.get_str(DOC_KEY_BODY);
        let version = self.get_u64(DOC_KEY_VERSION);

        html! {
            <div>
                <header>
                    <hgroup>
                        <h1>{ "Collaborative Editor" }</h1>
                        <p>{ format!("v{}", version) }</p>
                    </hgroup>

                    // Connection status
                    if self.ws.is_some() {
                        <p><mark>{ format!("Connected - {} user(s)", self.users.len()) }</mark></p>
                    } else {
                        <p>{"Connecting to server..."}</p>
                    }

                    // Online users
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

                if self.mode == Mode::View {
                    <article class="view-mode">
                        <header>
                            <button onclick={ctx.link().callback(|_| Msg::ToggleMode)}>
                                {"Edit"}
                            </button>
                        </header>
                        <h2>{ title }</h2>
                        <p><em>{ keywords }</em></p>
                        <hr/>
                        <div class="body-content">{Html::from_html_unchecked(body.into())}</div>
                    </article>
                } else {
                    <article class="edit-mode">
                        <header>
                            <button onclick={ctx.link().callback(|_| Msg::ToggleMode)}>
                                {"View"}
                            </button>
                        </header>
                        
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
                            <div
                                id="body-editor"
                                class="inline-editor"
                            ></div>
                        </div>
                    </article>
                }
            </div>
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

    /// Set a document field, bump the version, and broadcast the new snapshot.
    /// Returns whether the field actually changed.
    fn set_field(&mut self, key: &'static str, value: String) -> bool {
        if self.get_str(key) == value {
            return false;
        }
        self.doc.put(automerge::ROOT, key, value).unwrap();
        let version = self.get_u64(DOC_KEY_VERSION);
        self.doc.put(automerge::ROOT, DOC_KEY_VERSION, version + 1).unwrap();
        self.broadcast_doc();
        true
    }

    fn sync_body_from_tinymce(&mut self) {
        if let Some(editor) = get("body-editor") {
            self.set_field(DOC_KEY_BODY, editor.get_content());
        }
    }

    /// Merge or replace an incoming document and refresh the editor if the body changed.
    fn apply_incoming_doc(&mut self, mut incoming: AutoCommit, replace: bool) -> bool {
        let body_before = self.get_str(DOC_KEY_BODY);
        if replace {
            self.doc = incoming;
        } else if let Err(e) = self.doc.merge(&mut incoming) {
            log_1(&format!("[WS] Merge failed: {:?}", e).into());
            return false;
        }
        let body_after = self.get_str(DOC_KEY_BODY);
        if body_before != body_after && self.tinymce_initialized {
            self.refresh_editor_body(&body_after);
        }
        true
    }

    fn refresh_editor_body(&self, body: &str) {
        let Some(editor) = get("body-editor") else { return };
        if editor.get_content() == body {
            return;
        }
        if editor.has_focus() {
            // Preserve the cursor position across the content swap.
            let bookmark = editor.selection().get_bookmark(2);
            editor.set_content(body);
            editor.selection().move_to_bookmark(&bookmark);
        } else {
            editor.set_content(body);
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

    fn init_tinymce(&mut self, ctx: &Context<Self>) {
        if self.tinymce_initialized {
            return;
        }
        self.tinymce_initialized = true;

        let link = ctx.link().clone();
        let body_content = self.get_str(DOC_KEY_BODY);

        let config = TinyMceConfig {
            selector: "#body-editor",
            inline: true,
            menubar: true,
            plugins: "lists link image table code",
            toolbar: "formatselect | undo redo | bold italic underline | alignleft aligncenter alignright | outdent indent | bullist numlist | link image | code",
            block_formats: "Paragraph=p;Heading 1=h1;Heading 2=h2;Heading 3=h3;Heading 4=h4",
            license_key: "gpl",
        };
        let options: js_sys::Object = serde_wasm_bindgen::to_value(&config)
            .unwrap()
            .unchecked_into();

        let setup_fn = Closure::wrap(Box::new(move |editor: JsValue| {
            let on_method = js_sys::Reflect::get(&editor, &"on".into()).unwrap();
            let on_fn = on_method.unchecked_into::<js_sys::Function>();

            // Set the initial content once the editor reports it is ready.
            let body = body_content.clone();
            let on_init = Closure::wrap(Box::new(move || {
                if let Some(ed) = get("body-editor") {
                    ed.set_content(&body);
                }
            }) as Box<dyn Fn()>);
            let _ = on_fn.call2(&editor, &"init".into(), on_init.as_ref().unchecked_ref());
            on_init.forget();

            // Push local edits into the Automerge document.
            let link_inner = link.clone();
            let on_change = Closure::wrap(Box::new(move || {
                link_inner.send_message(Msg::SyncBodyFromTinyMCE);
            }) as Box<dyn Fn()>);
            let _ = on_fn.call2(&editor, &"change".into(), on_change.as_ref().unchecked_ref());
            let _ = on_fn.call2(&editor, &"keyup".into(), on_change.as_ref().unchecked_ref());
            on_change.forget();
        }) as Box<dyn Fn(JsValue)>);

        js_sys::Reflect::set(&options, &"setup".into(), setup_fn.as_ref().unchecked_ref()).unwrap();
        setup_fn.forget();

        init(&options.into());
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}