//! Settings, platform info, provider API keys, autostart, and backup bundles.

use tauri::{AppHandle, State};
use tauri_plugin_global_shortcut::Shortcut;

use super::shortcuts::swap_global_shortcut;
use crate::command_mode;
use crate::config::{self, Settings};
use crate::secrets;
use crate::state::AppState;

/// Cross-platform info the frontend can't derive on its own. `os` is the Rust
/// `std::env::consts::OS` value ("windows" | "macos" | "linux"); `is_wayland`
/// distinguishes the Linux session type (always false off Linux) since JS can't
/// see it. Phase 1: lets the UI relabel modifiers (Alt -> Option on macOS) and,
/// in later phases, hide Wayland-incompatible trigger pickers.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub os: String,
    pub is_wayland: bool,
}

#[tauri::command]
pub fn get_platform_info() -> PlatformInfo {
    PlatformInfo {
        os: std::env::consts::OS.to_string(),
        is_wayland: crate::platform::is_wayland(),
    }
}

/// Phase 2 (macOS): whether Eve is trusted for Accessibility, which the event
/// tap (bare-modifier + mouse triggers) needs. Always `true` off macOS, where no
/// such permission exists, so the frontend can gate its banner on `!trusted`
/// uniformly.
#[tauri::command]
pub fn check_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::permissions::is_trusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Phase 2 (macOS): open the system Accessibility prompt (deep-links to System
/// Settings -> Privacy & Security -> Accessibility) and return the trust state
/// afterward. No-op returning `true` off macOS.
#[tauri::command]
pub fn request_accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::permissions::prompt_trust()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().clone()
}

#[tauri::command]
pub fn update_settings(state: State<AppState>, settings: Settings) -> Result<(), String> {
    *state.settings.lock() = settings.clone();
    // Parity A3/A4: bare-modifier / mouse-button triggers live in the low-level
    // hooks (Windows) / event tap (macOS), which read from atomics - republish so
    // edits apply immediately.
    #[cfg(windows)]
    crate::hooks::update_triggers(&settings);
    #[cfg(target_os = "macos")]
    crate::platform::macos::input::update_triggers(&settings);
    #[cfg(target_os = "linux")]
    crate::platform::linux::x11::update_triggers(&settings);
    config::save(&state.settings_path, &settings).map_err(|e| e.to_string())
}

/// Available microphone names for the Settings picker. The UI prepends a
/// "System default" choice (the empty string) itself.
#[tauri::command]
pub fn list_input_devices() -> Vec<String> {
    crate::audio::input_devices()
}

#[tauri::command]
pub fn store_api_key(key: String) -> Result<(), String> {
    secrets::set_api_key(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn has_api_key() -> bool {
    secrets::has_api_key()
}

#[tauri::command]
pub fn clear_api_key() -> Result<(), String> {
    secrets::delete_api_key().map_err(|e| e.to_string())
}

// --- Phase 2 providers: per-provider API keys ---------------------------------

/// Parse a provider id from the wire; unknown ids surface as an error rather
/// than silently writing to the wrong keychain slot.
fn parse_provider(provider: &str) -> Result<secrets::ProviderKey, String> {
    secrets::ProviderKey::parse(provider)
        .ok_or_else(|| format!("Unknown provider \"{provider}\""))
}

/// Store an API key for one of the supported cloud providers. The key goes to
/// the OS keychain via `secrets.rs` - never to settings or disk.
#[tauri::command]
pub fn store_provider_key(provider: String, key: String) -> Result<(), String> {
    let p = parse_provider(&provider)?;
    secrets::set_provider_key(p, &key).map_err(|e| e.to_string())
}

/// Whether a key is configured for the given provider.
#[tauri::command]
pub fn has_provider_key(provider: String) -> Result<bool, String> {
    let p = parse_provider(&provider)?;
    Ok(secrets::has_provider_key(p))
}

/// Remove the stored key for the given provider.
#[tauri::command]
pub fn clear_provider_key(provider: String) -> Result<(), String> {
    let p = parse_provider(&provider)?;
    secrets::delete_provider_key(p).map_err(|e| e.to_string())
}

// --- 4.6: full backup bundle --------------------------------------------------

/// Export settings (secrets excluded - keys live only in the OS keychain),
/// dictionary, snippets, Flow Styles, transforms, and optionally all history to
/// a single JSON file at `path`.
#[tauri::command]
pub fn export_backup(
    state: State<AppState>,
    path: String,
    include_history: bool,
) -> Result<(), String> {
    let settings = state.settings.lock().clone();
    let bundle =
        crate::backup::build(&state.db, &settings, include_history).map_err(|e| e.to_string())?;
    crate::backup::write_to(&bundle, std::path::Path::new(&path)).map_err(|e| e.to_string())
}

/// Restore a backup bundle written by [`export_backup`]: merge data rows and
/// apply the bundled settings, re-registering shortcuts/triggers so everything
/// is live without a restart.
#[tauri::command]
pub fn import_backup(
    app: AppHandle,
    state: State<AppState>,
    path: String,
) -> Result<crate::backup::ImportSummary, String> {
    let bundle =
        crate::backup::read_from(std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    let summary = crate::backup::merge_data(&state.db, &bundle).map_err(|e| e.to_string())?;

    apply_bundled_settings(&app, &state, bundle.settings)?;

    state.hot_cache.invalidate();
    command_mode::register_transform_shortcuts(&app, &state);
    command_mode::register_style_shortcuts(&app, &state);
    Ok(summary)
}

/// Apply a settings snapshot from a bundle, swapping any registered global
/// shortcut whose accelerator changed so the imported triggers work live.
fn apply_bundled_settings(
    app: &AppHandle,
    state: &State<AppState>,
    next: Settings,
) -> Result<(), String> {
    use std::str::FromStr;
    let swap_if_changed = |field: &std::sync::Arc<parking_lot::Mutex<Shortcut>>,
                           old_accel: &str,
                           new_accel: &str|
     -> Result<(), String> {
        if old_accel == new_accel {
            return Ok(());
        }
        let new_sc = Shortcut::from_str(new_accel)
            .map_err(|_| format!("\"{new_accel}\" isn't a supported shortcut"))?;
        swap_global_shortcut(app, *field.lock(), new_sc)?;
        *field.lock() = new_sc;
        Ok(())
    };

    let prev = state.settings.lock().clone();
    swap_if_changed(&state.main_shortcut, &prev.shortcut, &next.shortcut)?;
    swap_if_changed(&state.copy_shortcut, &prev.copy_shortcut, &next.copy_shortcut)?;
    swap_if_changed(
        &state.command_shortcut,
        &prev.command_shortcut,
        &next.command_shortcut,
    )?;
    swap_if_changed(
        &state.scratchpad_shortcut,
        &prev.scratchpad_shortcut,
        &next.scratchpad_shortcut,
    )?;
    swap_if_changed(&state.undo_shortcut, &prev.undo_shortcut, &next.undo_shortcut)?;

    // Republish bare-modifier / mouse-button triggers to the low-level backends.
    #[cfg(windows)]
    crate::hooks::update_triggers(&next);
    #[cfg(target_os = "macos")]
    crate::platform::macos::input::update_triggers(&next);
    #[cfg(target_os = "linux")]
    crate::platform::linux::x11::update_triggers(&next);

    *state.settings.lock() = next.clone();
    config::save(&state.settings_path, &next).map_err(|e| e.to_string())
}

// --- Phase 11: startup --------------------------------------------------------

/// Toggle launch-at-startup (registers/unregisters Eve with the OS) and persist
/// the choice.
#[tauri::command]
pub fn set_autostart(app: AppHandle, state: State<AppState>, enabled: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    if enabled {
        mgr.enable().map_err(|e| e.to_string())?;
    } else {
        mgr.disable().map_err(|e| e.to_string())?;
    }
    let mut s = state.settings.lock();
    s.launch_at_startup = enabled;
    config::save(&state.settings_path, &s).map_err(|e| e.to_string())?;
    Ok(())
}
