//! Shared Groq chat-completions helper (Phase 7). Factored out of `polish.rs`
//! so polish, Command Mode, and Transforms all hit the same code path. Pure
//! request/response: callers own any prompt building and output post-processing
//! (e.g. `polish::strip_wrapping`).

use crate::secrets;

/// Default chat model — Groq Llama, matching the polisher.
pub const DEFAULT_MODEL: &str = "llama-3.1-8b-instant";

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

    let resp = reqwest::Client::new()
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
