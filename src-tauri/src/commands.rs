//! Tauri commands invoked from the frontend (Hub settings UI).

use tauri::{AppHandle, State};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::config::{self, Settings};
use crate::db::dictionary::{self, DictionaryEntry};
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

// --- Phase 4: dictionary -----------------------------------------------------

#[tauri::command]
pub fn get_dictionary(
    state: State<AppState>,
    query: Option<String>,
) -> Result<Vec<DictionaryEntry>, String> {
    dictionary::list(&state.db.lock(), query.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_dictionary_entry(
    state: State<AppState>,
    word: String,
    replacement: Option<String>,
    is_starred: bool,
) -> Result<i64, String> {
    let word = word.trim();
    if word.is_empty() {
        return Err("Word cannot be empty".into());
    }
    // Normalize an empty replacement to NULL (boost-only term).
    let replacement = replacement
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty());
    let now = chrono::Utc::now().timestamp_millis();
    dictionary::upsert(
        &state.db.lock(),
        word,
        replacement.as_deref(),
        is_starred,
        "user",
        now,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_dictionary_entry(state: State<AppState>, id: i64) -> Result<(), String> {
    dictionary::delete(&state.db.lock(), id).map_err(|e| e.to_string())
}

/// Import `word,replacement,starred` rows. The header line is optional; blank
/// lines and empty words are skipped. Returns the number of rows imported.
#[tauri::command]
pub fn import_dictionary_csv(state: State<AppState>, csv: String) -> Result<i64, String> {
    let now = chrono::Utc::now().timestamp_millis();
    let conn = state.db.lock();
    let mut count = 0i64;
    for line in csv.lines() {
        let fields = parse_csv_line(line);
        let word = fields.first().map(|s| s.trim()).unwrap_or("");
        if word.is_empty() || word.eq_ignore_ascii_case("word") {
            continue; // skip blanks and an optional header row
        }
        let replacement = fields
            .get(1)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let is_starred = fields
            .get(2)
            .map(|s| matches!(s.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "star"))
            .unwrap_or(false);
        if dictionary::upsert(&conn, word, replacement.as_deref(), is_starred, "import", now)
            .is_ok()
        {
            count += 1;
        }
    }
    Ok(count)
}

/// Export the whole dictionary as `word,replacement,starred` CSV with a header.
#[tauri::command]
pub fn export_dictionary_csv(state: State<AppState>) -> Result<String, String> {
    let entries = dictionary::list(&state.db.lock(), None).map_err(|e| e.to_string())?;
    let mut out = String::from("word,replacement,starred\n");
    for e in entries {
        out.push_str(&csv_field(&e.word));
        out.push(',');
        out.push_str(&csv_field(e.replacement.as_deref().unwrap_or("")));
        out.push(',');
        out.push_str(if e.is_starred { "1" } else { "0" });
        out.push('\n');
    }
    Ok(out)
}

/// Minimal RFC-4180-ish CSV line parser: handles `"`-quoted fields with escaped
/// `""` and embedded commas. Sufficient for our 3-column dictionary export.
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes => {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
            '"' => in_quotes = true,
            ',' if !in_quotes => {
                fields.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

/// Quote a CSV field if it contains a comma, quote, or newline.
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
