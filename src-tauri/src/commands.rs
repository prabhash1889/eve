//! Tauri commands invoked from the frontend (Hub settings UI).

use tauri::{AppHandle, State};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::command_mode;
use crate::config::{self, Settings};
use crate::db::dictionary::{self, DictionaryEntry};
use crate::db::flow_styles::{self, FlowStyle};
use crate::db::queries::{self, HistoryPage, Stats};
use crate::db::scratchpad::{self, ScratchpadTab};
use crate::db::snippets::{self, Snippet, SnippetImport};
use crate::db::transforms::{self, Transform};
use crate::models::{self, ModelStatus};
use crate::secrets;
use crate::state::{self, AppState};
use crate::window_mgmt;

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

// --- Phase 5: snippets -------------------------------------------------------

#[tauri::command]
pub fn get_snippets(
    state: State<AppState>,
    query: Option<String>,
) -> Result<Vec<Snippet>, String> {
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
    snippets::upsert(&state.db.lock(), trigger, expansion, is_active, now).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_snippet(state: State<AppState>, id: i64) -> Result<(), String> {
    snippets::delete(&state.db.lock(), id).map_err(|e| e.to_string())
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

// --- Phase 6: Flow Styles ----------------------------------------------------

#[tauri::command]
pub fn get_flow_styles(state: State<AppState>) -> Result<Vec<FlowStyle>, String> {
    flow_styles::list(&state.db.lock()).map_err(|e| e.to_string())
}

/// Insert or update the Flow Style for an app category (one style per category).
/// `name` defaults to the category label when blank.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn upsert_flow_style(
    state: State<AppState>,
    name: String,
    app_category: String,
    tone: String,
    system_prompt: String,
    writing_sample: String,
    is_active: bool,
) -> Result<i64, String> {
    let category = app_category.trim();
    if category.is_empty() {
        return Err("Category cannot be empty".into());
    }
    let name = {
        let n = name.trim();
        if n.is_empty() { category } else { n }
    };
    let now = chrono::Utc::now().timestamp_millis();
    flow_styles::upsert(
        &state.db.lock(),
        name,
        category,
        tone.trim(),
        system_prompt.trim(),
        writing_sample.trim(),
        is_active,
        now,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_flow_style(state: State<AppState>, id: i64) -> Result<(), String> {
    flow_styles::delete(&state.db.lock(), id).map_err(|e| e.to_string())
}

// --- Phase 7: Command Mode + Transforms --------------------------------------

/// Set (and re-register) the Command Mode push-to-talk shortcut.
#[tauri::command]
pub fn set_command_shortcut(
    app: AppHandle,
    state: State<AppState>,
    shortcut: String,
) -> Result<(), String> {
    let new_shortcut = state::parse_shortcut(&shortcut);
    let old_shortcut = state.command_shortcut.lock().clone();

    let gs = app.global_shortcut();
    let _ = gs.unregister(old_shortcut);
    gs.register(new_shortcut.clone()).map_err(|e| e.to_string())?;

    *state.command_shortcut.lock() = new_shortcut;

    let mut s = state.settings.lock();
    s.command_shortcut = shortcut;
    let _ = config::save(&state.settings_path, &s);
    Ok(())
}

/// Run the Command Mode LLM step directly (selection rewrite or inline
/// generation). Exposed for the UI / scripting; the live flow calls the same
/// `command_mode::run_command` internally.
#[tauri::command]
pub async fn command_mode_rewrite(
    selected_text: Option<String>,
    instruction: String,
) -> Result<String, String> {
    command_mode::run_command(selected_text.as_deref(), &instruction)
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
    command_mode::register_transform_shortcuts(&app, &state);
    Ok(new_id)
}

#[tauri::command]
pub fn delete_transform(app: AppHandle, state: State<AppState>, id: i64) -> Result<(), String> {
    transforms::delete(&state.db.lock(), id).map_err(|e| e.to_string())?;
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
    command_mode::run_transform(&transform.system_prompt, &text)
        .await
        .map_err(|e| e.to_string())
}

// --- Phase 9: scratchpad -----------------------------------------------------

/// Set (and re-register) the global shortcut that opens the Scratchpad window.
#[tauri::command]
pub fn set_scratchpad_shortcut(
    app: AppHandle,
    state: State<AppState>,
    shortcut: String,
) -> Result<(), String> {
    let new_shortcut = state::parse_shortcut(&shortcut);
    let old_shortcut = state.scratchpad_shortcut.lock().clone();

    let gs = app.global_shortcut();
    let _ = gs.unregister(old_shortcut);
    gs.register(new_shortcut.clone()).map_err(|e| e.to_string())?;

    *state.scratchpad_shortcut.lock() = new_shortcut;

    let mut s = state.settings.lock();
    s.scratchpad_shortcut = shortcut;
    let _ = config::save(&state.settings_path, &s);
    Ok(())
}

/// Show (and focus) the Scratchpad window — wired to the Hub sidebar item.
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

// --- Local models ------------------------------------------------------------

/// The local-model catalog with per-model installed/active/downloading flags.
#[tauri::command]
pub fn list_models(app: AppHandle, state: State<AppState>) -> Vec<ModelStatus> {
    models::list(&app, &state)
}

/// Start (or no-op if already running) a streamed download. Progress is reported
/// via `model://progress` / `model://done` / `model://error` events.
#[tauri::command]
pub fn download_model(app: AppHandle, id: String) -> Result<(), String> {
    models::start_download(app, id)
}

/// Request cancellation of an in-flight download.
#[tauri::command]
pub fn cancel_model_download(state: State<AppState>, id: String) -> Result<(), String> {
    models::cancel(&state, &id);
    Ok(())
}

/// Delete a downloaded model file from disk.
#[tauri::command]
pub fn delete_model(app: AppHandle, id: String) -> Result<(), String> {
    models::delete(&app, &id)
}

// --- Phase 11: startup & auto-update -----------------------------------------

/// Toggle launch-at-startup (registers/unregisters Eve with the OS) and persist
/// the choice.
#[tauri::command]
pub fn set_autostart(
    app: AppHandle,
    state: State<AppState>,
    enabled: bool,
) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    if enabled {
        mgr.enable().map_err(|e| e.to_string())?;
    } else {
        mgr.disable().map_err(|e| e.to_string())?;
    }
    let mut s = state.settings.lock();
    s.launch_at_startup = enabled;
    let _ = config::save(&state.settings_path, &s);
    Ok(())
}

/// Check the configured GitHub Releases feed for a newer version. Returns the
/// new version string if an update is available, else `None`.
#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(Some(update.version)),
        Ok(None) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Download and install the available update, then relaunch the app. No-op (and
/// returns `false`) when there's nothing to install.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_updater::UpdaterExt;
    let updater = app.updater().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
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
