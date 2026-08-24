use std::sync::Arc;

use parking_lot::Mutex;

use crate::config::Settings;
use crate::{llm, secrets};

use super::provider::{api_base, CloudStt};
use super::routing::CloudSpeechTarget;

/// Cloud speech-to-text. Resolves the provider + model from live `Settings`
/// per call, then dispatches to the OpenAI-compatible multipart adapter (Groq,
/// OpenAI) or Deepgram's raw-body REST adapter.
///
/// Note: cloud STT deliberately shares `llm::groq_client()` (the single
/// process-wide reqwest client) so the trigger-down prewarm handshake is reused
/// by every upload. Do not introduce a second HTTP client here.
pub struct CloudTranscriber {
    settings: Arc<Mutex<Settings>>,
}

impl CloudTranscriber {
    pub fn new(settings: Arc<Mutex<Settings>>) -> Self {
        Self { settings }
    }

    /// Transcribe one WAV against a resolved cloud target.
    pub(crate) async fn transcribe_target(
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
