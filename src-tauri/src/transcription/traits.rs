use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;

/// Resampled 16 kHz mono audio in both forms the backends need: raw f32 samples
/// (the local path feeds these straight into whisper.cpp, avoiding a WAV
/// encode→decode round-trip) and the pre-encoded WAV (cloud upload + history).
/// The pipeline builds this once after resampling; `samples` is `Arc`-shared so
/// routing can hand the local backend a cheap clone while keeping the WAV for a
/// Groq fallback.
pub struct Audio {
    pub samples: Arc<Vec<f32>>,
    pub wav: Vec<u8>,
}

/// Phase 2: readiness of the selected local Whisper model, surfaced to the UI so
/// it can show whether the model is loaded (and how long the last load took).
/// Always defined — the fields are only ever populated under `local-whisper`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperStatus {
    /// Catalog id selected in Settings ("" when none).
    pub model: String,
    /// A cold model load is currently in flight.
    pub loading: bool,
    /// The selected model is loaded and cached, ready for instant inference.
    pub ready: bool,
    /// Wall-clock cost of the last cold load, for the status panel.
    pub last_load_ms: Option<u64>,
    /// Phase 4: wall-clock cost of the last local transcription (inference only),
    /// for the status panel.
    pub last_transcribe_ms: Option<u64>,
    /// Build/runtime label for the local backend.
    pub backend: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionBenchmark {
    pub mode: String,
    pub model: String,
    pub profile: String,
    pub backend: String,
    pub clip_duration_ms: u64,
    pub transcribe_ms: u64,
    pub words_produced: usize,
    pub vad_trimmed: bool,
}

/// A speech-to-text backend. Takes 16 kHz mono WAV bytes, returns raw text.
#[async_trait]
pub trait Transcriber: Send + Sync {
    async fn transcribe(
        &self,
        wav: Vec<u8>,
        language: Option<String>,
        hints: Vec<String>,
    ) -> anyhow::Result<String>;

    /// Phase 2: sample-based path. Transcribe already-resampled 16 kHz mono f32
    /// samples without a WAV round-trip. The default encodes nothing extra — it
    /// reuses the pre-encoded WAV and defers to `transcribe`, so cloud backends
    /// are unaffected; the local backend overrides this to feed whisper.cpp the
    /// samples directly.
    async fn transcribe_audio(
        &self,
        audio: Audio,
        language: Option<String>,
        hints: Vec<String>,
    ) -> anyhow::Result<String> {
        self.transcribe(audio.wav, language, hints).await
    }

    /// Phase 2: preload the selected local model so the first dictation after a
    /// launch / model switch isn't slowed by a cold load. No-op for cloud.
    async fn prewarm(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Free any local model this backend has loaded that isn't the active
    /// selection (releasing its memory - VRAM on the CUDA build), then prewarm
    /// the one that is when `prewarm` is set. Called after the speech backend or
    /// selected model changes so an unused model doesn't keep occupying the GPU.
    /// No-op for cloud backends.
    async fn reconcile(&self, _prewarm: bool) {}

    /// Phase 2: local Whisper readiness for the UI. `None` for cloud-only backends.
    fn whisper_status(&self) -> Option<WhisperStatus> {
        None
    }
}
