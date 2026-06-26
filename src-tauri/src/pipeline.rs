//! The dictation pipeline that runs after the key is released:
//! drain audio → resample/encode → transcribe (Groq) → polish → inject.

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::config::CleanupLevel;
use crate::db::{dictionary, queries, snippets, Db};
use crate::state::AppState;
use crate::{audio, events, injection, text_processing, window_mgmt};

pub async fn process(app: AppHandle) {
    // Snapshot the Arc-backed state up front so we never hold the guard across an await.
    let (buffer, sample_rate, settings, transcriber, polisher, last_transcript, db, hwnd) = {
        let st = app.state::<AppState>();
        (
            st.audio_buffer.clone(),
            st.sample_rate.clone(),
            st.settings.clone(),
            st.transcriber.clone(),
            st.polisher.clone(),
            st.last_transcript.clone(),
            st.db.clone(),
            st.foreground_hwnd.load(Ordering::SeqCst),
        )
    };

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

    let (language, lang_label, level, strategy, store_audio) = {
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

    // LLM polish (no-op for CleanupLevel::None; fall back to raw text on error).
    let polished = polisher
        .polish(corrected.clone(), level)
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
    let text = text_processing::expand_snippets(&finalized, &expansions);
    if text.is_empty() {
        window_mgmt::fail(&app, "No speech detected");
        return;
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

    // Phase 3: persist this dictation to history (after all awaits, so we never
    // hold the DB guard across one). Best-effort — a failed insert must not
    // break the user-visible flow.
    persist(&app, &db, &raw, &text, level, &lang_label, duration_ms, audio_bytes);

    let _ = app.emit_to(events::FLOWBAR, events::DONE, events::DonePayload { text });
    window_mgmt::hide_flowbar_after(app, 900);
}

/// Save the dictation to the history DB, optionally writing the WAV to disk for
/// replay/retention. `app_*` fields stay empty until Phase 6 adds context.
#[allow(clippy::too_many_arguments)]
fn persist(
    app: &AppHandle,
    db: &Db,
    raw: &str,
    text: &str,
    level: CleanupLevel,
    language: &str,
    duration_ms: i64,
    wav: Option<Vec<u8>>,
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
        app_process: String::new(),
        app_title: String::new(),
        app_category: String::new(),
        word_count,
        duration_ms,
        was_polished,
    };
    let _ = queries::insert_transcript(&db.lock(), &row);
}

fn friendly_error(err: &str) -> String {
    if err.contains("API key") {
        "Set your Groq API key in Settings".into()
    } else if err.contains("401") || err.contains("invalid_api_key") {
        "Invalid Groq API key".into()
    } else if err.contains("429") {
        "Rate limited — try again in a moment".into()
    } else {
        "Transcription failed — check your connection".into()
    }
}
