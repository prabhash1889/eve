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
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::db::transforms;
use crate::state::AppState;
use crate::transcription::{local_backend_label_for, Audio, TranscriptionBenchmark};
use crate::{audio, events, hotkey, injection, llm, polish, window_mgmt};

// --- Command Mode push-to-talk ----------------------------------------------

/// Key-down: start capturing the spoken instruction, flagged as Command Mode so
/// key-up routes to `process_command`. Mirrors `hotkey::on_press` but tags the
/// Flow Bar with the "command" mode for a distinct look.
pub fn on_press(app: &AppHandle, st: &AppState) {
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

    let device_name = st.settings.lock().input_device.clone();
    st.capture.start(
        app.clone(),
        st.is_recording.clone(),
        st.audio_buffer.clone(),
        st.sample_rate.clone(),
        st.current_amplitude.clone(),
        device_name,
    );
}

/// Key-up: stop recording and run the command pipeline.
pub fn on_release(app: &AppHandle, st: &AppState) {
    if !st.is_recording.swap(false, Ordering::SeqCst) {
        return;
    }
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
    let (buffer, sample_rate, settings, transcriber, last_transcript, last_benchmark, hwnd) = {
        let st = app.state::<AppState>();
        (
            st.audio_buffer.clone(),
            st.sample_rate.clone(),
            st.settings.clone(),
            st.transcriber.clone(),
            st.last_transcript.clone(),
            st.last_transcription_benchmark.clone(),
            st.foreground_hwnd.load(Ordering::SeqCst),
        )
    };

    // Let the capture thread flush its final samples.
    let _ = tauri::async_runtime::spawn_blocking(|| std::thread::sleep(Duration::from_millis(60)))
        .await;

    let samples = {
        let mut b = buffer.lock();
        std::mem::take(&mut *b)
    };
    let rate = sample_rate.load(Ordering::SeqCst);
    let duration_ms = (samples.len() as u64 * 1000) / rate.max(1) as u64;

    if duration_ms < 1000 {
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

    let (language, strategy, backend, vad_enabled, correctness_rescue, profile, model) = {
        let s = settings.lock();
        let lang = if s.language == "auto" {
            None
        } else {
            Some(s.language.clone())
        };
        (
            lang,
            s.inject_strategy.clone(),
            s.transcription_backend.clone(),
            s.local_vad_enabled,
            s.local_correctness_rescue,
            s.local_transcription_profile.clone(),
            if s.transcription_backend == "local" {
                s.local_whisper_model.clone()
            } else {
                String::new()
            },
        )
    };

    // Build the same dual-form audio payload as dictation mode. The local path
    // consumes samples directly; the WAV stays available for Groq/fallback.
    let backend_for_preprocess = backend.clone();
    let profile_for_preprocess = profile.clone();
    let processed = match tauri::async_runtime::spawn_blocking(move || {
        let mut resampled = audio::resample_to_16k(&samples, rate);
        let mut vad_trimmed = false;
        if backend_for_preprocess == "local" && vad_enabled {
            let pre = audio::preprocess_local(
                &resampled,
                audio::VadParams::for_profile(&profile_for_preprocess, correctness_rescue),
            );
            if !pre.speech_detected {
                return anyhow::Ok((resampled, Vec::new(), vad_trimmed, false));
            }
            vad_trimmed = pre.trimmed;
            resampled = pre.samples;
        }
        let wav = audio::encode_wav(&resampled)?;
        anyhow::Ok((resampled, wav, vad_trimmed, true))
    })
    .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(_)) | Err(_) => {
            window_mgmt::fail(&app, "Audio processing failed");
            return;
        }
    };
    let (samples16k, wav, vad_trimmed, speech_detected) = processed;
    if !speech_detected {
        window_mgmt::fail(&app, "No speech detected");
        return;
    }

    let transcribe_started = std::time::Instant::now();
    let instruction = match transcriber
        .transcribe_audio(
            Audio {
                samples: std::sync::Arc::new(samples16k),
                wav,
            },
            language,
            Vec::new(),
        )
        .await
    {
        Ok(t) => t,
        Err(e) => {
            window_mgmt::fail(&app, &command_error(&e.to_string()));
            return;
        }
    };
    let transcribe_ms = transcribe_started.elapsed().as_millis() as u64;
    if instruction.trim().is_empty() {
        window_mgmt::fail(&app, "No instruction heard");
        return;
    }
    *last_benchmark.lock() = Some(TranscriptionBenchmark {
        mode: "command".into(),
        backend: if settings.lock().transcription_backend == "local" {
            local_backend_label_for(&model).to_string()
        } else {
            "Groq".into()
        },
        model: if model.is_empty() {
            "whisper-large-v3-turbo".into()
        } else {
            model
        },
        profile,
        clip_duration_ms: duration_ms,
        transcribe_ms,
        words_produced: instruction.split_whitespace().count(),
        vad_trimmed,
    });

    // Read the selection from the still-focused target app (blocking key sim).
    let app_for_sel = app.clone();
    let selection = tauri::async_runtime::spawn_blocking(move || {
        injection::capture_selection(&app_for_sel, hwnd)
    })
    .await
    .ok()
    .flatten();

    let result = match run_command(selection.as_deref(), &instruction).await {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => {
            window_mgmt::fail(&app, "Command produced no text");
            return;
        }
        Err(e) => {
            window_mgmt::fail(&app, &command_error(&e.to_string()));
            return;
        }
    };

    inject_and_finish(&app, &result, hwnd, &strategy).await;
    *last_transcript.lock() = Some(result);
}

// --- Transform shortcuts -----------------------------------------------------

/// A transform accelerator fired: rewrite the current selection with the saved
/// transform's prompt. No-op while a dictation/command capture is in flight.
pub fn on_transform(app: &AppHandle, st: &AppState, id: i64) {
    if st.is_recording.load(Ordering::SeqCst) {
        return;
    }

    let hwnd = crate::platform::frontmost(app).handle;

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        run_transform_shortcut(handle, id, hwnd).await;
    });
}

async fn run_transform_shortcut(app: AppHandle, id: i64, hwnd: isize) {
    let (db, strategy, last_transcript, bubble) = {
        let st = app.state::<AppState>();
        let s = st.settings.lock();
        (
            st.db.clone(),
            s.inject_strategy.clone(),
            st.last_transcript.clone(),
            (s.bubble_scale, s.bubble_opacity),
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

    let result = match run_transform(&transform.system_prompt, &selection).await {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => {
            window_mgmt::fail(&app, "Transform produced no text");
            return;
        }
        Err(e) => {
            window_mgmt::fail(&app, &command_error(&e.to_string()));
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

    let reserved = [
        *st.main_shortcut.lock(),
        *st.copy_shortcut.lock(),
        *st.command_shortcut.lock(),
        st.escape_shortcut,
    ];

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

// --- LLM steps (also exposed as commands) ------------------------------------

/// The Command Mode LLM step: rewrite `selection` per `instruction`, or generate
/// fresh text from `instruction` when nothing is selected. Output is unwrapped
/// so stray quotes/preambles don't leak into the injected text.
pub async fn run_command(selection: Option<&str>, instruction: &str) -> anyhow::Result<String> {
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
    let out = llm::chat(&system, &user).await?;
    Ok(polish::strip_wrapping(&out))
}

/// Apply a saved transform's `system_prompt` to `text`. Shared by the transform
/// shortcut, the `apply_transform` command, and auto-apply in the pipeline.
pub async fn run_transform(system_prompt: &str, text: &str) -> anyhow::Result<String> {
    let system = format!(
        "{}\n\nApply this to the user's text below. Output ONLY the resulting \
         text — no preamble, labels, quotes, or explanation.",
        system_prompt.trim()
    );
    let out = llm::chat(&system, text).await?;
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

/// Map an LLM/transcription error to a short Flow Bar message.
fn command_error(err: &str) -> String {
    if err.contains("API key") {
        "Set your Groq API key in Settings".into()
    } else if err.contains("401") || err.contains("invalid_api_key") {
        "Invalid Groq API key".into()
    } else if err.contains("429") {
        "Rate limited — try again in a moment".into()
    } else {
        "Command failed — check your connection".into()
    }
}
