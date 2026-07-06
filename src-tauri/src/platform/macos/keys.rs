//! Paste/copy keystroke synthesis on macOS via enigo (Cmd+V / Cmd+C).
//!
//! enigo is already a dependency and uses CGEvent underneath. The keystrokes are
//! wrapped in the process-global `INJECTING` flag (see `injection.rs`) so the
//! Phase 2 event tap drops our own synthetic Cmd+V/Cmd+C instead of treating it
//! as a bare-modifier trigger.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// Press Cmd, click `key`, release Cmd - the standard menu-command chord. Held
/// inside the `INJECTING` guard for the whole combo.
fn cmd_combo(key: char) -> anyhow::Result<()> {
    let _injecting = crate::injection::injecting::guard();

    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| anyhow::anyhow!("enigo init: {e}"))?;
    enigo
        .key(Key::Meta, Direction::Press)
        .map_err(|e| anyhow::anyhow!("enigo press Cmd: {e}"))?;
    enigo
        .key(Key::Unicode(key), Direction::Click)
        .map_err(|e| anyhow::anyhow!("enigo click {key}: {e}"))?;
    enigo
        .key(Key::Meta, Direction::Release)
        .map_err(|e| anyhow::anyhow!("enigo release Cmd: {e}"))?;
    Ok(())
}

/// Simulate Cmd+V into the focused app.
pub fn paste() -> anyhow::Result<()> {
    cmd_combo('v')
}
