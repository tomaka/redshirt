// Copyright(c) 2019 Pierre Krieger

#[link(wasm_import_module = "")]
extern "C" {
    pub(crate) fn register_interface() -> i32;
}
