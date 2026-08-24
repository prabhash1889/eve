//! Flow Style commands.

use tauri::{AppHandle, State};

use crate::command_mode;
use crate::db::flow_styles::{self, FlowStyle};
use crate::state::AppState;

// --- Phase 6: Flow Styles ----------------------------------------------------

#[tauri::command]
pub fn get_flow_styles(state: State<AppState>) -> Result<Vec<FlowStyle>, String> {
    flow_styles::list(&state.db.lock()).map_err(|e| e.to_string())
}

/// Insert or update a Flow Style for an app category. `app_process` empty =
/// whole-category default; otherwise the style is scoped to that exact process
/// name (normalized to lowercase, matching how capture reports processes) and
/// takes precedence over the category default in the pipeline. `name` defaults
/// to the category label when blank.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn upsert_flow_style(
    app: AppHandle,
    state: State<AppState>,
    name: String,
    app_category: String,
    app_process: String,
    tone: String,
    system_prompt: String,
    writing_sample: String,
    is_active: bool,
    shortcut: String,
) -> Result<i64, String> {
    let category = app_category.trim();
    if category.is_empty() {
        return Err("Category cannot be empty".into());
    }
    let process = app_process.trim().to_ascii_lowercase();
    let now = chrono::Utc::now().timestamp_millis();
    let id = flow_styles::upsert(
        &state.db.lock(),
        name.trim(),
        category,
        &process,
        tone.trim(),
        system_prompt.trim(),
        writing_sample.trim(),
        is_active,
        shortcut.trim(),
        now,
    )
    .map_err(|e| e.to_string())?;
    state.hot_cache.invalidate();
    // 4.4: a changed/added/removed accelerator takes effect immediately.
    command_mode::register_style_shortcuts(&app, &state);
    Ok(id)
}

#[tauri::command]
pub fn delete_flow_style(
    app: AppHandle,
    state: State<AppState>,
    id: i64,
) -> Result<(), String> {
    flow_styles::delete(&state.db.lock(), id).map_err(|e| e.to_string())?;
    state.hot_cache.invalidate();
    command_mode::register_style_shortcuts(&app, &state);
    Ok(())
}
