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

#[cfg(target_os = "macos")]
pub mod macos;

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

/// Whether we can portably answer "is our own Scratchpad window focused?" - the
/// one piece of frontmost() that needs no OS-specific window API. Shared by the
/// macOS and stub backends.
#[cfg(not(windows))]
fn scratchpad_is_focused(app: &AppHandle) -> bool {
    use tauri::Manager;
    app.get_webview_window(crate::events::SCRATCHPAD)
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

/// macOS backend (Phase 1): the handle is the frontmost app's pid. Focused-window
/// title / bundle-id context resolution lands in Phase 2, so `ctx` is unknown for
/// now (Flow Styles + history attribution fall back to their defaults).
#[cfg(target_os = "macos")]
pub fn frontmost(app: &AppHandle) -> Frontmost {
    let is_scratchpad = scratchpad_is_focused(app);
    let handle = macos::focus::frontmost_pid().unwrap_or(0) as isize;
    Frontmost {
        handle,
        ctx: AppContext::unknown(),
        is_scratchpad,
    }
}

/// Fallback backend for platforms without a native focus capture yet (Linux;
/// lands in Phases 3-4). Reports an unknown app (handle 0, no context) but still
/// answers Scratchpad routing portably.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn frontmost(app: &AppHandle) -> Frontmost {
    Frontmost {
        handle: 0,
        ctx: AppContext::unknown(),
        is_scratchpad: scratchpad_is_focused(app),
    }
}

/// Whether the current Linux session is Wayland (vs X11). Always `false` off
/// Linux. Surfaced to the frontend via `get_platform_info` so the UI can hide
/// features the Wayland portal can't express (later phases).
pub fn is_wayland() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("WAYLAND_DISPLAY").is_some()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}
