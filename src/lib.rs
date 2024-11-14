#![recursion_limit = "128000"]
mod chat_component;
mod chat_service;
use chat_component::ChatComponent;
use chat_service::WebRTCChatService;
use wasm_bindgen::prelude::*;
use yew::{function_component, html, Html};

mod chat;

#[function_component(App)]
fn app() -> Html {
    let joke_service = WebRTCChatService::new("stun:stun.l.google.com:19302");

    html! {
        <ChatComponent<WebRTCChatService> service={joke_service} />
    }
}

// Called when the wasm module is instantiated
#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    yew::Renderer::<App>::new().render();
    Ok(())
}
