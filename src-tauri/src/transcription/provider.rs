use crate::secrets::ProviderKey;

/// Cloud STT providers Eve can route speech-to-text through (Phase 3
/// providers B). Groq, OpenAI, and OpenRouter share the OpenAI-compatible
/// multipart transcription API and `{text}` response; Deepgram speaks its own
/// REST shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudStt {
    Groq,
    OpenAi,
    Deepgram,
    OpenRouter,
}

impl CloudStt {
    /// Parse the wire form stored in `Settings.transcription_provider`.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "groq" => Some(CloudStt::Groq),
            "openai" => Some(CloudStt::OpenAi),
            "deepgram" => Some(CloudStt::Deepgram),
            "openrouter" => Some(CloudStt::OpenRouter),
            _ => None,
        }
    }

    /// Human label used in errors and benchmark rows ("OpenAI").
    pub fn label(self) -> &'static str {
        match self {
            CloudStt::Groq => "Groq",
            CloudStt::OpenAi => "OpenAI",
            CloudStt::Deepgram => "Deepgram",
            CloudStt::OpenRouter => "OpenRouter",
        }
    }

    /// Which keychain slot holds this provider's credential.
    pub fn key_slot(self) -> ProviderKey {
        match self {
            CloudStt::Groq => ProviderKey::Groq,
            CloudStt::OpenAi => ProviderKey::OpenAi,
            CloudStt::Deepgram => ProviderKey::Deepgram,
            CloudStt::OpenRouter => ProviderKey::OpenRouter,
        }
    }

    /// Default STT model used when `transcription_cloud_model` is empty.
    pub fn default_model(self) -> &'static str {
        match self {
            CloudStt::Groq => "whisper-large-v3-turbo",
            CloudStt::OpenAi => "whisper-1",
            CloudStt::Deepgram => "nova-3",
            // OpenRouter model ids are namespaced by the upstream provider.
            CloudStt::OpenRouter => "openai/whisper-1",
        }
    }

    /// Model forced when translate-to-English is on (the translations endpoint
    /// has a smaller compatible set than transcriptions). OpenAI's whisper-1
    /// serves both endpoints, so no override is needed there.
    pub(crate) fn translate_model(self) -> &'static str {
        match self {
            CloudStt::Groq => "whisper-large-v3",
            CloudStt::OpenAi => "whisper-1",
            CloudStt::Deepgram => self.default_model(),
            CloudStt::OpenRouter => self.default_model(),
        }
    }

    /// Whether the provider can translate audio to English. Deepgram's listen
    /// endpoint can't and OpenRouter documents no translations endpoint; a
    /// user with translation enabled gets a clear error instead of silently
    /// ignored audio.
    pub fn supports_translate(self) -> bool {
        !matches!(self, CloudStt::Deepgram | CloudStt::OpenRouter)
    }

    /// Whether the provider honors Whisper-style vocabulary hints. Deepgram
    /// takes per-word `keyword` params instead of a free-form prompt, so the
    /// configured `whisper_prompt` degrades there (surfaced in UI copy).
    /// OpenRouter accepts a `prompt` field but silently ignores it, so hints
    /// are not sent there either.
    pub fn supports_prompt_hints(self) -> bool {
        !matches!(self, CloudStt::Deepgram | CloudStt::OpenRouter)
    }

    /// Upload cap for the encoded WAV, if the provider enforces one. The
    /// OpenAI-compatible providers (including OpenRouter) reject over 25 MB
    /// (~13 min of 16 kHz mono); Deepgram accepts much larger bodies so no cap
    /// applies.
    pub fn max_wav_bytes(self) -> Option<usize> {
        match self {
            CloudStt::Groq | CloudStt::OpenAi | CloudStt::OpenRouter => Some(25 * 1024 * 1024),
            CloudStt::Deepgram => None,
        }
    }
}

/// Scheme + host + version path for an OpenAI-compatible STT provider.
pub(crate) fn api_base(provider: CloudStt) -> &'static str {
    match provider {
        CloudStt::Groq => "https://api.groq.com/openai/v1",
        CloudStt::OpenAi => "https://api.openai.com/v1",
        CloudStt::OpenRouter => "https://openrouter.ai/api/v1",
        CloudStt::Deepgram => unreachable!("Deepgram uses its own adapter"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openrouter_parses_from_wire_form() {
        assert_eq!(CloudStt::parse("openrouter"), Some(CloudStt::OpenRouter));
    }

    #[test]
    fn openrouter_label_and_key_slot() {
        assert_eq!(CloudStt::OpenRouter.label(), "OpenRouter");
        assert_eq!(
            CloudStt::OpenRouter.key_slot(),
            ProviderKey::OpenRouter
        );
    }

    #[test]
    fn openrouter_default_model_is_namespaced() {
        assert_eq!(CloudStt::OpenRouter.default_model(), "openai/whisper-1");
        assert_eq!(CloudStt::OpenRouter.translate_model(), "openai/whisper-1");
    }

    #[test]
    fn openrouter_lacks_translate_and_prompt_hints() {
        assert!(!CloudStt::OpenRouter.supports_translate());
        assert!(!CloudStt::OpenRouter.supports_prompt_hints());
    }

    #[test]
    fn openrouter_caps_wav_at_25_mb() {
        assert_eq!(CloudStt::OpenRouter.max_wav_bytes(), Some(25 * 1024 * 1024));
    }
}
