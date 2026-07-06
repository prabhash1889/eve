//! Linux platform backend.
//!
//! Thin siblings to the Windows/macOS OS-integration code, wired in through
//! `platform::frontmost`, `injection.rs`, and the trigger init in `lib.rs`.
//! One binary serves both display servers; [`session`] picks the backend at
//! runtime:
//!
//! - **X11** (Phase 3): real focus capture/restore (EWMH), context
//!   (`_NET_WM_PID` -> `/proc`, `_NET_WM_NAME`), and bare-modifier/mouse triggers
//!   (XI2 raw keys + `GrabButton`) - all in `x11.rs`/`context.rs`.
//! - **Wayland** (Phase 4): foreign-window focus is inaccessible and the tauri
//!   global-shortcut plugin is a no-op, so the X11 paths stay dormant and the
//!   portal-based backend takes over. Until then Wayland degrades to typing the
//!   text out (see `injection.rs`).
//!
//! The `isize` focus handle carries an **X window id** on X11 (Windows carries an
//! HWND, macOS a pid); on Wayland it is always 0.

pub mod context;
pub mod keys;
pub mod x11;

use std::sync::OnceLock;

/// Which display server this session is running under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    X11,
    Wayland,
    /// Neither `WAYLAND_DISPLAY` nor `DISPLAY` is set (e.g. a headless run).
    Unknown,
}

static SESSION: OnceLock<Session> = OnceLock::new();

/// The detected display server, resolved once and cached. Wayland iff
/// `WAYLAND_DISPLAY` is set (tie-broken by `XDG_SESSION_TYPE`); else X11 iff
/// `DISPLAY` is set.
pub fn session() -> Session {
    *SESSION.get_or_init(detect_session)
}

fn detect_session() -> Session {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return Session::Wayland;
    }
    if let Ok(kind) = std::env::var("XDG_SESSION_TYPE") {
        if kind.eq_ignore_ascii_case("wayland") {
            return Session::Wayland;
        }
    }
    if std::env::var_os("DISPLAY").is_some() {
        return Session::X11;
    }
    Session::Unknown
}
