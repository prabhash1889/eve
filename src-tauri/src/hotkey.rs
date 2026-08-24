//! Push-to-talk handlers. Wired from the global-shortcut handler in `lib.rs`
//! (and, for bare-modifier/mouse triggers, from `hooks`): trigger down/up flows
//! through `on_main_pressed`/`on_main_released`, which apply the activation
//! mode (hold / toggle / hybrid) before delegating to the start/stop
//! primitives `on_press`/`on_release`. Esc → cancel.

use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::state::AppState;
use crate::{events, injection, llm, pipeline, window_mgmt};

/// Parity A1: in hybrid mode, a press shorter than this is a "tap" that arms a
/// hands-free toggle; holding past it behaves like push-to-talk.
const HOLD_THRESHOLD: Duration = Duration::from_millis(300);

/// Trigger went down (key, bare modifier, or mouse button). Applies the
/// activation mode: when idle, always starts recording; while recording, a
/// *new* press (not an OS key-repeat) stops it in toggle/hybrid mode.
pub fn on_main_pressed(app: &AppHandle, st: &AppState) {
    // Key-repeat fires `Pressed` continuously while the trigger is held; the
    // physical-down latch drops everything but the fresh press. This must run
    // first: after a toggle/hybrid stop-press the app is idle again, and a
    // repeat arriving once the pipeline finishes would otherwise start an
    // unintended new recording.
    if st.trigger_down.swap(true, Ordering::SeqCst) {
        return;
    }
    if st.is_recording.load(Ordering::SeqCst) {
        // A fresh press of a *different* trigger while the starting one is
        // still held (no release observed yet) must not stop the recording.
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
    st.trigger_down.store(false, Ordering::SeqCst);
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

/// Remember the app that had focus so we can paste back into it, resolve its
/// context (process/title/category) for per-app Flow Styles + history, apply the
/// Phase 10 privacy-pause gate, and set the Scratchpad routing flag. Returns
/// `false` when the focused app is privacy-paused: recording is suppressed
/// (`is_recording` reset), the Flow Bar flashes the paused hint, and the caller
/// must bail. Platform-neutral - the OS-specific foreground capture lives behind
/// [`crate::platform::frontmost`].
pub fn capture_focus_and_gate(app: &AppHandle, st: &AppState) -> bool {
    // Reset the Scratchpad routing flag each press; set below if our own
    // Scratchpad window had focus (Phase 9 focus-aware dictation).
    st.to_scratchpad.store(false, Ordering::SeqCst);

    let front = crate::platform::frontmost(app);
    st.foreground_hwnd.store(front.handle, Ordering::SeqCst);

    // Phase 10 auto-pause: if the focused app is on the privacy pause list,
    // suppress recording entirely and flash a hint on the Flow Bar.
    let (paused_apps, context_awareness) = {
        let s = st.settings.lock();
        (s.paused_apps.clone(), s.context_awareness)
    };
    let proc = front.ctx.process.to_ascii_lowercase();
    if !proc.is_empty()
        && paused_apps
            .iter()
            .any(|p| p.trim().to_ascii_lowercase() == proc)
    {
        st.is_recording.store(false, Ordering::SeqCst);
        window_mgmt::show_flowbar(app);
        let _ = app.emit_to(events::FLOWBAR, events::PAUSED, ());
        window_mgmt::hide_flowbar_after(app.clone(), 1400);
        return false;
    }

    // Phase 10 privacy: only store the resolved title/category when context
    // awareness is on; otherwise fall back to an unknown context so history and
    // Flow Styles see nothing app-specific.
    *st.current_context.lock() = Some(if context_awareness {
        front.ctx
    } else {
        crate::context::active_window::AppContext::unknown()
    });
    if front.is_scratchpad {
        st.to_scratchpad.store(true, Ordering::SeqCst);
    }
    true
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

    // 1.P3: open connections to the transcription + polish hosts while the user
    // is still talking so release-time requests skip the TCP+TLS handshake.
    // Fire-and-forget (spawned, errors discarded) - can never delay or fail
    // this callback.
    llm::prewarm_connection(&st.settings.lock().clone());

    crate::sound::play_start_sound(&st.settings.lock());

    // Capture the paste target + its context, apply the privacy-pause gate, and
    // flag Scratchpad routing. Bails (recording already reset, paused hint shown)
    // when the focused app is on the privacy pause list.
    if !capture_focus_and_gate(app, st) {
        return;
    }

    // Allow Esc to cancel while recording (registered off the callback thread).
    register_escape(app, st);

    // Tell the (event-only) Flow Bar how to size/fade itself for this session.
    let (bubble_scale, bubble_opacity, toggle_hint) = {
        let s = st.settings.lock();
        (
            s.bubble_scale,
            s.bubble_opacity,
            s.activation_mode != "hold",
        )
    };

    // Start the microphone before touching the Flow Bar: positioning it can
    // fall back to UIA COM (tens of ms on this callback thread), which would
    // delay capture start long enough to clip first syllables.
    let (device_name, live_noise_gate) = {
        let s = st.settings.lock();
        (s.input_device.clone(), s.live_noise_gate)
    };
    st.capture.start(
        app.clone(),
        st.is_recording.clone(),
        st.audio_buffer.clone(),
        st.sample_rate.clone(),
        st.current_amplitude.clone(),
        device_name,
        live_noise_gate,
    );

    // Tell the (event-only) Flow Bar how to size/fade itself for this session.
    // Show it at the default position right away, then let a spawned lookup
    // move it next to the caret once the result arrives.
    window_mgmt::show_flowbar_default(app);
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
    window_mgmt::position_flowbar_near_caret_async(app.clone());

    // 4.5: hands-free auto-stop. In toggle/hybrid modes with the feature on, a
    // watcher ends the recording after the configured seconds of silence - the
    // user never has to press the trigger again to stop.
    let mode = st.settings.lock().activation_mode.clone();
    if (mode == "toggle" || mode == "hybrid") && st.settings.lock().auto_stop_silence_secs > 0 {
        spawn_silence_watcher(app.clone(), st);
    }
}

/// Peak-amplitude level below which a capture tick counts as silence for the
/// auto-stop watcher. Deliberately low: it should only trip on true quiet, not
/// on soft speech or ordinary room noise.
const SILENCE_PEAK_THRESHOLD: f32 = 0.02;

/// How often the watcher samples the amplitude (and how quickly it reacts once
/// the silence budget is spent).
const SILENCE_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Minimum recording age before auto-stop may fire, so an immediate pause
/// before speaking doesn't kill the session before it starts.
const SILENCE_MIN_RECORD_MS: u64 = 1_000;

fn spawn_silence_watcher(app: AppHandle, st: &AppState) {
    let is_recording = st.is_recording.clone();
    let amp = st.current_amplitude.clone();
    let started = std::time::Instant::now();
    tauri::async_runtime::spawn_blocking(move || {
        let mut silent_since: Option<Instant> = None;
        while is_recording.load(Ordering::SeqCst) {
            thread::sleep(SILENCE_POLL_INTERVAL);
            if !is_recording.load(Ordering::SeqCst) {
                return;
            }
            // Re-read live so a settings change applies without a restart; 0
            // disables mid-session and retires the watcher.
            let limit_secs = app
                .state::<AppState>()
                .settings
                .lock()
                .auto_stop_silence_secs;
            if limit_secs == 0 {
                return;
            }
            if started.elapsed() < Duration::from_millis(SILENCE_MIN_RECORD_MS) {
                continue;
            }
            let level = *amp.lock();
            if level >= SILENCE_PEAK_THRESHOLD {
                silent_since = None;
            } else {
                let since = *silent_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_secs(limit_secs.max(1) as u64) {
                    let state = app.state::<AppState>();
                    on_release(&app, &state);
                    return;
                }
            }
        }
    });
}

pub fn on_release(app: &AppHandle, st: &AppState) {
    // Only act if we were actually recording (ignore stray release events).
    if !st.is_recording.swap(false, Ordering::SeqCst) {
        return;
    }
    // Mark the pipeline in-flight; `process` clears it via a drop guard on every
    // exit path (success, error, or early return).
    st.is_processing.store(true, Ordering::SeqCst);
    st.capture.stop();
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
    st.capture.stop();
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

/// Undo-last-injection shortcut (4.2): re-focus the last paste target and send
/// one Backspace per injected character. No-op while a capture/pipeline is in
/// flight (which covers the "never while INJECTING" rule - the injection runs
/// inside the guarded pipeline), or when nothing has been injected yet.
pub fn on_undo(app: &AppHandle, st: &AppState) {
    if st.is_recording.load(Ordering::SeqCst) || st.is_processing.load(Ordering::SeqCst) {
        return;
    }
    let Some(last) = injection::take_last_injection() else {
        return;
    };
    if last.chars == 0 {
        return;
    }
    window_mgmt::show_flowbar(app);
    let _ = app.emit_to(
        events::FLOWBAR,
        events::STAGE,
        events::StagePayload {
            label: "Undoing".into(),
        },
    );
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || match injection::undo_last_injection(last) {
        Ok(()) => {
            let _ = handle.emit_to(
                events::FLOWBAR,
                events::DONE,
                events::DonePayload { text: String::new() },
            );
            window_mgmt::hide_flowbar_after(handle, 900);
        }
        Err(_) => {
            window_mgmt::fail(&handle, "Couldn't undo - target window is gone");
        }
    });
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
