//! The dictation pipeline that runs after the key is released:
//! drain audio → resample/encode → transcribe (Groq) → polish → inject.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::config::CleanupLevel;
use crate::context::AppContext;
use crate::db::{dictionary, flow_styles, queries, snippets, transforms, Db};
use crate::polish::StyleHint;
use crate::state::AppState;
use crate::timing::Timings;
use crate::transcription::{local_backend_label, Audio, TranscriptionBenchmark};
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
struct ProcessingGuard(Arc<AtomicBool>);
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
        hwnd,
        context,
        to_scratchpad,
    ) = {
        let st = app.state::<AppState>();
        // Bind the guarded clone to a local so the MutexGuard temporary drops
        // before the block's value (the tuple) is returned.
        let context = st.current_context.lock().clone();
        (
            st.audio_buffer.clone(),
            st.sample_rate.clone(),
            st.settings.clone(),
            st.transcriber.clone(),
            st.polisher.clone(),
            st.last_transcript.clone(),
            st.last_transcription_benchmark.clone(),
            st.db.clone(),
            st.foreground_hwnd.load(Ordering::SeqCst),
            context,
            st.to_scratchpad.load(Ordering::SeqCst),
        )
    };
    let context = context.unwrap_or_else(AppContext::unknown);

    // Phase 1: structured stage timing for the whole release-to-done flow. The
    // breakdown is logged + persisted on completion (see `timings.finish`).
    let mut timings = Timings::new();

    // Give the capture thread a moment to stop and flush its last samples.
    let _ = tauri::async_runtime::spawn_blocking(|| std::thread::sleep(Duration::from_millis(60)))
        .await;

    let samples = {
        let mut b = buffer.lock();
        std::mem::take(&mut *b)
    };
    let rate = sample_rate.load(Ordering::SeqCst);
    timings.mark("drain");

    // Reject clips that are too short to be real speech (~125 ms).
    if samples.len() < (rate as usize / 8).max(800) {
        window_mgmt::fail(&app, "Didn't catch that — hold the key a little longer");
        return;
    }

    // Capture length BEFORE `samples` is moved into the encode closure.
    let duration_ms = (samples.len() as i64 * 1000) / (rate.max(1) as i64);

    // Resample to 16 kHz + WAV-encode (CPU-bound → off the async runtime). We
    // keep BOTH the f32 samples (fed straight to the local backend, no WAV
    // round-trip) and the encoded WAV (cloud upload + history replay).
    let (samples16k, wav) = match tauri::async_runtime::spawn_blocking(move || {
        let resampled = audio::resample_to_16k(&samples, rate);
        let wav = audio::encode_wav(&resampled)?;
        anyhow::Ok((resampled, wav))
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
        transcription_backend,
        transcriber_model,
        vad_enabled,
        correctness_rescue,
        profile,
        debug_timing,
    ) = {
        let s = settings.lock();
        let lang = if s.language == "auto" {
            None
        } else {
            Some(s.language.clone())
        };
        // Record the local model only when local STT is actually in effect.
        let model = if s.transcription_backend == "local" {
            s.local_whisper_model.clone()
        } else {
            String::new()
        };
        (
            lang,
            s.language.clone(),
            s.cleanup_level,
            s.inject_strategy.clone(),
            s.vibe_coding,
            s.transcription_backend.clone(),
            model,
            s.local_vad_enabled,
            s.local_correctness_rescue,
            s.local_transcription_profile.clone(),
            s.debug_timing,
        )
    };

    // Groq rejects uploads over 25 MB (≈13 min of 16 kHz mono WAV). Detect that
    // here and surface a clear "too long" message rather than letting the request
    // fail with a generic "check your connection".
    if transcription_backend == "groq"
        && wav.len() > crate::transcription::GROQ_MAX_WAV_BYTES
    {
        window_mgmt::fail(
            &app,
            "Recording too long — keep dictations under about 13 minutes",
        );
        return;
    }

    // Audio is never persisted — history keeps transcript text only.
    let audio_bytes: Option<Vec<u8>> = None;

    // Phase 4: load dictionary terms to boost recognition (Whisper `prompt`).
    let hints = {
        let conn = db.lock();
        dictionary::hints(&conn, 100).unwrap_or_default()
    };

    timings.set_context(&transcription_backend, &transcriber_model, &profile);

    // Phase 3 (optimization): local-only silence trimming + normalization. The
    // full WAV (built above) is what Groq uploads and what history replays; only
    // the f32 samples handed to the on-device backend are trimmed. A clip that
    // reads as all-silence fails fast here rather than after a wasted inference.
    let mut vad_trimmed = false;
    let samples16k = if transcription_backend == "local" && vad_enabled {
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
        model: if transcription_backend == "local" {
            transcriber_model.clone()
        } else {
            "whisper-large-v3-turbo".into()
        },
        profile: profile.clone(),
        backend: if transcription_backend == "local" {
            local_backend_label().to_string()
        } else {
            "Groq".into()
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
    let corrections = {
        let conn = db.lock();
        dictionary::corrections(&conn).unwrap_or_default()
    };
    let dict_corrected = text_processing::apply_corrections(&raw, &corrections);

    // Deterministic course-correction runs BEFORE the LLM so the model never
    // sees the retracted clause.
    let corrected = text_processing::course_correct(&dict_corrected);

    // Phase 6: look up the active Flow Style for the focused app's category and
    // turn it into a StyleHint that shapes the polish prompt (tone, per-app
    // context, optional custom instruction + writing sample).
    let style = {
        let conn = db.lock();
        flow_styles::active_for(&conn, context.category.as_str())
            .ok()
            .flatten()
    };
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

    // Deterministic spoken-punctuation + list formatting runs AFTER the LLM so
    // it can't reflow the structure we just inserted.
    let finalized = text_processing::finalize(&polished);

    // Phase 5: expand snippet triggers ("my email" → the full address) last,
    // just before injection, so the expansion text is injected verbatim.
    let expansions = {
        let conn = db.lock();
        snippets::active_expansions(&conn).unwrap_or_default()
    };
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
    let auto_transforms = {
        let conn = db.lock();
        transforms::auto_apply_for(&conn, context.category.as_str()).unwrap_or_default()
    };
    for t in auto_transforms {
        if let Ok(out) = crate::command_mode::run_transform(&t.system_prompt, &text).await {
            if !out.is_empty() {
                text = out;
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

    // Phase 3: persist this dictation to history (after all awaits, so we never
    // hold the DB guard across one). Best-effort — a failed insert must not
    // break the user-visible flow.
    persist(
        &app,
        &db,
        &raw,
        &text,
        level,
        &lang_label,
        duration_ms,
        corrections,
        audio_bytes,
        &context,
    );

    timings.mark("inject");
    // Phase 1: log + persist the full stage breakdown for this session. Phase 5:
    // when debug-timing is on, also print the detailed per-stage breakdown.
    timings.finish(&app, debug_timing);

    let _ = app.emit_to(events::FLOWBAR, events::DONE, events::DonePayload { text });
    window_mgmt::hide_flowbar_after(app, 900);
}

/// Save the dictation to the history DB, optionally writing the WAV to disk for
/// replay/retention. `app_*` fields carry the Phase 6 focused-app context.
#[allow(clippy::too_many_arguments)]
fn persist(
    app: &AppHandle,
    db: &Db,
    raw: &str,
    text: &str,
    level: CleanupLevel,
    language: &str,
    duration_ms: i64,
    corrections: i64,
    wav: Option<Vec<u8>>,
    context: &AppContext,
) {
    let created_at = chrono::Utc::now().timestamp_millis();
    let word_count = text.split_whitespace().count() as i64;
    let was_polished = !matches!(level, CleanupLevel::None);

    let audio_path = wav.and_then(|bytes| {
        let dir = app.path().app_data_dir().ok()?.join("audio");
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join(format!("{created_at}.wav"));
        std::fs::write(&path, bytes).ok()?;
        Some(path.to_string_lossy().into_owned())
    });

    let row = queries::NewTranscript {
        created_at,
        raw_text: raw.to_string(),
        polished_text: text.to_string(),
        cleanup_level: level.as_str().to_string(),
        language: language.to_string(),
        audio_path,
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
        "Set your Groq API key in Settings".into()
    } else if err.contains("401") || err.contains("invalid_api_key") {
        "Invalid Groq API key".into()
    } else if err.contains("429") {
        "Rate limited — try again in a moment".into()
    } else if err.contains("413") || err.contains("too large") || err.contains("too long") {
        "Recording too long — keep dictations under about 13 minutes".into()
    } else {
        "Transcription failed — check your connection".into()
    }
}
