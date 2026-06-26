//! The dictation pipeline that runs after the key is released:
//! drain audio → resample/encode → transcribe (Groq) → polish → inject.

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::config::CleanupLevel;
use crate::context::AppContext;
use crate::db::{dictionary, flow_styles, queries, snippets, transforms, Db};
use crate::polish::StyleHint;
use crate::state::AppState;
use crate::{audio, events, injection, text_processing, window_mgmt};

pub async fn process(app: AppHandle) {
    // Snapshot the Arc-backed state up front so we never hold the guard across an await.
    let (buffer, sample_rate, settings, transcriber, polisher, last_transcript, db, hwnd, context) = {
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
            st.db.clone(),
            st.foreground_hwnd.load(Ordering::SeqCst),
            context,
        )
    };
    let context = context.unwrap_or_else(AppContext::unknown);

    // Give the capture thread a moment to stop and flush its last samples.
    let _ = tauri::async_runtime::spawn_blocking(|| {
        std::thread::sleep(Duration::from_millis(60))
    })
    .await;

    let samples = {
        let mut b = buffer.lock();
        std::mem::take(&mut *b)
    };
    let rate = sample_rate.load(Ordering::SeqCst);

    // Reject clips that are too short to be real speech (~125 ms).
    if samples.len() < (rate as usize / 8).max(800) {
        window_mgmt::fail(&app, "Didn't catch that — hold the key a little longer");
        return;
    }

    // Capture length BEFORE `samples` is moved into the encode closure.
    let duration_ms = (samples.len() as i64 * 1000) / (rate.max(1) as i64);

    // Resample to 16 kHz + WAV-encode (CPU-bound → off the async runtime).
    let wav = match tauri::async_runtime::spawn_blocking(move || {
        let resampled = audio::resample_to_16k(&samples, rate);
        audio::encode_wav(&resampled)
    })
    .await
    {
        Ok(w) => w,
        Err(_) => {
            window_mgmt::fail(&app, "Audio processing failed");
            return;
        }
    };

    let (language, lang_label, level, strategy, store_audio, vibe_coding) = {
        let s = settings.lock();
        let lang = if s.language == "auto" {
            None
        } else {
            Some(s.language.clone())
        };
        (
            lang,
            s.language.clone(),
            s.cleanup_level,
            s.inject_strategy.clone(),
            s.audio_storage_policy != "never",
            s.vibe_coding,
        )
    };

    // Keep a copy of the WAV for storage/replay unless the user opted out.
    let audio_bytes = if store_audio { Some(wav.clone()) } else { None };

    // Phase 4: load dictionary terms to boost recognition (Whisper `prompt`).
    let hints = {
        let conn = db.lock();
        dictionary::hints(&conn, 100).unwrap_or_default()
    };

    // Transcribe.
    let raw = match transcriber.transcribe(wav, language, hints).await {
        Ok(t) => t,
        Err(e) => {
            window_mgmt::fail(&app, &friendly_error(&e.to_string()));
            return;
        }
    };
    if raw.trim().is_empty() {
        window_mgmt::fail(&app, "No speech detected");
        return;
    }

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

    // LLM polish (no-op for CleanupLevel::None; fall back to raw text on error).
    let polished = polisher
        .polish(corrected.clone(), level, style_hint)
        .await
        .unwrap_or(corrected);

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

    // Inject into the focused app (blocking: clipboard + key simulation).
    let app_for_inject = app.clone();
    let inject_text = text.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        injection::inject(&app_for_inject, &inject_text, hwnd, &strategy)
    })
    .await;

    *last_transcript.lock() = Some(text.clone());

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
    } else {
        "Transcription failed — check your connection".into()
    }
}
