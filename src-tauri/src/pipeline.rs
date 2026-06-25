//! The dictation pipeline that runs after the key is released:
//! drain audio → resample/encode → transcribe (Groq) → polish → inject.

use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;
use crate::{audio, events, injection, text_processing, window_mgmt};

pub async fn process(app: AppHandle) {
    // Snapshot the Arc-backed state up front so we never hold the guard across an await.
    let (buffer, sample_rate, settings, transcriber, polisher, last_transcript, hwnd) = {
        let st = app.state::<AppState>();
        (
            st.audio_buffer.clone(),
            st.sample_rate.clone(),
            st.settings.clone(),
            st.transcriber.clone(),
            st.polisher.clone(),
            st.last_transcript.clone(),
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

    let (language, level, strategy) = {
        let s = settings.lock();
        let lang = if s.language == "auto" {
            None
        } else {
            Some(s.language.clone())
        };
        (lang, s.cleanup_level, s.inject_strategy.clone())
    };

    // Transcribe.
    let raw = match transcriber.transcribe(wav, language, Vec::new()).await {
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

    // Deterministic course-correction runs BEFORE the LLM so the model never
    // sees the retracted clause.
    let corrected = text_processing::course_correct(&raw);

    // LLM polish (no-op for CleanupLevel::None; fall back to raw text on error).
    let polished = polisher
        .polish(corrected.clone(), level)
        .await
        .unwrap_or(corrected);

    // Deterministic spoken-punctuation + list formatting runs AFTER the LLM so
    // it can't reflow the structure we just inserted.
    let text = text_processing::finalize(&polished);
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
    let _ = app.emit_to(events::FLOWBAR, events::DONE, events::DonePayload { text });
    window_mgmt::hide_flowbar_after(app, 900);
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
