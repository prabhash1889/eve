//! Transcription providers. v1 ships the Groq backend; `LocalTranscriber` is a
//! stub so a local whisper-rs engine can drop in later without touching callers.

use async_trait::async_trait;

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

/// Placeholder for a future on-device backend (whisper-rs). Not wired in v1.
#[allow(dead_code)]
pub struct LocalTranscriber;

#[async_trait]
impl Transcriber for LocalTranscriber {
    async fn transcribe(
        &self,
        _wav: Vec<u8>,
        _language: Option<String>,
        _hints: Vec<String>,
    ) -> anyhow::Result<String> {
        anyhow::bail!("Local transcription is not implemented yet")
    }
}
