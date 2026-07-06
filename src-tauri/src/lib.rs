mod audio;
mod command_mode;
mod commands;
mod config;
mod context;
mod db;
mod events;
mod file_transcribe;
#[cfg(windows)]
mod hooks;
mod hotkey;
mod injection;
mod llm;
mod models;
mod pipeline;
mod polish;
mod secrets;
mod state;
mod text_processing;
mod timing;
mod transcription;
mod tray;
mod window_mgmt;
mod sound;

use tauri::{Manager, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        // Phase C: native file picker for "Transcribe files…".
        .plugin(tauri_plugin_dialog::init())
        // Phase 11: self-update + launch-at-startup. Autostart registers Eve with
        // no extra CLI args; the updater reads its endpoint/pubkey from
        // `tauri.conf.json` (`plugins.updater`).
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    let state = app.state::<AppState>();
                    let st: &AppState = &state;
                    let is_main = *st.main_shortcut.lock() == *shortcut;
                    let is_escape = st.escape_shortcut == *shortcut;
                    let is_copy = *st.copy_shortcut.lock() == *shortcut;
                    let is_command = *st.command_shortcut.lock() == *shortcut;
                    let is_scratchpad = *st.scratchpad_shortcut.lock() == *shortcut;
                    // Phase 7: transform accelerators (linear-scanned, like the
                    // reserved shortcuts above).
                    let transform_id = st
                        .transform_shortcuts
                        .lock()
                        .iter()
                        .find(|(sc, _)| sc == shortcut)
                        .map(|(_, id)| *id);
                    match event.state() {
                        ShortcutState::Pressed => {
                            if is_main {
                                hotkey::on_main_pressed(app, st);
                            } else if is_command {
                                command_mode::on_press(app, st);
                            } else if is_escape {
                                hotkey::on_cancel(app, st);
                            } else if is_copy {
                                hotkey::on_copy(app, st);
                            } else if is_scratchpad {
                                window_mgmt::open_scratchpad(app);
                            } else if let Some(id) = transform_id {
                                command_mode::on_transform(app, st, id);
                            }
                        }
                        ShortcutState::Released => {
                            if is_main {
                                hotkey::on_main_released(app, st);
                            } else if is_command {
                                command_mode::on_release(app, st);
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

            // Local-models: on-device weights live alongside the DB.
            let models_dir = data_dir.join("models");
            std::fs::create_dir_all(&models_dir).ok();

            app.manage(AppState::new(settings, settings_path, db, models_dir));

            // Retention: prune saved audio past the configured window. Done AFTER
            // `manage()` and off the setup thread so it can't delay state
            // registration — a release-build webview may `invoke("get_settings")`
            // the instant it loads, and that call rejects if `AppState` isn't
            // managed yet (which would strand the Hub on default settings and
            // re-show first-run onboarding every launch).
            {
                let st = app.state::<AppState>();
                let db = st.db.clone();
                let settings = st.settings.lock().clone();
                std::thread::spawn(move || {
                    prune_audio_on_launch(&db, &settings);
                });
            }

            // Register the push-to-talk shortcut + the copy-last-transcript
            // shortcut. The copy shortcut is best-effort: a bad/duplicate
            // accelerator shouldn't stop the app from launching.
            {
                let state = app.state::<AppState>();
                let main = *state.main_shortcut.lock();
                app.global_shortcut().register(main)?;
                let copy = *state.copy_shortcut.lock();
                let _ = app.global_shortcut().register(copy);
                // Phase 7: Command Mode + any transform accelerators (best-effort).
                let command = *state.command_shortcut.lock();
                let _ = app.global_shortcut().register(command);
                // Phase 9: Scratchpad open shortcut (best-effort).
                let scratchpad = *state.scratchpad_shortcut.lock();
                let _ = app.global_shortcut().register(scratchpad);
                command_mode::register_transform_shortcuts(app.handle(), &state);
            }

            // Parity A3/A4: low-level keyboard/mouse hooks for bare-modifier and
            // mouse-button triggers. Installed once; inert unless configured.
            #[cfg(windows)]
            {
                let settings = app.state::<AppState>().settings.lock().clone();
                hooks::update_triggers(&settings);
                hooks::init(app.handle().clone());
            }

            // Phase 11: reconcile the OS autostart registration with the saved
            // setting (best-effort — a failure here shouldn't block launch).
            {
                use tauri_plugin_autostart::ManagerExt;
                let want = app.state::<AppState>().settings.lock().launch_at_startup;
                let mgr = app.autolaunch();
                let _ = if want { mgr.enable() } else { mgr.disable() };
            }

            // Phase 2: if local STT is the active backend, prewarm the selected
            // model in the background so the first dictation isn't slowed by a
            // cold load. Best-effort and off the UI thread. Phase 5: honor the
            // prewarm-enabled toggle so a user who opted out keeps a cold start.
            {
                let st = app.state::<AppState>();
                let want_prewarm = {
                    let s = st.settings.lock();
                    s.transcription_backend == "local" && s.local_prewarm_enabled
                };
                if want_prewarm {
                    let transcriber = st.transcriber.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = transcriber.prewarm().await;
                    });
                }
            }

            tray::setup(app.handle())?;
            window_mgmt::position_flowbar(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the Hub (or the Scratchpad) hides it rather than quitting —
            // the app keeps running in the tray, and the Scratchpad's tabs stay
            // loaded so reopening is instant.
            if matches!(window.label(), "main" | "scratchpad") {
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
            commands::list_input_devices,
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
            commands::get_snippets,
            commands::upsert_snippet,
            commands::delete_snippet,
            commands::import_snippets_json,
            commands::export_snippets_json,
            commands::get_flow_styles,
            commands::upsert_flow_style,
            commands::delete_flow_style,
            commands::set_command_shortcut,
            commands::command_mode_rewrite,
            commands::get_transforms,
            commands::upsert_transform,
            commands::delete_transform,
            commands::apply_transform,
            commands::set_scratchpad_shortcut,
            commands::open_scratchpad,
            commands::get_scratchpad_tabs,
            commands::create_scratchpad_tab,
            commands::save_scratchpad_tab,
            commands::delete_scratchpad_tab,
            commands::list_models,
            commands::download_model,
            commands::cancel_model_download,
            commands::delete_model,
            commands::prewarm_local_model,
            commands::get_local_whisper_status,
            commands::get_local_transcription_benchmark,
            commands::transcribe_files,
            commands::cancel_queue_item,
            commands::set_autostart,
            commands::check_for_update,
            commands::install_update,
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
    let cutoff =
        chrono::Utc::now().timestamp_millis() - (settings.audio_retention_hours as i64) * 3_600_000;
    let stale = {
        let conn = db.lock();
        db::queries::prune_audio(&conn, cutoff).unwrap_or_default()
    };
    for path in stale {
        let _ = std::fs::remove_file(&path);
    }
}
