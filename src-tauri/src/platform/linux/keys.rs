//! Paste/copy keystroke synthesis on Linux via enigo (Ctrl+V / Ctrl+C).
//!
//! The Linux sibling of `platform::macos::keys` - same structure, Ctrl instead
//! of Cmd. enigo is already a dependency; on X11 it drives XTEST underneath. The
//! keystrokes are wrapped in the process-global `INJECTING` flag (see
//! `injection.rs`) so the Phase 3 XI2 trigger listener drops our own synthetic
//! Ctrl+V/Ctrl+C instead of treating a Ctrl-based bare-modifier trigger as a
//! real press (the analogue of the Windows `LLKHF_INJECTED` check).

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// Press Ctrl, click `key`, release Ctrl - the standard paste/copy chord. Held
/// inside the `INJECTING` guard for the whole combo.
fn ctrl_combo(key: char) -> anyhow::Result<()> {
    let _injecting = crate::injection::injecting::guard();

    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| anyhow::anyhow!("enigo init: {e}"))?;
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| anyhow::anyhow!("enigo press Ctrl: {e}"))?;
    enigo
        .key(Key::Unicode(key), Direction::Click)
        .map_err(|e| anyhow::anyhow!("enigo click {key}: {e}"))?;
    enigo
        .key(Key::Control, Direction::Release)
        .map_err(|e| anyhow::anyhow!("enigo release Ctrl: {e}"))?;
    Ok(())
}

/// Simulate Ctrl+V into the focused app.
pub fn paste() -> anyhow::Result<()> {
    ctrl_combo('v')
}

/// Simulate Ctrl+C to copy the focused app's current selection (Command Mode /
/// transforms). Held inside the same `INJECTING` guard so the trigger listener
/// doesn't mistake it for a bare-modifier trigger.
pub fn copy() -> anyhow::Result<()> {
    ctrl_combo('c')
}
