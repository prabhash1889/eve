//! Transcription providers. `GroqTranscriber` calls the Groq Whisper API;
//! `LocalTranscriber` runs whisper.cpp on-device (behind the `local-models`
//! Cargo feature). `RoutingTranscriber` picks between them per call from the
//! live `Settings`, falling back to Groq when the local backend errors.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::config::Settings;
use crate::secrets;

/// A speech-to-text backend. Takes 16 kHz mono WAV bytes, returns raw text.
#[async_trait]
pub trait Transcriber: Send + Sync {
    async fn transcribe(
        &self,
        wav: Vec<u8>,
        language: Option<String>,
        hints: Vec<String>,
    ) -> anyhow::Result<String>;
}

/// Groq Whisper (`whisper-large-v3-turbo`) over the OpenAI-compatible API.
pub struct GroqTranscriber {
    client: reqwest::Client,
    model: String,
}

impl GroqTranscriber {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            model: "whisper-large-v3-turbo".into(),
        }
    }
}

#[async_trait]
impl Transcriber for GroqTranscriber {
    async fn transcribe(
        &self,
        wav: Vec<u8>,
        language: Option<String>,
        hints: Vec<String>,
    ) -> anyhow::Result<String> {
        let key = secrets::get_api_key()
            .map_err(|_| anyhow::anyhow!("Set your Groq API key in Settings"))?;

        let part = reqwest::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let mut form = reqwest::multipart::Form::new()
            .text("model", self.model.clone())
            .text("response_format", "json")
            .text("temperature", "0")
            .part("file", part);

        if let Some(lang) = language {
            form = form.text("language", lang);
        }
        if !hints.is_empty() {
            // Whisper uses `prompt` as a soft vocabulary hint (dictionary terms).
            form = form.text("prompt", hints.join(", "));
        }

        let resp = self
            .client
            .post("https://api.groq.com/openai/v1/audio/transcriptions")
            .bearer_auth(key)
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Groq error {}: {}", status, body);
        }

        let value: serde_json::Value = resp.json().await?;
        let text = value
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();
        Ok(text)
    }
}

/// On-device Whisper (whisper.cpp via `whisper-rs`). The selected model id and
/// language come from `Settings`; the GGML weights live under `models_dir`. The
/// loaded context is cached and reused, reloading only when the selection
/// changes. Real inference is compiled only with the `local-models` feature.
pub struct LocalTranscriber {
    models_dir: PathBuf,
    settings: Arc<Mutex<Settings>>,
    #[cfg(feature = "local-models")]
    cache: Mutex<Option<(String, Arc<whisper_rs::WhisperContext>)>>,
}

impl LocalTranscriber {
    pub fn new(models_dir: PathBuf, settings: Arc<Mutex<Settings>>) -> Self {
        Self {
            models_dir,
            settings,
            #[cfg(feature = "local-models")]
            cache: Mutex::new(None),
        }
    }

    /// Resolve the selected model id to an on-disk path, erroring with a
    /// user-facing message when nothing is selected or the file is missing.
    #[cfg(feature = "local-models")]
    fn resolve(&self) -> anyhow::Result<(String, PathBuf)> {
        let id = self.settings.lock().local_whisper_model.clone();
        if id.is_empty() {
            anyhow::bail!("No local speech model selected — pick one in Models");
        }
        let info = crate::models::find(&id)
            .ok_or_else(|| anyhow::anyhow!("Unknown local model: {id}"))?;
        let path = self.models_dir.join(info.file_name);
        if !path.exists() {
            anyhow::bail!("Model '{}' is not downloaded yet", info.name);
        }
        Ok((id, path))
    }
}

#[async_trait]
impl Transcriber for LocalTranscriber {
    #[cfg(not(feature = "local-models"))]
    async fn transcribe(
        &self,
        _wav: Vec<u8>,
        _language: Option<String>,
        _hints: Vec<String>,
    ) -> anyhow::Result<String> {
        // Touch fields so they don't read as dead when the feature is off.
        let _ = (&self.models_dir, &self.settings);
        anyhow::bail!("Local transcription was not built in (enable the `local-models` feature)")
    }

    #[cfg(feature = "local-models")]
    async fn transcribe(
        &self,
        wav: Vec<u8>,
        language: Option<String>,
        hints: Vec<String>,
    ) -> anyhow::Result<String> {
        use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

        let (id, path) = self.resolve()?;

        // Load (or reuse the cached) context for the selected model.
        let ctx = {
            let mut cache = self.cache.lock();
            match cache.as_ref() {
                Some((cached_id, ctx)) if *cached_id == id => ctx.clone(),
                _ => {
                    let path_str = path.to_string_lossy().to_string();
                    let loaded = WhisperContext::new_with_params(
                        &path_str,
                        WhisperContextParameters::default(),
                    )
                    .map_err(|e| anyhow::anyhow!("Failed to load Whisper model: {e}"))?;
                    let ctx = Arc::new(loaded);
                    *cache = Some((id.clone(), ctx.clone()));
                    ctx
                }
            }
        };

        // Decode our own 16 kHz mono i16 WAV bytes back to f32 samples.
        let samples = decode_wav_f32(&wav)?;

        // whisper.cpp inference is sync + CPU-heavy: run it off the async runtime.
        let prompt = if hints.is_empty() { None } else { Some(hints.join(", ")) };
        tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<String> {
            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            if let Some(lang) = language.as_deref() {
                params.set_language(Some(lang));
            }
            if let Some(p) = prompt.as_deref() {
                params.set_initial_prompt(p);
            }

            let mut state = ctx
                .create_state()
                .map_err(|e| anyhow::anyhow!("Whisper state error: {e}"))?;
            state
                .full(params, &samples)
                .map_err(|e| anyhow::anyhow!("Whisper inference failed: {e}"))?;

            let n = state.full_n_segments();
            let mut out = String::new();
            for i in 0..n {
                if let Some(seg) = state.get_segment(i) {
                    if let Ok(text) = seg.to_str() {
                        out.push_str(text);
                    }
                }
            }
            Ok(out.trim().to_string())
        })
        .await
        .map_err(|e| anyhow::anyhow!("Transcription task failed: {e}"))?
    }
}

/// Decode a 16-bit PCM WAV (our `audio::encode_wav` output) into normalized f32
/// samples in [-1, 1] for whisper.cpp.
#[cfg(feature = "local-models")]
fn decode_wav_f32(wav: &[u8]) -> anyhow::Result<Vec<f32>> {
    let cursor = std::io::Cursor::new(wav);
    let mut reader = hound::WavReader::new(cursor)
        .map_err(|e| anyhow::anyhow!("Bad WAV data: {e}"))?;
    let samples = reader
        .samples::<i16>()
        .map(|s| s.map(|v| v as f32 / 32768.0))
        .collect::<Result<Vec<f32>, _>>()
        .map_err(|e| anyhow::anyhow!("WAV decode error: {e}"))?;
    Ok(samples)
}

/// Routes each call to the Groq or local backend per the live `Settings`, with
/// automatic fallback to Groq when the local backend errors and a key exists.
pub struct RoutingTranscriber {
    groq: GroqTranscriber,
    local: LocalTranscriber,
    settings: Arc<Mutex<Settings>>,
}

impl RoutingTranscriber {
    pub fn new(models_dir: PathBuf, settings: Arc<Mutex<Settings>>) -> Self {
        Self {
            groq: GroqTranscriber::new(),
            local: LocalTranscriber::new(models_dir, settings.clone()),
            settings,
        }
    }
}

#[async_trait]
impl Transcriber for RoutingTranscriber {
    async fn transcribe(
        &self,
        wav: Vec<u8>,
        language: Option<String>,
        hints: Vec<String>,
    ) -> anyhow::Result<String> {
        let use_local = self.settings.lock().transcription_backend == "local";
        if use_local {
            match self
                .local
                .transcribe(wav.clone(), language.clone(), hints.clone())
                .await
            {
                Ok(text) => return Ok(text),
                Err(e) if secrets::has_api_key() => {
                    eprintln!("Local transcription failed ({e}); falling back to Groq");
                }
                Err(e) => return Err(e),
            }
        }
        self.groq.transcribe(wav, language, hints).await
    }
}
