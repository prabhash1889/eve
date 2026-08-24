//! Shared cloud chat-completions helpers. Factored out of `polish.rs` so
//! polish, Command Mode, and Transforms all hit the same code path. Pure
//! request/response: callers own any prompt building and output post-processing
//! (e.g. `polish::strip_wrapping`).
//!
//! Multi-provider (Phase 2 providers A): every provider that speaks the
//! OpenAI-compatible chat API (Groq, OpenAI, OpenRouter) goes through
//! [`openai_compat_chat`]; Anthropic's distinct `/v1/messages` shape gets its
//! own adapter. Callers resolve a [`CloudChat`] target from live `Settings` via
//! [`resolve_chat`] so a settings change hot-swaps without rebuilding state.

use std::sync::OnceLock;
use std::time::Duration;

use crate::config::Settings;
use crate::secrets::{self, ProviderKey};

/// Default Groq chat model, matching the original polisher.
pub const DEFAULT_MODEL: &str = "llama-3.1-8b-instant";

/// Shared HTTP client for all cloud API calls (chat completions and multipart
/// audio uploads). Built once with finite timeouts (10s to connect, 120s
/// overall) so a dead connection can never hang the pipeline forever, and
/// reused everywhere so there is exactly one warm connection pool: a pre-warm
/// handshake on shortcut-down is then reused by transcription and polish alike.
pub fn groq_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            // Generous enough for long-clip transcription uploads; only raises
            // the ceiling vs the previous chat-only client (was 60s).
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// Cloud LLM providers that can run polish / Command Mode / Transforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudLlm {
    Groq,
    OpenAi,
    OpenRouter,
    Anthropic,
}

impl CloudLlm {
    /// Parse the wire form stored in `Settings.polish_provider`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "groq" => Some(CloudLlm::Groq),
            "openai" => Some(CloudLlm::OpenAi),
            "openrouter" => Some(CloudLlm::OpenRouter),
            "anthropic" => Some(CloudLlm::Anthropic),
            _ => None,
        }
    }

    /// Human label used in error messages ("OpenAI error 401: ...").
    pub fn label(self) -> &'static str {
        match self {
            CloudLlm::Groq => "Groq",
            CloudLlm::OpenAi => "OpenAI",
            CloudLlm::OpenRouter => "OpenRouter",
            CloudLlm::Anthropic => "Anthropic",
        }
    }

    /// Which keychain slot holds this provider's credential.
    pub fn key_slot(self) -> ProviderKey {
        match self {
            CloudLlm::Groq => ProviderKey::Groq,
            CloudLlm::OpenAi => ProviderKey::OpenAi,
            CloudLlm::OpenRouter => ProviderKey::OpenRouter,
            CloudLlm::Anthropic => ProviderKey::Anthropic,
        }
    }

    /// Default chat model used when `polish_cloud_model` is empty.
    pub fn default_model(self) -> &'static str {
        match self {
            CloudLlm::Groq => DEFAULT_MODEL,
            CloudLlm::OpenAi => "gpt-4o-mini",
            // OpenRouter model ids are namespaced by upstream vendor.
            CloudLlm::OpenRouter => "openai/gpt-4o-mini",
            CloudLlm::Anthropic => "claude-3-5-haiku-latest",
        }
    }

    /// OpenAI-compatible base URL (scheme + host + version path). Only valid
    /// for the OpenAI-compat variants; Anthropic has its own adapter.
    fn api_base(self) -> &'static str {
        match self {
            CloudLlm::Groq => "https://api.groq.com/openai/v1",
            CloudLlm::OpenAi => "https://api.openai.com/v1",
            CloudLlm::OpenRouter => "https://openrouter.ai/api/v1",
            CloudLlm::Anthropic => unreachable!("Anthropic uses its own adapter"),
        }
    }
}

/// A resolved cloud chat target: which provider and which model id.
#[derive(Debug, Clone)]
pub struct CloudChat {
    pub provider: CloudLlm,
    pub model: String,
}

/// Resolve the configured cloud chat target from live settings. An empty or
/// unknown provider string falls back to Groq; an empty model picks the
/// provider default. Read per call so settings changes take effect immediately
/// (same hot-swap pattern as the backend routers).
pub fn resolve_chat(s: &Settings) -> CloudChat {
    let provider = CloudLlm::parse(&s.polish_provider).unwrap_or(CloudLlm::Groq);
    let configured = s.polish_cloud_model.trim();
    let model = if configured.is_empty() {
        provider.default_model().to_string()
    } else {
        configured.to_string()
    };
    CloudChat { provider, model }
}

/// Fire-and-forget connection pre-warm (1.P3). Dials the configured speech and
/// polish hosts so TCP+TLS (~100-300ms after an idle period) overlaps the
/// recording instead of adding to release-to-transcript latency. Never blocks
/// or fails the caller: network errors are discarded and key checks are
/// best-effort (local-only users skip the pointless dial-out).
pub fn prewarm_connection(settings: &Settings) {
    let mut hosts: Vec<&'static str> = Vec::new();
    // Speech target (Phase 3): whichever cloud STT provider is selected, when
    // its key is configured. A local selection dials nothing.
    if let crate::transcription::SpeechBackend::Cloud(t) =
        crate::transcription::resolve_speech(settings)
    {
        if secrets::has_provider_key(t.provider.key_slot()) {
            hosts.push(match t.provider {
                crate::transcription::CloudStt::Groq => "https://api.groq.com",
                crate::transcription::CloudStt::OpenAi => "https://api.openai.com",
                crate::transcription::CloudStt::Deepgram => "https://api.deepgram.com",
            });
        }    }
    // The polish target may be a different provider/host; warm whichever one
    // has a key configured so the first chat request after release reuses the
    // handshake.
    let chat = resolve_chat(settings);
    if !matches!(chat.provider, CloudLlm::Groq)
        && secrets::has_provider_key(chat.provider.key_slot())
    {
        hosts.push(match chat.provider {
            CloudLlm::OpenAi => "https://api.openai.com",
            CloudLlm::OpenRouter => "https://openrouter.ai",
            _ => "https://api.anthropic.com",
        });
    }
    tauri::async_runtime::spawn(async move {
        for host in hosts {
            let _ = groq_client().get(host).send().await;
        }
    });
}

/// True when the error is an authentication/authorization failure. Routers must
/// NOT silently fall back to another provider on these: a wrong primary key
/// should surface, not be masked by a working secondary.
pub fn is_auth_error(err: &anyhow::Error) -> bool {
    let msg = format!("{err:#}");
    msg.contains("401") || msg.contains("403") || msg.contains("invalid_api_key")
}

/// One-shot system+user chat completion at the default temperature.
pub async fn chat(chat: &CloudChat, system: &str, user: &str) -> anyhow::Result<String> {
    chat_with(chat, system, user, 0.3).await
}

/// One-shot chat completion with explicit temperature. Returns the assistant
/// message content verbatim (no trimming/unwrapping).
pub async fn chat_with(
    chat: &CloudChat,
    system: &str,
    user: &str,
    temperature: f32,
) -> anyhow::Result<String> {
    match chat.provider {
        p @ (CloudLlm::Groq | CloudLlm::OpenAi | CloudLlm::OpenRouter) => {
            openai_compat_chat(p, &chat.model, system, user, temperature).await
        }
        CloudLlm::Anthropic => anthropic_chat(&chat.model, system, user, temperature).await,
    }
}

/// Chat completion against any OpenAI-compatible endpoint (Groq, OpenAI,
/// OpenRouter share the `{choices[0].message.content}` response shape).
async fn openai_compat_chat(
    provider: CloudLlm,
    model: &str,
    system: &str,
    user: &str,
    temperature: f32,
) -> anyhow::Result<String> {
    let key = secrets::get_provider_key(provider.key_slot()).map_err(|_| {
        anyhow::anyhow!("Set your {} API key in Settings", provider.label())
    })?;

    let body = serde_json::json!({
        "model": model,
        "temperature": temperature,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
    });

    let mut req = groq_client()
        .post(format!("{}/chat/completions", provider.api_base()))
        .bearer_auth(key)
        .json(&body);
    if matches!(provider, CloudLlm::OpenRouter) {
        // Optional attribution headers requested by OpenRouter for app traffic.
        req = req
            .header("HTTP-Referer", "https://eve.app")
            .header("X-Title", "Eve");
    }
    let resp = req.send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("{} error {}: {}", provider.label(), status, text);
    }

    let value: serde_json::Value = resp.json().await?;
    extract_openai_content(&value).ok_or_else(|| anyhow::anyhow!("Malformed chat response"))
}

fn extract_openai_content(value: &serde_json::Value) -> Option<String> {
    value
        .get("choices")?
        .get(0)?
        .get("message")?
        .get("content")?
        .as_str()
        .map(str::to_string)
}

/// Anthropic Messages API adapter: POST /v1/messages with `x-api-key` +
/// `anthropic-version` headers, the system prompt as a top-level field, and the
/// answer at `content[0].text`.
async fn anthropic_chat(
    model: &str,
    system: &str,
    user: &str,
    temperature: f32,
) -> anyhow::Result<String> {
    const ANTHROPIC_API_BASE: &str = "https://api.anthropic.com";
    const ANTHROPIC_VERSION: &str = "2023-06-01";

    let key = secrets::get_provider_key(ProviderKey::Anthropic)
        .map_err(|_| anyhow::anyhow!("Set your Anthropic API key in Settings"))?;

    let body = serde_json::json!({
        "model": model,
        "max_tokens": 2048,
        "temperature": temperature,
        "system": system,
        "messages": [
            { "role": "user", "content": user },
        ],
    });

    let resp = groq_client()
        .post(format!("{ANTHROPIC_API_BASE}/v1/messages"))
        .header("x-api-key", key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Anthropic error {}: {}", status, text);
    }

    let value: serde_json::Value = resp.json().await?;
    value
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("Malformed Anthropic response"))
}
