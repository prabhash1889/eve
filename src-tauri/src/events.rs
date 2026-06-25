//! Event names emitted to the Flow Bar window, plus their payloads.
//! These string constants must stay in sync with `src/lib/api.ts` (the `EVT` map).

use serde::Serialize;

pub const START: &str = "session://start";
pub const PROCESSING: &str = "session://processing";
pub const AMPLITUDE: &str = "session://amplitude";
pub const DONE: &str = "session://done";
pub const ERROR: &str = "session://error";
pub const CANCEL: &str = "session://cancel";
/// Phase 2: the raw transcript, emitted before polish so the bar can preview it.
pub const TRANSCRIPT_RAW: &str = "session://transcript-raw";
/// Phase 2: the polished/finalized text, emitted just before injection.
pub const TRANSCRIPT_POLISHED: &str = "session://transcript-polished";
/// Phase 2: copy-last-transcript shortcut fired and copied to the clipboard.
pub const COPIED: &str = "session://copied";

pub const FLOWBAR: &str = "flowbar";

#[derive(Clone, Serialize)]
pub struct DonePayload {
    pub text: String,
}

#[derive(Clone, Serialize)]
pub struct ErrorPayload {
    pub message: String,
}

/// A transcript snapshot (raw or polished) for the Flow Bar preview.
#[derive(Clone, Serialize)]
pub struct TranscriptPayload {
    pub text: String,
}

/// Flow Bar appearance, sent with `START` so the (event-only) bar can size and
/// fade itself without invoking a command. Mirrors `Settings.bubble*`.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartPayload {
    pub bubble_scale: f32,
    pub bubble_opacity: f32,
}
