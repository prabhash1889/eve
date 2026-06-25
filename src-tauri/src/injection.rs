//! Inject transcribed text into whatever app currently has focus.
//!
//! Primary strategy ("paste"): save the clipboard, write our text, restore
//! focus to the target window (captured at record start), simulate Ctrl+V via
//! Win32 SendInput, then restore the clipboard. Fallback ("type"): synthesize
//! the characters with enigo for apps that block paste.

use std::thread;
use std::time::Duration;

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

#[cfg(windows)]
use windows::Win32::Foundation::HWND;
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

pub fn inject(app: &AppHandle, text: &str, hwnd: isize, strategy: &str) -> anyhow::Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    if strategy == "type" {
        return inject_type(text);
    }
    inject_paste(app, text, hwnd)
}

#[cfg(windows)]
fn inject_paste(app: &AppHandle, text: &str, hwnd: isize) -> anyhow::Result<()> {
    let clip = app.clipboard();
    let previous = clip.read_text().ok();

    clip.write_text(text.to_string())
        .map_err(|e| anyhow::anyhow!("clipboard write failed: {e}"))?;

    restore_focus(hwnd);
    thread::sleep(Duration::from_millis(40));
    send_ctrl_v();
    thread::sleep(Duration::from_millis(150));

    // Restore the user's prior clipboard (skip silently if it was empty).
    if let Some(prev) = previous {
        let _ = clip.write_text(prev);
    }
    Ok(())
}

#[cfg(not(windows))]
fn inject_paste(_app: &AppHandle, text: &str, _hwnd: isize) -> anyhow::Result<()> {
    // Non-Windows fallback for now: type the text out.
    inject_type(text)
}

fn inject_type(text: &str) -> anyhow::Result<()> {
    use enigo::{Enigo, Keyboard, Settings};
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| anyhow::anyhow!("enigo init: {e}"))?;
    enigo
        .text(text)
        .map_err(|e| anyhow::anyhow!("enigo type: {e}"))?;
    Ok(())
}

#[cfg(windows)]
fn restore_focus(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        let _ = SetForegroundWindow(HWND(hwnd as *mut core::ffi::c_void));
    }
}

#[cfg(windows)]
fn send_ctrl_v() {
    unsafe {
        keybd_event(VK_CONTROL.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(VK_V.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(VK_V.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }
}
