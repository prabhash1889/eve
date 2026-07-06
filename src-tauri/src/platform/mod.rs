//! Platform seam for foreground-app capture.
//!
//! Cross-platform code asks for the [`Frontmost`] app via [`frontmost`] instead
//! of calling OS APIs inline. Windows is the only real backend today (a verbatim
//! lift of the `GetForegroundWindow` + `active_window::resolve` logic that used
//! to live in `hotkey.rs`); macOS and Linux are stubs that report "unknown" so
//! the app compiles and runs everywhere with no behavior change on Windows.
//!
//! The `isize` handle keeps its historical name (`foreground_hwnd`) throughout
//! state/pipeline; only its per-OS meaning differs: Windows = HWND, macOS =
//! frontmost app pid (later), Linux/X11 = X window id (later), Linux/Wayland =
//! always 0. See `cross-platform-plan.md`.

use tauri::AppHandle;

use crate::context::active_window::AppContext;

/// The app that had focus when a capture started: the paste-target `handle`, its
/// resolved `ctx` (process/title/category for Flow Styles + history), and whether
/// it is our own Scratchpad window (Phase 9 focus-aware routing).
pub struct Frontmost {
    pub handle: isize,
    pub ctx: AppContext,
    pub is_scratchpad: bool,
}

/// Capture the current foreground app. Windows resolves the real HWND + context;
/// other platforms return a stub (handle 0, unknown context) until their
/// backends land in later phases.
#[cfg(windows)]
pub fn frontmost(app: &AppHandle) -> Frontmost {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let hwnd = unsafe { GetForegroundWindow() };
    let handle = hwnd.0 as isize;
    let ctx = crate::context::active_window::resolve(hwnd);
    let is_scratchpad = handle != 0 && crate::window_mgmt::scratchpad_hwnd(app) == Some(handle);
    Frontmost {
        handle,
        ctx,
        is_scratchpad,
    }
}

#[cfg(not(windows))]
pub fn frontmost(app: &AppHandle) -> Frontmost {
    use tauri::Manager;

    // Foreign-window focus capture is a later-phase backend; for now report an
    // unknown app (handle 0, no context). Scratchpad routing is the one piece we
    // can answer portably: ask Tauri whether our own Scratchpad window is focused.
    let is_scratchpad = app
        .get_webview_window(crate::events::SCRATCHPAD)
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    Frontmost {
        handle: 0,
        ctx: AppContext::unknown(),
        is_scratchpad,
    }
}
