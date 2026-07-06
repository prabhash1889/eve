//! Push-to-talk handlers. Wired from the global-shortcut handler in `lib.rs`
//! (and, for bare-modifier/mouse triggers, from `hooks`): trigger down/up flows
//! through `on_main_pressed`/`on_main_released`, which apply the activation
//! mode (hold / toggle / hybrid) before delegating to the start/stop
//! primitives `on_press`/`on_release`. Esc → cancel.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::state::AppState;
use crate::{audio, events, pipeline, window_mgmt};

#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

/// Parity A1: in hybrid mode, a press shorter than this is a "tap" that arms a
/// hands-free toggle; holding past it behaves like push-to-talk.
const HOLD_THRESHOLD: Duration = Duration::from_millis(300);

/// Trigger went down (key, bare modifier, or mouse button). Applies the
/// activation mode: when idle, always starts recording; while recording, a
/// *new* press (not an OS key-repeat) stops it in toggle/hybrid mode.
pub fn on_main_pressed(app: &AppHandle, st: &AppState) {
    if st.is_recording.load(Ordering::SeqCst) {
        // Key-repeat fires `Pressed` continuously while the trigger is held;
        // only a press that follows a release can mean "stop".
        if !st.saw_release.load(Ordering::SeqCst) {
            return;
        }
        let mode = st.settings.lock().activation_mode.clone();
        if mode == "toggle" || mode == "hybrid" {
            on_release(app, st);
        }
        return;
    }
    st.saw_release.store(false, Ordering::SeqCst);
    *st.press_at.lock() = Some(Instant::now());
    on_press(app, st);
}

/// Trigger came back up. Hold mode stops immediately; toggle mode just records
/// that the release happened; hybrid stops only when the press was a genuine
/// hold (>= [`HOLD_THRESHOLD`]) - a quick tap leaves the recording running.
pub fn on_main_released(app: &AppHandle, st: &AppState) {
    if !st.is_recording.load(Ordering::SeqCst) {
        return;
    }
    let mode = st.settings.lock().activation_mode.clone();
    match mode.as_str() {
        "toggle" => {
            st.saw_release.store(true, Ordering::SeqCst);
        }
        "hybrid" => {
            // Only the release of the *starting* press decides tap-vs-hold;
            // later releases (of the stop-press) are handled via `on_press`.
            if !st.saw_release.swap(true, Ordering::SeqCst) {
                let held = st
                    .press_at
                    .lock()
                    .map(|t| t.elapsed())
                    .unwrap_or(Duration::ZERO);
                if held >= HOLD_THRESHOLD {
                    on_release(app, st);
                }
            }
        }
        _ => on_release(app, st),
    }
}

pub fn on_press(app: &AppHandle, st: &AppState) {
    // Refuse to start a new capture while the previous dictation is still being
    // processed (transcribe → polish → inject). Without this, a rapid
    // press-release-press could spawn two overlapping pipelines that inject out
    // of order or into the wrong window.
    if st.is_processing.load(Ordering::SeqCst) {
        return;
    }
    // Ignore the key-repeat that Windows fires while the key is held.
    if st.is_recording.swap(true, Ordering::SeqCst) {
        return;
    }

    crate::sound::play_start_sound(&st.settings.lock());

    // Remember the app that had focus so we can paste back into it, and resolve
    // its context (process/title/category) for per-app Flow Styles + history.
    // Reset the Scratchpad routing flag each press; set it below if our own
    // Scratchpad window had focus (Phase 9 focus-aware dictation).
    st.to_scratchpad.store(false, Ordering::SeqCst);
    #[cfg(windows)]
    unsafe {
        let hwnd = GetForegroundWindow();
        let fg = hwnd.0 as isize;
        st.foreground_hwnd.store(fg, Ordering::SeqCst);
        let ctx = crate::context::active_window::resolve(hwnd);

        // Phase 10 auto-pause: if the focused app is on the privacy pause list,
        // suppress recording entirely and flash a hint on the Flow Bar.
        let (paused_apps, context_awareness) = {
            let s = st.settings.lock();
            (s.paused_apps.clone(), s.context_awareness)
        };
        let proc = ctx.process.to_ascii_lowercase();
        if !proc.is_empty()
            && paused_apps
                .iter()
                .any(|p| p.trim().to_ascii_lowercase() == proc)
        {
            st.is_recording.store(false, Ordering::SeqCst);
            window_mgmt::show_flowbar(app);
            let _ = app.emit_to(events::FLOWBAR, events::PAUSED, ());
            window_mgmt::hide_flowbar_after(app.clone(), 1400);
            return;
        }

        // Phase 10 privacy: only store the resolved title/category when context
        // awareness is on; otherwise fall back to an unknown context so history
        // and Flow Styles see nothing app-specific.
        *st.current_context.lock() = Some(if context_awareness {
            ctx
        } else {
            crate::context::active_window::AppContext::unknown()
        });
        if fg != 0 && window_mgmt::scratchpad_hwnd(app) == Some(fg) {
            st.to_scratchpad.store(true, Ordering::SeqCst);
        }
    }

    // Tell the (event-only) Flow Bar how to size/fade itself for this session.
    let (bubble_scale, bubble_opacity, toggle_hint) = {
        let s = st.settings.lock();
        (
            s.bubble_scale,
            s.bubble_opacity,
            s.activation_mode != "hold",
        )
    };
    window_mgmt::show_flowbar(app);
    let _ = app.emit_to(
        events::FLOWBAR,
        events::START,
        events::StartPayload {
            bubble_scale,
            bubble_opacity,
            mode: "dictation".into(),
            toggle_hint,
        },
    );

    // Allow Esc to cancel while recording (registered off the callback thread).
    register_escape(app, st);

    let device_name = st.settings.lock().input_device.clone();
    audio::start_capture(
        app.clone(),
        st.is_recording.clone(),
        st.audio_buffer.clone(),
        st.sample_rate.clone(),
        st.current_amplitude.clone(),
        device_name,
    );
}

pub fn on_release(app: &AppHandle, st: &AppState) {
    // Only act if we were actually recording (ignore stray release events).
    if !st.is_recording.swap(false, Ordering::SeqCst) {
        return;
    }
    // Mark the pipeline in-flight; `process` clears it via a drop guard on every
    // exit path (success, error, or early return).
    st.is_processing.store(true, Ordering::SeqCst);
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
    // Reset Command Mode too — Esc cancels either capture.
    st.is_command_mode.store(false, Ordering::SeqCst);
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

pub(crate) fn register_escape(app: &AppHandle, st: &AppState) {
    let handle = app.clone();
    let esc = st.escape_shortcut;
    tauri::async_runtime::spawn(async move {
        let _ = handle.global_shortcut().register(esc);
    });
}

pub(crate) fn unregister_escape(app: &AppHandle, st: &AppState) {
    let handle = app.clone();
    let esc = st.escape_shortcut;
    tauri::async_runtime::spawn(async move {
        let _ = handle.global_shortcut().unregister(esc);
    });
}
