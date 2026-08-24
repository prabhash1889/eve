//! Session-level building blocks shared by the three dictation-like flows
//! (`pipeline::process`, `command_mode::process_command`,
//! `file_transcribe::process_one`): the stop-and-drain handshake, audio
//! preparation (resample / WAV / local VAD), benchmarked speech-to-text,
//! polish bounded by a timeout with a degrade-to-raw fallback, and the
//! user-facing error mapping in `errors`.
//!
//! Extracted in phase 5.1. Prime directive for this module: every helper
//! preserves the observable behavior and call order of the flow it was
//! extracted from - the stage-timing marks (drain, resample_encode,
//! preprocess, transcribe, polish, inject), the emitted events, and every
//! user-facing error string are frozen by tests.

pub mod errors;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};

use crate::audio;
use crate::config::CleanupLevel;
use crate::events;
use crate::polish::{Polisher, StyleHint};
use crate::transcription::{Audio, Transcriber, TranscriptionBenchmark};

/// Clips shorter than this are rejected before any processing with
/// `PrepareError::TooShort`. Mic dictation and Command Mode share the
/// threshold; file transcription passes `None` (no minimum-length check).
pub const MIN_DURATION_MS: i64 = 1000;

/// Unified bound on the polish round-trip for every session flow. Dictation
/// previously used 20s and the file queue 30s; 20s won because latency-to-text
/// matters more than salvaging polish - a long file whose polish stalls now
/// degrades to its course-corrected raw transcript after 20s instead of
/// holding the queue hostage.
pub const POLISH_TIMEOUT: Duration = Duration::from_secs(20);

// --- Stop handshake + drain ---------------------------------------------------

/// Raw capture output after [`stop_and_drain`]: the drained buffer and the
/// device sample rate it was captured at.
pub struct DrainedAudio {
    pub samples: Vec<f32>,
    pub rate: u32,
}

/// Deterministic stop handshake plus buffer drain, shared by mic-driven flows:
/// wait (off the async runtime) for the capture thread to ack that the stream
/// is dropped and the final samples are flushed into the shared buffer, then
/// take the buffer and read the sample rate. Typically returns in ~0-33 ms
/// (one poll tick); on timeout the helper falls back to waiting at most the
/// full `STOP_ACK_TIMEOUT` (60 ms), exactly the previous fixed-sleep behavior.
pub async fn stop_and_drain(
    capture: &audio::CaptureHandle,
    buffer: Arc<Mutex<Vec<f32>>>,
    sample_rate: Arc<AtomicU32>,
) -> DrainedAudio {
    let capture = capture.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        capture.stop_and_wait(audio::STOP_ACK_TIMEOUT);
    })
    .await;

    let samples = {
        let mut b = buffer.lock();
        std::mem::take(&mut *b)
    };
    let rate = sample_rate.load(Ordering::SeqCst);
    DrainedAudio { samples, rate }
}

// --- Audio preparation --------------------------------------------------------

/// Per-caller preparation behavior for [`prepare_audio`]. The two mic flows
/// disagree on WHEN the WAV is encoded relative to VAD trimming, and both
/// orderings are load-bearing, so the ordering is explicit here rather than
/// hidden:
///
/// - Dictation (`encode_before_vad = true`): cloud providers upload the FULL
///   clip; only the f32 samples handed to an on-device backend are trimmed.
/// - Command Mode (`encode_before_vad = false`): the post-VAD WAV is what a
///   potential cloud fallback uploads.
#[derive(Clone, Copy)]
pub struct PrepareOptions {
    /// Encode the WAV from the pre-VAD resampled clip (dictation order) instead
    /// of from the trimmed clip (Command Mode order).
    pub encode_before_vad: bool,
    /// When false the WAV is not encoded at all and comes back empty - mirrors
    /// `audio::prepare_16k`'s `need_wav` skip when no consumer exists.
    pub need_wav: bool,
    /// Some(params) runs the local silence-trim + peak-normalization pass over
    /// the resampled samples; None leaves them untouched (cloud path).
    pub vad: Option<audio::VadParams>,
    /// Reject clips shorter than this many milliseconds with
    /// `PrepareError::TooShort`; None performs no length check.
    pub min_duration_ms: Option<i64>,
}

/// Why [`prepare_audio`] refused a clip. Callers map each variant to their own
/// exact UI payload (mic flows emit Flow Bar ERROR/hide pairs; the file queue
/// emits queue://error), so the variants must stay distinguishable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepareError {
    /// Clip shorter than `min_duration_ms`.
    TooShort,
    /// Resampling or WAV encoding failed (or the blocking task panicked).
    AudioFailed,
    /// Local VAD found no speech in the clip.
    NoSpeech,
}

/// Fully prepared session audio: 16 kHz mono samples (optionally VAD-trimmed),
/// the encoded WAV per [`PrepareOptions`], and the clip duration computed from
/// the INPUT samples before resampling (the number every caller persisted).
pub struct PreparedAudio {
    pub samples: Arc<Vec<f32>>,
    pub wav: Vec<u8>,
    pub duration_ms: i64,
    pub vad_trimmed: bool,
}

/// Resample + optional WAV encode + optional local VAD inside ONE
/// `spawn_blocking`. See [`PrepareOptions`] for the two supported
/// wav-vs-vad orderings; both produce byte-identical output to the code each
/// flow ran before the extraction.
pub async fn prepare_audio(
    samples: Vec<f32>,
    src_rate: u32,
    opts: PrepareOptions,
) -> Result<PreparedAudio, PrepareError> {
    // Duration is computed from the input samples BEFORE resampling, exactly as
    // every caller did prior to the extraction.
    let duration_ms = (samples.len() as i64 * 1000) / (src_rate.max(1) as i64);
    if let Some(min_ms) = opts.min_duration_ms {
        if duration_ms < min_ms {
            return Err(PrepareError::TooShort);
        }
    }

    let vad = opts.vad;
    let need_wav = opts.need_wav;
    let encode_before_vad = opts.encode_before_vad;

    match tauri::async_runtime::spawn_blocking(move || {
        let mut vad_trimmed = false;
        let (samples_out, wav);

        if encode_before_vad {
            // Dictation / file-queue order: reuse the frozen
            // `audio::prepare_16k` (resample + optional WAV of the FULL clip),
            // then trim only the f32 samples handed to a local backend.
            let (resampled, built_wav) =
                audio::prepare_16k(samples, src_rate, need_wav)
                    .map_err(|_| PrepareError::AudioFailed)?;
            if let Some(params) = vad {
                let pre = audio::preprocess_local(&resampled, params);
                if !pre.speech_detected {
                    return Err(PrepareError::NoSpeech);
                }
                vad_trimmed = pre.trimmed;
                (samples_out, wav) = (pre.samples, built_wav);
            } else {
                (samples_out, wav) = (resampled, built_wav);
            }
        } else {
            // Command Mode order: resample, trim FIRST, then encode the
            // trimmed clip (mirrors its former inline closure).
            let mut resampled = if src_rate == 16_000 || samples.is_empty() {
                samples
            } else {
                let out = audio::resample_to_16k(&samples, src_rate);
                drop(samples);
                out
            };
            if let Some(params) = vad {
                let pre = audio::preprocess_local(&resampled, params);
                if !pre.speech_detected {
                    return Err(PrepareError::NoSpeech);
                }
                vad_trimmed = pre.trimmed;
                resampled = pre.samples;
            }
            let built_wav = if need_wav {
                audio::encode_wav(&resampled).map_err(|_| PrepareError::AudioFailed)?
            } else {
                Vec::new()
            };
            (samples_out, wav) = (resampled, built_wav);
        }

        Ok(PreparedAudio {
            samples: Arc::new(samples_out),
            wav,
            duration_ms,
            vad_trimmed,
        })
    })
    .await
    {
        Ok(Ok(prepared)) => Ok(prepared),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(PrepareError::AudioFailed),
    }
}

// --- Benchmarked speech-to-text ----------------------------------------------

/// Caller-resolved benchmark metadata for [`run_stt_benchmarked`]. Each flow
/// derives the fields exactly as it did before the extraction (dictation names
/// the local engine or provider label; Command Mode additionally defaults an
/// empty model id to "whisper-large-v3-turbo").
pub struct SttMeta {
    pub mode: &'static str,
    pub model: String,
    pub backend: String,
    pub profile: String,
    pub clip_duration_ms: u64,
    pub vad_trimmed: bool,
}

/// Transcribe with wall-clock measurement, producing the same
/// `TranscriptionBenchmark` row the caller used to assemble inline.
pub async fn run_stt_benchmarked(
    transcriber: &dyn Transcriber,
    audio_input: Audio,
    language: Option<String>,
    hints: Vec<String>,
    meta: SttMeta,
) -> anyhow::Result<(String, TranscriptionBenchmark)> {
    let started = std::time::Instant::now();
    let text = transcriber
        .transcribe_audio(audio_input, language, hints)
        .await?;
    let transcribe_ms = started.elapsed().as_millis() as u64;
    let words_produced = text.split_whitespace().count();
    Ok((
        text,
        TranscriptionBenchmark {
            mode: meta.mode.to_string(),
            model: meta.model,
            profile: meta.profile,
            backend: meta.backend,
            clip_duration_ms: meta.clip_duration_ms,
            transcribe_ms,
            words_produced,
            vad_trimmed: meta.vad_trimmed,
        },
    ))
}

// --- Bounded polish -----------------------------------------------------------

/// Run one polish round-trip bounded by `timeout`, degrading to the input text
/// on timeout or error instead of stalling the session. On either failure arm
/// this emits `events::DEGRADED` ("Polish unavailable - using raw") so the Flow
/// Bar can hint that polish was skipped (phase 5.6); the timeout arm also keeps
/// the historical stderr line. Returns the polished-or-fallback text plus a
/// degraded flag for callers that want to react further.
///
/// Latency note: see [`POLISH_TIMEOUT`] for why every flow shares the single
/// 20s bound.
pub async fn run_polish_bounded(
    app: &AppHandle,
    polisher: &dyn Polisher,
    text: String,
    level: CleanupLevel,
    style_hint: Option<StyleHint>,
    timeout: Duration,
) -> (String, bool) {
    let fallback = text.clone();
    match tokio::time::timeout(timeout, polisher.polish(text, level, style_hint)).await {
        Ok(Ok(p)) => (p, false),
        Ok(Err(_)) => {
            let _ = app.emit_to(
                events::FLOWBAR,
                events::DEGRADED,
                events::StagePayload {
                    label: "Polish unavailable - using raw".to_string(),
                },
            );
            (fallback, true)
        }
        Err(_) => {
            eprintln!("[polish] timed out after {timeout:?}; injecting raw transcript");
            let _ = app.emit_to(
                events::FLOWBAR,
                events::DEGRADED,
                events::StagePayload {
                    label: "Polish unavailable - using raw".to_string(),
                },
            );
            (fallback, true)
        }
    }
}
