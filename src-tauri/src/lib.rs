mod audio;
mod commands;
mod config;
mod db;
mod events;
mod hotkey;
mod injection;
mod pipeline;
mod polish;
mod secrets;
mod state;
mod text_processing;
mod transcription;
mod tray;
mod window_mgmt;

use tauri::{Manager, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    let state = app.state::<AppState>();
                    let st: &AppState = &state;
                    let is_main = *st.main_shortcut.lock() == *shortcut;
                    let is_escape = st.escape_shortcut == *shortcut;
                    let is_copy = *st.copy_shortcut.lock() == *shortcut;
                    match event.state() {
                        ShortcutState::Pressed => {
                            if is_main {
                                hotkey::on_press(app, st);
                            } else if is_escape {
                                hotkey::on_cancel(app, st);
                            } else if is_copy {
                                hotkey::on_copy(app, st);
                            }
                        }
                        ShortcutState::Released => {
                            if is_main {
                                hotkey::on_release(app, st);
                            }
                        }
                    }
                })
                .build(),
        )
        .setup(|app| {
            // Settings live in the app config dir; the API key lives in the keychain.
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir).ok();
            let settings_path = config_dir.join("settings.json");
            let settings = config::load(&settings_path);

            // History DB lives in the app data dir (Phase 3).
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir).ok();
            let db = db::open(&data_dir.join("eve.db"))?;

            // Retention: on launch, prune saved audio past the configured window.
            prune_audio_on_launch(&db, &settings);

            app.manage(AppState::new(settings, settings_path, db));

            // Register the push-to-talk shortcut + the copy-last-transcript
            // shortcut. The copy shortcut is best-effort: a bad/duplicate
            // accelerator shouldn't stop the app from launching.
            {
                let state = app.state::<AppState>();
                let main = state.main_shortcut.lock().clone();
                app.global_shortcut().register(main)?;
                let copy = state.copy_shortcut.lock().clone();
                let _ = app.global_shortcut().register(copy);
            }

            tray::setup(app.handle())?;
            window_mgmt::position_flowbar(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the Hub hides it (the app keeps running in the tray).
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::update_settings,
            commands::set_shortcut,
            commands::set_copy_shortcut,
            commands::store_api_key,
            commands::has_api_key,
            commands::clear_api_key,
            commands::get_history,
            commands::delete_transcript,
            commands::recover_transcript,
            commands::clear_history,
            commands::get_stats,
            commands::get_dictionary,
            commands::upsert_dictionary_entry,
            commands::delete_dictionary_entry,
            commands::import_dictionary_csv,
            commands::export_dictionary_csv,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Phase 3 retention: when the policy is "delete24h", remove saved audio (file +
/// DB pointer) older than `audio_retention_hours`. Best-effort; runs on launch.
fn prune_audio_on_launch(db: &db::Db, settings: &config::Settings) {
    if settings.audio_storage_policy != "delete24h" {
        return;
    }
    let cutoff = chrono::Utc::now().timestamp_millis()
        - (settings.audio_retention_hours as i64) * 3_600_000;
    let stale = {
        let conn = db.lock();
        db::queries::prune_audio(&conn, cutoff).unwrap_or_default()
    };
    for path in stale {
        let _ = std::fs::remove_file(&path);
    }
}
