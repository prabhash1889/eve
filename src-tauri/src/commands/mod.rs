//! Tauri commands invoked from the frontend (Hub settings UI), grouped into
//! topical submodules. Every command function is re-exported here so the
//! `generate_handler![commands::x]` list in `lib.rs` keeps its flat paths.

mod dictionary;
mod files;
mod history;
mod models;
mod scratchpad;
mod settings;
mod shortcuts;
mod snippets;
mod styles;
mod transforms;
mod update;

// Glob re-exports so each command's hidden `#[tauri::command]` companion
// macros travel with the function itself, keeping the flat
// `generate_handler![commands::x]` paths in `lib.rs` valid.
pub use dictionary::*;
pub use files::*;
pub use history::*;
pub use models::*;
pub use scratchpad::*;
pub use settings::*;
pub use shortcuts::*;
pub use snippets::*;
pub use styles::*;
pub use transforms::*;
pub use update::*;
