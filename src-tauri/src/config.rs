//! User settings, persisted as JSON in the app config directory.
//! The API key is NOT stored here â€” it lives in the OS keychain (see `secrets`).

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub shortcut: String,
    pub language: String,
    pub cleanup_level: CleanupLevel,
    /// "paste" (clipboard + Ctrl+V) or "type" (char-by-char).
    pub inject_strategy: String,
    /// Capture device name (as reported by cpal). Empty = follow the Windows
    /// default input device. Resolved fresh at each record start; falls back to
    /// the system default if the named device is unplugged.
    #[serde(default)]
    pub input_device: String,
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
    /// 4.2: global shortcut that undoes (recalls) the last injection by
    /// re-focusing its target and sending one Backspace per character.
    #[serde(default = "default_undo_shortcut")]
    pub undo_shortcut: String,
    /// Flow Bar size multiplier (1.0 = default). Phase 2 appearance setting.
    #[serde(default = "default_bubble_scale")]
    pub bubble_scale: f32,
    /// Flow Bar opacity (0.0â€“1.0). Phase 2 appearance setting.
    #[serde(default = "default_bubble_opacity")]
    pub bubble_opacity: f32,
    /// Local-models: which backend runs speechâ†’text. "groq" (cloud) or "local"
    /// (on-device whisper.cpp). Falls back to Groq if the local model fails.
    #[serde(default = "default_backend")]
    pub transcription_backend: String,
    /// Local-models: which backend runs polish. "groq" or "local" (on-device
    /// llama.cpp). Falls back to Groq on local failure.
    #[serde(default = "default_backend")]
    pub polish_backend: String,
    /// Multi-provider speech (Phase 3 providers B): which backend runs
    /// speech-to-text. One of "groq" | "openai" | "deepgram" | "openrouter" |
    /// "local". Empty =
    /// legacy install, resolve from `transcription_backend` ("local" -> local,
    /// anything else -> Groq) so existing settings keep working unchanged.
    #[serde(default)]
    pub transcription_provider: String,
    /// Optional model override for `transcription_provider`. Empty = the
    /// provider's default STT model (e.g. "whisper-large-v3-turbo",
    /// "whisper-1", "nova-3").
    #[serde(default)]
    pub transcription_cloud_model: String,
    /// Secondary cloud STT provider tried when the primary fails with a
    /// transient error (e.g. 429). Empty = none; a local primary with no
    /// explicit fallback keeps falling back to Groq when its key exists.
    /// Auth errors on the primary are never masked - they surface so a wrong
    /// key is visible.
    #[serde(default)]
    pub fallback_transcription_provider: String,
    /// Multi-provider polish (Phase 2 providers A): which cloud LLM runs polish,
    /// Command Mode, and Transforms. One of "groq" | "openai" | "openrouter" |
    /// "anthropic". An empty/unknown value falls back to "groq".
    #[serde(default = "default_polish_provider")]
    pub polish_provider: String,
    /// Optional model override for `polish_provider`. Empty = the provider's
    /// default chat model. OpenRouter ids are namespaced
    /// (e.g. "anthropic/claude-3.5-haiku").
    #[serde(default)]
    pub polish_cloud_model: String,
    /// Secondary cloud provider tried when the primary fails with a transient
    /// error. Empty = none. Auth errors on the primary are never masked - they
    /// surface so a wrong key is visible.
    #[serde(default)]
    pub fallback_polish_provider: String,
    /// Catalog id of the local speech model to use (whisper `whisper-*.bin` or
    /// `parakeet-*`). Empty until the user downloads and selects one, except in
    /// the Store edition where it defaults to the bundled Parakeet model.
    #[serde(default = "default_stt_model")]
    pub local_whisper_model: String,
    /// Catalog id of the local polish LLM to use (e.g. "qwen2.5-1.5b-instruct").
    #[serde(default)]
    pub local_llm_model: String,
    /// Phase 4 (optimization): local transcription performance profile â€”
    /// "fast", "balanced", or "accurate". Guides model recommendations and tunes
    /// how aggressively silence is trimmed (VAD). Does not silently replace the
    /// user's selected model.
    #[serde(default = "default_local_profile")]
    pub local_transcription_profile: String,
    /// Phase 4 (optimization): explicit whisper.cpp thread count. `None` lets Eve
    /// pick from the available cores (cores âˆ’ 2, clamped to 1..=8).
    #[serde(default)]
    pub local_whisper_threads: Option<u32>,
    /// Phase 3 (optimization): trim leading/trailing silence (and normalize) the
    /// samples fed to the local Whisper backend before inference. On by default.
    #[serde(default = "default_true")]
    pub local_vad_enabled: bool,
    /// Local Whisper: opt into beam search on the *balanced* profile for higher
    /// quality at the cost of speed. Off by default â€” greedy decoding is ~2â€“3Ã—
    /// faster and fine for dictation. Fast always stays greedy; accurate and
    /// correctness rescue always use beam search regardless of this toggle.
    #[serde(default)]
    pub local_beam_search_enabled: bool,
    /// Local Whisper: quality-first rescue mode for difficult clips. Uses
    /// gentler VAD/normalization, beam search, and prefers large-v3-turbo when
    /// it is downloaded.
    #[serde(default)]
    pub local_correctness_rescue: bool,
    /// Phase 4 (optimization): prewarm the selected local model when the speech
    /// backend is switched to local or a new model is picked. On by default.
    #[serde(default = "default_true")]
    pub local_prewarm_enabled: bool,
    /// Phase 5 (optimization): debug timing mode. When on, each dictation prints
    /// a detailed per-stage latency breakdown (each stage's share of total) to
    /// the console, on top of the always-on one-line log + CSV row. Off by default.
    #[serde(default)]
    pub debug_timing: bool,
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
    /// Parity A1: how the main trigger starts/stops recording. "hold"
    /// (push-to-talk, the original behavior), "toggle" (press to start, press
    /// again to stop), or "hybrid" (a quick tap toggles; holding past ~300 ms
    /// behaves like push-to-talk).
    #[serde(default = "default_activation_mode")]
    pub activation_mode: String,
    /// 4.5: hands-free auto-stop for toggle/hybrid mode - end the recording
    /// after this many seconds below the silence threshold. 0 = off.
    #[serde(default)]
    pub auto_stop_silence_secs: u32,
    /// Parity A3: a bare modifier key (e.g. "right_alt") as an additional
    /// record trigger, handled by a low-level keyboard hook because the
    /// global-shortcut plugin can't express modifier-only accelerators.
    /// Empty = none.
    #[serde(default)]
    pub modifier_trigger: String,
    /// Parity A4: a mouse button ("middle", "x1", "x2") as an additional record
    /// trigger. The bound button is consumed so its normal click never reaches
    /// the app under the cursor. Empty = none.
    #[serde(default)]
    pub mouse_trigger: String,
    /// Parity D: Translate all audio to English
    #[serde(default)]
    pub translate_to_english: bool,
    /// Parity D: Initial prompt passed to the Whisper transcriber
    #[serde(default)]
    pub whisper_prompt: String,
    /// Parity E2: Play a sound when recording starts.
    #[serde(default)]
    pub sound_on_start: bool,
    /// Parity E5: Automatically correct spacing in CJK languages.
    #[serde(default = "default_true")]
    pub cjk_autocorrect: bool,
    /// 4.7: live noise gate - drop digital silence in the capture callback
    /// before it reaches the buffer (benefits the cloud path, which otherwise
    /// uploads raw lead-in/tail silence). On by default; thresholds are
    /// conservative so soft speech is never gated.
    #[serde(default = "default_true")]
    pub live_noise_gate: bool,
    /// Parity E6: Flow Bar window position ("fixed" or "near_caret").
    #[serde(default = "default_bar_position")]
    pub bar_position: String,
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
/// Settings â†’ Privacy.
fn default_paused_apps() -> Vec<String> {
    vec![
        // Windows executables.
        "1password.exe".into(),
        "keepass.exe".into(),
        "keepassxc.exe".into(),
        "bitwarden.exe".into(),
        // Linux comm names (equality match, so these are inert on Windows and
        // vice-versa - same additive approach as the `classify` lists).
        "keepassxc".into(),
        "keepass2".into(),
        "bitwarden".into(),
        "1password".into(),
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
fn default_undo_shortcut() -> String {
    "CmdOrCtrl+Shift+Alt+Z".into()
}
fn default_bubble_scale() -> f32 {
    1.0
}
fn default_bubble_opacity() -> f32 {
    1.0
}
fn default_backend() -> String {
    // Store edition is offline-first: default both backends to the on-device
    // path so a fresh install works with no Groq key. Other builds default to
    // Groq (fast cloud) as before.
    if cfg!(feature = "store-edition") {
        "local".into()
    } else {
        "groq".into()
    }
}
fn default_polish_provider() -> String {
    "groq".into()
}
/// Default speech-to-text model id. The Store edition ships Parakeet bundled and
/// selected by default; other builds start unset until the user downloads one.
fn default_stt_model() -> String {
    if cfg!(feature = "store-edition") {
        "parakeet-tdt-0.6b-v2".into()
    } else {
        String::new()
    }
}
fn default_local_profile() -> String {
    "balanced".into()
}
fn default_activation_mode() -> String {
    "hold".into()
}
fn default_true() -> bool {
    true
}
fn default_bar_position() -> String {
    "fixed".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            shortcut: "F8".into(),
            language: "auto".into(),
            cleanup_level: CleanupLevel::None,
            inject_strategy: "paste".into(),
            input_device: String::new(),
            copy_shortcut: default_copy_shortcut(),
            command_shortcut: default_command_shortcut(),
            scratchpad_shortcut: default_scratchpad_shortcut(),
            undo_shortcut: default_undo_shortcut(),
            bubble_scale: default_bubble_scale(),
            bubble_opacity: default_bubble_opacity(),
            transcription_backend: default_backend(),
            polish_backend: default_backend(),
            transcription_provider: String::new(),
            transcription_cloud_model: String::new(),
            fallback_transcription_provider: String::new(),
            polish_provider: default_polish_provider(),
            polish_cloud_model: String::new(),
            fallback_polish_provider: String::new(),
            local_whisper_model: default_stt_model(),
            local_llm_model: String::new(),
            local_transcription_profile: default_local_profile(),
            local_whisper_threads: None,
            local_vad_enabled: true,
            local_beam_search_enabled: false,
            local_correctness_rescue: false,
            local_prewarm_enabled: true,
            debug_timing: false,
            vibe_coding: default_vibe_coding(),
            languages: default_languages(),
            paused_apps: default_paused_apps(),
            context_awareness: default_context_awareness(),
            onboarding_complete: false,
            launch_at_startup: false,
            activation_mode: default_activation_mode(),
            auto_stop_silence_secs: 0,
            modifier_trigger: String::new(),
            mouse_trigger: String::new(),
            translate_to_english: false,
            whisper_prompt: String::new(),
            sound_on_start: false,
            cjk_autocorrect: true,
            live_noise_gate: true,
            bar_position: "fixed".into(),
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
    // On a serialize error, return the error and leave the existing file intact
    // rather than truncating it to an empty string (which would wipe settings).
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn save_then_load_round_trips_every_field() {
        let mut s = Settings::default();
        s.shortcut = "F9".into();
        s.language = "de".into();
        s.cleanup_level = CleanupLevel::High;
        s.inject_strategy = "type".into();
        s.input_device = "Mic Array".into();
        s.copy_shortcut = "CmdOrCtrl+Shift+X".into();
        s.command_shortcut = "CmdOrCtrl+Shift+Alt+C".into();
        s.scratchpad_shortcut = "CmdOrCtrl+Shift+D".into();
        s.undo_shortcut = "CmdOrCtrl+Shift+Alt+Y".into();
        s.bubble_scale = 1.5;
        s.bubble_opacity = 0.75;
        s.transcription_provider = "deepgram".into();
        s.transcription_cloud_model = "nova-2".into();
        s.fallback_transcription_provider = "openai".into();
        s.polish_provider = "openrouter".into();
        s.polish_cloud_model = "anthropic/claude-3.5-haiku".into();
        s.fallback_polish_provider = "groq".into();
        s.local_whisper_model = "whisper-small.bin".into();
        s.local_llm_model = "qwen2.5-1.5b-instruct".into();
        s.local_transcription_profile = "accurate".into();
        s.local_whisper_threads = Some(4);
        s.local_vad_enabled = false;
        s.local_beam_search_enabled = true;
        s.local_correctness_rescue = true;
        s.local_prewarm_enabled = false;
        s.debug_timing = true;
        s.vibe_coding = false;
        s.languages = vec!["en".into(), "de".into()];
        s.paused_apps = vec!["secret.exe".into()];
        s.context_awareness = false;
        s.onboarding_complete = true;
        s.launch_at_startup = true;
        s.activation_mode = "hybrid".into();
        s.auto_stop_silence_secs = 5;
        s.modifier_trigger = "right_alt".into();
        s.mouse_trigger = "x1".into();
        s.translate_to_english = true;
        s.whisper_prompt = "vocab".into();
        s.sound_on_start = true;
        s.cjk_autocorrect = false;
        s.live_noise_gate = false;
        s.bar_position = "near_caret".into();

        let dir = std::env::temp_dir().join(format!("eve-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        save(&path, &s).unwrap();
        let loaded = load(&path);
        std::fs::remove_file(&path).ok();

        assert_eq!(loaded.shortcut, s.shortcut);
        assert_eq!(loaded.language, s.language);
        assert_eq!(loaded.cleanup_level, s.cleanup_level);
        assert_eq!(loaded.inject_strategy, s.inject_strategy);
        assert_eq!(loaded.input_device, s.input_device);
        assert_eq!(loaded.copy_shortcut, s.copy_shortcut);
        assert_eq!(loaded.command_shortcut, s.command_shortcut);
        assert_eq!(loaded.scratchpad_shortcut, s.scratchpad_shortcut);
        assert_eq!(loaded.undo_shortcut, s.undo_shortcut);
        assert_eq!(loaded.bubble_scale, s.bubble_scale);
        assert_eq!(loaded.bubble_opacity, s.bubble_opacity);
        assert_eq!(loaded.transcription_provider, s.transcription_provider);
        assert_eq!(loaded.transcription_cloud_model, s.transcription_cloud_model);
        assert_eq!(
            loaded.fallback_transcription_provider,
            s.fallback_transcription_provider
        );
        assert_eq!(loaded.polish_provider, s.polish_provider);
        assert_eq!(loaded.polish_cloud_model, s.polish_cloud_model);
        assert_eq!(loaded.fallback_polish_provider, s.fallback_polish_provider);
        assert_eq!(loaded.local_whisper_model, s.local_whisper_model);
        assert_eq!(loaded.local_llm_model, s.local_llm_model);
        assert_eq!(
            loaded.local_transcription_profile,
            s.local_transcription_profile
        );
        assert_eq!(loaded.local_whisper_threads, s.local_whisper_threads);
        assert_eq!(loaded.local_vad_enabled, s.local_vad_enabled);
        assert_eq!(loaded.local_beam_search_enabled, s.local_beam_search_enabled);
        assert_eq!(loaded.local_correctness_rescue, s.local_correctness_rescue);
        assert_eq!(loaded.local_prewarm_enabled, s.local_prewarm_enabled);
        assert_eq!(loaded.debug_timing, s.debug_timing);
        assert_eq!(loaded.vibe_coding, s.vibe_coding);
        assert_eq!(loaded.languages, s.languages);
        assert_eq!(loaded.paused_apps, s.paused_apps);
        assert_eq!(loaded.context_awareness, s.context_awareness);
        assert_eq!(loaded.onboarding_complete, s.onboarding_complete);
        assert_eq!(loaded.launch_at_startup, s.launch_at_startup);
        assert_eq!(loaded.activation_mode, s.activation_mode);
        assert_eq!(loaded.auto_stop_silence_secs, s.auto_stop_silence_secs);
        assert_eq!(loaded.modifier_trigger, s.modifier_trigger);
        assert_eq!(loaded.mouse_trigger, s.mouse_trigger);
        assert_eq!(loaded.translate_to_english, s.translate_to_english);
        assert_eq!(loaded.whisper_prompt, s.whisper_prompt);
        assert_eq!(loaded.sound_on_start, s.sound_on_start);
        assert_eq!(loaded.cjk_autocorrect, s.cjk_autocorrect);
        assert_eq!(loaded.live_noise_gate, s.live_noise_gate);
        assert_eq!(loaded.bar_position, s.bar_position);
    }

    /// A settings.json written by an older Eve (only the original fields) must
    /// deserialize with serde defaults filling every later field - never reset
    /// the whole file to `Default`.
    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn legacy_settings_json_gets_serde_defaults() {
        let legacy = r#"{
            "shortcut": "F9",
            "language": "en",
            "cleanupLevel": "light",
            "injectStrategy": "paste"
        }"#;
        let s: Settings = serde_json::from_str(legacy).unwrap();
        assert_eq!(s.shortcut, "F9");
        assert_eq!(s.cleanup_level, CleanupLevel::Light);
        // Serde defaults for fields added later.
        assert_eq!(s.copy_shortcut, default_copy_shortcut());
        assert_eq!(s.command_shortcut, default_command_shortcut());
        assert_eq!(s.scratchpad_shortcut, default_scratchpad_shortcut());
        assert_eq!(s.undo_shortcut, default_undo_shortcut());
        assert_eq!(s.bubble_scale, 1.0);
        assert_eq!(s.bubble_opacity, 1.0);
        assert_eq!(s.transcription_backend, default_backend());
        assert_eq!(s.polish_backend, default_backend());
        assert_eq!(s.transcription_provider, "");
        assert_eq!(s.fallback_transcription_provider, "");
        assert_eq!(s.polish_provider, "groq");
        assert_eq!(s.polish_cloud_model, "");
        assert_eq!(s.fallback_polish_provider, "");
        assert!(s.local_vad_enabled);
        assert!(!s.debug_timing);
        assert!(s.vibe_coding);
        assert_eq!(s.languages, vec!["auto".to_string()]);
        assert!(s.paused_apps.contains(&"1password.exe".to_string()));
        assert!(s.context_awareness);
        assert_eq!(s.activation_mode, "hold");
        assert!(s.cjk_autocorrect);
        assert!(s.live_noise_gate);
        assert_eq!(s.bar_position, "fixed");
    }

    #[test]
    fn malformed_or_missing_settings_fall_back_to_default() {
        let dir = std::env::temp_dir().join(format!("eve-config-test2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let missing = dir.join("absent.json");
        assert_eq!(load(&missing).shortcut, Settings::default().shortcut);

        let bad = dir.join("bad.json");
        std::fs::write(&bad, "{not json").unwrap();
        let s = load(&bad);
        std::fs::remove_file(&bad).ok();
        assert_eq!(s, Settings::default());
    }
}
