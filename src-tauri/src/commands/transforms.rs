//! Transform commands plus the Command Mode rewrite entry point.

use tauri::{AppHandle, State};

use crate::command_mode;
use crate::db::transforms::{self, Transform};
use crate::state::AppState;

/// Run the Command Mode LLM step directly (selection rewrite or inline
/// generation). Exposed for the UI / scripting; the live flow calls the same
/// `command_mode::run_command` internally.
#[tauri::command]
pub async fn command_mode_rewrite(
    state: State<'_, AppState>,
    selected_text: Option<String>,
    instruction: String,
) -> Result<String, String> {
    let settings = state.settings.lock().clone();
    command_mode::run_command(&settings, selected_text.as_deref(), &instruction)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_transforms(state: State<AppState>) -> Result<Vec<Transform>, String> {
    transforms::list(&state.db.lock()).map_err(|e| e.to_string())
}

/// Insert (id `None`) or update a transform, then re-register transform
/// accelerators so a changed/added/removed shortcut takes effect immediately.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn upsert_transform(
    app: AppHandle,
    state: State<AppState>,
    id: Option<i64>,
    name: String,
    system_prompt: String,
    shortcut: String,
    auto_apply: bool,
    app_category: String,
    is_active: bool,
) -> Result<i64, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name cannot be empty".into());
    }
    let now = chrono::Utc::now().timestamp_millis();
    let new_id = transforms::upsert(
        &state.db.lock(),
        id,
        name,
        system_prompt.trim(),
        shortcut.trim(),
        auto_apply,
        app_category.trim(),
        is_active,
        now,
    )
    .map_err(|e| e.to_string())?;
    state.hot_cache.invalidate();
    command_mode::register_transform_shortcuts(&app, &state);
    Ok(new_id)
}

#[tauri::command]
pub fn delete_transform(app: AppHandle, state: State<AppState>, id: i64) -> Result<(), String> {
    transforms::delete(&state.db.lock(), id).map_err(|e| e.to_string())?;
    state.hot_cache.invalidate();
    command_mode::register_transform_shortcuts(&app, &state);
    Ok(())
}

/// Apply a saved transform's prompt to arbitrary text and return the result.
#[tauri::command]
pub async fn apply_transform(
    state: State<'_, AppState>,
    id: i64,
    text: String,
) -> Result<String, String> {
    let transform = {
        let conn = state.db.lock();
        transforms::get(&conn, id).map_err(|e| e.to_string())?
    };
    let transform = transform.ok_or_else(|| "Transform not found".to_string())?;
    let settings = state.settings.lock().clone();
    command_mode::run_transform(&settings, &transform.system_prompt, &text)
        .await
        .map_err(|e| e.to_string())
}
