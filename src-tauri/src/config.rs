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
}

fn default_copy_shortcut() -> String {
    "CmdOrCtrl+Shift+C".into()
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

impl Default for Settings {
    fn default() -> Self {
        Self {
            shortcut: "F8".into(),
            language: "auto".into(),
            cleanup_level: CleanupLevel::None,
            inject_strategy: "paste".into(),
            copy_shortcut: default_copy_shortcut(),
            bubble_scale: default_bubble_scale(),
            bubble_opacity: default_bubble_opacity(),
            audio_storage_policy: default_audio_storage_policy(),
            audio_retention_hours: default_audio_retention_hours(),
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
