use wasm_bindgen::prelude::*;

pub fn set_panic_hook() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = debug)]
    pub fn console_debug(s: &str);
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    pub fn console_log(s: &str);
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    pub fn console_error(s: &str);
}


macro_rules! debug {
    ($($t:tt)*) => (utils::console_debug(&format_args!($($t)*).to_string()))
}

macro_rules! log {
    ($($t:tt)*) => (utils::console_log(&format_args!($($t)*).to_string()))
}

macro_rules! error {
    ($($t:tt)*) => (utils::console_error(&format_args!($($t)*).to_string()))
}