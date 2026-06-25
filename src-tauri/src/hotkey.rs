//! Push-to-talk handlers. Wired from the global-shortcut handler in `lib.rs`:
//! key down → start capture, key up → run the pipeline, Esc → cancel.

use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::state::AppState;
use crate::{audio, events, pipeline, window_mgmt};

#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

pub fn on_press(app: &AppHandle, st: &AppState) {
    // Ignore the key-repeat that Windows fires while the key is held.
    if st.is_recording.swap(true, Ordering::SeqCst) {
        return;
    }

    // Remember the app that had focus so we can paste back into it.
    #[cfg(windows)]
    unsafe {
        let hwnd = GetForegroundWindow();
        st.foreground_hwnd.store(hwnd.0 as isize, Ordering::SeqCst);
    }

    // Tell the (event-only) Flow Bar how to size/fade itself for this session.
    let (bubble_scale, bubble_opacity) = {
        let s = st.settings.lock();
        (s.bubble_scale, s.bubble_opacity)
    };
    window_mgmt::show_flowbar(app);
    let _ = app.emit_to(
        events::FLOWBAR,
        events::START,
        events::StartPayload {
            bubble_scale,
            bubble_opacity,
        },
    );

    // Allow Esc to cancel while recording (registered off the callback thread).
    register_escape(app, st);

    audio::start_capture(
        app.clone(),
        st.is_recording.clone(),
        st.audio_buffer.clone(),
        st.sample_rate.clone(),
        st.current_amplitude.clone(),
    );
}

pub fn on_release(app: &AppHandle, st: &AppState) {
    // Only act if we were actually recording (ignore stray release events).
    if !st.is_recording.swap(false, Ordering::SeqCst) {
        return;
    }
    unregister_escape(app, st);
    let _ = app.emit_to(events::FLOWBAR, events::PROCESSING, ());

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        pipeline::process(handle).await;
    });
}

pub fn on_cancel(app: &AppHandle, st: &AppState) {
    if !st.is_recording.swap(false, Ordering::SeqCst) {
        return;
    }
    unregister_escape(app, st);
    st.audio_buffer.lock().clear();
    let _ = app.emit_to(events::FLOWBAR, events::CANCEL, ());
    window_mgmt::hide_flowbar_after(app.clone(), 400);
}

/// Copy-last-transcript shortcut: put the most recent transcript on the
/// clipboard and flash a confirmation on the Flow Bar. No-op while recording or
/// when there's nothing to copy.
pub fn on_copy(app: &AppHandle, st: &AppState) {
    if st.is_recording.load(Ordering::SeqCst) {
        return;
    }
    let text = st.last_transcript.lock().clone();
    let Some(text) = text.filter(|t| !t.is_empty()) else {
        return;
    };
    if app.clipboard().write_text(text).is_err() {
        return;
    }
    window_mgmt::show_flowbar(app);
    let _ = app.emit_to(events::FLOWBAR, events::COPIED, ());
    window_mgmt::hide_flowbar_after(app.clone(), 1200);
}

fn register_escape(app: &AppHandle, st: &AppState) {
    let handle = app.clone();
    let esc = st.escape_shortcut.clone();
    tauri::async_runtime::spawn(async move {
        let _ = handle.global_shortcut().register(esc);
    });
}

fn unregister_escape(app: &AppHandle, st: &AppState) {
    let handle = app.clone();
    let esc = st.escape_shortcut.clone();
    tauri::async_runtime::spawn(async move {
        let _ = handle.global_shortcut().unregister(esc);
    });
}
