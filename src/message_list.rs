use yew::{function_component, html, Html, Properties};

use crate::chat::chat_model::{Message, MessageSender};

#[derive(Properties, PartialEq)]
pub struct Props {
    pub messages: Vec<Message>,
}

#[function_component(MessageList)]
pub fn get_messages_as_html(props: &Props) -> Html {
    html! {
        <ul class="space-y-4 px-4">
            {
                for props.messages.iter().map(|m| {
                    let (message_class, align_class) = if m.sender == MessageSender::Other {
                        ("bg-gray-100 text-gray-800", "self-start")
                    } else {
                        ("bg-blue-500 text-white", "self-end")
                    };
                    html! {
                        <div class={format!("flex flex-col {}", align_class)}>
                            <div class={format!("max-w-[70%] break-words rounded-lg px-4 py-2 shadow-sm {}", message_class)}>
                                <div class="text-xs opacity-75 mb-1">
                                    { if m.sender == MessageSender::Other { "Friend" } else { "Me" } }
                                </div>
                                <div>
                                    { m.content.clone() }
                                </div>
                            </div>
                        </div>
                    }
                })
            }
        </ul>
    }
}
