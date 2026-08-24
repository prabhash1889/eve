use crate::secrets::ProviderKey;

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
    pub(crate) fn translate_model(self) -> &'static str {
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

/// Scheme + host + version path for an OpenAI-compatible STT provider.
pub(crate) fn api_base(provider: CloudStt) -> &'static str {
    match provider {
        CloudStt::Groq => "https://api.groq.com/openai/v1",
        CloudStt::OpenAi => "https://api.openai.com/v1",
        CloudStt::Deepgram => unreachable!("Deepgram uses its own adapter"),
    }
}
