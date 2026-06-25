//! Tauri commands invoked from the frontend (Hub settings UI).

use tauri::{AppHandle, State};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::config::{self, Settings};
use crate::db::queries::{self, HistoryPage, Stats};
use crate::secrets;
use crate::state::{self, AppState};

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().clone()
}

#[tauri::command]
pub fn update_settings(state: State<AppState>, settings: Settings) -> Result<(), String> {
    *state.settings.lock() = settings.clone();
    config::save(&state.settings_path, &settings).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_shortcut(
    app: AppHandle,
    state: State<AppState>,
    shortcut: String,
) -> Result<(), String> {
    let new_shortcut = state::parse_shortcut(&shortcut);
    let old_shortcut = state.main_shortcut.lock().clone();

    let gs = app.global_shortcut();
    let _ = gs.unregister(old_shortcut);
    gs.register(new_shortcut.clone()).map_err(|e| e.to_string())?;

    *state.main_shortcut.lock() = new_shortcut;

    let mut s = state.settings.lock();
    s.shortcut = shortcut;
    let _ = config::save(&state.settings_path, &s);
    Ok(())
}

#[tauri::command]
pub fn set_copy_shortcut(
    app: AppHandle,
    state: State<AppState>,
    shortcut: String,
) -> Result<(), String> {
    let new_shortcut = state::parse_shortcut(&shortcut);
    let old_shortcut = state.copy_shortcut.lock().clone();

    let gs = app.global_shortcut();
    let _ = gs.unregister(old_shortcut);
    gs.register(new_shortcut.clone()).map_err(|e| e.to_string())?;

    *state.copy_shortcut.lock() = new_shortcut;

    let mut s = state.settings.lock();
    s.copy_shortcut = shortcut;
    let _ = config::save(&state.settings_path, &s);
    Ok(())
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

// --- Phase 3: history & stats -------------------------------------------------

#[tauri::command]
pub fn get_history(
    state: State<AppState>,
    page: i64,
    per_page: i64,
    query: Option<String>,
) -> Result<HistoryPage, String> {
    queries::get_history(&state.db.lock(), page, per_page, query).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_transcript(state: State<AppState>, id: i64) -> Result<(), String> {
    let now = chrono::Utc::now().timestamp_millis();
    queries::soft_delete(&state.db.lock(), id, now).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn recover_transcript(state: State<AppState>, id: i64) -> Result<(), String> {
    queries::recover(&state.db.lock(), id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_history(state: State<AppState>) -> Result<(), String> {
    let now = chrono::Utc::now().timestamp_millis();
    queries::clear_history(&state.db.lock(), now).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_stats(state: State<AppState>, range: String) -> Result<Stats, String> {
    let since = range_since(&range);
    queries::get_stats(&state.db.lock(), since).map_err(|e| e.to_string())
}

/// Map a UI range token to an epoch-ms lower bound (0 = all time).
fn range_since(range: &str) -> i64 {
    use chrono::{Duration, Utc};
    let now = Utc::now();
    let start = match range {
        "day" => now - Duration::days(1),
        "week" => now - Duration::days(7),
        "month" => now - Duration::days(30),
        _ => return 0,
    };
    start.timestamp_millis()
}
