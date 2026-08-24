//! Unified user-facing error mapping for the session flows (phase 5.1).
//!
//! Every function here maps a raw provider/backend error string to the exact
//! Flow Bar / queue message that existed at its extraction site; the strings
//! are frozen by the tests in this module and must never drift. The em dashes
//! inside message literals are part of those frozen strings.
//!
//! Four mappers exist because the four call sites genuinely disagree:
//!
//! - [`transcription_error`] (from `pipeline.rs::friendly_error`) checks the
//!   local-model branches first, then auth/rate-limit, then oversize.
//! - [`file_transcribe_error`] (from `file_transcribe.rs::
//!   friendly_transcribe_error`) checks auth/rate-limit BEFORE its local
//!   passthrough branch, lacks the not-built-in, model-load-failure, and
//!   oversize branches entirely (those inputs fall to its generic), and so
//!   produces different output than [`transcription_error`] for real inputs
//!   such as a "not built in" error. Unifying them would change user-visible
//!   strings, so it stays a separate function by design.
//! - [`command_error`] covers LLM/command failures (adds a 403 branch, generic
//!   names the command flow).
//! - [`decode_error`] covers file decoding, an unrelated failure domain.

/// Map a speech-to-text error for the dictation pipeline. Ported verbatim from
/// `pipeline.rs::friendly_error`.
pub fn transcription_error(err: &str) -> String {
    if err.contains("not downloaded") || err.contains("No local") {
        // Local backend selected but no usable model (and no Groq fallback).
        err.to_string()
    } else if err.contains("not built in") {
        "Local models aren't available in this build".into()
    } else if err.contains("Failed to load") || err.contains("load model") {
        "Local model failed to load \u{2014} try re-downloading it".into()
    } else if err.contains("API key") {
        // Precedence quirk preserved on purpose: this generic "API key"
        // contains-check deliberately precedes the 401/invalid_api_key check
        // below, so an error mentioning both maps to the key hint. A reorder
        // during any future unification must be a conscious choice (the
        // freeze test asserts this).
        "Set your provider API key in Settings".into()
    } else if err.contains("401") || err.contains("invalid_api_key") {
        "Invalid API key \u{2014} check Settings".into()
    } else if err.contains("429") {
        "Rate limited \u{2014} try again in a moment".into()
    } else if err.contains("413") || err.contains("too large") || err.contains("too long") {
        "Recording too long \u{2014} keep dictations under about 13 minutes".into()
    } else {
        "Transcription failed \u{2014} check your connection".into()
    }
}

/// Map a Command Mode / transform error. Ported verbatim from
/// `command_mode.rs::command_error`. Note the same precedence quirk as
/// [`transcription_error`]: the generic "API key" branch wins over 401.
pub fn command_error(err: &str) -> String {
    if err.contains("API key") {
        "Set your provider API key in Settings".into()
    } else if err.contains("401") || err.contains("invalid_api_key") {
        "Invalid API key \u{2014} check Settings".into()
    } else if err.contains("403") {
        "Access denied \u{2014} check your API key".into()
    } else if err.contains("429") {
        "Rate limited \u{2014} try again in a moment".into()
    } else {
        "Command failed \u{2014} check your connection".into()
    }
}

/// Map a speech-to-text error for the file queue. Ported verbatim from
/// `file_transcribe.rs::friendly_transcribe_error`. Deliberately NOT merged
/// into [`transcription_error`]: it orders auth/rate-limit before its local
/// passthrough and omits the local-build/model/oversize branches, so the two
/// functions answer differently for several real inputs (see module docs).
pub fn file_transcribe_error(err: &str) -> String {
    if err.contains("API key") {
        "Set your provider API key in Settings".into()
    } else if err.contains("401") || err.contains("invalid_api_key") {
        "Invalid API key \u{2014} check Settings".into()
    } else if err.contains("429") {
        "Rate limited \u{2014} try again in a moment".into()
    } else if err.contains("not downloaded") || err.contains("No local") {
        err.to_string()
    } else {
        "Transcription failed \u{2014} check your connection".into()
    }
}

/// Map a file-decode error. Ported verbatim from
/// `file_transcribe.rs::friendly_decode_error`.
pub fn decode_error(err: &str) -> String {
    if err.contains("unsupported") || err.contains("No decodable") || err.contains("No audio") {
        "Unsupported or corrupt audio file".into()
    } else {
        "Couldn't decode the audio file".into()
    }
}

#[cfg(test)]
mod tests {
    use super::{command_error, decode_error, file_transcribe_error, transcription_error};

    /// Freeze the dictation error-mapping strings before the 5.1 session-module
    /// extraction moves them. Each raw provider/backend error maps to exactly
    /// one user-facing Flow Bar message.
    #[test]
    fn transcription_error_strings_are_frozen() {
        // Local backend selected but the model is missing: message passes through.
        assert_eq!(
            transcription_error("Model 'whisper-small' is not downloaded yet"),
            "Model 'whisper-small' is not downloaded yet"
        );
        assert_eq!(
            transcription_error("No local speech model selected - pick one in Models"),
            "No local speech model selected - pick one in Models"
        );
        assert_eq!(
            transcription_error(
                "Local transcription was not built in (enable the `local-whisper` feature)"
            ),
            "Local models aren't available in this build"
        );
        assert_eq!(
            transcription_error("Failed to load Whisper model: bad ggml"),
            "Local model failed to load \u{2014} try re-downloading it"
        );
        assert_eq!(
            transcription_error("Set your Groq API key in Settings"),
            "Set your provider API key in Settings"
        );
        assert_eq!(
            transcription_error("Groq error 401 Unauthorized: invalid_api_key"),
            "Invalid API key \u{2014} check Settings"
        );
        assert_eq!(
            transcription_error("Groq error 429 Too Many Requests"),
            "Rate limited \u{2014} try again in a moment"
        );
        assert_eq!(
            transcription_error("413 Payload Too Large"),
            "Recording too long \u{2014} keep dictations under about 13 minutes"
        );
        assert_eq!(
            transcription_error("error sending request for url"),
            "Transcription failed \u{2014} check your connection"
        );
    }

    /// Ordering matters: today the generic "API key" branch is checked before
    /// the 401 branch, so an error containing both maps to the key hint. Freeze
    /// that precedence so a reorder during extraction is a conscious choice.
    #[test]
    fn transcription_error_key_hint_precedes_auth_check() {
        let mapped = transcription_error("invalid_api_key with 'API key' in body");
        assert_eq!(mapped, "Set your provider API key in Settings");
    }

    /// Freeze the Command Mode / transform error strings before the 5.1
    /// extraction unifies them behind `session::errors`.
    #[test]
    fn command_error_strings_are_frozen() {
        assert_eq!(
            command_error("Set your OpenRouter API key in Settings"),
            "Set your provider API key in Settings"
        );
        assert_eq!(
            command_error("OpenAI error 401: invalid_api_key"),
            "Invalid API key \u{2014} check Settings"
        );
        assert_eq!(
            command_error("Anthropic error 403 Forbidden"),
            "Access denied \u{2014} check your API key"
        );
        assert_eq!(
            command_error("Groq error 429 rate limit exceeded"),
            "Rate limited \u{2014} try again in a moment"
        );
        assert_eq!(
            command_error("connection reset by peer"),
            "Command failed \u{2014} check your connection"
        );
    }

    #[test]
    fn command_error_key_hint_precedes_auth_check() {
        let mapped = command_error("invalid_api_key with 'API key' in body");
        assert_eq!(mapped, "Set your provider API key in Settings");
    }

    /// Freeze the file-queue decode error strings before the 5.1 extraction.
    #[test]
    fn decode_error_strings_are_frozen() {
        assert_eq!(
            decode_error("No decodable audio track"),
            "Unsupported or corrupt audio file"
        );
        assert_eq!(
            decode_error("unsupported codec"),
            "Unsupported or corrupt audio file"
        );
        assert_eq!(
            decode_error("No audio decoded from file"),
            "Unsupported or corrupt audio file"
        );
        assert_eq!(decode_error("io error while reading"), "Couldn't decode the audio file");
    }

    /// Freeze the file-queue transcribe error strings before the 5.1
    /// extraction.
    #[test]
    fn file_transcribe_error_strings_are_frozen() {
        assert_eq!(
            file_transcribe_error("Set your Deepgram API key in Settings"),
            "Set your provider API key in Settings"
        );
        assert_eq!(
            file_transcribe_error("Groq error 401: invalid_api_key"),
            "Invalid API key \u{2014} check Settings"
        );
        assert_eq!(
            file_transcribe_error("Deepgram error 429"),
            "Rate limited \u{2014} try again in a moment"
        );
        assert_eq!(
            file_transcribe_error("Model 'x' is not downloaded yet"),
            "Model 'x' is not downloaded yet"
        );
        assert_eq!(
            file_transcribe_error("connection closed"),
            "Transcription failed \u{2014} check your connection"
        );
    }

    /// Cross-mapper freeze: on the branches the mappers genuinely share
    /// (provider key hint, 401, 429, local-model passthrough), the dictation
    /// and file-queue mappers agree byte-for-byte. Divergent branches are
    /// intentionally NOT asserted equal here (see module docs).
    #[test]
    fn shared_branches_agree_across_mappers() {
        for raw in [
            "Set your OpenRouter API key in Settings",
            "Groq error 401 Unauthorized: invalid_api_key",
            "Groq error 429 Too Many Requests",
            "Model 'whisper-small' is not downloaded yet",
            "No local speech model selected - pick one in Models",
        ] {
            assert_eq!(
                transcription_error(raw),
                file_transcribe_error(raw),
                "diverged for input: {raw}"
            );
        }
        for raw in [
            "Set your OpenRouter API key in Settings",
            "OpenAI error 401: invalid_api_key",
            "Groq error 429 rate limit exceeded",
        ] {
            assert_eq!(
                transcription_error(raw),
                command_error(raw),
                "diverged for input: {raw}"
            );
        }
    }
}
