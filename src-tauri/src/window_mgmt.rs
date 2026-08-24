//! Show / hide / position the floating Flow Bar window, plus a shared failure
//! helper that surfaces an error in the bar and then dismisses it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, Monitor, PhysicalPosition, WebviewWindow};

use crate::events;
use crate::state::AppState;

pub fn show_flowbar(app: &AppHandle) {
    position_flowbar(app);
    if let Some(w) = app.get_webview_window(events::FLOWBAR) {
        let _ = w.show();
    }
}

/// Hot-path variant used by `hotkey::on_press`: shows the Flow Bar at its
/// default position *without* the caret lookup, whose UIA COM fallback can
/// cost tens of milliseconds on the shortcut-callback thread. The bar is
/// moved next to the caret afterwards by
/// [`position_flowbar_near_caret_async`].
pub fn show_flowbar_default(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(events::FLOWBAR) {
        let default_monitor = w.primary_monitor().ok().flatten();
        position_at_default(&w, default_monitor);
        let _ = w.show();
    }
}

/// Bumped once per press so a stale caret lookup from a previous session
/// (fast re-trigger while an old spawn_blocking lookup is still pending) can
/// never reposition the newer session's bar.
static CARET_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Resolve the caret position off the shortcut-callback thread (blocking
/// Win32/UIA COM) and move the Flow Bar next to it. Applies the result only
/// while the same recording session is still active; otherwise the bar has
/// already been hidden or belongs to a newer session and must not be moved.
pub fn position_flowbar_near_caret_async(app: AppHandle) {
    // Cheap settings check first: skip the COM/UIA lookup entirely when the
    // bar is not anchored to the caret.
    let bar_position = app
        .state::<AppState>()
        .settings
        .lock()
        .bar_position
        .clone();
    if bar_position != "near_caret" {
        return;
    }
    let generation = CARET_GENERATION.fetch_add(1, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || {
        let Some((cx, cy)) = get_caret_position() else {
            return;
        };
        let Some(w) = app.get_webview_window(events::FLOWBAR) else {
            return;
        };
        if !app.state::<AppState>().is_recording.load(Ordering::SeqCst)
            || CARET_GENERATION.load(Ordering::SeqCst) != generation
        {
            return;
        }
        let default_monitor = w.primary_monitor().ok().flatten();
        position_near_caret(&w, cx, cy, default_monitor);
    });
}

/// Position the Flow Bar. If settings dictate, anchors it near the caret;
/// otherwise, centers it near the bottom of the primary monitor.
pub fn position_flowbar(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(events::FLOWBAR) {
        let (bar_position, default_monitor) = {
            let st = app.state::<AppState>();
            let s = st.settings.lock();
            (s.bar_position.clone(), w.primary_monitor().ok().flatten())
        };

        if bar_position == "near_caret"
            && get_caret_position()
                .is_some_and(|(cx, cy)| position_near_caret(&w, cx, cy, default_monitor.clone()))
        {
            return;
        }

        position_at_default(&w, default_monitor);
    }
}

/// Anchor the bar just below the screen-space caret (`cx`, `cy`), clamped to
/// the work area of the monitor containing that point (or `default_monitor`
/// when the point matches no monitor). Returns whether the bar was moved.
fn position_near_caret(
    w: &WebviewWindow,
    cx: i32,
    cy: i32,
    default_monitor: Option<Monitor>,
) -> bool {
    // Find the monitor that contains the caret
    let mut target_monitor = None;
    if let Ok(monitors) = w.available_monitors() {
        for m in monitors {
            let pos = m.position();
            let size = m.size();
            if cx >= pos.x && cx < pos.x + size.width as i32
                && cy >= pos.y && cy < pos.y + size.height as i32 {
                target_monitor = Some(m);
                break;
            }
        }
    }

    let monitor = target_monitor.or(default_monitor);
    if let Some(m) = monitor {
        if let Ok(ws) = w.outer_size() {
            let work_pos = m.work_area().position;
            let work_size = m.work_area().size;

            let x = cx - (ws.width as i32 / 2);
            let y = cy + 24; // 24px below the caret

            let min_x = work_pos.x;
            let max_x = work_pos.x + work_size.width as i32 - ws.width as i32;
            let min_y = work_pos.y;
            let max_y = work_pos.y + work_size.height as i32 - ws.height as i32;

            let x = x.clamp(min_x, max_x);
            let y = y.clamp(min_y, max_y);

            let _ = w.set_position(PhysicalPosition::new(x, y));
            return true;
        }
    }
    false
}

/// Fallback / fixed position at the bottom-center of the primary monitor.
fn position_at_default(w: &WebviewWindow, default_monitor: Option<Monitor>) {
    if let Some(monitor) = default_monitor {
        let ms = monitor.size();
        if let Ok(ws) = w.outer_size() {
            let x = (ms.width as i32 - ws.width as i32) / 2;
            let y = ms.height as i32 - ws.height as i32 - 96;
            let _ = w.set_position(PhysicalPosition::new(x.max(0), y.max(0)));
        }
    }
}

#[cfg(windows)]
fn get_caret_position() -> Option<(i32, i32)> {
    unsafe {
        // 1. Try classic Win32 GetGUIThreadInfo
        if let Some(pos) = get_caret_from_gui_thread_info() {
            return Some(pos);
        }
        // 2. Try modern UI Automation
        if let Some(pos) = get_caret_from_uia() {
            return Some(pos);
        }
    }
    None
}

#[cfg(not(windows))]
fn get_caret_position() -> Option<(i32, i32)> {
    None
}

#[cfg(windows)]
unsafe fn get_caret_from_gui_thread_info() -> Option<(i32, i32)> {
    use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GUITHREADINFO};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::Foundation::POINT;

    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };

    if GetGUIThreadInfo(0, &mut info).is_ok() {
        // `rcCaret` is relative to `hwndCaret`'s client area (which can be a
        // child control of `hwndFocus`); a null `hwndCaret` means the thread
        // has no caret at all. A caret at client x=0 or y=0 is legitimate, so
        // only a degenerate (height-less) rect is rejected.
        if !info.hwndCaret.is_invalid() && info.rcCaret.bottom > info.rcCaret.top {
            let mut pt = POINT {
                x: info.rcCaret.left,
                y: info.rcCaret.top,
            };
            if ClientToScreen(info.hwndCaret, &mut pt).as_bool() {
                return Some((pt.x, pt.y));
            }
        }
    }
    None
}

#[cfg(windows)]
unsafe fn get_caret_from_uia() -> Option<(i32, i32)> {
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern2,
        UIA_TextPattern2Id,
    };
    use windows::Win32::System::Com::{
        CoInitializeEx, CoCreateInstance, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::Ole::{
        SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
        SafeArrayUnaccessData,
    };
    use windows::core::Interface;

    // Best effort COM initialization
    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

    let automation: IUIAutomation = CoCreateInstance(
        &CUIAutomation,
        None,
        CLSCTX_INPROC_SERVER,
    ).ok()?;

    let focused: IUIAutomationElement = automation.GetFocusedElement().ok()?;

    let pattern_ptr = focused.GetCurrentPattern(UIA_TextPattern2Id).ok()?;
    let text_pattern2: IUIAutomationTextPattern2 = pattern_ptr.cast().ok()?;

    let mut is_active = windows::Win32::Foundation::BOOL::default();
    let range = text_pattern2.GetCaretRange(&mut is_active).ok()?;

    let rects = range.GetBoundingRectangles().ok()?;
    if rects.is_null() {
        return None;
    }

    // The array holds [left, top, width, height] per rectangle. A degenerate
    // caret range can legitimately return an *empty* (non-null) array, so
    // bound-check before dereferencing. The array is owned by us: destroy it
    // on every path or it leaks each time the bar is shown.
    let mut pos = None;
    let lbound = SafeArrayGetLBound(rects, 1).unwrap_or(0);
    let ubound = SafeArrayGetUBound(rects, 1).unwrap_or(-1);
    if ubound - lbound + 1 >= 4 {
        let mut data_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        if SafeArrayAccessData(rects, &mut data_ptr).is_ok() {
            let f64_ptr = data_ptr as *const f64;
            pos = Some((*f64_ptr as i32, (*f64_ptr.add(1)) as i32));
            let _ = SafeArrayUnaccessData(rects);
        }
    }
    let _ = SafeArrayDestroy(rects);

    pos
}

/// Phase 9: show (and focus) the floating Scratchpad window. Created hidden in
/// `tauri.conf.json`; opened via the Hub button or the global shortcut.
pub fn open_scratchpad(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(events::SCRATCHPAD) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Phase 9: the Scratchpad window's top-level HWND (as isize), or `None` if the
/// window doesn't exist. Used to detect when a dictation should route into the
/// editor rather than OS-paste.
#[cfg(windows)]
pub fn scratchpad_hwnd(app: &AppHandle) -> Option<isize> {
    app.get_webview_window(events::SCRATCHPAD)
        .and_then(|w| w.hwnd().ok())
        .map(|h| h.0 as isize)
}

pub fn hide_flowbar_after(app: AppHandle, ms: u64) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(ms));
        if let Some(w) = app.get_webview_window(events::FLOWBAR) {
            let _ = w.hide();
        }
    });
}

/// Emit an error to the Flow Bar and dismiss it shortly after.
pub fn fail(app: &AppHandle, msg: &str) {
    let _ = app.emit_to(
        events::FLOWBAR,
        events::ERROR,
        events::ErrorPayload {
            message: msg.to_string(),
        },
    );
    hide_flowbar_after(app.clone(), 2600);
}
