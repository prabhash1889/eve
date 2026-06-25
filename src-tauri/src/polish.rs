//! Polish providers — the "flow" cleanup layer. v1 shipped a no-op pass-through;
//! Phase 2 adds `GroqPolisher` (llama-3.1-8b-instant) behind this same trait.
//!
//! `pipeline::process` always installs `GroqPolisher` and passes the per-dictation
//! `CleanupLevel`; the polisher itself short-circuits to a pass-through for
//! `CleanupLevel::None`, so changing the level in Settings takes effect without
//! rebuilding state. On any API error the pipeline falls back to the raw text.

use async_trait::async_trait;

use crate::config::CleanupLevel;
use crate::secrets;

/// Turns a raw transcript into cleaned-up text per the chosen cleanup level.
#[async_trait]
pub trait Polisher: Send + Sync {
    async fn polish(&self, text: String, level: CleanupLevel) -> anyhow::Result<String>;
}

/// Returns the transcript unchanged. Kept as a fallback and for tests.
#[allow(dead_code)]
pub struct NoOpPolisher;

#[async_trait]
impl Polisher for NoOpPolisher {
    async fn polish(&self, text: String, _level: CleanupLevel) -> anyhow::Result<String> {
        Ok(text)
    }
}

/// Groq Llama (`llama-3.1-8b-instant`) over the OpenAI-compatible chat API.
pub struct GroqPolisher {
    client: reqwest::Client,
    model: String,
}

impl GroqPolisher {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            model: "llama-3.1-8b-instant".into(),
        }
    }
}

#[async_trait]
impl Polisher for GroqPolisher {
    async fn polish(&self, text: String, level: CleanupLevel) -> anyhow::Result<String> {
        // No LLM round-trip when cleanup is off.
        if matches!(level, CleanupLevel::None) || text.trim().is_empty() {
            return Ok(text);
        }

        let key = secrets::get_api_key()
            .map_err(|_| anyhow::anyhow!("Set your Groq API key in Settings"))?;

        let body = serde_json::json!({
            "model": self.model,
            "temperature": 0.2,
            "messages": [
                { "role": "system", "content": system_prompt(level) },
                { "role": "user", "content": text },
            ],
        });

        let resp = self
            .client
            .post("https://api.groq.com/openai/v1/chat/completions")
            .bearer_auth(key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Groq error {}: {}", status, body);
        }

        let value: serde_json::Value = resp.json().await?;
        let content = value
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or_default();

        let cleaned = strip_wrapping(content);
        if cleaned.is_empty() {
            // Model returned nothing usable — let the caller fall back to raw.
            anyhow::bail!("Polish returned empty output");
        }
        Ok(cleaned)
    }
}

/// Build the system prompt for a given cleanup level. Every level ends with the
/// same hard rule: emit only the resulting text, nothing else.
fn system_prompt(level: CleanupLevel) -> String {
    let role = match level {
        CleanupLevel::None => unreachable!("None never calls the LLM"),
        CleanupLevel::Light => {
            "Lightly tidy this dictated text. Fix capitalization and obvious \
             punctuation and remove stray filler words (um, uh). Keep the \
             speaker's exact wording and meaning otherwise."
        }
        CleanupLevel::Medium => {
            "Clean up this dictated text. Remove filler words (um, uh, like, \
             you know), fix punctuation, capitalization, and obvious grammar or \
             transcription errors, and resolve spoken self-corrections (e.g. \
             'I mean', 'actually'). Keep the speaker's voice and meaning; do not \
             add new ideas or commentary."
        }
        CleanupLevel::High => {
            "Rewrite this dictated text into clear, well-punctuated prose. Remove \
             all filler and false starts, fix grammar, resolve self-corrections, \
             and format clearly enumerated spoken lists as a list. Preserve the \
             original meaning, intent, and every factual detail; never invent \
             information or add commentary."
        }
    };

    format!(
        "{role}\n\nPreserve existing line breaks and paragraph structure. \
         Output ONLY the resulting text — no preamble, labels, quotes, or \
         explanation. If the input is already clean, return it unchanged."
    )
}

/// Defend against a model that wraps its answer in quotes or a "Here is…:"
/// preamble despite instructions.
fn strip_wrapping(s: &str) -> String {
    let trimmed = s.trim();
    // Drop a single layer of surrounding quotes if they wrap the whole thing.
    let unquoted = if (trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() > 1)
        || (trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() > 1)
    {
        trimmed[1..trimmed.len() - 1].trim()
    } else {
        trimmed
    };
    unquoted.to_string()
}
