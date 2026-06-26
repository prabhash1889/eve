//! Active-window context (Phase 6). Resolves the focused app's process name,
//! window title, and a coarse `AppCategory` at record start, so the polish
//! prompt can adapt per-app and history can attribute each dictation.

pub mod active_window;

pub use active_window::AppContext;
