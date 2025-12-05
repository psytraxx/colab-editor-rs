use automerge::{
    sync::{self, SyncDoc},
    transaction::Transactable,
    AutoCommit, ReadDoc,
};
use common::{WsMessage, DOC_KEY_BODY, DOC_KEY_DESCRIPTION, DOC_KEY_TITLE, DOC_KEY_VERSION};
use futures::{channel::mpsc::Sender, SinkExt, StreamExt};
use gloo::net::websocket::{futures::WebSocket, Message};
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
}

impl Component for App {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let link = ctx.link().clone();
        spawn_local(async move {
            let ws = WebSocket::open("ws://127.0.0.1:3000/ws").unwrap();
            let (mut write, mut read) = ws.split();
            let (tx, mut rx) = futures::channel::mpsc::channel::<Message>(1000);

            link.send_message(Msg::WsConnected(tx));

            spawn_local(async move {
                while let Some(msg) = rx.next().await {
                    write.send(msg).await.unwrap();
                }
            });

            while let Some(msg) = read.next().await {
                if let Ok(Message::Text(text)) = msg {
                    if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                        link.send_message(Msg::WsMessage(ws_msg));
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
                    WsMessage::Sync(binary) => {
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
                    _ => false,
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
                        Mode::View
                    }
                };
                true
            }
            Msg::InitTinyMCE => {
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
        let description = self.get_str(DOC_KEY_DESCRIPTION);
        let body = self.get_str(DOC_KEY_BODY);
        let version = self.get_u64(DOC_KEY_VERSION);

        html! {
            <div>
                <header style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 20px;">
                    <h1>{ "Colab Editor Demo" }</h1>
                    <div>
                        <span class="version">{ format!("Version: {}", version) }</span>
                        <button onclick={ctx.link().callback(|_| Msg::ToggleMode)} style="margin-left: 10px;">
                            { match self.mode { Mode::View => "Edit", Mode::Edit => "View" } }
                        </button>
                    </div>
                </header>

                if self.mode == Mode::View {
                    <div class="view-mode">
                        <span class="mode-badge mode-view">{"VIEW MODE"}</span>
                        <h2>{ title }</h2>
                        <p style="font-style: italic;">{ description }</p>
                        <hr/>
                        <div class="body-content">{Html::from_html_unchecked(body.into())}</div>
                    </div>
                } else {
                    <div class="edit-mode">
                        <span class="mode-badge mode-edit">{"EDIT MODE (CRDT Active)"}</span>
                        
                        <div class="field">
                            <label>{ "Title" }</label>
                            <input 
                                type="text" 
                                value={title} 
                                oninput={ctx.link().callback(|e: InputEvent| {
                                    let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                    Msg::UpdateField(DOC_KEY_TITLE, input.value())
                                })}
                            />
                        </div>

                        <div class="field">
                            <label>{ "Description" }</label>
                            <input 
                                type="text" 
                                value={description}
                                oninput={ctx.link().callback(|e: InputEvent| {
                                    let input: web_sys::HtmlInputElement = e.target_unchecked_into();
                                    Msg::UpdateField(DOC_KEY_DESCRIPTION, input.value())
                                })}
                            />
                        </div>

                        <div class="field">
                            <label>{ "Body" }</label>
                            <div id="body-editor" class="inline-editor"></div>
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
}

fn main() {
    yew::Renderer::<App>::new().render();
}
