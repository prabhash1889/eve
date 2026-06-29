//! Shared Groq chat-completions helper (Phase 7). Factored out of `polish.rs`
//! so polish, Command Mode, and Transforms all hit the same code path. Pure
//! request/response: callers own any prompt building and output post-processing
//! (e.g. `polish::strip_wrapping`).

use std::sync::OnceLock;
use std::time::Duration;

use crate::secrets;

/// Default chat model — Groq Llama, matching the polisher.
pub const DEFAULT_MODEL: &str = "llama-3.1-8b-instant";

/// Shared HTTP client for Groq API calls. Built once with finite timeouts
/// (10s to connect, 60s overall) so a dead connection can never hang the
/// pipeline forever, and reused across calls so we don't churn the connection
/// pool / leak TIME_WAIT sockets.
pub fn groq_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// One-shot system+user chat completion at the default model/temperature.
pub async fn chat(system: &str, user: &str) -> anyhow::Result<String> {
    chat_with(system, user, DEFAULT_MODEL, 0.3).await
}

/// One-shot chat completion with explicit model + temperature. Returns the
/// assistant message content verbatim (no trimming/unwrapping).
pub async fn chat_with(
    system: &str,
    user: &str,
    model: &str,
    temperature: f32,
) -> anyhow::Result<String> {
    let key =
        secrets::get_api_key().map_err(|_| anyhow::anyhow!("Set your Groq API key in Settings"))?;

    let body = serde_json::json!({
        "model": model,
        "temperature": temperature,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
    });

    let resp = groq_client()
        .post("https://api.groq.com/openai/v1/chat/completions")
        .bearer_auth(key)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Groq error {}: {}", status, text);
    }

    let value: serde_json::Value = resp.json().await?;
    let content = value
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or_default();

    Ok(content.to_string())
}
