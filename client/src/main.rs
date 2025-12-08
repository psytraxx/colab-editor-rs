use automerge::{
    transaction::Transactable,
    AutoCommit, ReadDoc,
};
use web_sys::{console::log_1, WebSocket, MessageEvent, CloseEvent, ErrorEvent};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
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
    user_name: String,
    online: bool,
    editing: bool,
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
    my_name: Option<String>,
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
    LocalUpdate,
    InitTinyMCE,
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
                    let log_txt = if txt.len() > 100 {
                        format!("{}...", &txt[..100])
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
            my_name: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::WsMessage(ws_msg) => {
                match ws_msg {
                    WsMessage::Init { user_id, snapshot, users } => {
                        log_1(&format!("[WS] Init! My ID: {}", user_id).into());
                        self.my_id = Some(user_id.clone());
                        self.my_name = Some(user_id.clone());

                        // Load snapshot if present
                        if let Some(data) = snapshot {
                             if let Ok(doc) = AutoCommit::load(&data) {
                                 self.doc = doc;
                                 if self.tinymce_initialized {
                                     if let Some(editor) = get("body-editor") {
                                         let body = self.get_str(DOC_KEY_BODY);
                                         editor.set_content(&body);
                                     }
                                 }
                             } else {
                                 log_1(&"[WS] Failed to load snapshot".into());
                             }
                        }

                        // Populate users
                        self.users.clear();
                        for user in users {
                            if user.online {
                                self.users.insert(user.user_id.clone(), user);
                            }
                        }
                        
                        // Add self to users list (if not in list from server)
                        self.users.insert(user_id.clone(), UserState {
                            user_id: user_id.clone(),
                            user_name: user_id,
                            online: true,
                            editing: false,
                        });
                        
                        true
                    }
                    WsMessage::Content(data) => {
                        log_1(&format!("[WS] Received Content update, {} bytes", data.len()).into());
                        if let Ok(mut remote_doc) = AutoCommit::load(&data) {
                            let body_before = self.get_str(DOC_KEY_BODY);
                            
                            // Merge remote state into local
                            if let Err(e) = self.doc.merge(&mut remote_doc) {
                                log_1(&format!("[WS] Merge failed: {:?}", e).into());
                                return false;
                            }
                            
                            let body_after = self.get_str(DOC_KEY_BODY);

                            // Update TinyMCE if body changed from remote and we're in edit mode
                            if body_before != body_after && self.tinymce_initialized {
                                if let Some(editor) = get("body-editor") {
                                    let current_content = editor.get_content();
                                    if body_after != current_content {
                                        if editor.has_focus() {
                                            // Save cursor position before updating content
                                            let selection = editor.selection();
                                            let bookmark = selection.get_bookmark(2);
                                            editor.set_content(&body_after);
                                            // Restore cursor position
                                            let selection = editor.selection();
                                            selection.move_to_bookmark(&bookmark);
                                        } else {
                                            editor.set_content(&body_after);
                                        }
                                    }
                                }
                            }
                            true
                        } else {
                            log_1(&"[WS] Failed to load remote content".into());
                            false
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
                // Get current value to check if it actually changed
                let current = self.get_str(key);
                if current != value {
                    self.doc.put(automerge::ROOT, key, value).unwrap();
                    // Increment version only when content actually changes
                    let current_version = self.get_u64(DOC_KEY_VERSION);
                    self.doc.put(automerge::ROOT, DOC_KEY_VERSION, current_version + 1).unwrap();
                    ctx.link().send_message(Msg::LocalUpdate);
                    true
                } else {
                    false
                }
            }
            Msg::ToggleMode => {
                self.mode = match self.mode {
                    Mode::View => {
                        // Initialize TinyMCE after switching to edit mode
                        ctx.link().send_message(Msg::InitTinyMCE);
                        // Broadcast that we're entering edit mode
                        self.broadcast_my_state(true);
                        // Update our own user state
                        if let Some(my_id) = &self.my_id {
                            if let Some(user) = self.users.get_mut(my_id) {
                                user.editing = true;
                            }
                        }
                        Mode::Edit
                    }
                    Mode::Edit => {
                        // Sync body from TinyMCE before switching to view
                        if self.tinymce_initialized {
                            if let Some(editor) = get("body-editor") {
                                let content = editor.get_content();
                                let current = self.get_str(DOC_KEY_BODY);
                                if current != content {
                                    self.doc.put(automerge::ROOT, DOC_KEY_BODY, content).unwrap();
                                    let current_version = self.get_u64(DOC_KEY_VERSION);
                                    self.doc.put(automerge::ROOT, DOC_KEY_VERSION, current_version + 1).unwrap();
                                    ctx.link().send_message(Msg::LocalUpdate);
                                }
                            }
                            remove("#body-editor");
                            self.tinymce_initialized = false;
                        }
                        // Broadcast that we're leaving edit mode
                        self.broadcast_my_state(false);
                        // Update our own user state
                        if let Some(my_id) = &self.my_id {
                            if let Some(user) = self.users.get_mut(my_id) {
                                user.editing = false;
                            }
                        }
                        Mode::View
                    }
                };
                true
            }
            Msg::InitTinyMCE => {
                self.init_tinymce(ctx);
                false
            }
            Msg::SyncBodyFromTinyMCE => {
                if let Some(editor) = get("body-editor") {
                    let content = editor.get_content();
                    let current = self.get_str(DOC_KEY_BODY);
                    if current != content {
                        self.doc.put(automerge::ROOT, DOC_KEY_BODY, content).unwrap();
                        let current_version = self.get_u64(DOC_KEY_VERSION);
                        self.doc.put(automerge::ROOT, DOC_KEY_VERSION, current_version + 1).unwrap();
                        ctx.link().send_message(Msg::LocalUpdate);
                    }
                }
                false
            }
            Msg::LocalUpdate => {
                // Send FULL content (state-based sync)
                if let Some(ws) = &self.ws {
                    let data = self.doc.save();
                    log_1(&format!("[WS] Sending Content update, {} bytes", data.len()).into());
                    let ws_msg = WsMessage::Content(data);
                    if let Ok(json) = serde_json::to_string(&ws_msg) {
                        let _ = ws.send_with_str(&json);
                    }
                } else {
                    log_1(&"[WS] No WebSocket connection!".into());
                }
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let title = self.get_str(DOC_KEY_TITLE);
        let keywords = self.get_str(DOC_KEY_KEYWORDS);
        let body = self.get_str(DOC_KEY_BODY);
        let version = self.get_u64(DOC_KEY_VERSION);

        // log_1(&format!("[VIEW] Rendering - title: '{}', keywords: '{}', body: '{}', version: {}",
        //    title, keywords, &body[..body.len().min(50)], version).into());

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
                                    { &user.user_name }
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
    fn get_str(&self, key: &str) -> String {
        self.doc
            .get(automerge::ROOT, key)
            .ok()
            .flatten()
            .map(|(v, _)| match v {
                automerge::Value::Scalar(std::borrow::Cow::Owned(automerge::ScalarValue::Str(s))) => s.into(),
                automerge::Value::Scalar(std::borrow::Cow::Borrowed(automerge::ScalarValue::Str(s))) => s.to_string(),
                automerge::Value::Scalar(s) => s.to_string(),
                automerge::Value::Object(_) => String::new(),
            })
            .unwrap_or_default()
    }
    
    fn get_u64(&self, key: &str) -> u64 {
         self.doc
            .get(automerge::ROOT, key)
            .ok()
            .flatten()
            .map(|(v, _)| match v {
                automerge::Value::Scalar(std::borrow::Cow::Owned(automerge::ScalarValue::Uint(u))) => u,
                automerge::Value::Scalar(std::borrow::Cow::Borrowed(automerge::ScalarValue::Uint(u))) => *u,
                automerge::Value::Scalar(std::borrow::Cow::Owned(automerge::ScalarValue::Int(i))) => i as u64,
                automerge::Value::Scalar(std::borrow::Cow::Borrowed(automerge::ScalarValue::Int(i))) => *i as u64,
                automerge::Value::Scalar(std::borrow::Cow::Owned(automerge::ScalarValue::F64(f))) => f as u64,
                automerge::Value::Scalar(std::borrow::Cow::Borrowed(automerge::ScalarValue::F64(f))) => *f as u64,
                _ => 0,
            })
            .unwrap_or(0)
    }

    fn handle_user_state(&mut self, user_state: UserState) -> bool {
        log_1(&format!(
            "[CLIENT] Received UserState: id={} name={} online={}",
            user_state.user_id, user_state.user_name, user_state.online
        ).into());
        
        // Don't overwrite our own state from server - we manage it locally
        let dominated_by_local = self.my_id.as_ref() == Some(&user_state.user_id);
        
        if user_state.online {
            if dominated_by_local {
                // Only update our entry if we don't have one yet
                self.users.entry(user_state.user_id.clone()).or_insert(user_state);
            } else {
                self.users.insert(user_state.user_id.clone(), user_state);
            }
        } else {
            self.users.remove(&user_state.user_id);
        }
        
        log_1(&format!(
            "[CLIENT] Total users in map: {}",
            self.users.len()
        ).into());
        
        true
    }

    fn broadcast_my_state(&self, editing: bool) {
        if let (Some(my_id), Some(my_name), Some(ws)) = (&self.my_id, &self.my_name, &self.ws) {
            let user_state = UserState {
                user_id: my_id.clone(),
                user_name: my_name.clone(),
                online: true,
                editing,
            };
            let ws_msg = WsMessage::UserState(user_state);
            if let Ok(json) = serde_json::to_string(&ws_msg) {
                let _ = ws.send_with_str(&json);
            }
        }
    }

    fn init_tinymce(&mut self, ctx: &Context<Self>) {
        if !self.tinymce_initialized {
            let link = ctx.link().clone();
            let body_content = self.get_str(DOC_KEY_BODY);
            
            // Delay initialization to ensure DOM is ready
            spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(50).await;
                
                let options = js_sys::Object::new();
                js_sys::Reflect::set(&options, &"selector".into(), &"#body-editor".into()).unwrap();
                js_sys::Reflect::set(&options, &"inline".into(), &true.into()).unwrap();
                // enable menubar so users can access format/insert options
                js_sys::Reflect::set(&options, &"menubar".into(), &true.into()).unwrap();
                // add common structural plugins
                js_sys::Reflect::set(&options, &"plugins".into(), &"lists link image table code".into()).unwrap();
                // richer toolbar including format selector, alignment and indenting
                js_sys::Reflect::set(&options, &"toolbar".into(), &"formatselect | undo redo | bold italic underline | alignleft aligncenter alignright | outdent indent | bullist numlist | link image | code".into()).unwrap();
                // define available block formats (paragraphs and headings)
                js_sys::Reflect::set(&options, &"block_formats".into(), &"Paragraph=p;Heading 1=h1;Heading 2=h2;Heading 3=h3;Heading 4=h4".into()).unwrap();
                js_sys::Reflect::set(&options, &"license_key".into(), &"gpl".into()).unwrap();
                
                // Setup callback for changes
                let link_clone = link.clone();
                let setup_fn = Closure::wrap(Box::new(move |editor: JsValue| {
                    let link_inner = link_clone.clone();
                    let on_change = Closure::wrap(Box::new(move || {
                        link_inner.send_message(Msg::SyncBodyFromTinyMCE);
                    }) as Box<dyn Fn()>);
                    
                    // Register change and keyup events
                    let on_method = js_sys::Reflect::get(&editor, &"on".into()).unwrap();
                    let on_fn = on_method.unchecked_into::<js_sys::Function>();
                    let _ = on_fn.call2(&editor, &"change".into(), on_change.as_ref().unchecked_ref());
                    let _ = on_fn.call2(&editor, &"keyup".into(), on_change.as_ref().unchecked_ref());
                    on_change.forget();
                }) as Box<dyn Fn(JsValue)>);
                
                js_sys::Reflect::set(&options, &"setup".into(), setup_fn.as_ref().unchecked_ref()).unwrap();
                setup_fn.forget();
                
                init(&options.into());
                
                // Set initial content after a brief delay
                gloo_timers::future::TimeoutFuture::new(100).await;
                if let Some(editor) = get("body-editor") {
                    editor.set_content(&body_content);
                }
            });
            
            self.tinymce_initialized = true;
        }
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}