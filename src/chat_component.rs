use yew::{callback, function_component, html, use_state, Callback, Html, NodeRef, Properties};

use crate::{
    chat::{
        chat_model::{Message, MessageSender},
        web_rtc_manager::connection_state::{self, ConnectionState, State},
    },
    chat_service::ChatService,
    message_list::MessageList,
};

#[derive(Properties, PartialEq)]
pub struct Props<T>
where
    T: PartialEq + ChatService + Clone + 'static,
{
    pub service: T,
}

#[function_component(ChatComponent)]
pub fn chat_component<T>(props: &Props<T>) -> Html
where
    T: PartialEq + ChatService + Clone + 'static,
{
    let messages: yew::UseStateHandle<Vec<Message>> = use_state(Vec::new);

    let text_input = use_state(String::new);
    let node_ref = NodeRef::default();
    let state = use_state(|| State::Default);
    let is_loading = use_state(|| false);

    let state = props.service.get_state();

    let on_start_server = {
        let service = props.service.clone();
        let is_loading = is_loading.clone();
        Callback::from(move |_| {
            is_loading.set(true);
        })
    };

    let on_connect_server = {
        let is_loading = is_loading.clone();
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

    html! {

        match state {
            Ok(State::Default) => {
                html! {
                    <main class="flex flex-row justify-center items-center h-screen" ref={node_ref}>
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
                        </div>
                    </main>
                }
            }
            Ok(State::Server(connection_state)) => {
                html! {
                    <div>
                        <p>{format!("Server state {:?}",connection_state)}</p>
                    </div>
                }
            }
            Ok(State::Client(connection_state)) => {
                html! {
                    <div>
                        <p>{"Client state"}</p>
                    </div>
                }
            }
            Err(_) => {
                html! {
                    <div>
                        <p>{"Failed to get state"}</p>
                    </div>
                }
            }
        }


    }
}
