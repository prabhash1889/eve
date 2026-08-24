//! The dictation pipeline that runs after the key is released:
//! drain audio → resample/encode → transcribe (Groq) → polish → inject.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::config::CleanupLevel;
use crate::context::AppContext;
use crate::db::{queries, Db};
use crate::polish::StyleHint;
use crate::state::AppState;
use crate::timing::Timings;
use crate::transcription::{local_backend_label_for, wav_needed, Audio, TranscriptionBenchmark};
use crate::{audio, events, injection, text_processing, window_mgmt};

/// Emit a coarse processing-stage label to the Flow Bar (Phase 1 visibility).
fn stage(app: &AppHandle, label: &str) {
    let _ = app.emit_to(
        events::FLOWBAR,
        events::STAGE,
        events::StagePayload {
            label: label.to_string(),
        },
    );
}

/// Clears `is_processing` on drop, so the concurrency guard is released on every
/// exit path of `process` — including the many early returns and any panic.
pub struct ProcessingGuard(pub Arc<AtomicBool>);
impl Drop for ProcessingGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

pub async fn process(app: AppHandle) {
    // Release the concurrency flag whenever this function returns.
    let _processing = ProcessingGuard(app.state::<AppState>().is_processing.clone());

    // Snapshot the Arc-backed state up front so we never hold the guard across an await.
    let (
        buffer,
        sample_rate,
        settings,
        transcriber,
        polisher,
        last_transcript,
        last_benchmark,
        db,
        hot_cache,
        hwnd,
        context,
        to_scratchpad,
        pending_style_id,
    ) = {
        let st = app.state::<AppState>();
        // Bind the guarded clone to a local so the MutexGuard temporary drops
        // before the block's value (the tuple) is returned.
        let context = st.current_context.lock().clone();
        // 4.4: take (consume) a style armed by its accelerator - one-shot by
        // design, so it applies to this dictation only.
        let pending_style_id = st.pending_style_id.lock().take();
        (
            st.audio_buffer.clone(),
            st.sample_rate.clone(),
            st.settings.clone(),
            st.transcriber.clone(),
            st.polisher.clone(),
            st.last_transcript.clone(),
            st.last_transcription_benchmark.clone(),
            st.db.clone(),
            st.hot_cache.clone(),
            st.foreground_hwnd.load(Ordering::SeqCst),
            context,
            st.to_scratchpad.load(Ordering::SeqCst),
            pending_style_id,
        )
    };
    let context = context.unwrap_or_else(AppContext::unknown);

    // Phase 1: structured stage timing for the whole release-to-done flow. The
    // breakdown is logged + persisted on completion (see `timings.finish`).
    let mut timings = Timings::new();

    // Deterministic stop handshake (replaces the old fixed 60 ms sleep): wait
    // for the capture thread to ack that the stream is dropped and the final
    // samples are flushed into the shared buffer. Typically returns in ~0-33 ms
    // (one poll tick); on timeout we fall back to waiting the full 60 ms,
    // exactly the previous behavior.
    let app_for_handshake = app.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        app_for_handshake
            .state::<AppState>()
            .capture
            .stop_and_wait(audio::STOP_ACK_TIMEOUT);
    })
    .await;

    let samples = {
        let mut b = buffer.lock();
        std::mem::take(&mut *b)
    };
    let rate = sample_rate.load(Ordering::SeqCst);
    timings.mark("drain");

    // Capture length BEFORE `samples` is moved into the encode closure.
    let duration_ms = (samples.len() as i64 * 1000) / (rate.max(1) as i64);

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

    // Resample to 16 kHz + WAV-encode (CPU-bound → off the async runtime). We
    // keep BOTH the f32 samples (fed straight to the local backend, no WAV
    // round-trip) and the encoded WAV (cloud upload). When the local backend is
    // selected and no cloud fallback key exists, the WAV would be discarded
    // unread - skip the encode entirely (`wav_needed`).
    let need_wav = wav_needed(&settings.lock());
    let (samples16k, wav) = match tauri::async_runtime::spawn_blocking(move || {
        audio::prepare_16k(samples, rate, need_wav)
    })
    .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(_)) | Err(_) => {
            window_mgmt::fail(&app, "Audio processing failed");
            return;
        }
    };
    timings.mark("resample_encode");

    let (
        language,
        lang_label,
        level,
        strategy,
        vibe_coding,
        speech_is_local,
        speech_backend_label,
        transcriber_model,
        max_wav_bytes,
        vad_enabled,
        correctness_rescue,
        profile,
        debug_timing,
        cjk_autocorrect,
    ) = {
        let s = settings.lock();
        let lang = if s.language == "auto" {
            None
        } else {
            Some(s.language.clone())
        };
        // Resolve the speech backend once: local keeps its selected model id;
        // cloud carries the provider label + resolved model for the benchmark.
        let (speech_is_local, speech_backend_label, transcriber_model) =
            match crate::transcription::resolve_speech(&s) {
                crate::transcription::SpeechBackend::Local => {
                    (true, String::new(), s.local_whisper_model.clone())
                }
                crate::transcription::SpeechBackend::Cloud(t) => (
                    false,
                    t.provider.label().to_string(),
                    t.model,
                ),
            };
        (
            lang,
            s.language.clone(),
            s.cleanup_level,
            s.inject_strategy.clone(),
            s.vibe_coding,
            speech_is_local,
            speech_backend_label,
            transcriber_model,
            crate::transcription::max_wav_bytes_for(&s),
            s.local_vad_enabled,
            s.local_correctness_rescue,
            s.local_transcription_profile.clone(),
            s.debug_timing,
            s.cjk_autocorrect,
        )
    };

    // The effective provider's upload cap (Groq/OpenAI reject over 25 MB,
    // ~13 min of 16 kHz mono WAV; Deepgram has none). Detect an over-length
    // clip here and surface a clear "too long" message rather than letting the
    // request fail with a generic "check your connection".
    if let Some(cap) = max_wav_bytes {
        if wav.len() > cap {
            window_mgmt::fail(
                &app,
                "Recording too long — keep dictations under about 13 minutes",
            );
            return;
        }
    }

    // 4.8: audio is never persisted - history keeps transcript text only.

    // Phase 4: load dictionary terms to boost recognition (Whisper `prompt`).
    // 1.P5: served from the hot-path cache (invalidated on every dictionary
    // write) so the session doesn't take the DB lock here.
    let hints = hot_cache.hints(&db);

    timings.set_context(
        if speech_is_local { "local" } else { &speech_backend_label },
        &transcriber_model,
        &profile,
    );

    // Phase 3 (optimization): local-only silence trimming + normalization. The
    // full WAV (built above) is what cloud providers upload and what history
    // replays; only the f32 samples handed to the on-device backend are
    // trimmed. A clip that reads as all-silence fails fast here rather than
    // after a wasted inference.
    let mut vad_trimmed = false;
    let samples16k = if speech_is_local && vad_enabled {
        let params = audio::VadParams::for_profile(&profile, correctness_rescue);
        match tauri::async_runtime::spawn_blocking(move || {
            audio::preprocess_local(&samples16k, params)
        })
        .await
        {
            Ok(pre) if pre.speech_detected => {
                vad_trimmed = pre.trimmed;
                Arc::new(pre.samples)
            }
            Ok(_) => {
                window_mgmt::fail(&app, "No speech detected");
                return;
            }
            Err(_) => {
                window_mgmt::fail(&app, "Audio processing failed");
                return;
            }
        }
    } else {
        Arc::new(samples16k)
    };
    timings.mark("preprocess");

    // Transcribe. Pass the f32 samples + WAV together so the local backend skips
    // the WAV decode while Groq still gets the bytes it uploads.
    stage(&app, "Transcribing");
    let audio_input = Audio {
        samples: samples16k,
        wav,
    };
    let transcribe_started = std::time::Instant::now();
    let raw = match transcriber
        .transcribe_audio(audio_input, language, hints)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            window_mgmt::fail(&app, &friendly_error(&e.to_string()));
            return;
        }
    };
    let transcribe_ms = transcribe_started.elapsed().as_millis() as u64;
    timings.mark("transcribe");
    if raw.trim().is_empty() {
        window_mgmt::fail(&app, "No speech detected");
        return;
    }
    *last_benchmark.lock() = Some(TranscriptionBenchmark {
        mode: "dictation".into(),
        model: transcriber_model.clone(),
        profile: profile.clone(),
        backend: if speech_is_local {
            local_backend_label_for(&transcriber_model).to_string()
        } else {
            speech_backend_label
        },
        clip_duration_ms: duration_ms.max(0) as u64,
        transcribe_ms,
        words_produced: raw.split_whitespace().count(),
        vad_trimmed,
    });

    // Preview the raw transcript on the Flow Bar before polishing.
    let _ = app.emit_to(
        events::FLOWBAR,
        events::TRANSCRIPT_RAW,
        events::TranscriptPayload { text: raw.clone() },
    );

    // Phase 4: apply dictionary misspelling→correction mappings before any
    // other processing so downstream steps and the LLM see the corrected terms.
    let corrections = hot_cache.corrections(&db);
    let dict_corrected = text_processing::apply_corrections(&raw, &corrections);

    // Deterministic course-correction runs BEFORE the LLM so the model never
    // sees the retracted clause.
    let corrected = text_processing::course_correct(&dict_corrected);

    // Phase 6: look up the active Flow Style for the focused app's category and
    // turn it into a StyleHint that shapes the polish prompt (tone, per-app
    // context, optional custom instruction + writing sample). 4.4: a style
    // armed by its accelerator wins over both the exact-app profile and the
    // category default; a stale/deleted/disabled armed style is ignored.
    let style = match pending_style_id {
        Some(id) => {
            let conn = db.lock();
            crate::db::flow_styles::get(&conn, id)
                .ok()
                .flatten()
                .filter(|s| s.is_active)
        }
        None => None,
    }
    .or_else(|| hot_cache.active_style(&db, context.category.as_str(), &context.process));
    let style_hint = style.map(|s| StyleHint {
        category: s.app_category,
        tone: s.tone,
        system_prompt: s.system_prompt,
        writing_sample: s.writing_sample,
    });

    // Phase 5: polish is optional and never allowed to stall the pipeline.
    //  - CleanupLevel::None skips the LLM entirely (no round-trip at all).
    //  - Otherwise the call is bounded by POLISH_TIMEOUT; on timeout *or* error
    //    we inject the best available transcript (the course-corrected text)
    //    rather than making the user wait on a slow/hung model.
    const POLISH_TIMEOUT: Duration = Duration::from_secs(20);
    let polished = if matches!(level, CleanupLevel::None) {
        corrected
    } else {
        stage(&app, "Polishing");
        let fallback = corrected.clone();
        match tokio::time::timeout(
            POLISH_TIMEOUT,
            polisher.polish(corrected, level, style_hint),
        )
        .await
        {
            Ok(Ok(p)) => p,
            Ok(Err(_)) => fallback,
            Err(_) => {
                eprintln!("[polish] timed out after {POLISH_TIMEOUT:?}; injecting raw transcript");
                fallback
            }
        }
    };
    timings.mark("polish");

    let finalized = text_processing::finalize(&polished, cjk_autocorrect, &lang_label);

    // Phase 5: expand snippet triggers ("my email" → the full address) last,
    // just before injection, so the expansion text is injected verbatim.
    let expansions = hot_cache.snippet_expansions(&db);
    let mut text = text_processing::expand_snippets(&finalized, &expansions);
    if text.is_empty() {
        window_mgmt::fail(&app, "No speech detected");
        return;
    }

    // Phase 8: vibe-coding — in code editors, wrap spoken "backtick X backtick"
    // spans in literal backticks. Gated on the setting + the focused-app being
    // the Code category (Phase 6 context).
    if vibe_coding && context.category.as_str() == "code" {
        text = text_processing::apply_vibe_coding(&text);
    }

    // Phase 7: auto-apply transforms for the focused app's category (after
    // polish, just before injection). Each runs its saved prompt over the text
    // via the LLM; on error or empty output we keep the prior text so a
    // transform failure never blocks the dictation.
    let auto_transforms = hot_cache.auto_transforms(&db, context.category.as_str());
    if !auto_transforms.is_empty() {
        // Snapshot for the LLM calls; never hold the guard across `.await`.
        let transform_settings = settings.lock().clone();
        for t in auto_transforms {
            if let Ok(out) =
                crate::command_mode::run_transform(&transform_settings, &t.system_prompt, &text)
                    .await
            {
                if !out.is_empty() {
                    text = out;
                }
            }
        }
    }

    // Show the polished/finalized result before injecting.
    let _ = app.emit_to(
        events::FLOWBAR,
        events::TRANSCRIPT_POLISHED,
        events::TranscriptPayload { text: text.clone() },
    );

    // Record the transcript before injecting so the copy-last shortcut can still
    // retrieve it even if the paste below fails (e.g. the target window closed).
    *last_transcript.lock() = Some(text.clone());

    // Phase 9: if the Scratchpad window had focus at record start, route the
    // text into its editor (the window listens for `scratchpad://insert`)
    // instead of OS-pasting into a foreign app.
    stage(&app, "Inserting");
    if to_scratchpad {
        let _ = app.emit_to(
            events::SCRATCHPAD,
            events::SCRATCHPAD_INSERT,
            events::TranscriptPayload { text: text.clone() },
        );
    } else {
        // Inject into the focused app (blocking: clipboard + key simulation).
        let app_for_inject = app.clone();
        let inject_text = text.clone();
        let inject_result = tauri::async_runtime::spawn_blocking(move || {
            injection::inject(&app_for_inject, &inject_text, hwnd, &strategy)
        })
        .await;
        match inject_result {
            Ok(Ok(())) => {}
            // Surface a real failure (e.g. the target window was closed before
            // release) instead of silently dropping the text into nowhere.
            Ok(Err(e)) => {
                window_mgmt::fail(&app, &e.to_string());
                return;
            }
            Err(_) => {
                window_mgmt::fail(&app, "Couldn't paste the transcribed text");
                return;
            }
        }
    }

    // Phase 8: how much cleanup this dictation needed (raw → final word edits),
    // folded into the daily rollup for the Insights page.
    let corrections = text_processing::count_edits(&raw, &text) as i64;

    timings.mark("inject");
    // Phase 1: log + persist the full stage breakdown for this session. Phase 5:
    // when debug-timing is on, also print the detailed per-stage breakdown.
    timings.finish(&app, debug_timing);

    // Emit DONE before persisting: persistence is best-effort (SQLite insert +
    // possible WAV file I/O) and must never delay the user-visible completion
    // signal. It runs off the async runtime in spawn_blocking so we never block
    // the tokio workers with disk I/O.
    let _ = app.emit_to(
        events::FLOWBAR,
        events::DONE,
        events::DonePayload { text: text.clone() },
    );
    window_mgmt::hide_flowbar_after(app.clone(), 900);

    let _ = tauri::async_runtime::spawn_blocking(move || {
        persist(
            &db,
            &raw,
            &text,
            level,
            &lang_label,
            duration_ms,
            corrections,
            &context,
        );
    })
    .await;
}

/// Save the dictation to the history DB. 4.8: no WAV is written - the audio
/// replay plumbing was removed (YAGNI); history keeps transcript text only.
#[allow(clippy::too_many_arguments)]
fn persist(
    db: &Db,
    raw: &str,
    text: &str,
    level: CleanupLevel,
    language: &str,
    duration_ms: i64,
    corrections: i64,
    context: &AppContext,
) {
    let created_at = chrono::Utc::now().timestamp_millis();
    let word_count = text.split_whitespace().count() as i64;
    let was_polished = !matches!(level, CleanupLevel::None);

    let row = queries::NewTranscript {
        created_at,
        raw_text: raw.to_string(),
        polished_text: text.to_string(),
        cleanup_level: level.as_str().to_string(),
        language: language.to_string(),
        app_process: context.process.clone(),
        app_title: context.title.clone(),
        app_category: context.category.as_str().to_string(),
        word_count,
        duration_ms,
        was_polished,
        source_file: None,
    };
    let conn = db.lock();
    let _ = queries::insert_transcript(&conn, &row);
    // Phase 8: fold this session into the daily rollup for the Insights page.
    let _ = queries::record_daily(
        &conn,
        created_at,
        word_count,
        duration_ms,
        corrections,
        context.category.as_str(),
    );
}

fn friendly_error(err: &str) -> String {
    if err.contains("not downloaded") || err.contains("No local") {
        // Local backend selected but no usable model (and no Groq fallback).
        err.to_string()
    } else if err.contains("not built in") {
        "Local models aren't available in this build".into()
    } else if err.contains("Failed to load") || err.contains("load model") {
        "Local model failed to load — try re-downloading it".into()
    } else if err.contains("API key") {
        "Set your provider API key in Settings".into()
    } else if err.contains("401") || err.contains("invalid_api_key") {
        "Invalid API key — check Settings".into()
    } else if err.contains("429") {
        "Rate limited — try again in a moment".into()
    } else if err.contains("413") || err.contains("too large") || err.contains("too long") {
        "Recording too long — keep dictations under about 13 minutes".into()
    } else {
        "Transcription failed — check your connection".into()
    }
}

#[cfg(test)]
mod tests {
    use super::friendly_error;

    /// Freeze the error-mapping strings before the 5.1 session-module
    /// extraction moves them. Each raw provider/backend error maps to exactly
    /// one user-facing Flow Bar message.
    #[test]
    fn friendly_error_strings_are_frozen() {
        // Local backend selected but the model is missing: message passes through.
        assert_eq!(
            friendly_error("Model 'whisper-small' is not downloaded yet"),
            "Model 'whisper-small' is not downloaded yet"
        );
        assert_eq!(
            friendly_error("No local speech model selected - pick one in Models"),
            "No local speech model selected - pick one in Models"
        );
        assert_eq!(
            friendly_error("Local transcription was not built in (enable the `local-whisper` feature)"),
            "Local models aren't available in this build"
        );
        assert_eq!(
            friendly_error("Failed to load Whisper model: bad ggml"),
            "Local model failed to load \u{2014} try re-downloading it"
        );
        assert_eq!(
            friendly_error("Set your Groq API key in Settings"),
            "Set your provider API key in Settings"
        );
        assert_eq!(
            friendly_error("Groq error 401 Unauthorized: invalid_api_key"),
            "Invalid API key \u{2014} check Settings"
        );
        assert_eq!(
            friendly_error("Groq error 429 Too Many Requests"),
            "Rate limited \u{2014} try again in a moment"
        );
        assert_eq!(
            friendly_error("413 Payload Too Large"),
            "Recording too long \u{2014} keep dictations under about 13 minutes"
        );
        assert_eq!(
            friendly_error("error sending request for url"),
            "Transcription failed \u{2014} check your connection"
        );
    }

    /// Ordering matters: today the generic "API key" branch is checked before
    /// the 401 branch, so an error containing both maps to the key hint. Freeze
    /// that precedence so a reorder during extraction is a conscious choice.
    #[test]
    fn friendly_error_key_hint_precedes_auth_check() {
        let mapped = friendly_error("invalid_api_key with 'API key' in body");
        assert_eq!(mapped, "Set your provider API key in Settings");
    }
}
