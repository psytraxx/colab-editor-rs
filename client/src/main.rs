use automerge::{
    sync::{self, SyncDoc},
    transaction::Transactable,
    AutoCommit, ReadDoc,
};
use common::{WsMessage, UserState, DOC_KEY_BODY, DOC_KEY_KEYWORDS, DOC_KEY_TITLE, DOC_KEY_VERSION};
use web_sys::console::log_1;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

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
}

// PeerJS bindings
#[wasm_bindgen(module = "/inline_peer.js")]
extern "C" {
    pub type Peer;
    
    #[wasm_bindgen(js_name = newPeer)]
    fn new_peer(id: Option<String>) -> Peer;
    
    #[wasm_bindgen(method, getter)]
    fn id(this: &Peer) -> Option<String>;
    
    #[wasm_bindgen(method)]
    fn connect(this: &Peer, peer_id: &str) -> DataConnection;
    
    #[wasm_bindgen(method)]
    fn on(this: &Peer, event: &str, callback: &JsValue);
    
    pub type DataConnection;
    
    #[wasm_bindgen(method)]
    fn on(this: &DataConnection, event: &str, callback: &JsValue);
    
    #[wasm_bindgen(method)]
    fn send(this: &DataConnection, data: &str);
    
    #[wasm_bindgen(method, getter)]
    fn peer(this: &DataConnection) -> String;
}

struct App {
    doc: AutoCommit,
    sync_states: HashMap<String, sync::State>, // Per-peer sync state
    mode: Mode,
    peer: Option<Peer>,
    connections: HashMap<String, DataConnection>, // peer_id -> connection
    tinymce_initialized: bool,
    users: HashMap<String, UserState>,
    my_id: Option<String>,
    my_name: Option<String>,
    peer_id_to_connect: String, // For UI to connect to specific peer
}

#[derive(PartialEq, Clone)]
enum Mode {
    View,
    Edit,
}

enum Msg {
    PeerMessage(String, WsMessage), // peer_id, message
    PeerConnected(String, DataConnection), // peer_id, connection
    PeerInitialized(Peer),
    UpdateField(&'static str, String),
    ToggleMode,
    SendSync,
    InitTinyMCE,
    SyncBodyFromTinyMCE,
    SetEditing(Option<String>), // field name or None to stop editing
    ConnectToPeer,
    UpdatePeerIdInput(String),
}

impl Component for App {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let link = ctx.link().clone();
        
        // Initialize PeerJS
        spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(100).await;
            
            // Create peer with random ID
            let peer = new_peer(None);
            
            // Setup "open" event - peer is ready
            let link_open = link.clone();
            let peer_js: JsValue = peer.into();
            let peer_js_clone = peer_js.clone();
            let on_open = Closure::wrap(Box::new(move |_peer_id: JsValue| {
                // Get the actual peer from JS and send it
                let peer_ref: Peer = peer_js_clone.clone().unchecked_into();
                if let Some(id) = peer_ref.id() {
                    log_1(&format!("[P2P] Peer initialized with ID: {}", id).into());
                    link_open.send_message(Msg::PeerInitialized(peer_ref));
                }
            }) as Box<dyn FnMut(JsValue)>);
            
            let peer_ref: &Peer = peer_js.unchecked_ref();
            peer_ref.on("open", on_open.as_ref().unchecked_ref());
            on_open.forget();
            
            // Setup "connection" event - incoming peer connection
            let link_conn = link.clone();
            let on_connection = Closure::wrap(Box::new(move |conn_js: JsValue| {
                let conn: DataConnection = conn_js.unchecked_into();
                let peer_id = conn.peer();
                log_1(&format!("[P2P] 📥 Incoming connection from: {}", peer_id).into());
                
                // Setup data handler for this connection
                let link_data = link_conn.clone();
                let peer_id_data = peer_id.clone();
                let on_data = Closure::wrap(Box::new(move |data: JsValue| {
                    if let Some(text) = data.as_string() {
                        log_1(&format!("[P2P] Received from {}: {}", peer_id_data, text).into());
                        if let Ok(msg) = serde_json::from_str::<WsMessage>(&text) {
                            link_data.send_message(Msg::PeerMessage(peer_id_data.clone(), msg));
                        }
                    }
                }) as Box<dyn FnMut(JsValue)>);
                
                conn.on("data", on_data.as_ref().unchecked_ref());
                on_data.forget();
                
                link_conn.send_message(Msg::PeerConnected(peer_id.clone(), conn));
            }) as Box<dyn FnMut(JsValue)>);
            peer_ref.on("connection", on_connection.as_ref().unchecked_ref());
            on_connection.forget();
            
            // Setup error handler for peer
            let on_error = Closure::wrap(Box::new(move |err: JsValue| {
                log_1(&format!("[P2P] ❌ Peer error: {:?}", err).into());
            }) as Box<dyn FnMut(JsValue)>);
            peer_ref.on("error", on_error.as_ref().unchecked_ref());
            on_error.forget();
            
            // Don't send PeerInitialized here - wait for "open" event
        });

        // Initialize document
        let mut doc = AutoCommit::new();
        doc.put(automerge::ROOT, DOC_KEY_TITLE, "Untitled").unwrap();
        doc.put(automerge::ROOT, DOC_KEY_BODY, "").unwrap();
        doc.put(automerge::ROOT, DOC_KEY_KEYWORDS, "").unwrap();
        doc.put(automerge::ROOT, DOC_KEY_VERSION, 0u64).unwrap();

        Self {
            doc,
            sync_states: HashMap::new(),
            mode: Mode::View,
            peer: None,
            connections: HashMap::new(),
            tinymce_initialized: false,
            users: HashMap::new(),
            my_id: None,
            my_name: None,
            peer_id_to_connect: String::new(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::PeerInitialized(peer) => {
                let peer_id = peer.id().unwrap_or_else(|| "unknown".to_string());
                self.my_id = Some(peer_id.clone());
                self.my_name = Some(peer_id.clone());
                
                // Add self to users list
                self.users.insert(peer_id.clone(), UserState {
                    user_id: peer_id.clone(),
                    user_name: peer_id,
                    editing: false,
                    field: None,
                    online: true,
                });
                
                self.peer = Some(peer);
                log_1(&format!("[P2P] My peer ID: {}", self.my_id.as_ref().unwrap()).into());
                true
            }
            Msg::PeerConnected(peer_id, conn) => {
                log_1(&format!("[P2P] Peer connected: {}", peer_id).into());
                self.connections.insert(peer_id.clone(), conn);
                self.sync_states.insert(peer_id.clone(), sync::State::new());
                
                // Add the remote peer to users list immediately
                self.users.insert(peer_id.clone(), UserState {
                    user_id: peer_id.clone(),
                    user_name: peer_id.clone(),
                    editing: false,
                    field: None,
                    online: true,
                });
                
                // Send initial sync to new peer
                ctx.link().send_message(Msg::SendSync);
                
                // Send our user state to new peer
                if let Some(my_id) = &self.my_id {
                    if let Some(my_state) = self.users.get(my_id) {
                        self.broadcast_user_state(my_state.clone());
                    }
                }
                true
            }
            Msg::PeerMessage(peer_id, ws_msg) => {
                match ws_msg {
                    WsMessage::Welcome(_) => false, // Not used in P2P
                    WsMessage::Sync(binary) => self.handle_sync_from_peer(ctx, &peer_id, binary),
                    WsMessage::UserState(user_state) => self.handle_user_state(user_state),
                }
            }
            Msg::ConnectToPeer => {
                if let Some(peer) = &self.peer {
                    let remote_peer_id = self.peer_id_to_connect.trim().to_string();
                    if !remote_peer_id.is_empty() && !self.connections.contains_key(&remote_peer_id) {
                        log_1(&format!("[P2P] Attempting to connect to peer: {}", remote_peer_id).into());
                        
                        let conn = peer.connect(&remote_peer_id);
                        
                        // Setup data handler
                        let link = ctx.link().clone();
                        let peer_id_clone = remote_peer_id.clone();
                        let on_data = Closure::wrap(Box::new(move |data: JsValue| {
                            if let Some(text) = data.as_string() {
                                log_1(&format!("[P2P] Received from {}: {}", peer_id_clone, text).into());
                                if let Ok(msg) = serde_json::from_str::<WsMessage>(&text) {
                                    link.send_message(Msg::PeerMessage(peer_id_clone.clone(), msg));
                                }
                            }
                        }) as Box<dyn FnMut(JsValue)>);
                        
                        conn.on("data", on_data.as_ref().unchecked_ref());
                        on_data.forget();
                        
                        // Setup open handler
                        let link_open = ctx.link().clone();
                        let peer_id_open = remote_peer_id.clone();
                        let conn_js: JsValue = conn.into();
                        let conn_js_clone = conn_js.clone();
                        let conn_js_clone2 = conn_js.clone();
                        
                        let on_open = Closure::wrap(Box::new(move || {
                            log_1(&format!("[P2P] ✅ Connection established to {}", peer_id_open).into());
                            let conn_ref: DataConnection = conn_js_clone.clone().unchecked_into();
                            link_open.send_message(Msg::PeerConnected(peer_id_open.clone(), conn_ref));
                        }) as Box<dyn Fn()>);
                        
                        // Setup error handler
                        let peer_id_error = remote_peer_id.clone();
                        let on_error = Closure::wrap(Box::new(move |err: JsValue| {
                            log_1(&format!("[P2P] ❌ Connection error to {}: {:?}", peer_id_error, err).into());
                        }) as Box<dyn FnMut(JsValue)>);
                        
                        // Setup close handler
                        let peer_id_close = remote_peer_id.clone();
                        let on_close = Closure::wrap(Box::new(move || {
                            log_1(&format!("[P2P] 🔌 Connection closed to {}", peer_id_close).into());
                        }) as Box<dyn Fn()>);
                        
                        let conn_ref: &DataConnection = conn_js_clone2.unchecked_ref();
                        conn_ref.on("open", on_open.as_ref().unchecked_ref());
                        conn_ref.on("error", on_error.as_ref().unchecked_ref());
                        conn_ref.on("close", on_close.as_ref().unchecked_ref());
                        
                        on_open.forget();
                        on_error.forget();
                        on_close.forget();
                    } else if remote_peer_id.is_empty() {
                        log_1(&"[P2P] ⚠️ Please enter a peer ID to connect".into());
                    } else {
                        log_1(&format!("[P2P] ⚠️ Already connected to {}", remote_peer_id).into());
                    }
                }
                false
            }
            Msg::UpdatePeerIdInput(value) => {
                self.peer_id_to_connect = value;
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
                    "[P2P] SetEditing called with field={:?}",
                    field
                ).into());
                
                // Update our own state locally and get a copy to broadcast
                let state_to_broadcast = if let Some(my_id) = &self.my_id {
                    if let Some(my_state) = self.users.get_mut(my_id) {
                        my_state.editing = field.is_some();
                        my_state.field = field.clone();
                        Some(my_state.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                
                // Broadcast to all peers
                if let Some(state) = state_to_broadcast {
                    self.broadcast_user_state(state);
                }
                true
            }
            Msg::SendSync => {
                // Send sync message to all connected peers
                for (peer_id, conn) in &self.connections {
                    if let Some(sync_state) = self.sync_states.get_mut(peer_id) {
                        if let Some(msg) = self.doc.sync().generate_sync_message(sync_state) {
                            let ws_msg = WsMessage::Sync(msg.encode());
                            if let Ok(json) = serde_json::to_string(&ws_msg) {
                                conn.send(&json);
                            }
                        }
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
                <header style="display: flex; flex-direction: column; gap: 15px; margin-bottom: 20px;">
                    <div style="display: flex; justify-content: space-between; align-items: center;">
                        <h1>{ "WebRTC Collaborative Editor" }</h1>
                    </div>
                    
                    // P2P Connection Info - only show when not connected
                    if self.connections.is_empty() {
                        <div style="background: #f5f5f5; padding: 12px; border-radius: 8px;">
                            <div style="margin-bottom: 8px;">
                                <strong>{"Your Peer ID: "}</strong>
                                <code style="background: white; padding: 4px 8px; border-radius: 4px;">
                                    { self.my_id.as_deref().unwrap_or("Initializing...") }
                                </code>
                            </div>
                            <div style="display: flex; gap: 8px; align-items: center;">
                                <input 
                                    type="text"
                                    placeholder="Enter peer ID to connect"
                                    value={self.peer_id_to_connect.clone()}
                                    oninput={ctx.link().callback(|e: InputEvent| {
                                        let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                        Msg::UpdatePeerIdInput(input.value())
                                    })}
                                    style="flex: 1; margin: 0;"
                                />
                                <button 
                                    onclick={ctx.link().callback(|_| Msg::ConnectToPeer)} 
                                    style="margin: 0;"
                                >
                                    {"Connect"}
                                </button>
                            </div>
                        </div>
                    } else {
                        <div style="background: #e8f5e9; padding: 8px 12px; border-radius: 8px; font-size: 0.9em; color: #2e7d32;">
                            {"✅ Connected to "}{ self.connections.len() }{ " peer(s)" }
                        </div>
                    }
                    
                    // Online users and version
                    <div style="display: flex; justify-content: space-between; align-items: center;">
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
                    </div>
                </header>

                if self.users.len() < 2 {
                    <div style="text-align: center; padding: 40px; background: #f9f9f9; border-radius: 8px;">
                        <h2>{"⏳ Waiting for collaborators..."}</h2>
                        <p style="color: #666;">{"Share your Peer ID above with others to start collaborating."}</p>
                        <p style="color: #888; font-size: 0.9em;">{"The editor will become available once another peer connects."}</p>
                    </div>
                } else if self.mode == Mode::View {
                    <div class="view-mode">
                        <div style="display: flex; align-items: center; gap: 10px; margin-bottom: 1rem;">
                            <span class="mode-badge mode-view">{"VIEW MODE"}</span>
                            <button onclick={ctx.link().callback(|_| Msg::ToggleMode)} style="margin: 0;">
                                {"✏️ Edit"}
                            </button>
                        </div>
                        <h2>{ title }</h2>
                        <p style="font-style: italic;">{ keywords }</p>
                        <hr/>
                        <div class="body-content">{Html::from_html_unchecked(body.into())}</div>
                    </div>
                } else {
                    <div class="edit-mode">
                        <div style="display: flex; align-items: center; gap: 10px; margin-bottom: 1rem;">
                            <span class="mode-badge mode-edit">{"EDIT MODE (CRDT Active)"}</span>
                            <button onclick={ctx.link().callback(|_| Msg::ToggleMode)} style="margin: 0;">
                                {"👁️ View"}
                            </button>
                        </div>
                        
                        <div class="field field-with-cursors">
                            <label>{ "Title" }</label>
                            <div class="input-wrapper">
                                <input 
                                    key="title"
                                    type="text" 
                                    value={title}
                                    oninput={ctx.link().callback(|e: InputEvent| {
                                        let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                        Msg::UpdateField(DOC_KEY_TITLE, input.value())
                                    })}
                                    onfocus={ctx.link().callback(|_| Msg::SetEditing(Some("title".to_string()))) }
                                    onblur={ctx.link().callback(|_| Msg::SetEditing(Some("general".to_string()))) }
                                />
                                <div class="cursors">
                                { for title_editors.iter().map(|user| {
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
                                    onfocus={ctx.link().callback(|_| Msg::SetEditing(Some("keywords".to_string()))) }
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

    fn handle_sync_from_peer(&mut self, _ctx: &Context<Self>, peer_id: &str, binary: Vec<u8>) -> bool {
        if let Ok(sync_msg) = sync::Message::decode(&binary) {
            let heads_before = self.doc.get_heads();
            
            // Get or create sync state for this peer
            let sync_state = self.sync_states.entry(peer_id.to_string()).or_insert_with(sync::State::new);
            
            self.doc.sync().receive_sync_message(sync_state, sync_msg).unwrap();
            let heads_after = self.doc.get_heads();
            
            // Update TinyMCE if body changed from remote and we're in edit mode
            if heads_before != heads_after && self.tinymce_initialized {
                if let Some(editor) = get("body-editor") {
                    let new_body = self.doc.get(automerge::ROOT, DOC_KEY_BODY)
                        .ok()
                        .flatten()
                        .and_then(|(v, _)| match v {
                            automerge::Value::Scalar(std::borrow::Cow::Owned(automerge::ScalarValue::Str(s))) => Some(s.into()),
                            automerge::Value::Scalar(std::borrow::Cow::Borrowed(automerge::ScalarValue::Str(s))) => Some(s.to_string()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    let current_content = editor.get_content();
                    if new_body != current_content {
                        editor.set_content(&new_body);
                    }
                }
            }
            
            // Send sync response back to this specific peer
            if let Some(conn) = self.connections.get(peer_id) {
                // Need to get sync_state again to avoid multiple mutable borrows
                if let Some(sync_state) = self.sync_states.get_mut(peer_id) {
                    if let Some(reply_msg) = self.doc.sync().generate_sync_message(sync_state) {
                        let ws_msg = WsMessage::Sync(reply_msg.encode());
                        if let Ok(json) = serde_json::to_string(&ws_msg) {
                            conn.send(&json);
                        }
                    }
                }
            }
            
            true
        } else {
            false
        }
    }
    
    fn broadcast_user_state(&self, user_state: UserState) {
        let ws_msg = WsMessage::UserState(user_state);
        if let Ok(json) = serde_json::to_string(&ws_msg) {
            for conn in self.connections.values() {
                conn.send(&json);
            }
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
