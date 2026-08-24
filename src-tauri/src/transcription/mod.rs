//! Transcription providers. Cloud speech-to-text goes through
//! `CloudTranscriber`, which resolves a provider (Groq / OpenAI share the
//! OpenAI-compatible multipart API; Deepgram speaks its own REST shape) from
//! live `Settings` per call. `LocalTranscriber` runs whisper.cpp on-device
//! (behind the `local-models` Cargo feature). `RoutingTranscriber` picks
//! between them per call from the live `Settings`, falling back across the
//! configured cloud chain when the selected backend errors.

mod cloud;
mod local_whisper;
mod provider;
mod routing;
mod traits;

// The `transcription` module is crate-private, so re-exports nothing outside
// the crate consumes can read as "unused" to rustc; they exist purely to keep
// the historical `crate::transcription::*` paths working.
#[allow(unused_imports)]
pub use cloud::CloudTranscriber;
#[allow(unused_imports)]
pub use local_whisper::LocalTranscriber;
pub use provider::CloudStt;
#[allow(unused_imports)]
pub use routing::{
    cloud_chain, local_backend_label, local_backend_label_for, max_wav_bytes_for, resolve_speech,
    wav_needed, CloudSpeechTarget, RoutingTranscriber, SpeechBackend,
};
pub use traits::{Audio, TranscriptionBenchmark, Transcriber, WhisperStatus};
