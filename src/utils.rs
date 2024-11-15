use wasm_bindgen::JsCast;

pub fn copy_content_to_clipboard(content: &str) {
    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();
    let aux = document.create_element("input").unwrap();
    let aux = aux.dyn_into::<web_sys::HtmlInputElement>().unwrap();

    let _result = aux.set_attribute("value", content);
    let document = window.document().unwrap();
    let _result = document.body().unwrap().append_child(&aux);
    aux.select();
    let html_document = document.dyn_into::<web_sys::HtmlDocument>().unwrap();
    let _result = html_document.exec_command("copy");
    let document = window.document().unwrap();
    let _result = document.body().unwrap().remove_child(&aux);
}
