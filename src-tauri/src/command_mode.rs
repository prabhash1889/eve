//! Phase 7: Command Mode + Transforms.
//!
//! **Command Mode** is a second push-to-talk: hold the command shortcut, speak
//! an instruction, release. We transcribe the instruction, then read the focused
//! app's current selection (Ctrl+C). A non-empty selection → "rewrite" the
//! selection per the instruction; empty → "generate" text inline from the
//! instruction. The result is injected like a normal dictation.
//!
//! **Transforms** are saved rewrite prompts. Each active transform with a
//! shortcut gets a global accelerator (registered at launch / after edits);
//! pressing it rewrites the current selection with that prompt. Auto-apply
//! transforms run inside `pipeline::process` after polish.

use std::str::FromStr;
use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::db::transforms;
use crate::session;
use crate::state::AppState;
use crate::transcription::{local_backend_label_for, Audio};
use crate::{audio, events, hotkey, injection, llm, polish, window_mgmt};

// --- Command Mode push-to-talk ----------------------------------------------

/// Key-down: start capturing the spoken instruction, flagged as Command Mode so
/// key-up routes to `process_command`. Mirrors `hotkey::on_press` but tags the
/// Flow Bar with the "command" mode for a distinct look.
pub fn on_press(app: &AppHandle, st: &AppState) {
    // Physical-down latch: the OS auto-repeats `Pressed` for the whole hold.
    // Refuse unless this is the first press of the key, so a repeat arriving
    // once `is_processing` clears mid-hold can't start a capture on the tail of
    // the instruction. Must run first, mirroring `hotkey::on_main_pressed`.
    if st.command_down.swap(true, Ordering::SeqCst) {
        return;
    }
    // Refuse to start while a pipeline (dictation, command, or transform) is
    // still in flight. Starting a capture here would clear the shared audio
    // buffer mid-drain and corrupt the running session.
    if st.is_processing.load(Ordering::SeqCst) {
        return;
    }
    if st.is_recording.swap(true, Ordering::SeqCst) {
        return;
    }
    crate::sound::play_start_sound(&st.settings.lock());
    st.is_command_mode.store(true, Ordering::SeqCst);

    // Remember the focused app (paste target) and its context, mirroring
    // `hotkey::on_press` but without the privacy-pause gate.
    let front = crate::platform::frontmost(app);
    st.foreground_hwnd.store(front.handle, Ordering::SeqCst);
    *st.current_context.lock() = Some(front.ctx);

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
            mode: "command".into(),
            toggle_hint: false,
        },
    );

    hotkey::register_escape(app, st);

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
}

/// Key-up: stop recording and run the command pipeline.
pub fn on_release(app: &AppHandle, st: &AppState) {
    // Clear the physical-down latch first, before the recording early-return, so
    // a refused (overlapping) press still re-arms the shortcut on key-up.
    st.command_down.store(false, Ordering::SeqCst);
    if !st.is_recording.swap(false, Ordering::SeqCst) {
        return;
    }
    // Mark the pipeline in-flight; `process_command` clears it via a drop guard
    // on every exit path, mirroring `hotkey::on_release` / `pipeline::process`.
    st.is_processing.store(true, Ordering::SeqCst);
    st.is_command_mode.store(false, Ordering::SeqCst);
    st.capture.stop();
    hotkey::unregister_escape(app, st);
    let _ = app.emit_to(events::FLOWBAR, events::PROCESSING, ());

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        process_command(handle).await;
    });
}

/// Post-release Command Mode flow: transcribe the instruction → capture the
/// selection → rewrite-or-generate via the LLM → inject.
async fn process_command(app: AppHandle) {
    // Release the concurrency flag on every exit path (mirrors `pipeline::process`).
    let _processing =
        crate::pipeline::ProcessingGuard(app.state::<AppState>().is_processing.clone());

    let (buffer, sample_rate, capture, settings, transcriber, last_transcript, last_benchmark, hwnd) = {
        let st = app.state::<AppState>();
        (
            st.audio_buffer.clone(),
            st.sample_rate.clone(),
            st.capture.clone(),
            st.settings.clone(),
            st.transcriber.clone(),
            st.last_transcript.clone(),
            st.last_transcription_benchmark.clone(),
            st.foreground_hwnd.load(Ordering::SeqCst),
        )
    };
    // Snapshot for the LLM step below; never hold the guard across `.await`.
    let settings_snapshot = settings.lock().clone();

    // Deterministic stop handshake + buffer drain (mirrors `pipeline::process`,
    // shared via `session::stop_and_drain`).
    let drained = session::stop_and_drain(&capture, buffer, sample_rate).await;

    let (language, strategy, speech_is_local, vad_enabled, correctness_rescue, profile, model) = {
        let s = settings.lock();
        let lang = if s.language == "auto" {
            None
        } else {
            Some(s.language.clone())
        };
        // Resolve the speech backend once: local keeps its selected model id;
        // cloud carries the resolved model for the benchmark row.
        let (speech_is_local, model) = match crate::transcription::resolve_speech(&s) {
            crate::transcription::SpeechBackend::Local => (true, s.local_whisper_model.clone()),
            crate::transcription::SpeechBackend::Cloud(t) => (false, t.model),
        };
        (
            lang,
            s.inject_strategy.clone(),
            speech_is_local,
            s.local_vad_enabled,
            s.local_correctness_rescue,
            s.local_transcription_profile.clone(),
            model,
        )
    };

    // Build the same dual-form audio payload as dictation mode via
    // `session::prepare_audio`, with Command Mode's own ordering: local VAD
    // runs FIRST and the WAV encodes the TRIMMED clip afterwards (that post-VAD
    // WAV is what a cloud fallback uploads), and it is always encoded.
    let prepared = match session::prepare_audio(
        drained.samples,
        drained.rate,
        session::PrepareOptions {
            encode_before_vad: false,
            need_wav: true,
            vad: if speech_is_local && vad_enabled {
                Some(audio::VadParams::for_profile(&profile, correctness_rescue))
            } else {
                None
            },
            min_duration_ms: Some(session::MIN_DURATION_MS),
        },
    )
    .await
    {
        Ok(p) => p,
        Err(session::PrepareError::TooShort) => {
            let _ = app.emit_to(
                events::FLOWBAR,
                events::ERROR,
                events::ErrorPayload {
                    message: "Too short".to_string(),
                },
            );
            window_mgmt::hide_flowbar_after(app, 1200);
            return;
        }
        Err(session::PrepareError::AudioFailed) => {
            window_mgmt::fail(&app, "Audio processing failed");
            return;
        }
        Err(session::PrepareError::NoSpeech) => {
            window_mgmt::fail(&app, "No speech detected");
            return;
        }
    };
    let duration_ms = prepared.duration_ms;

    let (instruction, benchmark) = match session::run_stt_benchmarked(
        transcriber.as_ref(),
        Audio {
            samples: prepared.samples,
            wav: prepared.wav,
        },
        language,
        Vec::new(),
        session::SttMeta {
            mode: "command",
            backend: if speech_is_local {
                local_backend_label_for(&model).to_string()
            } else {
                crate::transcription::resolve_speech(&settings.lock())
                    .label()
                    .to_string()
            },
            // Preserve the historical default label when no model id resolves.
            model: if model.is_empty() {
                "whisper-large-v3-turbo".into()
            } else {
                model
            },
            profile,
            clip_duration_ms: duration_ms.max(0) as u64,
            vad_trimmed: prepared.vad_trimmed,
        },
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            window_mgmt::fail(&app, &session::errors::command_error(&e.to_string()));
            return;
        }
    };
    if instruction.trim().is_empty() {
        window_mgmt::fail(&app, "No instruction heard");
        return;
    }
    *last_benchmark.lock() = Some(benchmark);

    // Read the selection from the still-focused target app (blocking key sim).
    let app_for_sel = app.clone();
    let selection = tauri::async_runtime::spawn_blocking(move || {
        injection::capture_selection(&app_for_sel, hwnd)
    })
    .await
    .ok()
    .flatten();

    let result = match run_command(&settings_snapshot, selection.as_deref(), &instruction).await
    {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => {
            window_mgmt::fail(&app, "Command produced no text");
            return;
        }
        Err(e) => {
            window_mgmt::fail(&app, &session::errors::command_error(&e.to_string()));
            return;
        }
    };

    inject_and_finish(&app, &result, hwnd, &strategy).await;
    *last_transcript.lock() = Some(result);
}

// --- Transform shortcuts -----------------------------------------------------

/// A transform accelerator fired: rewrite the current selection with the saved
/// transform's prompt. No-op while a dictation/command capture is in flight or
/// a pipeline is still processing.
pub fn on_transform(app: &AppHandle, st: &AppState, id: i64) {
    // Physical-down latch: the OS auto-repeats `Pressed` for the whole hold.
    // Refuse unless this is the first press, so a repeat arriving after the
    // transform finishes can't re-fire it (re-capture the selection + re-inject
    // again). Must run first, mirroring `hotkey::on_main_pressed`.
    if st.transform_down.swap(true, Ordering::SeqCst) {
        return;
    }
    if st.is_recording.load(Ordering::SeqCst) {
        return;
    }
    if st.is_processing.load(Ordering::SeqCst) {
        return;
    }
    // Mark the pipeline in-flight before spawning; `run_transform_shortcut`
    // clears it via a drop guard on every exit path. Set here (not inside the
    // task) so a second accelerator press racing the spawn still sees it.
    st.is_processing.store(true, Ordering::SeqCst);

    let hwnd = crate::platform::frontmost(app).handle;

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        run_transform_shortcut(handle, id, hwnd).await;
    });
}

/// Key-up for a transform accelerator: clear its physical-down latch so the
/// next fresh press is accepted. The `Released` event is dispatched to this
/// even when `on_transform` refused the press (overlap), so the latch is always
/// re-armed on key release.
pub fn on_transform_released(st: &AppState) {
    st.transform_down.store(false, Ordering::SeqCst);
}

async fn run_transform_shortcut(app: AppHandle, id: i64, hwnd: isize) {
    // Release the concurrency flag on every exit path (mirrors `pipeline::process`).
    let _processing =
        crate::pipeline::ProcessingGuard(app.state::<AppState>().is_processing.clone());

    let (db, strategy, last_transcript, bubble, settings_snapshot) = {
        let st = app.state::<AppState>();
        let s = st.settings.lock();
        (
            st.db.clone(),
            s.inject_strategy.clone(),
            st.last_transcript.clone(),
            (s.bubble_scale, s.bubble_opacity),
            s.clone(),
        )
    };

    let transform = {
        let conn = db.lock();
        transforms::get(&conn, id).ok().flatten()
    };
    let Some(transform) = transform else { return };

    // Show the bar in command-mode style, then the processing state.
    window_mgmt::show_flowbar(&app);
    let _ = app.emit_to(
        events::FLOWBAR,
        events::START,
        events::StartPayload {
            bubble_scale: bubble.0,
            bubble_opacity: bubble.1,
            mode: "command".into(),
            toggle_hint: false,
        },
    );
    let _ = app.emit_to(events::FLOWBAR, events::PROCESSING, ());

    let app_for_sel = app.clone();
    let selection = tauri::async_runtime::spawn_blocking(move || {
        injection::capture_selection(&app_for_sel, hwnd)
    })
    .await
    .ok()
    .flatten();

    let Some(selection) = selection.filter(|s| !s.trim().is_empty()) else {
        window_mgmt::fail(&app, "Select some text first");
        return;
    };

    let result = match run_transform(&settings_snapshot, &transform.system_prompt, &selection)
        .await
    {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => {
            window_mgmt::fail(&app, "Transform produced no text");
            return;
        }
        Err(e) => {
            window_mgmt::fail(&app, &session::errors::command_error(&e.to_string()));
            return;
        }
    };

    inject_and_finish(&app, &result, hwnd, &strategy).await;
    *last_transcript.lock() = Some(result);
}

/// Rebuild the global accelerators bound to transforms: drop the previous set,
/// then register each active transform with a parseable, non-reserved shortcut.
/// Best-effort — a bad/duplicate accelerator is skipped, not fatal.
pub fn register_transform_shortcuts(app: &AppHandle, st: &AppState) {
    // Phase 4: on Wayland the plugin can't register accelerators; the
    // GlobalShortcuts portal binds transforms alongside the reserved shortcuts,
    // so just trigger a re-bind (the transform DB rows are already committed).
    #[cfg(target_os = "linux")]
    if crate::platform::is_wayland() {
        crate::platform::linux::wayland::request_rebind();
        return;
    }

    let gs = app.global_shortcut();

    {
        let mut current = st.transform_shortcuts.lock();
        for (sc, _) in current.iter() {
            let _ = gs.unregister(*sc);
        }
        current.clear();
    }

    let mut reserved = vec![
        *st.main_shortcut.lock(),
        *st.copy_shortcut.lock(),
        *st.command_shortcut.lock(),
        *st.undo_shortcut.lock(),
        st.escape_shortcut,
    ];
    // Also exclude style accelerators so the two registries can't collide (4.4).
    reserved.extend(st.style_shortcuts.lock().iter().map(|(sc, _)| *sc));

    let rows = {
        let conn = st.db.lock();
        transforms::active_shortcuts(&conn).unwrap_or_default()
    };

    let mut current = st.transform_shortcuts.lock();
    for (id, accel) in rows {
        let Ok(sc) = Shortcut::from_str(&accel) else {
            continue;
        };
        if reserved.contains(&sc) || current.iter().any(|(existing, _)| *existing == sc) {
            continue;
        }
        if gs.register(sc).is_ok() {
            current.push((sc, id));
        }
    }
}

// --- Flow Style override accelerators (4.4) -----------------------------------

/// A style accelerator fired: arm that Flow Style for the NEXT dictation only.
/// No-op while a capture is in flight; harmless otherwise (the pending id is
/// consumed by the next pipeline run, whenever that is).
pub fn on_style_override(app: &AppHandle, st: &AppState, id: i64) {
    // Physical-down latch: drop OS key auto-repeat (mirrors `on_transform`).
    if st.style_down.swap(true, Ordering::SeqCst) {
        return;
    }
    if st.is_recording.load(Ordering::SeqCst) || st.is_processing.load(Ordering::SeqCst) {
        return;
    }
    *st.pending_style_id.lock() = Some(id);

    // Flash a confirmation on the Flow Bar with the armed style's name.
    let name = {
        let conn = st.db.lock();
        crate::db::flow_styles::get(&conn, id)
            .ok()
            .flatten()
            .map(|s| s.name)
            .unwrap_or_else(|| "style".into())
    };
    window_mgmt::show_flowbar(app);
    let _ = app.emit_to(
        events::FLOWBAR,
        events::STAGE,
        events::StagePayload {
            label: format!("Next dictation: {name}"),
        },
    );
    window_mgmt::hide_flowbar_after(app.clone(), 1400);
}

/// Key-up for a style accelerator: clear its physical-down latch so the next
/// fresh press is accepted (always re-arms, even when `on_style_override`
/// refused the press).
pub fn on_style_override_released(st: &AppState) {
    st.style_down.store(false, Ordering::SeqCst);
}

/// Rebuild the global accelerators bound to Flow Styles: drop the previous set,
/// then register each active style with a parseable shortcut not already taken
/// by a reserved shortcut or a transform. Best-effort - a bad/duplicate
/// accelerator is skipped, not fatal.
pub fn register_style_shortcuts(app: &AppHandle, st: &AppState) {
    // Wayland: the portal owns bindings; trigger a re-bind (rows committed above).
    #[cfg(target_os = "linux")]
    if crate::platform::is_wayland() {
        crate::platform::linux::wayland::request_rebind();
        return;
    }

    let gs = app.global_shortcut();

    {
        let mut current = st.style_shortcuts.lock();
        for (sc, _) in current.iter() {
            let _ = gs.unregister(*sc);
        }
        current.clear();
    }

    let mut reserved = vec![
        *st.main_shortcut.lock(),
        *st.copy_shortcut.lock(),
        *st.command_shortcut.lock(),
        *st.undo_shortcut.lock(),
        st.escape_shortcut,
    ];
    // Also exclude transform accelerators so the two registries can't collide.
    reserved.extend(st.transform_shortcuts.lock().iter().map(|(sc, _)| *sc));

    let rows = {
        let conn = st.db.lock();
        crate::db::flow_styles::active_shortcuts(&conn).unwrap_or_default()
    };

    let mut current = st.style_shortcuts.lock();
    for (id, accel) in rows {
        let Ok(sc) = Shortcut::from_str(&accel) else {
            continue;
        };
        if reserved.contains(&sc) || current.iter().any(|(existing, _)| *existing == sc) {
            continue;
        }
        if gs.register(sc).is_ok() {
            current.push((sc, id));
        }
    }
}

// --- LLM steps (also exposed as commands) ------------------------------------

/// The Command Mode LLM step: rewrite `selection` per `instruction`, or generate
/// fresh text from `instruction` when nothing is selected. The cloud target
/// (provider + model) resolves from the passed settings snapshot. Output is
/// unwrapped so stray quotes/preambles don't leak into the injected text.
pub async fn run_command(
    settings: &crate::config::Settings,
    selection: Option<&str>,
    instruction: &str,
) -> anyhow::Result<String> {
    let instruction = instruction.trim();
    let (system, user) = match selection.map(str::trim).filter(|s| !s.is_empty()) {
        Some(sel) => (
            "You are an editing assistant. Rewrite the user's selected text \
             according to their instruction. Preserve meaning and any factual \
             detail; change only what the instruction asks. Output ONLY the \
             rewritten text — no preamble, labels, quotes, or explanation."
                .to_string(),
            format!("Instruction: {instruction}\n\nSelected text:\n{sel}"),
        ),
        None => (
            "You are a writing assistant. Produce text that fulfills the user's \
             request, suitable to paste directly at their cursor. Output ONLY \
             that text — no preamble, labels, quotes, or explanation."
                .to_string(),
            instruction.to_string(),
        ),
    };
    let chat = llm::resolve_chat(settings);
    let out = llm::chat(&chat, &system, &user).await?;
    Ok(polish::strip_wrapping(&out))
}

/// Apply a saved transform's `system_prompt` to `text`. Shared by the transform
/// shortcut, the `apply_transform` command, and auto-apply in the pipeline.
pub async fn run_transform(
    settings: &crate::config::Settings,
    system_prompt: &str,
    text: &str,
) -> anyhow::Result<String> {
    let system = format!(
        "{}\n\nApply this to the user's text below. Output ONLY the resulting \
         text — no preamble, labels, quotes, or explanation.",
        system_prompt.trim()
    );
    let chat = llm::resolve_chat(settings);
    let out = llm::chat(&chat, &system, text).await?;
    Ok(polish::strip_wrapping(&out))
}

// --- helpers -----------------------------------------------------------------

/// Inject `text` into `hwnd`, previewing it on the Flow Bar and dismissing the
/// bar afterward. Shared by Command Mode and transform shortcuts.
async fn inject_and_finish(app: &AppHandle, text: &str, hwnd: isize, strategy: &str) {
    let _ = app.emit_to(
        events::FLOWBAR,
        events::TRANSCRIPT_POLISHED,
        events::TranscriptPayload {
            text: text.to_string(),
        },
    );

    let app_for_inject = app.clone();
    let inject_text = text.to_string();
    let strategy = strategy.to_string();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        injection::inject(&app_for_inject, &inject_text, hwnd, &strategy)
    })
    .await;

    let _ = app.emit_to(
        events::FLOWBAR,
        events::DONE,
        events::DonePayload {
            text: text.to_string(),
        },
    );
    window_mgmt::hide_flowbar_after(app.clone(), 900);
}

