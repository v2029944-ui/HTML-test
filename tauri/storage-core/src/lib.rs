// GUI-independent harness compiles the production storage implementation.
#[path = "../../src-tauri/src/storage.rs"]
pub mod storage;

#[cfg(test)]
#[path = "../../src-tauri/src/native_name_selection.rs"]
mod native_name_selection;
