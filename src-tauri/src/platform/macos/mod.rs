//! macOS platform backend.
//!
//! Thin siblings to the Windows OS-integration code, wired in through
//! `platform::frontmost` and `injection.rs` behind `#[cfg(target_os = "macos")]`
//! seams. The `isize` focus handle carries a frontmost-app **pid** on macOS
//! (Windows carries an HWND); see `focus.rs`.
//!
//! - Phase 1: `focus` (frontmost-pid capture / activate-by-pid restore) and
//!   `keys` (Cmd+V / Cmd+C via enigo).
//! - Phase 2: `input` (CGEventTap bare-modifier + mouse triggers - the analogue
//!   of the Windows `hooks.rs`), `permissions` (Accessibility trust for the
//!   tap), and `context` (bundle-id + AX focused-window-title resolution).

pub mod context;
pub mod focus;
pub mod input;
pub mod keys;
pub mod permissions;
