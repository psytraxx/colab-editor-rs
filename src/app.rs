use log::info;
use yew::prelude::*;
use yew::{html, Html};

#[derive(Properties, PartialEq)]
pub struct EditorProps {
    pub content: String,
    pub onchange: Callback<InputEvent>,
    pub onfocus: Callback<FocusEvent>,
}

#[function_component]
fn Editor(props: &EditorProps) -> Html {
    let EditorProps {
        content,
        onfocus,
        onchange,
    } = props;
    let content = AttrValue::from(content.clone());
    html! {
        <div class="bg-white shadow-md rounded px-8 pt-6 pb-8 mb-4">
            <label for="message" class="block text-sm font-medium text-gray-700 mb-2">{"Content"}</label>
            <textarea style={"caret-color: red;"} oninput={onchange} onfocus={onfocus}  id="message" type="text" name="message" rows="4" class="w-full px-3 py-2 border rounded-md focus:outline-none focus:ring focus:border-blue-300" placeholder={content}></textarea>
        </div>
    }
}

#[function_component]
pub fn App() -> Html {
    let message = use_state(String::new);

    let onfocus: Callback<FocusEvent> = Callback::from(|e: FocusEvent| (println!("E {:?}", e)));

    let onchange = {
        let message = message.clone();
        move |e: InputEvent| {
            let input = e.data().unwrap();

            let result = [message.as_str(), &input].join("");
            message.set(result);
            info!("message {}", *message);
        }
    };

    html! {
    <>
    <div>{&*message}</div>
    <Editor content={&*message} onchange={onchange} onfocus={onfocus} />
    </>
    }
}
