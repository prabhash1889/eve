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
    keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_C, VK_CONTROL, VK_V,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{IsWindow, SetForegroundWindow};

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
    // Abort before touching the clipboard if we can't restore focus to the
    // original target — otherwise Ctrl+V would fire into whatever now has focus.
    if !restore_focus(hwnd) {
        anyhow::bail!("Target window is no longer available — nothing was pasted");
    }

    let clip = app.clipboard();
    let previous = clip.read_text().ok();

    clip.write_text(text.to_string())
        .map_err(|e| anyhow::anyhow!("clipboard write failed: {e}"))?;

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

/// Phase 7 (Command Mode / Transforms): copy the focused app's current
/// selection by restoring focus to `hwnd` and simulating Ctrl+C, then reading
/// the clipboard. Returns `None` when nothing is selected. The user's prior
/// clipboard is preserved (we clear it first to detect an empty selection, then
/// restore it before returning).
#[cfg(windows)]
pub fn capture_selection(app: &AppHandle, hwnd: isize) -> Option<String> {
    // Bail if the target window is gone — copying from the wrong window would
    // return a bogus selection.
    if !restore_focus(hwnd) {
        return None;
    }

    let clip = app.clipboard();
    let previous = clip.read_text().ok();

    // Clear so a Ctrl+C with no selection leaves the clipboard empty (rather
    // than echoing whatever was there before).
    let _ = clip.write_text(String::new());

    thread::sleep(Duration::from_millis(40));
    send_ctrl_c();
    thread::sleep(Duration::from_millis(120));

    let selected = clip.read_text().ok().filter(|s| !s.is_empty());

    // Restore the user's prior clipboard.
    if let Some(prev) = previous {
        let _ = clip.write_text(prev);
    }
    selected
}

#[cfg(not(windows))]
pub fn capture_selection(_app: &AppHandle, _hwnd: isize) -> Option<String> {
    None
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

/// Bring the captured target window to the foreground. Returns `false` if the
/// HWND is null, no longer a valid window, or the OS refuses the foreground
/// switch — callers must not paste/copy in that case.
#[cfg(windows)]
fn restore_focus(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    unsafe {
        let target = HWND(hwnd as *mut core::ffi::c_void);
        if !IsWindow(target).as_bool() {
            return false;
        }
        SetForegroundWindow(target).as_bool()
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

#[cfg(windows)]
fn send_ctrl_c() {
    unsafe {
        keybd_event(VK_CONTROL.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(VK_C.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(VK_C.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }
}
