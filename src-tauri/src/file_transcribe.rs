//! Phase C: file transcription + queue. Dropped or picked audio files are read
//! in place (never copied or stored), decoded with symphonia, downmixed to mono,
//! resampled to 16 kHz, and run through the same transcriber → polish → history
//! path as mic dictation. Injection is skipped entirely for file items.
//!
//! Files are drained serially by a single background worker so a batch of long
//! files can't spawn N concurrent inferences (which would thrash a local model
//! or blow past Groq rate limits). The queue lives in `AppState`; this module
//! owns the worker loop and the decode step.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::config::CleanupLevel;
use crate::db::{dictionary, queries};
use crate::state::AppState;
use crate::transcription::{Audio, GROQ_MAX_WAV_BYTES};
use crate::{audio, events, text_processing};

/// A file waiting in (or moving through) the transcription queue. The path is
/// kept private to the backend; the UI only ever sees `id` + `file_name`.
#[derive(Clone)]
pub struct QueuedFile {
    pub id: u64,
    pub path: PathBuf,
    pub file_name: String,
}

/// The minimal per-item shape returned to the UI from `transcribe_files`, so it
/// can render the queue card immediately (later updates arrive as `queue://*`
/// events).
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub id: u64,
    pub file_name: String,
}

/// Decoded PCM from an audio file: interleaved-then-downmixed mono f32 at the
/// file's native sample rate (resampling to 16 kHz happens separately, reusing
/// the mic path's resampler).
struct DecodedAudio {
    samples: Vec<f32>,
    sample_rate: u32,
}

/// Enqueue `paths` for transcription and ensure the worker is draining. Returns
/// the freshly-assigned queue items (id + file name) for immediate UI render.
pub fn enqueue(app: &AppHandle, paths: Vec<String>) -> Vec<QueueItem> {
    let st = app.state::<AppState>();

    let mut items = Vec::new();
    {
        let mut queue = st.file_queue.lock();
        for p in paths {
            let path = PathBuf::from(&p);
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.clone());
            let id = st.queue_next_id.fetch_add(1, Ordering::SeqCst);
            items.push(QueueItem {
                id,
                file_name: file_name.clone(),
            });
            queue.push_back(QueuedFile {
                id,
                path,
                file_name,
            });
        }
    }

    // Tell the UI each file is queued (mirrors what `transcribe_files` returns,
    // but also reaches any other listener).
    for it in &items {
        emit_progress(app, it.id, &it.file_name, "Queued");
    }

    // Spawn the worker only if one isn't already running. The worker clears the
    // flag under the queue lock when it empties, so this never loses a wakeup.
    if !st.queue_worker_running.swap(true, Ordering::SeqCst) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            run_worker(app).await;
        });
    }

    items
}

/// Request cancellation of a queued item. A still-pending file is dropped from
/// the queue immediately; a file already being processed is abandoned at the
/// next stage boundary (the worker checks `queue_cancelled`).
pub fn cancel(app: &AppHandle, id: u64) {
    let st = app.state::<AppState>();
    st.queue_cancelled.lock().insert(id);
    st.file_queue.lock().retain(|f| f.id != id);
}

/// Drain the queue serially until empty, then clear the running flag. Holds the
/// queue lock only to pop the next item (and to clear the flag), never across an
/// await.
async fn run_worker(app: AppHandle) {
    loop {
        let next = {
            let st = app.state::<AppState>();
            let mut queue = st.file_queue.lock();
            match queue.pop_front() {
                Some(item) => Some(item),
                None => {
                    // Empty: clear the flag while still holding the lock so a
                    // concurrent `enqueue` either sees a non-empty queue (we'd
                    // have popped it) or a cleared flag (it spawns a fresh worker).
                    st.queue_worker_running.store(false, Ordering::SeqCst);
                    None
                }
            }
        };
        let Some(item) = next else { break };

        if cancelled(&app, item.id) {
            // Dropped before it started — the UI already removed the card.
            forget(&app, item.id);
            continue;
        }
        process_one(&app, &item).await;
        forget(&app, item.id);
    }
}

/// Has this item been cancelled?
fn cancelled(app: &AppHandle, id: u64) -> bool {
    app.state::<AppState>().queue_cancelled.lock().contains(&id)
}

/// Drop the cancellation bookkeeping for a finished/abandoned item.
fn forget(app: &AppHandle, id: u64) {
    app.state::<AppState>().queue_cancelled.lock().remove(&id);
}

/// Decode → resample → transcribe → polish → persist one file. Emits stage
/// progress along the way and a terminal `queue://done` or `queue://error`.
async fn process_one(app: &AppHandle, item: &QueuedFile) {
    let (settings, transcriber, polisher, db) = {
        let st = app.state::<AppState>();
        (
            st.settings.clone(),
            st.transcriber.clone(),
            st.polisher.clone(),
            st.db.clone(),
        )
    };

    // Decode off the async runtime (symphonia is sync + CPU-bound).
    emit_progress(app, item.id, &item.file_name, "Decoding");
    let path = item.path.clone();
    let decoded = match tauri::async_runtime::spawn_blocking(move || decode_file(&path)).await {
        Ok(Ok(d)) => d,
        Ok(Err(e)) => return fail(app, item, &friendly_decode_error(&e.to_string())),
        Err(_) => return fail(app, item, "Couldn't read the audio file"),
    };

    let duration_ms =
        (decoded.samples.len() as i64 * 1000) / (decoded.sample_rate.max(1) as i64);

    // Resample to 16 kHz + WAV-encode (the WAV is what Groq uploads; the samples
    // feed a local model directly).
    let (samples16k, wav) = match tauri::async_runtime::spawn_blocking(move || {
        let resampled = audio::resample_to_16k(&decoded.samples, decoded.sample_rate);
        let wav = audio::encode_wav(&resampled)?;
        anyhow::Ok((resampled, wav))
    })
    .await
    {
        Ok(Ok(v)) => v,
        Ok(Err(_)) | Err(_) => return fail(app, item, "Audio processing failed"),
    };

    // Snapshot the settings we need (guard drops before any await).
    let (language, lang_label, level, backend) = {
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
            s.transcription_backend.clone(),
        )
    };

    // Groq's 25 MB cap applies per file. Long files are not chunked yet — error
    // clearly instead of letting the upload fail generically. Local has no cap.
    if backend == "groq" && wav.len() > GROQ_MAX_WAV_BYTES {
        return fail(
            app,
            item,
            "File is too long for cloud transcription (about 13 min max). Switch to a local model for long files.",
        );
    }

    if cancelled(app, item.id) {
        return;
    }

    // Dictionary boost terms (same as the mic path).
    let hints = {
        let conn = db.lock();
        dictionary::hints(&conn, 100).unwrap_or_default()
    };

    emit_progress(app, item.id, &item.file_name, "Transcribing");
    let audio_input = Audio {
        samples: Arc::new(samples16k),
        wav,
    };
    let raw = match transcriber
        .transcribe_audio(audio_input, language, hints)
        .await
    {
        Ok(t) => t,
        Err(e) => return fail(app, item, &friendly_transcribe_error(&e.to_string())),
    };
    if raw.trim().is_empty() {
        return fail(app, item, "No speech found in this file");
    }

    if cancelled(app, item.id) {
        return;
    }

    // Deterministic dictionary corrections + course-correction before polish,
    // mirroring the mic pipeline (snippets/transforms/vibe-coding are
    // injection-context features and don't apply to file items).
    let corrections = {
        let conn = db.lock();
        dictionary::corrections(&conn).unwrap_or_default()
    };
    let dict_corrected = text_processing::apply_corrections(&raw, &corrections);
    let corrected = text_processing::course_correct(&dict_corrected);

    // Polish is optional and time-bounded so a slow model can't wedge the queue.
    const POLISH_TIMEOUT: Duration = Duration::from_secs(30);
    let polished = if matches!(level, CleanupLevel::None) {
        corrected
    } else {
        emit_progress(app, item.id, &item.file_name, "Polishing");
        let fallback = corrected.clone();
        match tokio::time::timeout(POLISH_TIMEOUT, polisher.polish(corrected, level, None)).await {
            Ok(Ok(p)) => p,
            Ok(Err(_)) | Err(_) => fallback,
        }
    };
    let text = text_processing::finalize(&polished);
    if text.trim().is_empty() {
        return fail(app, item, "No speech found in this file");
    }

    if cancelled(app, item.id) {
        return;
    }

    let transcript_id = persist(&db, &raw, &text, level, &lang_label, duration_ms, item);

    let _ = app.emit_to(
        events::MAIN,
        events::QUEUE_DONE,
        events::QueueDonePayload {
            id: item.id,
            file_name: item.file_name.clone(),
            transcript_id: transcript_id.unwrap_or(0),
            text,
        },
    );
}

/// Persist a file transcription to history with its source path. Returns the new
/// row id (best-effort — a failed insert must not break the queue).
fn persist(
    db: &crate::db::Db,
    raw: &str,
    text: &str,
    level: CleanupLevel,
    language: &str,
    duration_ms: i64,
    item: &QueuedFile,
) -> Option<i64> {
    let created_at = chrono::Utc::now().timestamp_millis();
    let word_count = text.split_whitespace().count() as i64;
    let row = queries::NewTranscript {
        created_at,
        raw_text: raw.to_string(),
        polished_text: text.to_string(),
        cleanup_level: level.as_str().to_string(),
        language: language.to_string(),
        audio_path: None,
        // No focused app for a file item; label History with the file name.
        app_process: String::new(),
        app_title: item.file_name.clone(),
        app_category: String::new(),
        word_count,
        duration_ms,
        was_polished: !matches!(level, CleanupLevel::None),
        source_file: Some(item.path.to_string_lossy().into_owned()),
    };
    let conn = db.lock();
    queries::insert_transcript(&conn, &row).ok()
}

/// Emit a stage-progress event for a queue item to the Hub window.
fn emit_progress(app: &AppHandle, id: u64, file_name: &str, stage: &str) {
    let _ = app.emit_to(
        events::MAIN,
        events::QUEUE_PROGRESS,
        events::QueueProgressPayload {
            id,
            file_name: file_name.to_string(),
            stage: stage.to_string(),
        },
    );
}

/// Emit a terminal error event for a queue item.
fn fail(app: &AppHandle, item: &QueuedFile, message: &str) {
    let _ = app.emit_to(
        events::MAIN,
        events::QUEUE_ERROR,
        events::QueueErrorPayload {
            id: item.id,
            file_name: item.file_name.clone(),
            message: message.to_string(),
        },
    );
}

/// Decode an audio file to mono f32 samples at its native rate. Supports the
/// container/codec set symphonia is built with (wav/mp3/m4a/flac/ogg).
fn decode_file(path: &Path) -> anyhow::Result<DecodedAudio> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::errors::Error as SymphoniaError;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow::anyhow!("No decodable audio track"))?;
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())?;

    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(16_000);
    let mut mono: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            // Clean end of stream (symphonia signals EOF as an IoError).
            Err(SymphoniaError::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                sample_rate = spec.rate;
                let channels = spec.channels.count().max(1);
                let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buf.copy_interleaved_ref(decoded);
                let samples = buf.samples();
                if channels == 1 {
                    mono.extend_from_slice(samples);
                } else {
                    for frame in samples.chunks(channels) {
                        let sum: f32 = frame.iter().copied().sum();
                        mono.push(sum / channels as f32);
                    }
                }
            }
            // A recoverable decode error on one packet — skip it, keep going.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }

    if mono.is_empty() {
        anyhow::bail!("No audio decoded from file");
    }
    Ok(DecodedAudio {
        samples: mono,
        sample_rate,
    })
}

fn friendly_decode_error(err: &str) -> String {
    if err.contains("unsupported") || err.contains("No decodable") || err.contains("No audio") {
        "Unsupported or corrupt audio file".into()
    } else {
        "Couldn't decode the audio file".into()
    }
}

fn friendly_transcribe_error(err: &str) -> String {
    if err.contains("API key") {
        "Set your Groq API key in Settings".into()
    } else if err.contains("401") || err.contains("invalid_api_key") {
        "Invalid Groq API key".into()
    } else if err.contains("429") {
        "Rate limited — try again in a moment".into()
    } else if err.contains("not downloaded") || err.contains("No local") {
        err.to_string()
    } else {
        "Transcription failed — check your connection".into()
    }
}
