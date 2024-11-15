#![recursion_limit = "128000"]
mod chat_component;
mod chat_service;
mod utils;

use std::{cell::RefCell, rc::Rc};

use chat_component::ChatComponent;
use chat_service::WebRTCChatService;
use wasm_bindgen::prelude::*;
use yew::{function_component, html, Html};

mod chat;

#[function_component(App)]
fn app() -> Html {
    let chat_service = Rc::new(RefCell::new(WebRTCChatService::new(
        "stun:stun.l.google.com:19302",
    )));

    html! {
        <ChatComponent<WebRTCChatService> service={chat_service} />
    }
}

// Called when the wasm module is instantiated
#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    yew::Renderer::<App>::new().render();
    Ok(())
}
