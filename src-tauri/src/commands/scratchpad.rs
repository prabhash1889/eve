//! Scratchpad window and tab commands.

use tauri::{AppHandle, State};

use crate::db::scratchpad::{self, ScratchpadTab};
use crate::state::AppState;
use crate::window_mgmt;

/// Show (and focus) the Scratchpad window - wired to the Hub sidebar item.
#[tauri::command]
pub fn open_scratchpad(app: AppHandle) {
    window_mgmt::open_scratchpad(&app);
}

#[tauri::command]
pub fn get_scratchpad_tabs(state: State<AppState>) -> Result<Vec<ScratchpadTab>, String> {
    scratchpad::list(&state.db.lock()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_scratchpad_tab(
    state: State<AppState>,
    title: Option<String>,
) -> Result<ScratchpadTab, String> {
    let now = chrono::Utc::now().timestamp_millis();
    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Untitled".into());
    scratchpad::create(&state.db.lock(), &title, now).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_scratchpad_tab(
    state: State<AppState>,
    id: i64,
    title: String,
    content: String,
) -> Result<(), String> {
    let title = title.trim();
    let title = if title.is_empty() { "Untitled" } else { title };
    let now = chrono::Utc::now().timestamp_millis();
    scratchpad::save(&state.db.lock(), id, title, &content, now).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_scratchpad_tab(state: State<AppState>, id: i64) -> Result<(), String> {
    scratchpad::delete(&state.db.lock(), id).map_err(|e| e.to_string())
}
