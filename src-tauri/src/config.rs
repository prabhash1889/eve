//! User settings, persisted as JSON in the app config directory.
//! The API key is NOT stored here — it lives in the OS keychain (see `secrets`).

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CleanupLevel {
    None,
    Light,
    Medium,
    High,
}

impl CleanupLevel {
    /// Stable string form persisted in the history DB (`cleanup_level` column).
    pub fn as_str(self) -> &'static str {
        match self {
            CleanupLevel::None => "none",
            CleanupLevel::Light => "light",
            CleanupLevel::Medium => "medium",
            CleanupLevel::High => "high",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub shortcut: String,
    pub language: String,
    pub cleanup_level: CleanupLevel,
    /// "paste" (clipboard + Ctrl+V) or "type" (char-by-char).
    pub inject_strategy: String,
    /// Global shortcut to copy the last transcript to the clipboard (Phase 2).
    /// `#[serde(default)]` so settings files written before this field existed
    /// still deserialize instead of resetting every field to defaults.
    #[serde(default = "default_copy_shortcut")]
    pub copy_shortcut: String,
    /// Phase 7: Command Mode push-to-talk shortcut. Hold it, speak an
    /// instruction; the focused selection is rewritten (or text is generated
    /// inline if nothing is selected).
    #[serde(default = "default_command_shortcut")]
    pub command_shortcut: String,
    /// Phase 9: global shortcut that opens (and focuses) the floating Scratchpad
    /// window. Dictating while it's focused routes text into the editor.
    #[serde(default = "default_scratchpad_shortcut")]
    pub scratchpad_shortcut: String,
    /// Flow Bar size multiplier (1.0 = default). Phase 2 appearance setting.
    #[serde(default = "default_bubble_scale")]
    pub bubble_scale: f32,
    /// Flow Bar opacity (0.0–1.0). Phase 2 appearance setting.
    #[serde(default = "default_bubble_opacity")]
    pub bubble_opacity: f32,
    /// Phase 3 audio retention: "store" (keep forever), "delete24h" (prune after
    /// `audio_retention_hours`), or "never" (don't save audio at all).
    #[serde(default = "default_audio_storage_policy")]
    pub audio_storage_policy: String,
    /// Hours to keep saved audio when the policy is "delete24h".
    #[serde(default = "default_audio_retention_hours")]
    pub audio_retention_hours: u32,
    /// Local-models: which backend runs speech→text. "groq" (cloud) or "local"
    /// (on-device whisper.cpp). Falls back to Groq if the local model fails.
    #[serde(default = "default_backend")]
    pub transcription_backend: String,
    /// Local-models: which backend runs polish. "groq" or "local" (on-device
    /// llama.cpp). Falls back to Groq on local failure.
    #[serde(default = "default_backend")]
    pub polish_backend: String,
    /// Catalog id of the local Whisper model to use (e.g. "whisper-base.en").
    /// Empty until the user downloads and selects one.
    #[serde(default)]
    pub local_whisper_model: String,
    /// Catalog id of the local polish LLM to use (e.g. "qwen2.5-1.5b-instruct").
    #[serde(default)]
    pub local_llm_model: String,
    /// Phase 8 vibe-coding: when the focused app is a code editor (Phase 6
    /// `Code` category), wrap spoken "backtick X backtick" spans in literal
    /// backticks before injection. Defaults on.
    #[serde(default = "default_vibe_coding")]
    pub vibe_coding: bool,
    /// Phase 10: languages enabled in the UI. `["auto"]` (or any list with more
    /// than one specific language) means auto-detect; a single specific language
    /// pins Whisper to it. The frontend derives the single `language` field above
    /// from this list, so the pipeline keeps reading `language`.
    #[serde(default = "default_languages")]
    pub languages: Vec<String>,
    /// Phase 10 auto-pause: process names (e.g. "1password.exe", lowercased)
    /// where recording is suppressed for privacy. Matched against the focused
    /// app's process at record start.
    #[serde(default = "default_paused_apps")]
    pub paused_apps: Vec<String>,
    /// Phase 10 privacy: when false, Eve does not resolve or store the focused
    /// app's title/category (disables Flow Styles + per-app history attribution).
    /// Auto-pause still resolves the bare process name to honor the pause list.
    #[serde(default = "default_context_awareness")]
    pub context_awareness: bool,
    /// Phase 10: set true once the first-run onboarding flow has been completed.
    #[serde(default)]
    pub onboarding_complete: bool,
    /// Phase 11: launch Eve automatically at OS login (via the autostart plugin).
    #[serde(default)]
    pub launch_at_startup: bool,
}

fn default_vibe_coding() -> bool {
    true
}
fn default_languages() -> Vec<String> {
    vec!["auto".into()]
}
fn default_context_awareness() -> bool {
    true
}
/// Sensitive desktop apps where dictation is suppressed by default. Process
/// names only (browsers can't be matched this way); users edit the list in
/// Settings → Privacy.
fn default_paused_apps() -> Vec<String> {
    vec![
        "1password.exe".into(),
        "keepass.exe".into(),
        "keepassxc.exe".into(),
        "bitwarden.exe".into(),
    ]
}

fn default_copy_shortcut() -> String {
    "CmdOrCtrl+Shift+C".into()
}
fn default_command_shortcut() -> String {
    "CmdOrCtrl+Shift+Alt+Space".into()
}
fn default_scratchpad_shortcut() -> String {
    "CmdOrCtrl+Shift+S".into()
}
fn default_bubble_scale() -> f32 {
    1.0
}
fn default_bubble_opacity() -> f32 {
    1.0
}
fn default_audio_storage_policy() -> String {
    "delete24h".into()
}
fn default_audio_retention_hours() -> u32 {
    24
}
fn default_backend() -> String {
    "groq".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shortcut: "F8".into(),
            language: "auto".into(),
            cleanup_level: CleanupLevel::None,
            inject_strategy: "paste".into(),
            copy_shortcut: default_copy_shortcut(),
            command_shortcut: default_command_shortcut(),
            scratchpad_shortcut: default_scratchpad_shortcut(),
            bubble_scale: default_bubble_scale(),
            bubble_opacity: default_bubble_opacity(),
            audio_storage_policy: default_audio_storage_policy(),
            audio_retention_hours: default_audio_retention_hours(),
            transcription_backend: default_backend(),
            polish_backend: default_backend(),
            local_whisper_model: String::new(),
            local_llm_model: String::new(),
            vibe_coding: default_vibe_coding(),
            languages: default_languages(),
            paused_apps: default_paused_apps(),
            context_awareness: default_context_awareness(),
            onboarding_complete: false,
            launch_at_startup: false,
        }
    }
}

/// Load settings from disk, falling back to defaults if missing or malformed.
pub fn load(path: &Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist settings to disk (best-effort).
pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(settings).unwrap_or_default();
    std::fs::write(path, json)
}
