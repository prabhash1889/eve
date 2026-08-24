//! Global-shortcut rebinding commands.

use tauri::{AppHandle, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::config;
use crate::state::{self, AppState};

/// Swap a registered global shortcut for a new one. The old accelerator is
/// re-registered when the new one can't be registered (e.g. another app owns
/// the combo), so a failed rebind never leaves the app without its trigger.
pub(crate) fn swap_global_shortcut(
    app: &AppHandle,
    old: Shortcut,
    new: Shortcut,
) -> Result<(), String> {
    // Phase 4: on Wayland the plugin is a no-op and the GlobalShortcuts portal
    // owns the bindings, so just ask the portal task to re-bind. It re-reads the
    // settings, which the caller commits synchronously right after this returns
    // (before the task wakes), so the new accelerator is picked up.
    #[cfg(target_os = "linux")]
    if crate::platform::is_wayland() {
        crate::platform::linux::wayland::request_rebind();
        return Ok(());
    }

    let gs = app.global_shortcut();
    let _ = gs.unregister(old);
    if let Err(e) = gs.register(new) {
        let _ = gs.register(old);
        return Err(e.to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn set_shortcut(
    app: AppHandle,
    state: State<AppState>,
    shortcut: String,
) -> Result<(), String> {
    // Parity A2: the UI lets users capture arbitrary key combos, so an
    // unparseable accelerator must surface as an error (not silently fall back
    // to F8 the way the startup path does).
    use std::str::FromStr;
    let new_shortcut = Shortcut::from_str(&shortcut)
        .map_err(|_| format!("\"{shortcut}\" isn't a supported shortcut"))?;
    let old_shortcut = *state.main_shortcut.lock();

    swap_global_shortcut(&app, old_shortcut, new_shortcut)?;

    *state.main_shortcut.lock() = new_shortcut;

    let mut s = state.settings.lock();
    s.shortcut = shortcut;
    config::save(&state.settings_path, &s).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn set_copy_shortcut(
    app: AppHandle,
    state: State<AppState>,
    shortcut: String,
) -> Result<(), String> {
    let new_shortcut = state::parse_shortcut(&shortcut);
    let old_shortcut = *state.copy_shortcut.lock();

    swap_global_shortcut(&app, old_shortcut, new_shortcut)?;

    *state.copy_shortcut.lock() = new_shortcut;

    let mut s = state.settings.lock();
    s.copy_shortcut = shortcut;
    config::save(&state.settings_path, &s).map_err(|e| e.to_string())?;
    Ok(())
}

/// Set (and re-register) the Command Mode push-to-talk shortcut.
#[tauri::command]
pub fn set_command_shortcut(
    app: AppHandle,
    state: State<AppState>,
    shortcut: String,
) -> Result<(), String> {
    let new_shortcut = state::parse_shortcut(&shortcut);
    let old_shortcut = *state.command_shortcut.lock();

    swap_global_shortcut(&app, old_shortcut, new_shortcut)?;

    *state.command_shortcut.lock() = new_shortcut;

    let mut s = state.settings.lock();
    s.command_shortcut = shortcut;
    config::save(&state.settings_path, &s).map_err(|e| e.to_string())?;
    Ok(())
}

/// Set (and re-register) the global shortcut that opens the Scratchpad window.
#[tauri::command]
pub fn set_scratchpad_shortcut(
    app: AppHandle,
    state: State<AppState>,
    shortcut: String,
) -> Result<(), String> {
    let new_shortcut = state::parse_shortcut(&shortcut);
    let old_shortcut = *state.scratchpad_shortcut.lock();

    swap_global_shortcut(&app, old_shortcut, new_shortcut)?;

    *state.scratchpad_shortcut.lock() = new_shortcut;

    let mut s = state.settings.lock();
    s.scratchpad_shortcut = shortcut;
    config::save(&state.settings_path, &s).map_err(|e| e.to_string())?;
    Ok(())
}

/// Set (and re-register) the global shortcut that deletes the last injection
/// (4.2 undo / recall).
#[tauri::command]
pub fn set_undo_shortcut(
    app: AppHandle,
    state: State<AppState>,
    shortcut: String,
) -> Result<(), String> {
    let new_shortcut = state::parse_shortcut(&shortcut);
    let old_shortcut = *state.undo_shortcut.lock();

    swap_global_shortcut(&app, old_shortcut, new_shortcut)?;

    *state.undo_shortcut.lock() = new_shortcut;

    let mut s = state.settings.lock();
    s.undo_shortcut = shortcut;
    config::save(&state.settings_path, &s).map_err(|e| e.to_string())?;
    Ok(())
}
