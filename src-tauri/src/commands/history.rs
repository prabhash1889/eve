//! History, stats, and re-injection commands.

use tauri::{AppHandle, State};

use crate::db::queries::{self, HistoryPage, Stats};
use crate::injection;
use crate::state::AppState;

/// Paste arbitrary text into whatever app currently has focus. Backs the
/// History page's "Paste" button (re-inject an old transcript): wraps the same
/// `injection::inject` path dictation uses, so focus restore, clipboard save/
/// restore, and the INJECTING guard all apply unchanged. The target window is
/// whatever is focused *now*, not where the transcript originally landed.
#[tauri::command]
pub async fn paste_text(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
) -> Result<(), String> {
    // Don't fight an in-flight capture/pipeline for focus or the clipboard.
    if state.is_recording.load(std::sync::atomic::Ordering::SeqCst)
        || state.is_processing.load(std::sync::atomic::Ordering::SeqCst)
    {
        return Err("Eve is busy - try again in a moment".into());
    }
    let front = crate::platform::frontmost(&app);
    let strategy = state.settings.lock().inject_strategy.clone();
    tauri::async_runtime::spawn_blocking(move || {
        injection::inject(&app, &text, front.handle, &strategy)
    })
    .await
    .map_err(|_| "Couldn't paste the text".to_string())?
    .map_err(|e| e.to_string())
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
