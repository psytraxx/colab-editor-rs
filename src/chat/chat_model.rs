use super::web_rtc_manager::{
    connection_state::{ConnectionState, State},
    IceCandidate, NetworkManager,
};
use base64::{self, prelude::BASE64_STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use std::str;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{console, Element, HtmlInputElement, InputEvent, RtcDataChannelState};
use yew::{html, html::NodeRef, Component, Context, Html, KeyboardEvent, TargetCast};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MessageSender {
    Me,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub sender: MessageSender,
    pub content: String,
}

impl Message {
    pub fn new(content: String, sender: MessageSender) -> Message {
        Message { content, sender }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConnectionString {
    pub ice_candidates: Vec<IceCandidate>,
    pub offer: String,
}

pub struct ChatModel<T: NetworkManager + 'static> {
    web_rtc_manager: Rc<RefCell<T>>,
    messages: Vec<Message>,
    text_input: String,
    node_ref: NodeRef,
}

#[derive(Clone, Debug)]
pub enum ChatModelMessage {
    StartAsServer,
    ConnectToServer,
    UpdateWebRTCState(State),
    Disconnect,
    SendMessage,
    NewMessage(Message),
    UpdateTextInput(String),
    OnKeyUp(KeyboardEvent),
    CopyToClipboard,
    ValidateOffer,
    ResetWebRTC,
}

impl<T: NetworkManager + 'static> Component for ChatModel<T> {
    type Message = ChatModelMessage;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        ChatModel {
            web_rtc_manager: T::new(create_message_callback(ctx))
                .expect("Failed to create WebRTC manager"),
            messages: vec![],
            text_input: "".into(),
            node_ref: NodeRef::default(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            ChatModelMessage::StartAsServer => self.start_as_server(),
            ChatModelMessage::ConnectToServer => self.connect_to_server(),
            ChatModelMessage::UpdateWebRTCState(web_rtc_state) => {
                self.update_webrtc_state(web_rtc_state)
            }
            ChatModelMessage::ResetWebRTC => self.reset_webrtc(ctx),
            ChatModelMessage::UpdateTextInput(val) => self.update_text_input(val),
            ChatModelMessage::ValidateOffer => self.validate_offer(),
            ChatModelMessage::NewMessage(message) => self.new_message(message),
            ChatModelMessage::SendMessage => self.send_message(),
            ChatModelMessage::Disconnect => self.disconnect(ctx),
            ChatModelMessage::OnKeyUp(event) => self.on_key_up(event),
            ChatModelMessage::CopyToClipboard => self.copy_to_clipboard(),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div class="h-full">
                { self.get_chat_header(ctx) }
                { self.get_content(ctx) }
            </div>
        }
    }
}

impl<T: NetworkManager + 'static> ChatModel<T> {
    fn start_as_server(&mut self) -> bool {
        self.web_rtc_manager
            .borrow_mut()
            .set_state(State::Server(ConnectionState::new()));
        T::start_web_rtc(self.web_rtc_manager.clone()).expect("Failed to start WebRTC manager");
        true
    }

    fn connect_to_server(&mut self) -> bool {
        self.web_rtc_manager
            .borrow_mut()
            .set_state(State::Client(ConnectionState::new()));
        T::start_web_rtc(self.web_rtc_manager.clone()).expect("Failed to start WebRTC manager");
        true
    }

    fn update_webrtc_state(&mut self, web_rtc_state: State) -> bool {
        self.text_input.clear();
        let debug = get_debug_state_string(&web_rtc_state);
        console::log_1(&debug.into());
        true
    }

    fn reset_webrtc(&mut self, ctx: &Context<Self>) -> bool {
        self.web_rtc_manager =
            T::new(create_message_callback(ctx)).expect("Failed to create WebRTC manager");
        self.messages.clear();
        self.text_input.clear();
        true
    }

    fn update_text_input(&mut self, val: String) -> bool {
        self.text_input = val;
        true
    }

    fn validate_offer(&self) -> bool {
        let state = self.web_rtc_manager.borrow().get_state();
        let result = match state {
            State::Server(_) => T::validate_answer(self.web_rtc_manager.clone(), &self.text_input),
            _ => T::validate_offer(self.web_rtc_manager.clone(), &self.text_input),
        };

        if let Err(err) = result {
            web_sys::Window::alert_with_message(
                &web_sys::window().unwrap(),
                &format!("Cannot use offer. Failure reason: {:?}", err),
            )
            .expect("alert should work");
        }
        true
    }

    fn new_message(&mut self, message: Message) -> bool {
        self.messages.push(message);
        self.scroll_top();
        true
    }

    fn send_message(&mut self) -> bool {
        let my_message = Message::new(self.text_input.clone(), MessageSender::Me);
        self.messages.push(my_message);
        self.web_rtc_manager.borrow().send_message(&self.text_input);
        self.text_input.clear();
        self.scroll_top();
        true
    }

    fn disconnect(&mut self, ctx: &Context<Self>) -> bool {
        self.web_rtc_manager =
            T::new(create_message_callback(ctx)).expect("Failed to create WebRTC manager");
        self.messages.clear();
        self.text_input.clear();
        true
    }

    fn on_key_up(&mut self, event: KeyboardEvent) -> bool {
        if event.key_code() == 13 && !self.text_input.is_empty() {
            let my_message = Message::new(self.text_input.clone(), MessageSender::Me);
            self.messages.push(my_message);
            self.web_rtc_manager.borrow().send_message(&self.text_input);
            self.text_input.clear();
            self.scroll_top();
        }
        true
    }

    fn copy_to_clipboard(&self) -> bool {
        self.copy_content_to_clipboard();
        true
    }

    fn scroll_top(&self) {
        let node_ref = self.node_ref.clone();
        spawn_local(async move {
            if let Some(chat_main) = node_ref.cast::<Element>() {
                let current_scroll_top = chat_main.scroll_top();
                chat_main.set_scroll_top(current_scroll_top + 100000000);
            }
        });
    }

    fn get_chat_header(&self, ctx: &Context<Self>) -> Html {
        let is_disconnect_button_visible =
            self.web_rtc_manager.borrow().get_state() != State::Default;
        html! {
            <header class="bg-gray-800 text-white p-4 shadow-md">
                <div class="container mx-auto flex justify-between items-center">
                    <div class="text-sm font-mono">
                        { self.get_debug_html() }
                    </div>
                    {
                        if is_disconnect_button_visible {
                            html! {
                                <button
                                    class="bg-red-500 hover:bg-red-600 text-white px-4 py-2 rounded-lg transition-colors"
                                    onclick={ctx.link().callback(move |_| ChatModelMessage::Disconnect)}>
                                    {"Disconnect"}
                                </button>
                            }
                        } else {
                            html! {}
                        }
                    }
                </div>
            </header>
        }
    }

    fn is_chat_enabled(&self) -> bool {
        match &self.web_rtc_manager.borrow().get_state() {
            State::Default => false,
            State::Server(connection_state) | State::Client(connection_state) => {
                connection_state.data_channel_state == Some(RtcDataChannelState::Open)
            }
        }
    }

    fn get_input_for_chat_message(&self, ctx: &Context<Self>) -> Html {
        if !self.is_chat_enabled() {
            return html! {};
        }
        let is_send_button_enabled = !self.text_input.is_empty();
        let button_class = if is_send_button_enabled {
            "bg-blue-500 hover:bg-blue-600 text-white"
        } else {
            "bg-gray-300 text-gray-500 cursor-not-allowed"
        };

        html! {
            <div class="border-t border-gray-200 p-4 bg-white">
                <div class="flex items-center gap-3 max-w-3xl mx-auto">
                    <input
                        type="text"
                        class="flex-grow p-3 border border-gray-300 rounded-lg shadow-sm focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                        placeholder={"Type a message..."}
                        id="chat-message-box"
                        value={self.text_input.clone()}
                        oninput={ctx.link().callback(|e: InputEvent| {ChatModelMessage::UpdateTextInput(e.target_unchecked_into::<HtmlInputElement>().value())})}
                        onkeyup={ctx.link().callback(move |e: KeyboardEvent| ChatModelMessage::OnKeyUp(e))}
                    />
                    <button
                        class={format!("{} font-medium py-3 px-6 rounded-lg transition-colors", button_class)}
                        disabled={!is_send_button_enabled}
                        onclick={ctx.link().callback(move |_| ChatModelMessage::SendMessage)}
                    >
                        {"Send"}
                    </button>
                </div>
            </div>
        }
    }

    fn get_validate_offer_or_answer(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div class="space-y-3">
                <textarea
                    class="w-full p-3 border border-gray-300 rounded-lg shadow-sm focus:ring-2 focus:ring-blue-500 focus:border-transparent resize-none min-h-[100px]"
                    value={self.text_input.clone()}
                    oninput={ctx.link().callback(|e: InputEvent| {ChatModelMessage::UpdateTextInput(e.target_unchecked_into::<HtmlInputElement>().value())})}
                    placeholder="Paste the connection code here"
                >
                </textarea>
                <button
                    class="w-full bg-blue-500 hover:bg-blue-600 text-white font-medium py-2 px-4 rounded-lg transition-colors"
                    onclick={ctx.link().callback(move |_| ChatModelMessage::ValidateOffer)}
                >
                    {"Connect"}
                </button>
            </div>
        }
    }

    fn get_serialized_offer_and_candidates(&self) -> String {
        let connection_string = ConnectionString {
            offer: self
                .web_rtc_manager
                .borrow()
                .get_offer()
                .expect("no offer yet"),
            ice_candidates: self.web_rtc_manager.borrow().get_ice_candidates(),
        };

        let serialized: String = serde_json::to_string(&connection_string).unwrap();
        BASE64_STANDARD.encode(serialized)
    }

    fn get_offer_and_candidates(&self, ctx: &Context<Self>) -> Html {
        let encoded = self.get_serialized_offer_and_candidates();
        html! {
            <div class="space-y-4">
                <div class="text-lg font-medium text-gray-700">
                    { "Share this connection code:" }
                </div>
                <div class="bg-gray-50 p-4 rounded-lg border border-gray-200">
                    <div class="break-all text-sm font-mono mb-3" id="copy-elem">{encoded}</div>
                    <button
                        class="bg-gray-600 hover:bg-gray-700 text-white font-medium py-2 px-4 rounded-lg transition-colors flex items-center gap-2"
                        onclick={ctx.link().callback(move |_| ChatModelMessage::CopyToClipboard)}
                    >
                        {"Copy to clipboard"}
                    </button>
                </div>
            </div>
        }
    }

    fn get_debug_html(&self) -> Html {
        let state = self.web_rtc_manager.borrow().get_state();
        html! {
            <div>
                { format!("{:?}", state) }
            </div>
        }
    }

    fn copy_content_to_clipboard(&self) {
        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();
        let aux = document.create_element("input").unwrap();
        let aux = aux.dyn_into::<web_sys::HtmlInputElement>().unwrap();
        let content: String = document
            .get_element_by_id("copy-elem")
            .unwrap()
            .inner_html();

        let _result = aux.set_attribute("value", &content);
        let document = window.document().unwrap();
        let _result = document.body().unwrap().append_child(&aux);
        aux.select();
        let html_document = document.dyn_into::<web_sys::HtmlDocument>().unwrap();
        let _result = html_document.exec_command("copy");
        let document = window.document().unwrap();
        let _result = document.body().unwrap().remove_child(&aux);
    }

    fn get_messages_as_html(&self) -> Html {
        html! {
            <ul class="space-y-4 px-4">
                {
                    for self.messages.iter().map(|a_message| {
                        let (message_class, align_class) = if a_message.sender == MessageSender::Other {
                            ("bg-gray-100 text-gray-800", "self-start")
                        } else {
                            ("bg-blue-500 text-white", "self-end")
                        };
                        html! {
                            <div class={format!("flex flex-col {}", align_class)}>
                                <div class={format!("max-w-[70%] break-words rounded-lg px-4 py-2 shadow-sm {}", message_class)}>
                                    <div class="text-xs opacity-75 mb-1">
                                        { if a_message.sender == MessageSender::Other { "Friend" } else { "Me" } }
                                    </div>
                                    <div>
                                        { a_message.content.clone() }
                                    </div>
                                </div>
                            </div>
                        }
                    })
                }
            </ul>
        }
    }

    fn get_content(&self, ctx: &Context<Self>) -> Html {
        match &self.web_rtc_manager.borrow().get_state() {
            State::Default => self.get_default_content(ctx),
            State::Server(connection_state) => self.get_server_content(ctx, connection_state),
            State::Client(connection_state) => self.get_client_content(ctx, connection_state),
        }
    }

    fn get_default_content(&self, ctx: &Context<Self>) -> Html {
        html! {
            <main class="flex flex-row justify-center items-center h-screen" ref={self.node_ref.clone()}>
                <div class="flex flex-row items-center space-x-2">
                    <button
                        class="bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded"
                        onclick={ctx.link().callback(move |_| ChatModelMessage::StartAsServer)}>
                        {"Start a new conversation"}
                    </button>
                    <span class="mx-2">{" or "}</span>
                    <button
                        class="bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded"
                        onclick={ctx.link().callback(move |_| ChatModelMessage::ConnectToServer)}>
                        {"Join a conversation"}
                    </button>
                </div>
            </main>
        }
    }

    fn get_server_content(&self, ctx: &Context<Self>, connection_state: &ConnectionState) -> Html {
        html! {
            <>
                { self.get_input_for_chat_message(ctx) }
                <main class="flex flex-col h-screen" ref={self.node_ref.clone()}>
                    {
                        if connection_state.data_channel_state == Some(RtcDataChannelState::Open) {
                            html! {
                                <div class="flex-grow overflow-y-auto p-4">
                                    { self.get_messages_as_html() }
                                </div>
                            }
                        } else if connection_state.ice_gathering_state.is_some() {
                            html! {
                                <div class="flex-grow p-4">
                                    <div class="mb-4">
                                        { self.get_offer_and_candidates(ctx) }
                                    </div>
                                    <div>
                                        { "Enter off response here:" }
                                        { self.get_validate_offer_or_answer(ctx) }
                                    </div>
                                </div>
                            }
                        } else {
                            html! {}
                        }
                    }
                </main>
            </>
        }
    }

    fn get_client_content(&self, ctx: &Context<Self>, connection_state: &ConnectionState) -> Html {
        html! {
            <>
                { self.get_input_for_chat_message(ctx) }
                <main class="flex flex-col h-screen" ref={self.node_ref.clone()}>
                    {
                        if connection_state.data_channel_state == Some(RtcDataChannelState::Open) {
                            html! {
                                <div class="flex-grow overflow-y-auto p-4">
                                    { self.get_messages_as_html() }
                                </div>
                            }
                        } else if connection_state.ice_gathering_state.is_some() {
                            html! {
                                <div class="flex-grow p-4">
                                    <div class="mb-4">
                                        { self.get_offer_and_candidates(ctx) }
                                    </div>
                                </div>
                            }
                        } else {
                            html! {
                                <div class="flex-grow p-4">
                                    { "Enter join token" }
                                    { self.get_validate_offer_or_answer(ctx) }
                                    <p class="mt-2 text-sm text-gray-600">
                                        { "If after a while the connection cannot be established, it is probably because there is a network issue between the 2 computers." }
                                    </p>
                                </div>
                            }
                        }
                    }
                </main>
            </>
        }
    }
}

fn create_message_callback(
    ctx: &Context<ChatModel<impl NetworkManager + 'static>>,
) -> Rc<dyn Fn(ChatModelMessage)> {
    let link = ctx.link().clone();
    Rc::new(move |msg: ChatModelMessage| {
        link.send_message(msg);
    })
}

fn get_debug_state_string(state: &State) -> String {
    match state {
        State::Default => "Default State".into(),
        State::Server(connection_state) => format!(
            "{}\nice gathering: {:?}\nice connection: {:?}\ndata channel: {:?}\n",
            "Server",
            connection_state.ice_gathering_state,
            connection_state.ice_connection_state,
            connection_state.data_channel_state,
        ),
        State::Client(connection_state) => format!(
            "{}\nice gathering: {:?}\nice connection: {:?}\ndata channel: {:?}\n",
            "Client",
            connection_state.ice_gathering_state,
            connection_state.ice_connection_state,
            connection_state.data_channel_state,
        ),
    }
}
