#![recursion_limit = "128000"]

use chat::{chat_model::ChatModel, web_rtc_manager::WebRTCManager};
use wasm_bindgen::prelude::*;

mod chat;

// Called when the wasm module is instantiated
#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    yew::Renderer::<ChatModel<WebRTCManager>>::new().render();
    Ok(())
}
