use std::{cell::RefCell, rc::Rc};

use crate::{
    chat::{
        chat_model::{Message, MessageSender},
        web_rtc_manager::connection_state::State,
    },
    chat_service::ChatService,
    utils::copy_content_to_clipboard,
};
use web_sys::{console, HtmlInputElement};
use yew::{
    function_component, html, use_state, Callback, Html, InputEvent, KeyboardEvent, Properties,
    TargetCast,
};

#[derive(Properties, PartialEq, Clone)]
pub struct Props<T>
where
    T: PartialEq + ChatService + 'static,
{
    pub service: Rc<RefCell<T>>,
}

#[function_component(ChatComponent)]
pub fn chat_component<T>(props: &Props<T>) -> Html
where
    T: PartialEq + ChatService + Clone + 'static,
{
    let messages: yew::UseStateHandle<Vec<Message>> = use_state(Vec::new);
    let is_loading = use_state(|| false);
    let service = props.service.clone();

    let on_start_server = {
        let service = service.clone();

        Callback::from(move |_| {
            service.borrow_mut().connect_client();
            console::log_1(&"on_start_server".into());
        })
    };

    let on_connect_server = {
        let messages = messages.clone();
        Callback::from(move |_| {
            is_loading.set(true);
            messages.set(
                [
                    (*messages).clone(),
                    vec![Message {
                        sender: MessageSender::Me,
                        content: "Hello".to_string(),
                    }],
                ]
                .concat(),
            );
        })
    };

    let on_disconnect = {
        let service = service.clone();
        let messages = messages.clone();

        Callback::from(move |_| {
            service.borrow_mut().disconnect();
            messages.set(Vec::new());
        })
    };

    let state = service.borrow().get_state();

    html! {
        {
            match state {
                State::Default => {
                    html! {
                        <main class="flex flex-row justify-center items-center h-screen">
                            <div class="flex flex-row items-center space-x-2">
                                <button
                                    class="bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded"
                                    onclick={on_start_server}>
                                    {"Start a new conversation"}
                                </button>
                                <span class="mx-2">{" or "}</span>
                                <button
                                    class="bg-blue-500 hover:bg-blue-700 text-white font-bold py-2 px-4 rounded"
                                    onclick={on_connect_server}>
                                    {"Join a conversation"}
                                </button>
                                <MessageList messages={(*messages).clone()} />
                                <MessageInput
                                    input_text={"ssss".to_string()}
                                    on_message_send={Callback::noop()} />
                                <OfferAndCandidates offer_or_answer={"3XXXXXX".to_string()} />
                                <ValidateOfferOrAnswer on_validate={Callback::noop()} />
                            </div>
                        </main>
                    }
                }
                State::Server(cs) => {
                    html! {
                        <div>
                            <ChatHeader
                            on_disconnect={on_disconnect}
                            state={State::Server(cs)} />
                        </div>
                    }
                }
                State::Client(cs) => {
                    html! {
                        <div>
                            <ChatHeader
                            on_disconnect={on_disconnect}
                            state={State::Client(cs)} />
                        </div>
                    }
                }
            }
        }
    }
}

#[derive(Properties, PartialEq)]
pub struct MessageInputProps {
    on_message_send: Callback<String>,
    input_text: String,
}

// MessageInput Component
#[function_component(MessageInput)]
fn message_input(props: &MessageInputProps) -> Html {
    let text = use_state(|| props.input_text.clone());

    let on_send = {
        let on_message_send = props.on_message_send.clone();
        let text = text.clone();
        Callback::from(move |_| {
            if !(*text).is_empty() {
                on_message_send.emit((*text).clone());
                text.set("".to_string());
            }
        })
    };

    let on_input = {
        let text = text.clone();
        Callback::from(move |e: InputEvent| {
            let input: HtmlInputElement = e.target_unchecked_into();
            text.set(input.value());
        })
    };

    let on_keyup = {
        let on_message_send = props.on_message_send.clone();
        let text = text.clone();
        Callback::from(move |e: KeyboardEvent| {
            if e.key_code() == 13 && !(*text).is_empty() {
                if let Some(input) = e.target_dyn_into::<web_sys::HtmlInputElement>() {
                    let value = input.value();
                    text.set(value.clone());
                    on_message_send.emit(value);
                }
            }
        })
    };

    let button_class = if !(*text).is_empty() {
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
                    placeholder="Type a message..."
                    id="chat-message-box"
                    value={(*text).clone()}
                    oninput={on_input}
                    onkeyup={on_keyup}
                />
                <button
                    class={format!("{} font-medium py-3 px-6 rounded-lg transition-colors", button_class)}
                    disabled={(*text).is_empty()}
                    onclick={on_send}
                >
                    {"Send"}
                </button>
            </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub struct MessageListProps {
    pub messages: Vec<Message>,
}

// MessageList Component
#[function_component(MessageList)]
fn message_list(props: &MessageListProps) -> Html {
    html! {
        <ul class="space-y-4 px-4">
            {
                for props.messages.iter().map(|a_message| {
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
                                    { &a_message.content }
                                </div>
                            </div>
                        </div>
                    }
                })
            }
        </ul>
    }
}

#[derive(Properties, PartialEq)]
pub struct ConnectionHeaderProps {
    pub is_connected: bool,
    pub state: State,
    pub on_disconnect: Callback<()>,
}

#[function_component(ConnectionHeader)]
fn connection_header(props: &ConnectionHeaderProps) -> Html {
    let on_click = {
        let on_disconnect = props.on_disconnect.clone();
        Callback::from(move |_| on_disconnect.emit(()))
    };

    let disconnect_button = if props.is_connected {
        html! {
            <button
                class="bg-red-500 hover:bg-red-600 text-white px-4 py-2 rounded-lg transition-colors"
                onclick={on_click}
            >
                {"Disconnect"}
            </button>
        }
    } else {
        html! {}
    };

    html! {
        <header class="bg-gray-800 text-white p-4 shadow-md">
            <div class="container mx-auto flex justify-between items-center">
                <div class="text-sm font-mono">
                    { format!("{:?}", props.state) }
                </div>
                { disconnect_button }
            </div>
        </header>
    }
}

#[derive(Properties, PartialEq, Clone)]
pub struct OfferAndCandidatesProps {
    pub offer_or_answer: String,
}

#[function_component(OfferAndCandidates)]
pub fn get_offer_and_candidates(props: &OfferAndCandidatesProps) -> Html {
    let offer_or_answer = props.offer_or_answer.clone();
    let offer_or_answer_for_click = offer_or_answer.clone();

    let onclick = Callback::from(move |_| {
        copy_content_to_clipboard(offer_or_answer_for_click.as_str());
    });

    html! {
        <div class="space-y-4">
            <div class="text-lg font-medium text-gray-700">
                { "Share this connection code:" }
            </div>
            <div class="bg-gray-50 p-4 rounded-lg border border-gray-200">
            <div class="break-all text-sm font-mono mb-3" id="copy-elem">{offer_or_answer}</div>
            <button
                class="bg-gray-600 hover:bg-gray-700 text-white font-medium py-2 px-4 rounded-lg transition-colors flex items-center gap-2"
                onclick={onclick}
            >
                {"Copy to clipboard"}
            </button>
        </div>
    </div>

    }
}

#[derive(Properties, PartialEq, Clone)]
pub struct ValidateOfferOrAnswerProps {
    on_validate: Callback<String>,
}

#[function_component(ValidateOfferOrAnswer)]
fn get_validate_offer_or_answer(props: &ValidateOfferOrAnswerProps) -> Html {
    let text_input = use_state(|| "".to_string());

    let on_input = {
        let text_input = text_input.clone();
        Callback::from(move |e: InputEvent| {
            let input: HtmlInputElement = e.target_unchecked_into();
            text_input.set(input.value());
        })
    };

    let onclick = {
        let text_input = text_input.clone();
        let on_validate = props.on_validate.clone();
        Callback::from(move |_| {
            if !(*text_input).is_empty() {
                on_validate.emit((*text_input).clone());
            }
        })
    };

    html! {
        <div class="space-y-3">
            <textarea
                class="w-full p-3 border border-gray-300 rounded-lg shadow-sm focus:ring-2 focus:ring-blue-500 focus:border-transparent resize-none min-h-[100px]"
                value={(*text_input).clone()}
                oninput={on_input}
                placeholder="Paste the connection code here"
            >
            </textarea>
            <button
                class="w-full bg-blue-500 hover:bg-blue-600 text-white font-medium py-2 px-4 rounded-lg transition-colors"
                onclick={onclick}
            >
                {"Connect"}
            </button>
        </div>
    }
}

#[derive(Properties, PartialEq, Clone)]
pub struct ChatHeaderProps {
    on_disconnect: Callback<()>,
    state: State,
}

#[function_component(ChatHeader)]
fn get_chat_header(props: &ChatHeaderProps) -> Html {
    let is_disconnect_button_visible = props.state != State::Default;

    let onclick = {
        let on_disconnect = props.on_disconnect.clone();
        Callback::from(move |_| {
            on_disconnect.emit(());
        })
    };

    html! {
        <header class="bg-gray-800 text-white p-4 shadow-md">
            <div class="container mx-auto flex justify-between items-center">
                <div class="text-sm font-mono">
                    { format!("{:?}", props.state) }
                </div>
                {
                    if is_disconnect_button_visible {
                        html! {
                            <button
                                class="bg-red-500 hover:bg-red-600 text-white px-4 py-2 rounded-lg transition-colors"
                                onclick={onclick}>
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
