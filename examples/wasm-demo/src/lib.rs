use wasm_bindgen::prelude::*;

pub mod messages;

#[wasm_bindgen]
pub fn start_main() {
    web_sys::console::log_1(&"ros-z wasm demo loaded".into());
}
