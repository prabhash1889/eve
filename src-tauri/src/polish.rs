//! Polish providers — the "flow" cleanup layer. v1 shipped a no-op pass-through;
//! Phase 2 adds `GroqPolisher` (llama-3.1-8b-instant) behind this same trait.
//!
//! `pipeline::process` always installs `GroqPolisher` and passes the per-dictation
//! `CleanupLevel`; the polisher itself short-circuits to a pass-through for
//! `CleanupLevel::None`, so changing the level in Settings takes effect without
//! rebuilding state. On any API error the pipeline falls back to the raw text.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::config::{CleanupLevel, Settings};
use crate::llm;
use crate::secrets;

/// Per-app voice for the polish prompt (Phase 6). Built from the active Flow
/// Style for the focused app's category; `None` means "no per-app styling".
/// Decoupled from the DB row so the polisher doesn't depend on `db::flow_styles`.
#[derive(Debug, Clone, Default)]
pub struct StyleHint {
    pub category: String,
    pub tone: String,
    pub system_prompt: String,
    pub writing_sample: String,
}

/// Turns a raw transcript into cleaned-up text per the chosen cleanup level,
/// optionally shaped by a per-app `StyleHint`.
#[async_trait]
pub trait Polisher: Send + Sync {
    async fn polish(
        &self,
        text: String,
        level: CleanupLevel,
        style: Option<StyleHint>,
    ) -> anyhow::Result<String>;
}

/// Returns the transcript unchanged. Kept as a fallback and for tests.
#[allow(dead_code)]
pub struct NoOpPolisher;

#[async_trait]
impl Polisher for NoOpPolisher {
    async fn polish(
        &self,
        text: String,
        _level: CleanupLevel,
        _style: Option<StyleHint>,
    ) -> anyhow::Result<String> {
        Ok(text)
    }
}

/// Groq Llama (`llama-3.1-8b-instant`) over the OpenAI-compatible chat API.
/// The HTTP round-trip lives in `llm::chat_with`; this just owns the model id
/// and the prompt-building/unwrapping around it.
pub struct GroqPolisher {
    model: String,
}

impl GroqPolisher {
    pub fn new() -> Self {
        Self {
            model: llm::DEFAULT_MODEL.into(),
        }
    }
}

#[async_trait]
impl Polisher for GroqPolisher {
    async fn polish(
        &self,
        text: String,
        level: CleanupLevel,
        style: Option<StyleHint>,
    ) -> anyhow::Result<String> {
        // No LLM round-trip when cleanup is off.
        if matches!(level, CleanupLevel::None) || text.trim().is_empty() {
            return Ok(text);
        }

        let system = system_prompt(level, style.as_ref());
        let content = llm::chat_with(&system, &text, &self.model, 0.2).await?;

        let cleaned = strip_wrapping(&content);
        if cleaned.is_empty() {
            // Model returned nothing usable — let the caller fall back to raw.
            anyhow::bail!("Polish returned empty output");
        }
        Ok(cleaned)
    }
}

/// Build the system prompt for a given cleanup level, optionally shaped by a
/// per-app `StyleHint` (Phase 6). Every prompt ends with the same hard rule:
/// emit only the resulting text, nothing else.
fn system_prompt(level: CleanupLevel, style: Option<&StyleHint>) -> String {
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

    let mut prompt = role.to_string();
    if let Some(style) = style {
        prompt.push_str(&style_clause(style));
    }
    prompt.push_str(
        "\n\nPreserve existing line breaks and paragraph structure. \
         Output ONLY the resulting text — no preamble, labels, quotes, or \
         explanation. If the input is already clean, return it unchanged.",
    );
    prompt
}

/// The per-app styling fragment appended to the base prompt: app-category
/// context, a tone instruction, an optional custom instruction, and an optional
/// writing sample to imitate.
fn style_clause(style: &StyleHint) -> String {
    let mut out = String::new();

    let context = match style.category.as_str() {
        "email" => Some("This text is an email."),
        "workmsg" => Some("This is a work chat message (e.g. Slack or Teams); keep it concise."),
        "personalmsg" => Some("This is a casual personal message to a friend."),
        "code" => Some(
            "This is going into a code editor; preserve technical terms and identifiers \
             verbatim and avoid heavy reformatting.",
        ),
        _ => None,
    };
    if let Some(c) = context {
        out.push_str("\n\n");
        out.push_str(c);
    }

    let tone = match style.tone.as_str() {
        "formal" => Some("Use a formal, professional tone."),
        "excited" => Some("Use an upbeat, enthusiastic tone."),
        "very_casual" => {
            Some("Use a very casual, relaxed tone with contractions; informality is fine.")
        }
        "casual" => Some("Use a casual, conversational tone."),
        _ => None,
    };
    if let Some(t) = tone {
        out.push(' ');
        out.push_str(t);
    }

    let custom = style.system_prompt.trim();
    if !custom.is_empty() {
        out.push_str("\n\n");
        out.push_str(custom);
    }

    let sample = style.writing_sample.trim();
    if !sample.is_empty() {
        out.push_str("\n\nMatch the voice and style of this writing sample:\n");
        out.push_str(sample);
    }

    out
}

/// Defend against a model that wraps its answer in quotes or a "Here is…:"
/// preamble despite instructions. Shared with Command Mode / Transforms.
pub fn strip_wrapping(s: &str) -> String {
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

/// On-device polish via a small instruct LLM (llama.cpp through `llama-cpp-2`).
/// The selected model id comes from `Settings`; GGUF weights live under
/// `models_dir`. Reuses `system_prompt`/`strip_wrapping` so output matches the
/// Groq backend. Real inference compiles only with the `local-models` feature.
pub struct LocalPolisher {
    models_dir: PathBuf,
    settings: Arc<Mutex<Settings>>,
    #[cfg(feature = "local-models")]
    cache: Mutex<Option<(String, Arc<llama_cpp_2::model::LlamaModel>)>>,
}

impl LocalPolisher {
    pub fn new(models_dir: PathBuf, settings: Arc<Mutex<Settings>>) -> Self {
        Self {
            models_dir,
            settings,
            #[cfg(feature = "local-models")]
            cache: Mutex::new(None),
        }
    }

    #[cfg(feature = "local-models")]
    fn resolve(&self) -> anyhow::Result<(String, PathBuf)> {
        let id = self.settings.lock().local_llm_model.clone();
        if id.is_empty() {
            anyhow::bail!("No local polish model selected — pick one in Models");
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
impl Polisher for LocalPolisher {
    #[cfg(not(feature = "local-models"))]
    async fn polish(
        &self,
        _text: String,
        _level: CleanupLevel,
        _style: Option<StyleHint>,
    ) -> anyhow::Result<String> {
        let _ = (&self.models_dir, &self.settings);
        anyhow::bail!("Local polish was not built in (enable the `local-models` feature)")
    }

    #[cfg(feature = "local-models")]
    async fn polish(
        &self,
        text: String,
        level: CleanupLevel,
        style: Option<StyleHint>,
    ) -> anyhow::Result<String> {
        if matches!(level, CleanupLevel::None) || text.trim().is_empty() {
            return Ok(text);
        }

        let (id, path) = self.resolve()?;
        let model = {
            let mut cache = self.cache.lock();
            match cache.as_ref() {
                Some((cached_id, m)) if *cached_id == id => m.clone(),
                _ => {
                    let m = Arc::new(load_llm(&path)?);
                    *cache = Some((id.clone(), m.clone()));
                    m
                }
            }
        };

        let system = system_prompt(level, style.as_ref());
        let user = text.clone();
        let fmt = chat_format_for(&id);
        let generated =
            tauri::async_runtime::spawn_blocking(move || generate(&model, fmt, &system, &user))
                .await
                .map_err(|e| anyhow::anyhow!("Polish task failed: {e}"))??;

        let cleaned = strip_wrapping(&generated);
        if cleaned.is_empty() {
            anyhow::bail!("Local polish returned empty output");
        }
        Ok(cleaned)
    }
}

/// Load a GGUF model. The llama.cpp backend is process-global and may only be
/// initialized once, so it lives in a `OnceLock`.
#[cfg(feature = "local-models")]
fn load_llm(path: &std::path::Path) -> anyhow::Result<llama_cpp_2::model::LlamaModel> {
    use llama_cpp_2::model::params::LlamaModelParams;
    use llama_cpp_2::model::LlamaModel;

    let backend = llm_backend()?;
    let params = LlamaModelParams::default(); // CPU only for v1
    LlamaModel::load_from_file(backend, path, &params)
        .map_err(|e| anyhow::anyhow!("Failed to load polish model: {e}"))
}

#[cfg(feature = "local-models")]
fn llm_backend() -> anyhow::Result<&'static llama_cpp_2::llama_backend::LlamaBackend> {
    use llama_cpp_2::llama_backend::LlamaBackend;
    use std::sync::OnceLock;
    static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
    if BACKEND.get().is_none() {
        let b = LlamaBackend::init().map_err(|e| anyhow::anyhow!("llama backend init: {e}"))?;
        let _ = BACKEND.set(b);
    }
    BACKEND
        .get()
        .ok_or_else(|| anyhow::anyhow!("llama backend unavailable"))
}

/// The chat prompt format an instruct GGUF expects. Different model families use
/// different turn delimiters; feeding a model the wrong ones badly degrades
/// output, so we pick per selected model.
#[cfg(feature = "local-models")]
#[derive(Debug, Clone, Copy)]
enum ChatFormat {
    /// ChatML — Qwen2.5 et al. (`<|im_start|>` / `<|im_end|>`).
    ChatMl,
    /// Llama 3.x Instruct (`<|start_header_id|>` headers / `<|eot_id|>`).
    Llama3,
}

/// Pick the chat format from a catalog model id. Llama models use the Llama 3
/// template; everything else defaults to ChatML (Qwen and most other small
/// instruct GGUFs).
#[cfg(feature = "local-models")]
fn chat_format_for(id: &str) -> ChatFormat {
    if id.starts_with("llama") {
        ChatFormat::Llama3
    } else {
        ChatFormat::ChatMl
    }
}

/// Build the single-turn chat prompt for the given format. The leading
/// begin-of-text/BOS token is added by the tokenizer (`AddBos::Always`), so it
/// is intentionally omitted from the Llama 3 template here.
#[cfg(feature = "local-models")]
fn build_chat_prompt(fmt: ChatFormat, system: &str, user: &str) -> String {
    match fmt {
        ChatFormat::ChatMl => format!(
            "<|im_start|>system\n{system}<|im_end|>\n<|im_start|>user\n{user}<|im_end|>\n<|im_start|>assistant\n"
        ),
        ChatFormat::Llama3 => format!(
            "<|start_header_id|>system<|end_header_id|>\n\n{system}<|eot_id|>\
             <|start_header_id|>user<|end_header_id|>\n\n{user}<|eot_id|>\
             <|start_header_id|>assistant<|end_header_id|>\n\n"
        ),
    }
}

/// Greedy single-turn generation using the model's chat template (ChatML for
/// Qwen, Llama 3 for Llama). Bounded to keep latency reasonable for a polish pass.
#[cfg(feature = "local-models")]
fn generate(
    model: &llama_cpp_2::model::LlamaModel,
    fmt: ChatFormat,
    system: &str,
    user: &str,
) -> anyhow::Result<String> {
    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::AddBos;
    use llama_cpp_2::sampling::LlamaSampler;
    use std::num::NonZeroU32;

    let backend = llm_backend()?;
    let ctx_params =
        LlamaContextParams::default().with_n_ctx(NonZeroU32::new(4096));
    let mut ctx = model
        .new_context(backend, ctx_params)
        .map_err(|e| anyhow::anyhow!("llama context: {e}"))?;

    let prompt = build_chat_prompt(fmt, system, user);
    let tokens = model
        .str_to_token(&prompt, AddBos::Always)
        .map_err(|e| anyhow::anyhow!("tokenize: {e}"))?;

    let mut batch = LlamaBatch::new(2048, 1);
    let last = tokens.len() as i32 - 1;
    for (i, tok) in tokens.iter().enumerate() {
        batch
            .add(*tok, i as i32, &[0], i as i32 == last)
            .map_err(|e| anyhow::anyhow!("batch add: {e}"))?;
    }
    ctx.decode(&mut batch).map_err(|e| anyhow::anyhow!("decode: {e}"))?;

    // Cap output relative to input so polish can't run away.
    let max_new = (tokens.len() + 256).min(2000);
    let mut sampler = LlamaSampler::greedy();
    let mut n_cur = batch.n_tokens();
    let mut out = String::new();
    // One decoder reused across tokens so multi-byte UTF-8 split across pieces
    // decodes correctly.
    let mut decoder = encoding_rs::UTF_8.new_decoder();

    while (n_cur as usize) < max_new {
        let token = sampler.sample(&ctx, batch.n_tokens() - 1);
        sampler.accept(token);
        if model.is_eog_token(token) {
            break;
        }
        let piece = model
            .token_to_piece(token, &mut decoder, false, None)
            .unwrap_or_default();
        out.push_str(&piece);

        batch.clear();
        batch
            .add(token, n_cur, &[0], true)
            .map_err(|e| anyhow::anyhow!("batch add: {e}"))?;
        n_cur += 1;
        ctx.decode(&mut batch).map_err(|e| anyhow::anyhow!("decode: {e}"))?;
    }

    // Strip any trailing turn-end marker the model may emit (ChatML or Llama 3).
    let out = out
        .split("<|im_end|>")
        .next()
        .unwrap_or("")
        .split("<|eot_id|>")
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    Ok(out)
}

/// Routes each polish call to Groq or the local LLM per the live `Settings`,
/// falling back to Groq on local error when a key exists. `CleanupLevel::None`
/// short-circuits before either backend is touched.
pub struct RoutingPolisher {
    groq: GroqPolisher,
    local: LocalPolisher,
    settings: Arc<Mutex<Settings>>,
}

impl RoutingPolisher {
    pub fn new(models_dir: PathBuf, settings: Arc<Mutex<Settings>>) -> Self {
        Self {
            groq: GroqPolisher::new(),
            local: LocalPolisher::new(models_dir, settings.clone()),
            settings,
        }
    }
}

#[async_trait]
impl Polisher for RoutingPolisher {
    async fn polish(
        &self,
        text: String,
        level: CleanupLevel,
        style: Option<StyleHint>,
    ) -> anyhow::Result<String> {
        if matches!(level, CleanupLevel::None) || text.trim().is_empty() {
            return Ok(text);
        }
        let use_local = self.settings.lock().polish_backend == "local";
        if use_local {
            match self.local.polish(text.clone(), level, style.clone()).await {
                Ok(out) => return Ok(out),
                Err(e) if secrets::has_api_key() => {
                    eprintln!("Local polish failed ({e}); falling back to Groq");
                }
                Err(e) => return Err(e),
            }
        }
        self.groq.polish(text, level, style).await
    }
}
