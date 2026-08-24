//! Text-expansion snippet commands.

use tauri::State;

use crate::db::snippets::{self, Snippet, SnippetImport};
use crate::state::AppState;

// --- Phase 5: snippets -------------------------------------------------------

#[tauri::command]
pub fn get_snippets(state: State<AppState>, query: Option<String>) -> Result<Vec<Snippet>, String> {
    snippets::list(&state.db.lock(), query.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_snippet(
    state: State<AppState>,
    trigger_phrase: String,
    expansion: String,
    is_active: bool,
) -> Result<i64, String> {
    let trigger = trigger_phrase.trim();
    let expansion = expansion.trim();
    if trigger.is_empty() {
        return Err("Trigger phrase cannot be empty".into());
    }
    if expansion.is_empty() {
        return Err("Expansion cannot be empty".into());
    }
    let now = chrono::Utc::now().timestamp_millis();
    let id = snippets::upsert(&state.db.lock(), trigger, expansion, is_active, now)
        .map_err(|e| e.to_string())?;
    state.hot_cache.invalidate();
    Ok(id)
}

#[tauri::command]
pub fn delete_snippet(state: State<AppState>, id: i64) -> Result<(), String> {
    snippets::delete(&state.db.lock(), id).map_err(|e| e.to_string())?;
    state.hot_cache.invalidate();
    Ok(())
}

/// Import a JSON array of `{ triggerPhrase, expansion, isActive? }` objects.
/// Empty triggers/expansions are skipped. Returns the number imported.
#[tauri::command]
pub fn import_snippets_json(state: State<AppState>, json: String) -> Result<i64, String> {
    let items: Vec<SnippetImport> =
        serde_json::from_str(&json).map_err(|e| format!("Invalid JSON: {e}"))?;
    let now = chrono::Utc::now().timestamp_millis();
    let conn = state.db.lock();
    let mut count = 0i64;
    for item in items {
        let trigger = item.trigger_phrase.trim();
        let expansion = item.expansion.trim();
        if trigger.is_empty() || expansion.is_empty() {
            continue;
        }
        if snippets::upsert(&conn, trigger, expansion, item.is_active, now).is_ok() {
            count += 1;
        }
    }
    state.hot_cache.invalidate();
    Ok(count)
}

/// Export all snippets as a pretty-printed JSON array.
#[tauri::command]
pub fn export_snippets_json(state: State<AppState>) -> Result<String, String> {
    let entries = snippets::list(&state.db.lock(), None).map_err(|e| e.to_string())?;
    let items: Vec<SnippetImport> = entries
        .into_iter()
        .map(|s| SnippetImport {
            trigger_phrase: s.trigger_phrase,
            expansion: s.expansion,
            is_active: s.is_active,
        })
        .collect();
    serde_json::to_string_pretty(&items).map_err(|e| e.to_string())
}
