//! Frontmost-app capture + focus restoration on macOS.
//!
//! macOS has no direct HWND-equivalent we can re-focus, so Eve works at the
//! **application** level: the focus handle is the frontmost app's pid. At record
//! start we snapshot `frontmost_pid()`; before pasting we `restore_focus(pid)`
//! (re-activate that app) and poll until it actually holds focus - the same
//! abort contract as the Windows `restore_focus`: return `false` and the caller
//! bails before touching the clipboard.
//!
//! `#[allow(unused_unsafe)]`: whether a given objc2-app-kit accessor is generated
//! `unsafe` shifts between binding versions. Wrapping the calls in `unsafe` keeps
//! us correct when they are, and the allow keeps the build clean when they aren't
//! (finalized against real macOS/CI per the Phase 1 plan).

use std::thread;
use std::time::Duration;

use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

/// PID of the app that currently owns the foreground, or `None` if it can't be
/// resolved (e.g. no frontmost app).
#[allow(unused_unsafe)]
pub fn frontmost_pid() -> Option<i32> {
    unsafe {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication()?;
        Some(app.processIdentifier())
    }
}

/// Bring the app identified by `pid` back to the foreground and wait until it
/// actually holds focus. Returns `false` when the pid is null/invalid, the app
/// has exited, or activation doesn't take within ~500 ms - callers must not
/// paste or copy in that case.
#[allow(unused_unsafe)]
pub fn restore_focus(pid: isize) -> bool {
    if pid == 0 {
        return false;
    }
    let pid = pid as i32;

    let app = unsafe { NSRunningApplication::runningApplicationWithProcessIdentifier(pid) };
    let Some(app) = app else {
        return false;
    };

    unsafe {
        // objc2 strips the type prefix from the flag, so the constant is
        // `ActivateIgnoringOtherApps` (not `NSApplicationActivate…`). It's
        // deprecated on macOS 14+ but still the working way to force the switch
        // even when another app is active; the poll below confirms it landed.
        #[allow(deprecated)]
        app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
    }

    // Poll up to ~500 ms for the activation to land before we let the caller
    // paste - mirrors the Windows SetForegroundWindow success gate.
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(10));
        if frontmost_pid() == Some(pid) {
            return true;
        }
    }
    false
}
