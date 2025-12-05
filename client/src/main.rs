use automerge::{
    sync::{self, SyncDoc},
    transaction::Transactable,
    AutoCommit, ReadDoc,
};
use common::{WsMessage, UserState, DOC_KEY_BODY, DOC_KEY_KEYWORDS, DOC_KEY_TITLE, DOC_KEY_VERSION};
use futures::{channel::mpsc::Sender, SinkExt, StreamExt};
use gloo::net::websocket::{futures::WebSocket, Message};
use web_sys::console::log_1;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

#[wasm_bindgen]
extern "C" {
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
}

struct App {
    doc: AutoCommit,
    sync_state: sync::State,
    mode: Mode,
    ws_sender: Option<Sender<Message>>,
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
    WsConnected(Sender<Message>),
    UpdateField(&'static str, String),
    ToggleMode,
    SendSync,
    InitTinyMCE,
    SyncBodyFromTinyMCE,
    SetEditing(Option<String>), // field name or None to stop editing
}

impl Component for App {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let link = ctx.link().clone();
        spawn_local(async move {
            // Build WS URL from current browser location (use same host, connect to backend on port 3000)
            let window = web_sys::window().expect("no global `window` exists");
            let location = window.location();
            let hostname = location.hostname().unwrap_or_else(|_| "127.0.0.1".to_string());
            let ws_url = format!("ws://{}:3000/ws", hostname);

            let ws = WebSocket::open(&ws_url).unwrap();
            let (mut write, mut read) = ws.split();
            let (tx, mut rx) = futures::channel::mpsc::channel::<Message>(1000);

            link.send_message(Msg::WsConnected(tx));

            spawn_local(async move {
                while let Some(msg) = rx.next().await {
                    write.send(msg).await.unwrap();
                }
            });

            while let Some(msg) = read.next().await {
                match &msg {
                    Ok(Message::Text(text)) => {
                        log_1(&format!("[CLIENT] Raw WS message: {}", text).into());
                        match serde_json::from_str::<WsMessage>(&text) {
                            Ok(ws_msg) => {
                                log_1(&"[CLIENT] Parsed WsMessage successfully".into());
                                link.send_message(Msg::WsMessage(ws_msg));
                            }
                            Err(e) => {
                                log_1(&format!("[CLIENT] Failed to parse WsMessage: {:?}", e).into());
                            }
                        }
                    }
                    Ok(other) => {
                        log_1(&format!("[CLIENT] Non-text WS message: {:?}", other).into());
                    }
                    Err(e) => {
                        log_1(&format!("[CLIENT] WS error: {:?}", e).into());
                    }
                }
            }
        });

        Self {
            doc: AutoCommit::new(),
            sync_state: sync::State::new(),
            mode: Mode::View,
            ws_sender: None,
            tinymce_initialized: false,
            users: HashMap::new(),
            my_id: None,
            my_name: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::WsConnected(sender) => {
                self.ws_sender = Some(sender);
                ctx.link().send_message(Msg::SendSync);
                false
            }
            Msg::WsMessage(ws_msg) => {
                match ws_msg {
                    WsMessage::Welcome(id) => {
                        self.my_id = Some(id.clone());
                        self.my_name = Some(id.clone());
                        log_1(&format!("[CLIENT] Welcome! My identity: {}", id).into());
                        true
                    }
                    WsMessage::Sync(binary) => self.handle_sync(ctx, binary),
                    WsMessage::UserState(user_state) => self.handle_user_state(user_state),
                }
            }
            Msg::UpdateField(key, value) => {
                // Get current value to check if it actually changed
                let current = self.get_str(key);
                if current != value {
                    self.doc.put(automerge::ROOT, key, value).unwrap();
                    // Increment version only when content actually changes
                    let current_version = self.get_u64(DOC_KEY_VERSION);
                    self.doc.put(automerge::ROOT, DOC_KEY_VERSION, current_version + 1).unwrap();
                    ctx.link().send_message(Msg::SendSync);
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
                        ctx.link().send_message(Msg::SetEditing(Some("general".to_string())));
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
                                    ctx.link().send_message(Msg::SendSync);
                                }
                            }
                            remove("#body-editor");
                            self.tinymce_initialized = false;
                        }
                        // Signal we stopped editing
                        ctx.link().send_message(Msg::SetEditing(None));
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
                        ctx.link().send_message(Msg::SendSync);
                    }
                }
                false
            }
            Msg::SetEditing(field) => {
                log_1(&format!(
                    "[CLIENT] SetEditing called with field={:?}",
                    field
                ).into());
                
                // Update our own state locally
                if let Some(my_id) = &self.my_id {
                    if let Some(my_state) = self.users.get_mut(my_id) {
                        my_state.editing = field.is_some();
                        my_state.field = field.clone();
                    }
                }
                
                if let Some(sender) = &mut self.ws_sender {
                    let user_state = UserState {
                        user_id: String::new(), // Server will fill this
                        user_name: String::new(),
                        editing: field.is_some(),
                        field: field.clone(),
                        online: true,
                    };
                    let ws_msg = WsMessage::UserState(user_state);
                    let json = serde_json::to_string(&ws_msg).unwrap();
                    
                    log_1(&format!(
                        "[CLIENT] Sending UserState: editing={} field={:?}",
                        field.is_some(), field
                    ).into());
                    
                    let mut tx = sender.clone();
                    spawn_local(async move {
                        tx.send(Message::Text(json)).await.unwrap();
                    });
                } else {
                    log_1(&"[CLIENT] No ws_sender available!".into());
                }
                true  // Return true to trigger re-render
            }
            Msg::SendSync => {
                if let Some(sender) = &mut self.ws_sender {
                    if let Some(msg) = self.doc.sync().generate_sync_message(&mut self.sync_state) {
                        let ws_msg = WsMessage::Sync(msg.encode());
                        let json = serde_json::to_string(&ws_msg).unwrap();
                        let mut tx = sender.clone();
                        spawn_local(async move {
                            tx.send(Message::Text(json)).await.unwrap();
                        });
                    }
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

        // Get OTHER users editing each field (exclude self)
        let my_id = self.my_id.as_deref();
        let title_editors: Vec<_> = self.users.values()
            .filter(|u| u.editing && u.field.as_deref() == Some("title") && Some(u.user_id.as_str()) != my_id)
            .collect();
        let keywords_editors: Vec<_> = self.users.values()
            .filter(|u| u.editing && u.field.as_deref() == Some("keywords") && Some(u.user_id.as_str()) != my_id)
            .collect();
        let body_editors: Vec<_> = self.users.values()
            .filter(|u| u.editing && u.field.as_deref() == Some("body") && Some(u.user_id.as_str()) != my_id)
            .collect();

        html! {
            <div>
                <header style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 20px;">
                    <h1>{ format!("Hello {}", self.my_name.as_deref().unwrap_or("...")) }</h1>
                    <div style="display: flex; align-items: center; gap: 10px;">
                        // Online users indicator
                        <div class="online-users">
                            { for self.users.values().map(|user| {
                                html! {
                                    <span 
                                        class={format!("user-badge {}", if user.editing { "active" } else { "inactive" })}
                                    >
                                        { &user.user_name }
                                    </span>
                                }
                            })}
                        </div>
                        <span class="version">{ format!("v{}", version) }</span>
                        <button onclick={ctx.link().callback(|_| Msg::ToggleMode)}>
                            { match self.mode { Mode::View => "Edit", Mode::Edit => "View" } }
                        </button>
                    </div>
                </header>

                if self.mode == Mode::View {
                    <div class="view-mode">
                        <span class="mode-badge mode-view">{"VIEW MODE"}</span>
                        <h2>{ title }</h2>
                        <p style="font-style: italic;">{ keywords }</p>
                        <hr/>
                        <div class="body-content">{Html::from_html_unchecked(body.into())}</div>
                    </div>
                } else {
                    <div class="edit-mode">
                        <span class="mode-badge mode-edit">{"EDIT MODE (CRDT Active)"}</span>
                        
                        <div class="field field-with-cursors">
                            <label>{ "Keywords" }</label>
                            <div class="input-wrapper">
                                <input 
                                    key="keywords"
                                    type="text" 
                                    value={keywords}
                                    oninput={ctx.link().callback(|e: InputEvent| {
                                        let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                        Msg::UpdateField(DOC_KEY_KEYWORDS, input.value())
                                    })}
                                    onfocus={ctx.link().callback(|_| Msg::SetEditing(Some("keywords".to_string())))}
                                    onblur={ctx.link().callback(|_| Msg::SetEditing(Some("general".to_string())))
                                />
                                <div class="cursors">
                                { for keywords_editors.iter().map(|user| {
                                    html! {
                                        <span 
                                            class="cursor-indicator" 
                                            title={user.user_name.clone()}
                                        >
                                            { &user.user_name }
                                        </span>
                                    }
                                })}
                                </div>
                            </div>
                        </div>

                        <div class="field field-with-cursors">
                            <label>{ "Keywords" }</label>
                            <div class="input-wrapper">
                                <input 
                                    key="keywords"
                                    type="text" 
                                    value={keywords}
                                    oninput={ctx.link().callback(|e: InputEvent| {
                                        let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                        Msg::UpdateField(DOC_KEY_KEYWORDS, input.value())
                                    })}
                                    onfocus={ctx.link().callback(|_| Msg::SetEditing(Some("keywords".to_string())))}
                                    onblur={ctx.link().callback(|_| Msg::SetEditing(Some("general".to_string()))) }
                                />
                                <div class="cursors">
                                { for keywords_editors.iter().map(|user| {
                                    html! {
                                        <span 
                                            class="cursor-indicator" 
                                            title={user.user_name.clone()}
                                        >
                                            { &user.user_name }
                                        </span>
                                    }
                                })}
                                </div>
                            </div>
                        </div>

                        <div class="field field-with-cursors">
                            <label>{ "Body" }</label>
                            <div class="input-wrapper">
                                <div 
                                    id="body-editor" 
                                    class="inline-editor"
                                    onfocus={ctx.link().callback(|_| Msg::SetEditing(Some("body".to_string())))}
                                    onblur={ctx.link().callback(|_| Msg::SetEditing(Some("general".to_string())))}
                                ></div>
                                <div class="cursors">
                                { for body_editors.iter().map(|user| {
                                    html! {
                                        <span 
                                            class="cursor-indicator" 
                                            title={user.user_name.clone()}
                                        >
                                            { &user.user_name }
                                        </span>
                                    }
                                })}
                                </div>
                            </div>
                        </div>
                    </div>
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
                _ => 0,
            })
            .unwrap_or(0)
    }

    fn handle_sync(&mut self, ctx: &Context<Self>, binary: Vec<u8>) -> bool {
        if let Ok(sync_msg) = sync::Message::decode(&binary) {
            let heads_before = self.doc.get_heads();
            self.doc.sync().receive_sync_message(&mut self.sync_state, sync_msg).unwrap();
            let heads_after = self.doc.get_heads();
            
            // Update TinyMCE if body changed from remote and we're in edit mode
            if heads_before != heads_after && self.tinymce_initialized {
                if let Some(editor) = get("body-editor") {
                    let new_body = self.get_str(DOC_KEY_BODY);
                    let current_content = editor.get_content();
                    if new_body != current_content {
                        editor.set_content(&new_body);
                    }
                }
            }
            
            ctx.link().send_message(Msg::SendSync);
            true
        } else {
            false
        }
    }

    fn handle_user_state(&mut self, user_state: UserState) -> bool {
        log_1(&format!(
            "[CLIENT] Received UserState: id={} name={} editing={} field={:?} online={}",
            user_state.user_id, user_state.user_name, user_state.editing, 
            user_state.field, user_state.online
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
                js_sys::Reflect::set(&options, &"menubar".into(), &false.into()).unwrap();
                js_sys::Reflect::set(&options, &"plugins".into(), &"lists link code".into()).unwrap();
                js_sys::Reflect::set(&options, &"toolbar".into(), &"undo redo | bold italic underline | bullist numlist | link code".into()).unwrap();
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
