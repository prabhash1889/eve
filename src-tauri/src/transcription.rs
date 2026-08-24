//! Transcription providers. Cloud speech-to-text goes through
//! `CloudTranscriber`, which resolves a provider (Groq / OpenAI share the
//! OpenAI-compatible multipart API; Deepgram speaks its own REST shape) from
//! live `Settings` per call. `LocalTranscriber` runs whisper.cpp on-device
//! (behind the `local-models` Cargo feature). `RoutingTranscriber` picks
//! between them per call from the live `Settings`, falling back across the
//! configured cloud chain when the selected backend errors.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::Serialize;

use crate::config::Settings;
use crate::{llm, secrets};
use secrets::ProviderKey;

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

/// Cloud STT providers Eve can route speech-to-text through (Phase 3
/// providers B). Groq and OpenAI share the OpenAI-compatible multipart
/// transcription API and `{text}` response; Deepgram speaks its own REST shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudStt {
    Groq,
    OpenAi,
    Deepgram,
}

impl CloudStt {
    /// Parse the wire form stored in `Settings.transcription_provider`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "groq" => Some(CloudStt::Groq),
            "openai" => Some(CloudStt::OpenAi),
            "deepgram" => Some(CloudStt::Deepgram),
            _ => None,
        }
    }

    /// Human label used in errors and benchmark rows ("OpenAI").
    pub fn label(self) -> &'static str {
        match self {
            CloudStt::Groq => "Groq",
            CloudStt::OpenAi => "OpenAI",
            CloudStt::Deepgram => "Deepgram",
        }
    }

    /// Which keychain slot holds this provider's credential.
    pub fn key_slot(self) -> ProviderKey {
        match self {
            CloudStt::Groq => ProviderKey::Groq,
            CloudStt::OpenAi => ProviderKey::OpenAi,
            CloudStt::Deepgram => ProviderKey::Deepgram,
        }
    }

    /// Default STT model used when `transcription_cloud_model` is empty.
    pub fn default_model(self) -> &'static str {
        match self {
            CloudStt::Groq => "whisper-large-v3-turbo",
            CloudStt::OpenAi => "whisper-1",
            CloudStt::Deepgram => "nova-3",
        }
    }

    /// Model forced when translate-to-English is on (the translations endpoint
    /// has a smaller compatible set than transcriptions). OpenAI's whisper-1
    /// serves both endpoints, so no override is needed there.
    fn translate_model(self) -> &'static str {
        match self {
            CloudStt::Groq => "whisper-large-v3",
            CloudStt::OpenAi => "whisper-1",
            CloudStt::Deepgram => self.default_model(),
        }
    }

    /// Whether the provider can translate audio to English. Deepgram's listen
    /// endpoint can't; a user with translation enabled gets a clear error
    /// instead of silently ignored audio.
    pub fn supports_translate(self) -> bool {
        !matches!(self, CloudStt::Deepgram)
    }

    /// Whether the provider honors Whisper-style vocabulary hints. Deepgram
    /// takes per-word `keyword` params instead of a free-form prompt, so the
    /// configured `whisper_prompt` degrades there (surfaced in UI copy).
    pub fn supports_prompt_hints(self) -> bool {
        !matches!(self, CloudStt::Deepgram)
    }

    /// Upload cap for the encoded WAV, if the provider enforces one. Both
    /// OpenAI-compatible providers reject over 25 MB (~13 min of 16 kHz mono);
    /// Deepgram accepts much larger bodies so no cap applies.
    pub fn max_wav_bytes(self) -> Option<usize> {
        match self {
            CloudStt::Groq | CloudStt::OpenAi => Some(25 * 1024 * 1024),
            CloudStt::Deepgram => None,
        }
    }
}

pub fn local_backend_label() -> &'static str {
    if cfg!(feature = "local-whisper-cuda") {
        "whisper.cpp CUDA"
    } else if cfg!(feature = "local-whisper") {
        "whisper.cpp CPU"
    } else {
        "local whisper unavailable"
    }
}

/// True when the encoded WAV will actually be consumed for a session with these
/// settings (1.P7a). `RoutingTranscriber::transcribe_audio` uploads it directly
/// for a cloud backend; a local backend receives an empty WAV and only needs the
/// real bytes when falling back to a cloud provider - which requires a key. When
/// this returns false the router discards the WAV, so callers can skip
/// `audio::encode_wav` entirely.
pub fn wav_needed(settings: &Settings) -> bool {
    match resolve_speech(settings) {
        SpeechBackend::Cloud(_) => true,
        SpeechBackend::Local => {
            // Only worth encoding if some cloud fallback could consume it.
            match CloudStt::parse(settings.fallback_transcription_provider.trim()) {
                Some(fb) => secrets::has_provider_key_fail_open(fb.key_slot()),
                // Legacy default: Groq is the implicit local-failure fallback.
                None => secrets::has_api_key_fail_open(),
            }
        }
    }
}

/// Upload cap for the effective speech provider, if it enforces one. The mic
/// pipeline and file queue pre-check the encoded WAV against this so an
/// over-length clip fails with a clear message instead of a generic error.
pub fn max_wav_bytes_for(settings: &Settings) -> Option<usize> {
    match resolve_speech(settings) {
        SpeechBackend::Cloud(t) => t.provider.max_wav_bytes(),
        SpeechBackend::Local => None,
    }
}

/// A resolved cloud STT target: which provider and which model id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudSpeechTarget {
    pub provider: CloudStt,
    pub model: String,
}

impl CloudSpeechTarget {
    fn with_default_model(provider: CloudStt) -> Self {
        CloudSpeechTarget {
            provider,
            model: provider.default_model().to_string(),
        }
    }
}

impl SpeechBackend {
    /// Label for benchmark/status rows: the cloud provider name, or "local".
    pub fn label(&self) -> &str {
        match self {
            SpeechBackend::Cloud(t) => t.provider.label(),
            SpeechBackend::Local => "local",
        }
    }
}

/// The effective speech backend for these settings: a cloud target or the
/// on-device engine (whisper.cpp / Parakeet, selected by model id).
#[derive(Debug, Clone, PartialEq)]
pub enum SpeechBackend {
    Cloud(CloudSpeechTarget),
    Local,
}

/// Resolve the configured speech backend from live settings. An empty
/// `transcription_provider` means a legacy install: fall back to
/// `transcription_backend` ("local" -> local, anything else -> Groq) so
/// existing settings keep working unchanged. An unknown provider string falls
/// back to Groq. An empty cloud model picks the provider default. Read per call
/// so settings changes hot-swap immediately (same pattern as `llm::resolve_chat`).
pub fn resolve_speech(s: &Settings) -> SpeechBackend {
    let raw = if s.transcription_provider.is_empty() {
        if s.transcription_backend == "local" {
            "local".to_string()
        } else {
            s.transcription_backend.clone()
        }
    } else {
        s.transcription_provider.clone()
    };
    if raw.trim() == "local" {
        return SpeechBackend::Local;
    }
    let provider = CloudStt::parse(raw.trim()).unwrap_or(CloudStt::Groq);
    let configured = s.transcription_cloud_model.trim();
    let model = if configured.is_empty() {
        provider.default_model().to_string()
    } else {
        configured.to_string()
    };
    SpeechBackend::Cloud(CloudSpeechTarget { provider, model })
}

/// Ordered cloud fallback chain for these settings: the primary cloud target
/// (when one is selected), then the configured fallback provider (only when its
/// key exists - falling back to an unconfigured provider would just trade one
/// error for a vaguer one). A local primary with no explicit fallback keeps the
/// pre-Phase-3 behavior: Groq as the implicit last resort when its key exists.
pub fn cloud_chain(s: &Settings) -> Vec<CloudSpeechTarget> {
    let choice = resolve_speech(s);
    let mut chain = match &choice {
        SpeechBackend::Cloud(t) => vec![t.clone()],
        SpeechBackend::Local => Vec::new(),
    };
    if let Some(fb) = CloudStt::parse(s.fallback_transcription_provider.trim()) {
        if !chain.iter().any(|c| c.provider == fb)
            && secrets::has_provider_key(fb.key_slot())
        {
            chain.push(CloudSpeechTarget::with_default_model(fb));
        }
    }
    if matches!(choice, SpeechBackend::Local)
        && chain.is_empty()
        && secrets::has_api_key()
    {
        chain.push(CloudSpeechTarget::with_default_model(CloudStt::Groq));
    }
    chain
}

/// Backend label for a specific local model id — Parakeet ids run on the ONNX
/// backend, everything else on whisper.cpp. Used by the pipeline/command-mode
/// benchmark rows so they name the engine that actually ran.
pub fn local_backend_label_for(model_id: &str) -> &'static str {
    if is_parakeet_id(model_id) {
        crate::parakeet::backend_label()
    } else {
        local_backend_label()
    }
}

/// Parakeet catalog ids are the only local speech models not run by
/// whisper.cpp; the id prefix is the routing key.
fn is_parakeet_id(model_id: &str) -> bool {
    model_id.starts_with("parakeet-")
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

/// Cloud speech-to-text. Resolves the provider + model from live `Settings`
/// per call, then dispatches to the OpenAI-compatible multipart adapter (Groq,
/// OpenAI) or Deepgram's raw-body REST adapter.
pub struct CloudTranscriber {
    settings: Arc<Mutex<Settings>>,
}

impl CloudTranscriber {
    pub fn new(settings: Arc<Mutex<Settings>>) -> Self {
        Self { settings }
    }

    /// Transcribe one WAV against a resolved cloud target.
    async fn transcribe_target(
        &self,
        target: &CloudSpeechTarget,
        wav: Vec<u8>,
        language: Option<String>,
        hints: Vec<String>,
    ) -> anyhow::Result<String> {
        match target.provider {
            p @ (CloudStt::Groq | CloudStt::OpenAi) => {
                self.openai_compat(p, &target.model, wav, language, hints)
                    .await
            }
            CloudStt::Deepgram => self.deepgram(&target.model, wav, language, hints).await,
        }
    }

    /// OpenAI-compatible transcription API (`/audio/transcriptions` +
    /// `/audio/translations`), shared verbatim in shape by Groq and OpenAI:
    /// multipart form with `model`/`file`(/`language`/`prompt`) and a `{text}`
    /// JSON response.
    async fn openai_compat(
        &self,
        provider: CloudStt,
        model: &str,
        wav: Vec<u8>,
        language: Option<String>,
        hints: Vec<String>,
    ) -> anyhow::Result<String> {
        let key = secrets::get_provider_key(provider.key_slot()).map_err(|_| {
            anyhow::anyhow!("Set your {} API key in Settings", provider.label())
        })?;

        let translate = self.settings.lock().translate_to_english;
        if translate && !provider.supports_translate() {
            anyhow::bail!(
                "Translate to English isn't supported by {} - pick another speech provider",
                provider.label()
            );
        }

        let part = reqwest::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;

        let model = if translate {
            provider.translate_model().to_string()
        } else {
            model.to_string()
        };

        let mut form = reqwest::multipart::Form::new()
            .text("model", model)
            .text("response_format", "json")
            .text("temperature", "0")
            .part("file", part);

        if !translate {
            if let Some(lang) = language {
                form = form.text("language", lang);
            }
        }

        let mut final_hints = Vec::new();
        let whisper_prompt = self.settings.lock().whisper_prompt.clone();
        if !whisper_prompt.trim().is_empty() {
            final_hints.push(whisper_prompt);
        }
        final_hints.extend(hints);

        if !final_hints.is_empty() && provider.supports_prompt_hints() {
            // Whisper uses `prompt` as a soft vocabulary hint (dictionary terms).
            form = form.text("prompt", final_hints.join(", "));
        }

        // Shared static client (see `llm::groq_client`) so the pre-warm
        // handshake fired on trigger-down is reused by this upload.
        let endpoint = format!(
            "{}/audio/{}",
            api_base(provider),
            if translate { "translations" } else { "transcriptions" }
        );
        let resp = llm::groq_client()
            .post(endpoint)
            .bearer_auth(key)
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("{} error {}: {}", provider.label(), status, body);
        }

        let value: serde_json::Value = resp.json().await?;
        extract_openai_text(&value).ok_or_else(|| anyhow::anyhow!("Malformed response"))
    }

    /// Deepgram REST adapter: POST /v1/listen with `Authorization: Token <key>`
    /// (not Bearer), the raw WAV as the body with Content-Type audio/wav (no
    /// multipart), and the transcript at
    /// `results.channels[0].alternatives[0].transcript`. Dictionary hints map
    /// to per-word `keyword` query params; Whisper-style prompts aren't
    /// supported and degrade silently (surfaced in UI copy).
    async fn deepgram(
        &self,
        model: &str,
        wav: Vec<u8>,
        language: Option<String>,
        hints: Vec<String>,
    ) -> anyhow::Result<String> {
        if self.settings.lock().translate_to_english {
            anyhow::bail!(
                "Translate to English isn't supported by {} - pick another speech provider",
                CloudStt::Deepgram.label()
            );
        }
        let key =
            secrets::get_provider_key(CloudStt::Deepgram.key_slot())
                .map_err(|_| anyhow::anyhow!("Set your Deepgram API key in Settings"))?;

        let mut params: Vec<(String, String)> =
            vec![("model".into(), model.to_string())];
        if let Some(lang) = &language {
            params.push(("language".into(), lang.clone()));
        } else {
            params.push(("detect_language".into(), "true".into()));
        }
        for h in hints.iter().filter(|h| !h.trim().is_empty()) {
            params.push(("keyword".into(), h.trim().to_string()));
        }

        let resp = llm::groq_client()
            .post("https://api.deepgram.com/v1/listen")
            .query(&params)
            .header("Authorization", format!("Token {key}"))
            .header("Content-Type", "audio/wav")
            .body(wav)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Deepgram error {}: {}", status, body);
        }

        let value: serde_json::Value = resp.json().await?;
        value
            .get("results")
            .and_then(|r| r.get("channels"))
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("alternatives"))
            .and_then(|a| a.get(0))
            .and_then(|a| a.get("transcript"))
            .and_then(|t| t.as_str())
            .map(|t| t.trim().to_string())
            .ok_or_else(|| anyhow::anyhow!("Malformed Deepgram response"))
    }
}

fn extract_openai_text(value: &serde_json::Value) -> Option<String> {
    value
        .get("text")
        .and_then(|t| t.as_str())
        .map(str::to_string)
}

/// Scheme + host + version path for an OpenAI-compatible STT provider.
fn api_base(provider: CloudStt) -> &'static str {
    match provider {
        CloudStt::Groq => "https://api.groq.com/openai/v1",
        CloudStt::OpenAi => "https://api.openai.com/v1",
        CloudStt::Deepgram => unreachable!("Deepgram uses its own adapter"),
    }
}

/// On-device Whisper (whisper.cpp via `whisper-rs`). The selected model id and
/// language come from `Settings`; the GGML weights live under `models_dir`. The
/// loaded context is cached and reused, reloading only when the selection
/// changes. Real inference is compiled only with the `local-whisper` feature.
pub struct LocalTranscriber {
    models_dir: PathBuf,
    settings: Arc<Mutex<Settings>>,
    #[cfg(feature = "local-whisper")]
    cache: Mutex<Option<(String, Arc<whisper_rs::WhisperContext>)>>,
    #[cfg(feature = "local-whisper")]
    load_gate: tokio::sync::Mutex<()>,
    /// Phase 2: cold-load state for `whisper_status` (loading flag + last load ms).
    #[cfg(feature = "local-whisper")]
    load_state: Mutex<LoadState>,
}

#[cfg(feature = "local-whisper")]
#[derive(Default, Clone)]
struct LoadState {
    loading: bool,
    last_load_ms: Option<u64>,
    /// Phase 4: cost of the most recent local inference, surfaced in the UI.
    last_transcribe_ms: Option<u64>,
}

/// Per-call inference knobs, snapshotted from `Settings` + the active profile.
/// Grouped into a struct so the speed/quality decisions (beam vs greedy,
/// temperature fallback, audio-context cap) live in one place.
#[cfg(feature = "local-whisper")]
#[derive(Clone)]
struct Tuning {
    threads: Option<u32>,
    profile: String,
    /// The `local_beam_search_enabled` toggle. Accurate/rescue force beam search
    /// regardless; fast always stays greedy.
    beam_search: bool,
    correctness_rescue: bool,
    translate: bool,
    whisper_prompt: String,
}

impl LocalTranscriber {
    pub fn new(models_dir: PathBuf, settings: Arc<Mutex<Settings>>) -> Self {
        Self {
            models_dir,
            settings,
            #[cfg(feature = "local-whisper")]
            cache: Mutex::new(None),
            #[cfg(feature = "local-whisper")]
            load_gate: tokio::sync::Mutex::new(()),
            #[cfg(feature = "local-whisper")]
            load_state: Mutex::new(LoadState::default()),
        }
    }

    /// Drop the cached whisper context, freeing its memory (VRAM on the CUDA
    /// build). The next use cold-loads again. No-op if nothing is loaded or the
    /// feature is off. An in-flight transcription keeps its own `Arc` clone, so
    /// this can't cut a running inference short.
    pub fn unload(&self) {
        #[cfg(feature = "local-whisper")]
        {
            *self.cache.lock() = None;
        }
    }

    /// Resolve the selected model id to an on-disk path, erroring with a
    /// user-facing message when nothing is selected or the file is missing.
    #[cfg(feature = "local-whisper")]
    fn resolve(&self) -> anyhow::Result<(String, PathBuf)> {
        let id = {
            let s = self.settings.lock();
            if s.local_correctness_rescue {
                let rescue_id = "whisper-large-v3-turbo";
                if let Some(info) = crate::models::find(rescue_id) {
                    if self.models_dir.join(info.file_name).exists() {
                        rescue_id.to_string()
                    } else {
                        s.local_whisper_model.clone()
                    }
                } else {
                    s.local_whisper_model.clone()
                }
            } else {
                s.local_whisper_model.clone()
            }
        };
        if id.is_empty() {
            anyhow::bail!("No local speech model selected — pick one in Models");
        }
        let info =
            crate::models::find(&id).ok_or_else(|| anyhow::anyhow!("Unknown local model: {id}"))?;
        let path = self.models_dir.join(info.file_name);
        if !path.exists() {
            anyhow::bail!("Model '{}' is not downloaded yet", info.name);
        }
        Ok((id, path))
    }

    /// Return the cached context for the selected model, cold-loading it off the
    /// async runtime if needed. Shared by `transcribe`, `transcribe_audio`, and
    /// `prewarm`. Holds no lock across the heavy load and records load timing.
    #[cfg(feature = "local-whisper")]
    async fn ensure_context(&self) -> anyhow::Result<Arc<whisper_rs::WhisperContext>> {
        use whisper_rs::{WhisperContext, WhisperContextParameters};

        let (id, path) = self.resolve()?;

        // Fast path: reuse the cached context if it matches the selection.
        let cached = {
            let cache = self.cache.lock();
            match cache.as_ref() {
                Some((cached_id, ctx)) if *cached_id == id => Some(ctx.clone()),
                _ => None,
            }
        };
        if let Some(ctx) = cached {
            return Ok(ctx);
        }

        let _load_gate = self.load_gate.lock().await;

        let cached = {
            let cache = self.cache.lock();
            match cache.as_ref() {
                Some((cached_id, ctx)) if *cached_id == id => Some(ctx.clone()),
                _ => None,
            }
        };
        if let Some(ctx) = cached {
            return Ok(ctx);
        }

        // Cold load: `WhisperContext::new_with_params` reads a large model file
        // and is CPU-heavy + blocking — run it off the async runtime and hold no
        // lock while it runs.
        self.load_state.lock().loading = true;
        let path_str = path.to_string_lossy().to_string();
        let t0 = std::time::Instant::now();
        let loaded = tauri::async_runtime::spawn_blocking(move || {
            let mut params = WhisperContextParameters::default();
            // Flash attention is a clear speedup on the CUDA backend and safe
            // here (it only conflicts with DTW, which we don't use). It has no
            // effect on the CPU build, so gate it to the CUDA feature to avoid
            // any chance of a CPU-path regression. `cfg!` folds to a constant,
            // so the branch is compiled out entirely off-CUDA.
            if cfg!(feature = "local-whisper-cuda") {
                params.flash_attn(true);
            }
            WhisperContext::new_with_params(&path_str, params)
                .map_err(|e| anyhow::anyhow!("Failed to load Whisper model: {e}"))
        })
        .await;
        let load_ms = t0.elapsed().as_millis() as u64;

        let loaded = match loaded {
            Ok(Ok(ctx)) => ctx,
            Ok(Err(e)) => {
                self.load_state.lock().loading = false;
                return Err(e);
            }
            Err(e) => {
                self.load_state.lock().loading = false;
                return Err(anyhow::anyhow!("Model load task failed: {e}"));
            }
        };
        let ctx = Arc::new(loaded);

        // Re-check the cache: a concurrent call may have loaded the same model
        // while we were loading. Prefer the existing entry to avoid a duplicate.
        let chosen = {
            let mut cache = self.cache.lock();
            match cache.as_ref() {
                Some((cached_id, existing)) if *cached_id == id => existing.clone(),
                _ => {
                    *cache = Some((id.clone(), ctx.clone()));
                    ctx
                }
            }
        };
        {
            let mut st = self.load_state.lock();
            st.loading = false;
            st.last_load_ms = Some(load_ms);
        }
        Ok(chosen)
    }

    /// Snapshot the inference knobs from the live `Settings`. Held briefly — the
    /// guard drops before any await.
    #[cfg(feature = "local-whisper")]
    fn tuning(&self) -> Tuning {
        let s = self.settings.lock();
        Tuning {
            threads: s.local_whisper_threads,
            profile: s.local_transcription_profile.clone(),
            beam_search: s.local_beam_search_enabled,
            correctness_rescue: s.local_correctness_rescue,
            translate: s.translate_to_english,
            whisper_prompt: s.whisper_prompt.clone(),
        }
    }

    /// Run inference off the async runtime, recording the wall-clock cost in
    /// `load_state.last_transcribe_ms` for the UI status panel (Phase 4). Shared
    /// by the WAV and sample-based paths.
    #[cfg(feature = "local-whisper")]
    async fn run_timed(
        &self,
        ctx: Arc<whisper_rs::WhisperContext>,
        samples: Arc<Vec<f32>>,
        language: Option<String>,
        hints: Vec<String>,
        tuning: Tuning,
    ) -> anyhow::Result<String> {
        let t0 = std::time::Instant::now();
        let out = tauri::async_runtime::spawn_blocking(move || {
            run_inference(ctx, samples, language, hints, tuning)
        })
        .await
        .map_err(|e| anyhow::anyhow!("Transcription task failed: {e}"))?;
        self.load_state.lock().last_transcribe_ms = Some(t0.elapsed().as_millis() as u64);
        out
    }
}

/// whisper.cpp inference for one clip. Sync + CPU-heavy → always called inside
/// `spawn_blocking`.
///
/// Two latency/quality regimes, chosen from the active profile:
///  - **fast / balanced** — greedy decoding, a single decode pass (temperature
///    fallback disabled), and the encoder's audio context capped to the clip.
///    Optimized for low, *predictable* latency.
///  - **accurate / correctness-rescue** — beam search, whisper's temperature
///    fallback, and the full 30 s audio context. Optimized for quality.
///
/// All the print/timestamp flags are off in both regimes.
#[cfg(feature = "local-whisper")]
fn run_inference(
    ctx: Arc<whisper_rs::WhisperContext>,
    samples: Arc<Vec<f32>>,
    language: Option<String>,
    hints: Vec<String>,
    tuning: Tuning,
) -> anyhow::Result<String> {
    use whisper_rs::{FullParams, SamplingStrategy};

    let mut final_hints = Vec::new();
    if !tuning.whisper_prompt.trim().is_empty() {
        final_hints.push(tuning.whisper_prompt.clone());
    }
    final_hints.extend(hints);

    let prompt = if final_hints.is_empty() {
        None
    } else {
        Some(final_hints.join(", "))
    };

    // Quality regime: the accurate profile, or correctness rescue for hard clips.
    let quality = tuning.profile == "accurate" || tuning.correctness_rescue;
    // Beam search costs ~2–3× a greedy decode for a marginal dictation-quality
    // gain, so it's opt-in on balanced (the toggle) and forced only when quality
    // matters; fast always stays greedy.
    let use_beam = tuning.profile != "fast" && (tuning.beam_search || quality);

    let sampling = if use_beam {
        SamplingStrategy::BeamSearch {
            beam_size: 5,
            patience: 1.0,
        }
    } else {
        SamplingStrategy::Greedy { best_of: 1 }
    };
    let mut params = FullParams::new(sampling);
    params.set_n_threads(whisper_threads(tuning.threads));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    // Each dictation is an independent clip — there is no prior context to carry,
    // and a fresh state starts empty anyway, so make that explicit.
    params.set_no_context(true);
    // Cap the encoder's audio context (whisper.cpp's `-ac`) to the actual clip.
    // Without this the encoder always processes the full 1500-token / 30 s
    // context, so a short dictation pays for ~30 s of silence. VAD has already
    // trimmed to speech, so the cap never truncates real audio. Quality keeps it.
    params.set_audio_ctx(audio_ctx_for(samples.len(), quality));
    // Outside the quality regime, disable whisper's temperature-fallback loop. A
    // hard/noisy clip otherwise re-decodes up to ~6× at rising temperature —
    // exactly the cause of the worst-case latency spikes seen in the metrics. A
    // single pass keeps latency bounded; quality modes keep the fallback.
    if !quality {
        params.set_temperature_inc(0.0);
    }
    // Pin the language when the user selected exactly one (passed through as
    // `Some`); `None` lets whisper auto-detect.
    if let Some(lang) = language.as_deref() {
        params.set_language(Some(lang));
    }
    if tuning.translate {
        params.set_translate(true);
    }
    if let Some(p) = prompt.as_deref() {
        params.set_initial_prompt(p);
    }

    let mut state = ctx
        .create_state()
        .map_err(|e| anyhow::anyhow!("Whisper state error: {e}"))?;
    state
        .full(params, samples.as_slice())
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
}

/// Encoder audio-context size (whisper.cpp's `-ac`) for a clip, in audio-context
/// tokens. whisper runs ~50 tokens/sec (1500 = 30 s); we cover the clip length
/// plus 20% headroom, clamped to [256, 1500]. The floor keeps very short clips
/// safe; the ceiling is whisper's full context. Quality keeps the full 1500.
#[cfg(feature = "local-whisper")]
fn audio_ctx_for(n_samples: usize, quality: bool) -> std::os::raw::c_int {
    const FULL: i64 = 1500;
    if quality {
        return FULL as std::os::raw::c_int;
    }
    let seconds = n_samples as f64 / 16_000.0;
    let tokens = (seconds * 50.0 * 1.2).ceil() as i64;
    tokens.clamp(256, FULL) as std::os::raw::c_int
}

/// Thread count for whisper.cpp. An explicit `override_n` (from
/// `Settings::local_whisper_threads`) wins; otherwise use available cores minus a
/// couple (kept for the UI + audio threads). Always clamped to [1, 8] since
/// whisper scales poorly past ~8.
#[cfg(feature = "local-whisper")]
fn whisper_threads(override_n: Option<u32>) -> std::os::raw::c_int {
    let n = match override_n {
        Some(v) => v as usize,
        None => std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .saturating_sub(2),
    };
    n.clamp(1, 8) as std::os::raw::c_int
}

#[async_trait]
impl Transcriber for LocalTranscriber {
    #[cfg(not(feature = "local-whisper"))]
    async fn transcribe(
        &self,
        _wav: Vec<u8>,
        _language: Option<String>,
        _hints: Vec<String>,
    ) -> anyhow::Result<String> {
        // Touch fields so they don't read as dead when the feature is off.
        let _ = (&self.models_dir, &self.settings);
        anyhow::bail!("Local transcription was not built in (enable the `local-whisper` feature)")
    }

    #[cfg(feature = "local-whisper")]
    async fn transcribe(
        &self,
        wav: Vec<u8>,
        language: Option<String>,
        hints: Vec<String>,
    ) -> anyhow::Result<String> {
        let ctx = self.ensure_context().await?;
        let tuning = self.tuning();
        // Decode our own 16 kHz mono i16 WAV bytes back to f32 samples. This WAV
        // path is kept for direct callers (e.g. history replay); the live
        // pipeline uses `transcribe_audio` and skips this decode entirely.
        let samples = Arc::new(decode_wav_f32(&wav)?);
        self.run_timed(ctx, samples, language, hints, tuning).await
    }

    #[cfg(feature = "local-whisper")]
    async fn transcribe_audio(
        &self,
        audio: Audio,
        language: Option<String>,
        hints: Vec<String>,
    ) -> anyhow::Result<String> {
        let ctx = self.ensure_context().await?;
        let tuning = self.tuning();
        self.run_timed(ctx, audio.samples, language, hints, tuning)
            .await
    }

    #[cfg(feature = "local-whisper")]
    async fn prewarm(&self) -> anyhow::Result<()> {
        self.ensure_context().await.map(|_| ())
    }

    #[cfg(feature = "local-whisper")]
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
            backend: local_backend_label().to_string(),
        })
    }
}

/// Decode a 16-bit PCM WAV (our `audio::encode_wav` output) into normalized f32
/// samples in [-1, 1] for whisper.cpp.
#[cfg(feature = "local-whisper")]
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

/// Routes each call across the speech backends per the live `Settings`
/// (Phase 3 providers B): a local selection runs first and falls back to the
/// cloud chain when it errors and any fallback key exists; a cloud primary
/// walks its ordered chain (primary -> configured fallback provider), never
/// masking auth errors - a wrong key must surface, not be traded for a vaguer
/// failure elsewhere. "Local" covers two engines selected by the model id:
/// whisper.cpp for the Whisper GGML catalog, Parakeet ONNX for `parakeet-*`
/// ids.
pub struct RoutingTranscriber {
    cloud: CloudTranscriber,
    local: LocalTranscriber,
    parakeet: crate::parakeet::LocalParakeetTranscriber,
    settings: Arc<Mutex<Settings>>,
}

impl RoutingTranscriber {
    pub fn new(
        models_dir: PathBuf,
        bundled_models_dir: Option<PathBuf>,
        settings: Arc<Mutex<Settings>>,
    ) -> Self {
        Self {
            cloud: CloudTranscriber::new(settings.clone()),
            local: LocalTranscriber::new(models_dir.clone(), settings.clone()),
            parakeet: crate::parakeet::LocalParakeetTranscriber::new(
                models_dir,
                bundled_models_dir,
                settings.clone(),
            ),
            settings,
        }
    }

    /// The local engine for the currently selected model id.
    fn local_engine(&self) -> &dyn Transcriber {
        if is_parakeet_id(&self.settings.lock().local_whisper_model) {
            &self.parakeet
        } else {
            &self.local
        }
    }

    /// Run the WAV through the cloud chain in order. Auth errors surface
    /// immediately; every other failure (429 rate limits included) moves to the
    /// next provider. No generic retry/backoff: latency matters more than
    /// salvaging one dictation.
    async fn transcribe_cloud_chain(
        &self,
        chain: &[CloudSpeechTarget],
        wav: Vec<u8>,
        language: Option<String>,
        hints: Vec<String>,
    ) -> anyhow::Result<String> {
        let mut last_err: Option<anyhow::Error> = None;
        for target in chain {
            match self
                .cloud
                .transcribe_target(target, wav.clone(), language.clone(), hints.clone())
                .await
            {
                Ok(text) => return Ok(text),
                Err(e) if llm::is_auth_error(&e) => return Err(e),
                Err(e) => {
                    eprintln!(
                        "Speech-to-text via {} failed ({e}); trying next provider",
                        target.provider.label()
                    );
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("No speech provider available")))
    }

    /// Snapshot the backend choice + fallback chain (guard dropped before any
    /// await).
    fn plan(&self) -> (SpeechBackend, Vec<CloudSpeechTarget>) {
        let s = self.settings.lock();
        let choice = resolve_speech(&s);
        let chain = cloud_chain(&s);
        (choice, chain)
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
        let (choice, chain) = self.plan();
        match choice {
            SpeechBackend::Cloud(_) => {
                self.transcribe_cloud_chain(&chain, wav, language, hints).await
            }
            SpeechBackend::Local => {
                match self
                    .local_engine()
                    .transcribe(wav.clone(), language.clone(), hints.clone())
                    .await
                {
                    Ok(text) => return Ok(text),
                    Err(e) if !chain.is_empty() => {
                        eprintln!("Local transcription failed ({e}); falling back to cloud");
                        self.transcribe_cloud_chain(&chain, wav, language, hints).await
                    }
                    Err(e) => return Err(e),
                }
            }
        }
    }

    async fn transcribe_audio(
        &self,
        audio: Audio,
        language: Option<String>,
        hints: Vec<String>,
    ) -> anyhow::Result<String> {
        let (choice, chain) = self.plan();
        match choice {
            SpeechBackend::Cloud(_) => {
                self.transcribe_cloud_chain(&chain, audio.wav, language, hints)
                    .await
            }
            SpeechBackend::Local => {
                // Hand the local backend a cheap Arc clone of the samples; keep the
                // WAV here in case we have to fall back to a cloud provider.
                let local_audio = Audio {
                    samples: audio.samples.clone(),
                    wav: Vec::new(),
                };
                match self
                    .local_engine()
                    .transcribe_audio(local_audio, language.clone(), hints.clone())
                    .await
                {
                    Ok(text) => return Ok(text),
                    Err(e) if !chain.is_empty() => {
                        eprintln!("Local transcription failed ({e}); falling back to cloud");
                        self.transcribe_cloud_chain(&chain, audio.wav, language, hints)
                            .await
                    }
                    Err(e) => return Err(e),
                }
            }
        }
    }

    async fn prewarm(&self) -> anyhow::Result<()> {
        self.local_engine().prewarm().await
    }

    fn whisper_status(&self) -> Option<WhisperStatus> {
        self.local_engine().whisper_status()
    }

    async fn reconcile(&self, prewarm: bool) {
        let (use_local, id) = {
            let s = self.settings.lock();
            let use_local = matches!(resolve_speech(&s), SpeechBackend::Local);
            (use_local, s.local_whisper_model.clone())
        };
        let whisper_active = use_local && !id.is_empty() && !is_parakeet_id(&id);
        let parakeet_active = use_local && is_parakeet_id(&id);
        // Free whichever local engine isn't the current selection so a
        // deselected / switched-away model stops holding memory.
        if !whisper_active {
            self.local.unload();
        }
        if !parakeet_active {
            self.parakeet.unload();
        }
        if prewarm && (whisper_active || parakeet_active) {
            let _ = self.local_engine().prewarm().await;
        }
    }
}
