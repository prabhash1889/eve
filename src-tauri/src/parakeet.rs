//! On-device NVIDIA Parakeet transcription (FastConformer-TDT over ONNX
//! Runtime, behind the `local-parakeet` Cargo feature). A sibling of
//! `transcription::LocalTranscriber`: `RoutingTranscriber` picks this backend
//! when the selected local speech model is a Parakeet catalog id. The whisper
//! path is never touched.
//!
//! Parakeet differences the router/pipeline should know about:
//! - English-only (v2) and no translate mode — `translate_to_english` errors
//!   here so routing falls back to Groq.
//! - No prompt/vocabulary biasing: `whisper_prompt` and dictionary hints are
//!   ignored.
//! - Punctuation and capitalization come from the model itself.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::config::Settings;
use crate::transcription::Transcriber;
#[cfg(feature = "local-parakeet")]
use crate::transcription::{Audio, WhisperStatus};

pub fn backend_label() -> &'static str {
    if cfg!(feature = "local-parakeet") {
        "Parakeet ONNX (CPU)"
    } else {
        "local parakeet unavailable"
    }
}

/// On-device Parakeet TDT. The model directory (encoder/decoder ONNX + vocab)
/// lives under `models_dir/<catalog id>/`; the loaded sessions are cached and
/// reused, reloading only when the selected id changes. Real inference is
/// compiled only with the `local-parakeet` feature.
pub struct LocalParakeetTranscriber {
    models_dir: PathBuf,
    /// Read-only fallback root (the app's bundled `resources/models` dir in the
    /// Store edition). Used only when the model isn't present under `models_dir`,
    /// so a bundled Parakeet works offline with no download and no disk copy.
    bundled_dir: Option<PathBuf>,
    settings: Arc<Mutex<Settings>>,
    /// (catalog id, loaded model). `parakeet_rs` needs `&mut` to transcribe, so
    /// the model sits behind its own Mutex, locked only inside `spawn_blocking`.
    #[cfg(feature = "local-parakeet")]
    cache: Mutex<Option<(String, Arc<Mutex<parakeet_rs::ParakeetTDT>>)>>,
    #[cfg(feature = "local-parakeet")]
    load_gate: tokio::sync::Mutex<()>,
    #[cfg(feature = "local-parakeet")]
    load_state: Mutex<LoadState>,
}

#[cfg(feature = "local-parakeet")]
#[derive(Default, Clone)]
struct LoadState {
    loading: bool,
    last_load_ms: Option<u64>,
    last_transcribe_ms: Option<u64>,
}

impl LocalParakeetTranscriber {
    pub fn new(
        models_dir: PathBuf,
        bundled_dir: Option<PathBuf>,
        settings: Arc<Mutex<Settings>>,
    ) -> Self {
        Self {
            models_dir,
            bundled_dir,
            settings,
            #[cfg(feature = "local-parakeet")]
            cache: Mutex::new(None),
            #[cfg(feature = "local-parakeet")]
            load_gate: tokio::sync::Mutex::new(()),
            #[cfg(feature = "local-parakeet")]
            load_state: Mutex::new(LoadState::default()),
        }
    }

    /// Drop the cached model, freeing its memory. The next use cold-loads again.
    /// No-op if nothing is loaded or the feature is off.
    pub fn unload(&self) {
        #[cfg(feature = "local-parakeet")]
        {
            *self.cache.lock() = None;
        }
    }

    /// Resolve the selected model id to its on-disk directory, erroring with a
    /// user-facing message when it isn't downloaded.
    #[cfg(feature = "local-parakeet")]
    fn resolve(&self) -> anyhow::Result<(String, PathBuf)> {
        let id = self.settings.lock().local_whisper_model.clone();
        let info = crate::models::find(&id)
            .ok_or_else(|| anyhow::anyhow!("Unknown local model: {id}"))?;
        // The primary file is enough as an existence probe; the downloader only
        // renames files into place after the full set completes. Prefer the
        // downloaded copy under `models_dir`; fall back to the bundled resource
        // dir (Store edition) so a shipped Parakeet loads with no download.
        for root in std::iter::once(&self.models_dir).chain(self.bundled_dir.iter()) {
            if root.join(info.file_name).exists() {
                let dir = root.join(&id);
                return Ok((id, dir));
            }
        }
        anyhow::bail!("Model '{}' is not downloaded yet", info.name);
    }

    /// Return the cached model for the selected id, cold-loading it off the
    /// async runtime if needed. Same double-checked shape as the whisper path:
    /// no lock is held across the heavy load.
    #[cfg(feature = "local-parakeet")]
    async fn ensure_model(&self) -> anyhow::Result<Arc<Mutex<parakeet_rs::ParakeetTDT>>> {
        let (id, dir) = self.resolve()?;

        let cached = {
            let cache = self.cache.lock();
            match cache.as_ref() {
                Some((cached_id, m)) if *cached_id == id => Some(m.clone()),
                _ => None,
            }
        };
        if let Some(m) = cached {
            return Ok(m);
        }

        let _load_gate = self.load_gate.lock().await;

        let cached = {
            let cache = self.cache.lock();
            match cache.as_ref() {
                Some((cached_id, m)) if *cached_id == id => Some(m.clone()),
                _ => None,
            }
        };
        if let Some(m) = cached {
            return Ok(m);
        }

        self.load_state.lock().loading = true;
        let t0 = std::time::Instant::now();
        let loaded = tauri::async_runtime::spawn_blocking(move || {
            parakeet_rs::ParakeetTDT::from_pretrained(&dir, None)
                .map_err(|e| anyhow::anyhow!("Failed to load Parakeet model: {e}"))
        })
        .await;
        let load_ms = t0.elapsed().as_millis() as u64;

        let model = match loaded {
            Ok(Ok(m)) => Arc::new(Mutex::new(m)),
            Ok(Err(e)) => {
                self.load_state.lock().loading = false;
                return Err(e);
            }
            Err(e) => {
                self.load_state.lock().loading = false;
                return Err(anyhow::anyhow!("Model load task failed: {e}"));
            }
        };

        *self.cache.lock() = Some((id, model.clone()));
        {
            let mut st = self.load_state.lock();
            st.loading = false;
            st.last_load_ms = Some(load_ms);
        }
        Ok(model)
    }

    /// Run inference off the async runtime, recording wall-clock cost for the
    /// UI status panel. Language and hints are ignored — Parakeet v2 is
    /// English-only and has no prompt biasing.
    #[cfg(feature = "local-parakeet")]
    async fn run_timed(&self, samples: Arc<Vec<f32>>) -> anyhow::Result<String> {
        if self.settings.lock().translate_to_english {
            anyhow::bail!("Parakeet cannot translate to English — use Groq or a Whisper model");
        }
        let model = self.ensure_model().await?;
        let t0 = std::time::Instant::now();
        let out = tauri::async_runtime::spawn_blocking(move || {
            use parakeet_rs::Transcriber as _;
            let mut m = model.lock();
            m.transcribe_samples(samples.as_ref().clone(), 16_000, 1, None)
                .map(|r| r.text)
                .map_err(|e| anyhow::anyhow!("Parakeet inference failed: {e}"))
        })
        .await
        .map_err(|e| anyhow::anyhow!("Transcription task failed: {e}"))?;
        self.load_state.lock().last_transcribe_ms = Some(t0.elapsed().as_millis() as u64);
        out.map(|t| t.trim().to_string())
    }
}

#[async_trait]
impl Transcriber for LocalParakeetTranscriber {
    #[cfg(not(feature = "local-parakeet"))]
    async fn transcribe(
        &self,
        _wav: Vec<u8>,
        _language: Option<String>,
        _hints: Vec<String>,
    ) -> anyhow::Result<String> {
        // Touch fields so they don't read as dead when the feature is off.
        let _ = (&self.models_dir, &self.bundled_dir, &self.settings);
        anyhow::bail!(
            "Local Parakeet was not built in (enable the `local-parakeet` feature)"
        )
    }

    #[cfg(feature = "local-parakeet")]
    async fn transcribe(
        &self,
        wav: Vec<u8>,
        _language: Option<String>,
        _hints: Vec<String>,
    ) -> anyhow::Result<String> {
        // WAV path kept for direct callers (history replay); the live pipeline
        // uses `transcribe_audio` and skips this decode.
        let samples = Arc::new(decode_wav_f32(&wav)?);
        self.run_timed(samples).await
    }

    #[cfg(feature = "local-parakeet")]
    async fn transcribe_audio(
        &self,
        audio: Audio,
        _language: Option<String>,
        _hints: Vec<String>,
    ) -> anyhow::Result<String> {
        self.run_timed(audio.samples).await
    }

    #[cfg(feature = "local-parakeet")]
    async fn prewarm(&self) -> anyhow::Result<()> {
        self.ensure_model().await.map(|_| ())
    }

    #[cfg(feature = "local-parakeet")]
    fn whisper_status(&self) -> Option<WhisperStatus> {
        let model = self.settings.lock().local_whisper_model.clone();
        let ready = !model.is_empty()
            && self
                .cache
                .lock()
                .as_ref()
                .map(|(id, _)| *id == model)
                .unwrap_or(false);
        let st = self.load_state.lock().clone();
        Some(WhisperStatus {
            model,
            loading: st.loading,
            ready,
            last_load_ms: st.last_load_ms,
            last_transcribe_ms: st.last_transcribe_ms,
            backend: backend_label().to_string(),
        })
    }
}

/// Decode a 16-bit PCM WAV (our `audio::encode_wav` output) into normalized f32
/// samples. Duplicated from the whisper path rather than shared — the two are
/// behind different Cargo features and the helper is ten lines.
#[cfg(feature = "local-parakeet")]
fn decode_wav_f32(wav: &[u8]) -> anyhow::Result<Vec<f32>> {
    let cursor = std::io::Cursor::new(wav);
    let mut reader =
        hound::WavReader::new(cursor).map_err(|e| anyhow::anyhow!("Bad WAV data: {e}"))?;
    let samples = reader
        .samples::<i16>()
        .map(|s| s.map(|v| v as f32 / 32768.0))
        .collect::<Result<Vec<f32>, _>>()
        .map_err(|e| anyhow::anyhow!("WAV decode error: {e}"))?;
    Ok(samples)
}
