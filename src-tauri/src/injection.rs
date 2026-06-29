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
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_C, VK_CONTROL, VK_V,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{IsWindow, SetForegroundWindow};

/// Restores a previously-saved clipboard value when dropped, so the user's prior
/// clipboard comes back even if we bail (or panic) between writing our payload
/// and the normal restore point.
#[cfg(windows)]
struct ClipboardGuard<'a> {
    app: &'a AppHandle,
    previous: Option<String>,
}

#[cfg(windows)]
impl Drop for ClipboardGuard<'_> {
    fn drop(&mut self) {
        if let Some(prev) = self.previous.take() {
            let _ = self.app.clipboard().write_text(prev);
        }
    }
}

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
    // Capture the prior clipboard and arm a drop guard so it is restored on every
    // exit path — including an early `?` return or a panic during the paste.
    let _restore = ClipboardGuard {
        app,
        previous: clip.read_text().ok(),
    };

    clip.write_text(text.to_string())
        .map_err(|e| anyhow::anyhow!("clipboard write failed: {e}"))?;

    // Phase 5: fixed injection delays, reviewed to trim latency while preserving
    // reliability. PRE lets the focus switch + clipboard write settle before we
    // send Ctrl+V; PASTE_SETTLE lets the target app actually read the clipboard
    // before the guard restores the prior contents. Cutting PASTE_SETTLE too far
    // risks the app pasting the *restored* clipboard, so it stays comfortably
    // above typical clipboard-read latency (trimmed 150 → 120 ms).
    const PRE: Duration = Duration::from_millis(40);
    const PASTE_SETTLE: Duration = Duration::from_millis(120);
    thread::sleep(PRE);
    send_ctrl_v();
    thread::sleep(PASTE_SETTLE);

    // `_restore` drops here, putting the user's prior clipboard back.
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
    // Restore the prior clipboard on every exit path (guard drops at return).
    let _restore = ClipboardGuard {
        app,
        previous: clip.read_text().ok(),
    };

    // Clear so a Ctrl+C with no selection leaves the clipboard empty (rather
    // than echoing whatever was there before).
    let _ = clip.write_text(String::new());

    thread::sleep(Duration::from_millis(40));
    send_ctrl_c();

    // Poll for the copied selection to land instead of a single fixed wait —
    // heavy apps can take longer than a flat 120 ms, after which they'd silently
    // fall back to "nothing selected". Re-read until non-empty or ~600 ms.
    let mut selected = None;
    for _ in 0..30 {
        thread::sleep(Duration::from_millis(20));
        if let Some(s) = clip.read_text().ok().filter(|s| !s.is_empty()) {
            selected = Some(s);
            break;
        }
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

/// Build a single keyboard `INPUT` event for `SendInput`.
#[cfg(windows)]
fn key_event(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Press `modifier`+`key` then release both via a single `SendInput` batch.
/// `SendInput` is the modern, non-deprecated path (`keybd_event` is deprecated)
/// and injects the four events atomically, which some security software trusts
/// where individual `keybd_event` calls are dropped.
#[cfg(windows)]
fn send_combo(modifier: VIRTUAL_KEY, key: VIRTUAL_KEY) {
    let inputs = [
        key_event(modifier, KEYBD_EVENT_FLAGS(0)),
        key_event(key, KEYBD_EVENT_FLAGS(0)),
        key_event(key, KEYEVENTF_KEYUP),
        key_event(modifier, KEYEVENTF_KEYUP),
    ];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(windows)]
fn send_ctrl_v() {
    send_combo(VK_CONTROL, VK_V);
}

#[cfg(windows)]
fn send_ctrl_c() {
    send_combo(VK_CONTROL, VK_C);
}
