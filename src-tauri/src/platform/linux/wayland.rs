//! Phase 4: the Linux/Wayland OS-integration backend - the Wayland sibling of
//! `x11.rs` (and of the Windows `hooks.rs` / macOS `input.rs` triggers).
//!
//! Wayland deliberately denies apps global input grabs and foreign-window focus,
//! so the X11 backend's XI2 listener + EWMH focus paths cannot work here and the
//! tauri global-shortcut plugin is a no-op. Instead this module drives the
//! `org.freedesktop.portal.GlobalShortcuts` XDG portal (via `ashpd`): the
//! compositor owns the key bindings and delivers `Activated`/`Deactivated`
//! signals - which are exactly push-to-talk's press/release - that we route into
//! the SAME handler entry points the global-shortcut plugin uses on the other
//! platforms.
//!
//! Injection is the shared `enigo` path built with its `wayland` feature
//! (zwp_virtual_keyboard_v1; works on KDE + wlroots, absent on GNOME - see
//! `injection.rs`). Foreign-window focus capture/restore is impossible on
//! Wayland, so `platform::frontmost` reports handle 0 / unknown context and the
//! documented degradations apply (no privacy-pause matching, default Flow Style,
//! blank history attribution, no Esc-cancel bind).
//!
//! Finalized against CI (ubuntu) + a Wayland session per the cross-platform
//! plan's verification model: the portal round-trip can only be exercised there.

use std::sync::OnceLock;

use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use ashpd::desktop::Session;
use futures_util::StreamExt;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::db::transforms;
use crate::state::AppState;
use crate::{command_mode, hotkey, window_mgmt};

/// Signals the portal task to re-read the shortcut settings and re-bind. `None`
/// until [`init`] runs (i.e. only on a Wayland session).
static REBIND: OnceLock<UnboundedSender<()>> = OnceLock::new();

/// Ask the portal task to re-bind after a shortcut setting changed. Called from
/// the Wayland arms of `commands::swap_global_shortcut` and
/// `command_mode::register_transform_shortcuts` once they've committed the
/// change. A no-op if the task never started (portal unavailable) or has stopped.
pub fn request_rebind() {
    if let Some(tx) = REBIND.get() {
        let _ = tx.send(());
    }
}

/// Start the GlobalShortcuts portal session on a background tokio task. Call once
/// from `setup` on a Wayland session, after `AppState` is managed. Best-effort:
/// if the portal is unavailable (older compositor, or the user denies it) the
/// task logs and exits, leaving the app usable without global triggers (the Hub
/// window and its dictation still work when focused).
pub fn init(app: AppHandle) {
    let (tx, rx) = unbounded_channel::<()>();
    if REBIND.set(tx).is_err() {
        return; // already initialized
    }
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run(app, rx).await {
            eprintln!("[linux-wayland] global-shortcuts portal unavailable: {e}");
        }
    });
}

async fn run(app: AppHandle, mut rebind_rx: UnboundedReceiver<()>) -> ashpd::Result<()> {
    let portal = GlobalShortcuts::new().await?;
    let session = portal.create_session().await?;
    bind(&portal, &session, &app).await?;

    // The signal streams borrow `&portal` immutably; `bind` re-borrows it the
    // same way, so both coexist (no &mut needed anywhere on the portal).
    let activated = portal.receive_activated().await?;
    let deactivated = portal.receive_deactivated().await?;
    futures_util::pin_mut!(activated, deactivated);

    loop {
        tokio::select! {
            Some(a) = activated.next() => dispatch(&app, a.shortcut_id(), true),
            Some(d) = deactivated.next() => dispatch(&app, d.shortcut_id(), false),
            Some(()) = rebind_rx.recv() => {
                // Coalesce a burst of setting edits into a single re-bind.
                while rebind_rx.try_recv().is_ok() {}
                let _ = bind(&portal, &session, &app).await;
            }
            else => break,
        }
    }
    Ok(())
}

/// Map a fired shortcut id back to the same handler entry points the
/// global-shortcut plugin uses on the other platforms. `Activated` ->
/// `pressed=true`, `Deactivated` -> `pressed=false` (push-to-talk's down/up).
/// The one-shot copy/scratchpad/transform actions only act on the press.
fn dispatch(app: &AppHandle, id: &str, pressed: bool) {
    let state = app.state::<AppState>();
    let st: &AppState = &state;
    match id {
        "main" => {
            if pressed {
                hotkey::on_main_pressed(app, st);
            } else {
                hotkey::on_main_released(app, st);
            }
        }
        "command" => {
            if pressed {
                command_mode::on_press(app, st);
            } else {
                command_mode::on_release(app, st);
            }
        }
        "copy" if pressed => hotkey::on_copy(app, st),
        "scratchpad" if pressed => window_mgmt::open_scratchpad(app),
        other if pressed => {
            if let Some(tid) = other
                .strip_prefix("transform:")
                .and_then(|s| s.parse::<i64>().ok())
            {
                command_mode::on_transform(app, st, tid);
            }
        }
        _ => {}
    }
}

/// (Re-)bind the full set of accelerators to the portal session. The compositor
/// owns the actual key combos; we only send a *preferred* trigger it may honor or
/// let the user override.
async fn bind<'a>(
    portal: &GlobalShortcuts<'a>,
    session: &Session<'a, GlobalShortcuts<'a>>,
    app: &AppHandle,
) -> ashpd::Result<()> {
    // Snapshot the accelerators + transform rows WITHOUT holding any lock across
    // the portal await (backend invariant: no parking_lot guard across `.await`).
    let specs = collect_specs(app);
    let shortcuts: Vec<NewShortcut> = specs
        .iter()
        .map(|(id, desc, trigger)| {
            NewShortcut::new(id.clone(), *desc).preferred_trigger(trigger.as_deref())
        })
        .collect();
    portal.bind_shortcuts(session, &shortcuts, None).await?;
    Ok(())
}

/// Collect `(id, human description, preferred XDG trigger)` for every reserved
/// shortcut plus each active transform. Done synchronously so the settings/DB
/// locks are released before the caller awaits the portal.
fn collect_specs(app: &AppHandle) -> Vec<(String, &'static str, Option<String>)> {
    let st = app.state::<AppState>();
    let s = st.settings.lock().clone();
    let mut specs = vec![
        ("main".to_string(), "Push to talk (dictate)", translate(&s.shortcut)),
        ("copy".to_string(), "Copy last transcript", translate(&s.copy_shortcut)),
        ("command".to_string(), "Command mode", translate(&s.command_shortcut)),
        (
            "scratchpad".to_string(),
            "Open scratchpad",
            translate(&s.scratchpad_shortcut),
        ),
    ];
    let rows = {
        let conn = st.db.lock();
        transforms::active_shortcuts(&conn).unwrap_or_default()
    };
    for (id, accel) in rows {
        specs.push((format!("transform:{id}"), "Transform selection", translate(&accel)));
    }
    specs
}

/// Translate a Tauri accelerator ("CmdOrCtrl+Shift+C") to the XDG "shortcuts"
/// spec trigger string the portal expects ("<Control><Shift>c"). Returns `None`
/// when the accelerator can't be expressed (unknown key or more than one
/// non-modifier), so the shortcut is bound with no preferred trigger and the
/// compositor lets the user pick one - the documented lossy mapping.
fn translate(accel: &str) -> Option<String> {
    let mut mods = String::new();
    let mut key: Option<String> = None;
    for tok in accel.split('+') {
        match tok {
            "Ctrl" | "Control" | "CmdOrCtrl" | "CommandOrControl" => mods.push_str("<Control>"),
            "Shift" => mods.push_str("<Shift>"),
            "Alt" | "Option" => mods.push_str("<Alt>"),
            "Super" | "Cmd" | "Command" | "Meta" => mods.push_str("<Super>"),
            other => {
                if key.is_some() {
                    return None; // more than one non-modifier key: not expressible
                }
                key = Some(xdg_key(other)?);
            }
        }
    }
    key.map(|k| format!("{mods}{k}"))
}

/// Map a Tauri key token to its XKB keysym name (what the portal spec uses).
/// Best-effort: single alphanumerics lowercase to their keysym, common named
/// keys map directly, F-keys pass through; anything else is `None`.
fn xdg_key(tok: &str) -> Option<String> {
    if tok.len() == 1 && tok.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Some(tok.to_ascii_lowercase());
    }
    let named = match tok {
        "Space" => "space",
        "Enter" | "Return" => "Return",
        "Tab" => "Tab",
        "Backspace" => "BackSpace",
        "Escape" | "Esc" => "Escape",
        f if f.len() > 1 && f.starts_with('F') && f[1..].bytes().all(|b| b.is_ascii_digit()) => {
            return Some(f.to_string());
        }
        _ => return None,
    };
    Some(named.to_string())
}

#[cfg(test)]
mod tests {
    use super::translate;

    #[test]
    fn translates_common_accelerators() {
        assert_eq!(translate("F8").as_deref(), Some("F8"));
        assert_eq!(translate("CmdOrCtrl+Shift+C").as_deref(), Some("<Control><Shift>c"));
        assert_eq!(
            translate("CmdOrCtrl+Shift+Alt+Space").as_deref(),
            Some("<Control><Shift><Alt>space")
        );
        // Unmappable key -> None (bound without a preferred trigger).
        assert_eq!(translate("CmdOrCtrl+PrintScreen"), None);
    }
}
