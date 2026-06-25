//! Show / hide / position the floating Flow Bar window, plus a shared failure
//! helper that surfaces an error in the bar and then dismisses it.

use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

use crate::events;

pub fn show_flowbar(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(events::FLOWBAR) {
        let _ = w.show();
    }
}

/// Pin the Flow Bar to the bottom-center of the primary monitor.
pub fn position_flowbar(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(events::FLOWBAR) {
        if let Ok(Some(monitor)) = w.primary_monitor() {
            let ms = monitor.size();
            if let Ok(ws) = w.outer_size() {
                let x = (ms.width as i32 - ws.width as i32) / 2;
                let y = ms.height as i32 - ws.height as i32 - 96;
                let _ = w.set_position(PhysicalPosition::new(x.max(0), y.max(0)));
            }
        }
    }
}

pub fn hide_flowbar_after(app: AppHandle, ms: u64) {
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(ms));
        if let Some(w) = app.get_webview_window(events::FLOWBAR) {
            let _ = w.hide();
        }
    });
}

/// Emit an error to the Flow Bar and dismiss it shortly after.
pub fn fail(app: &AppHandle, msg: &str) {
    let _ = app.emit_to(
        events::FLOWBAR,
        events::ERROR,
        events::ErrorPayload {
            message: msg.to_string(),
        },
    );
    hide_flowbar_after(app.clone(), 2600);
}
