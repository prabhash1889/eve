use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::config::Settings;
use crate::{llm, secrets};

use super::cloud::CloudTranscriber;
use super::local_whisper::LocalTranscriber;
use super::provider::CloudStt;
use super::traits::{Audio, Transcriber, WhisperStatus};

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
