//! System tray icon + menu. The app runs in the background; the tray is the
//! way back into the Hub.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};

use crate::events;

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Eve", true, None::<&str>)?;
    let update = MenuItem::with_id(app, "update", "Check for updates…", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &update, &sep, &quit])?;

    let _tray = TrayIconBuilder::with_id("eve-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Eve — voice dictation")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            // Phase 11: surface the check in the Hub's Settings (the Hub listens
            // for `app://check-update`, jumps to Settings, and runs the check).
            "update" => {
                show_main(app);
                let _ = app.emit_to(events::MAIN, events::CHECK_UPDATE, ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
