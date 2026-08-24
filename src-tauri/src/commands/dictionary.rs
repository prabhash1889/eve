//! Dictionary commands, including CSV import/export.

use tauri::State;

use crate::csv;
use crate::db::dictionary::{self, DictionaryEntry};
use crate::state::AppState;

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
    let id = dictionary::upsert(
        &state.db.lock(),
        word,
        replacement.as_deref(),
        is_starred,
        "user",
        now,
    )
    .map_err(|e| e.to_string())?;
    state.hot_cache.invalidate();
    Ok(id)
}

#[tauri::command]
pub fn delete_dictionary_entry(state: State<AppState>, id: i64) -> Result<(), String> {
    dictionary::delete(&state.db.lock(), id).map_err(|e| e.to_string())?;
    state.hot_cache.invalidate();
    Ok(())
}

/// Import `word,replacement,starred` rows. The header line is optional; blank
/// lines and empty words are skipped. Returns the number of rows imported.
#[tauri::command]
pub fn import_dictionary_csv(state: State<AppState>, csv: String) -> Result<i64, String> {
    let now = chrono::Utc::now().timestamp_millis();
    let conn = state.db.lock();
    let mut count = 0i64;
    for line in csv.lines() {
        let fields = csv::parse_line(line);
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
            .map(|s| {
                matches!(
                    s.trim().to_lowercase().as_str(),
                    "1" | "true" | "yes" | "star"
                )
            })
            .unwrap_or(false);
        if dictionary::upsert(
            &conn,
            word,
            replacement.as_deref(),
            is_starred,
            "import",
            now,
        )
        .is_ok()
        {
            count += 1;
        }
    }
    state.hot_cache.invalidate();
    Ok(count)
}

/// Export the whole dictionary as `word,replacement,starred` CSV with a header.
#[tauri::command]
pub fn export_dictionary_csv(state: State<AppState>) -> Result<String, String> {
    let entries = dictionary::list(&state.db.lock(), None).map_err(|e| e.to_string())?;
    let mut out = String::from("word,replacement,starred\n");
    for e in entries {
        out.push_str(&csv::field(&e.word));
        out.push(',');
        out.push_str(&csv::field(e.replacement.as_deref().unwrap_or("")));
        out.push(',');
        out.push_str(if e.is_starred { "1" } else { "0" });
        out.push('\n');
    }
    Ok(out)
}
