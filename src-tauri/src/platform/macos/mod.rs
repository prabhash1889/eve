//! macOS platform backend (Phase 1: core dictation).
//!
//! Thin siblings to the Windows OS-integration code, wired in through
//! `platform::frontmost` and `injection.rs` behind `#[cfg(target_os = "macos")]`
//! seams. The `isize` focus handle carries a frontmost-app **pid** on macOS
//! (Windows carries an HWND); see `focus.rs`.

pub mod focus;
pub mod keys;
