//! Phase 2: macOS bare-modifier + mouse-button triggers via a CGEventTap.
//!
//! The macOS analogue of the Windows low-level hooks (`hooks.rs`): a bare
//! modifier (e.g. Right Option) or a mouse button (middle / X1 / X2) becomes an
//! extra push-to-talk trigger, which the global-shortcut plugin cannot express.
//!
//! Design mirrors `hooks.rs`:
//! - The tap callback must return fast; it only flips atomics and pushes a
//!   press/release bool onto a channel. A dedicated dispatcher thread runs the
//!   real handlers (`hotkey::on_main_pressed/released`), so activation modes
//!   apply to tap triggers exactly as they do to the global shortcut.
//! - The bound mouse button is CONSUMED (the callback swallows it) so its normal
//!   click never reaches the app under the cursor - the analogue of the Windows
//!   `LRESULT(1)`. Bare modifiers are NOT consumed: swallowing Option/Ctrl/Shift
//!   would break the rest of the OS.
//! - Our own synthetic Cmd+V / Cmd+C (see `injection::injecting`) is dropped so a
//!   modifier bound as a trigger doesn't re-fire on every paste - the analogue of
//!   the Windows `LLKHF_INJECTED` check.
//!
//! An active tap needs Accessibility trust, so `init` spins up a thread that
//! waits until the process is trusted (see `permissions.rs`) before creating the
//! tap on a dedicated CFRunLoop. The OS disables a tap whose callback stalls; we
//! re-enable it on `TapDisabledByTimeout` / `TapDisabledByUserInput`.
//!
//! Finalized against real macOS/CI per the cross-platform plan: the exact
//! core-graphics tap ABI and the mouse-consume mechanism can only be verified on
//! hardware.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventTapProxy,
    CGEventType, EventField,
};

use crate::config::Settings;
use crate::hotkey;
use crate::state::AppState;

/// Virtual keycode of the configured bare-modifier trigger (0 = none).
static MODIFIER_KEYCODE: AtomicU32 = AtomicU32::new(0);
/// Device-dependent CGEventFlags mask for that modifier - disambiguates left vs
/// right, which the generic modifier masks don't (0 = none).
static MODIFIER_MASK: AtomicU64 = AtomicU64::new(0);
/// Configured mouse trigger as a CGEvent button number: 0 = none, 2 = middle,
/// 3 = X1 (back), 4 = X2 (forward).
static MOUSE_BTN: AtomicU32 = AtomicU32::new(0);
/// Debounce: modifier flagsChanged / mouse events can repeat.
static MOD_DOWN: AtomicBool = AtomicBool::new(false);
static MOUSE_DOWN: AtomicBool = AtomicBool::new(false);
/// Press(true)/release(false) events, drained by the dispatcher thread.
static TX: OnceLock<Sender<bool>> = OnceLock::new();
/// Leaked, thread-owned pointer to the live tap, used by its own callback to
/// re-enable after the OS disables it. Only ever touched from the tap thread; the
/// tap is leaked (lives for the app's lifetime) so the pointer never dangles.
static TAP_PTR: AtomicPtr<CGEventTap<'static>> = AtomicPtr::new(std::ptr::null_mut());

/// Map a `Settings::modifier_trigger` id to a (virtual keycode, device-flag mask)
/// pair. The mask bit is set in the event flags while that specific left/right
/// modifier is held.
fn modifier_key(id: &str) -> (u32, u64) {
    match id {
        "left_shift" => (56, 0x0000_0002),
        "right_shift" => (60, 0x0000_0004),
        "left_ctrl" => (59, 0x0000_0001),
        "right_ctrl" => (62, 0x0000_2000),
        "left_alt" => (58, 0x0000_0020),
        "right_alt" => (61, 0x0000_0040),
        _ => (0, 0),
    }
}

/// Map a `Settings::mouse_trigger` id to a CGEvent button number.
fn mouse_button(id: &str) -> u32 {
    match id {
        "middle" => 2,
        "x1" => 3,
        "x2" => 4,
        _ => 0,
    }
}

/// Publish the configured triggers to the tap callback. Called at startup and
/// whenever settings are saved, so changes apply without a restart.
pub fn update_triggers(settings: &Settings) {
    let (keycode, mask) = modifier_key(&settings.modifier_trigger);
    MODIFIER_KEYCODE.store(keycode, Ordering::Relaxed);
    MODIFIER_MASK.store(mask, Ordering::Relaxed);
    MOUSE_BTN.store(mouse_button(&settings.mouse_trigger), Ordering::Relaxed);
}

/// Install the tap and start the dispatcher. Call once from `setup`, after
/// `AppState` is managed. The tap stays installed for the app's lifetime and is
/// inert (immediate pass-through) while no trigger is configured.
pub fn init(app: AppHandle) {
    let (tx, rx) = channel::<bool>();
    if TX.set(tx).is_err() {
        return; // already initialized
    }

    // Dispatcher: runs the same press/release entry points as the global-shortcut
    // handler, so activation modes apply to tap triggers too.
    thread::spawn(move || {
        while let Ok(pressed) = rx.recv() {
            let st = app.state::<AppState>();
            if pressed {
                hotkey::on_main_pressed(&app, &st);
            } else {
                hotkey::on_main_released(&app, &st);
            }
        }
    });

    // Tap thread: owns a CFRunLoop; retries until Accessibility is granted.
    thread::spawn(run_tap);
}

fn send(pressed: bool) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(pressed);
    }
}

/// Create the event tap and run its CFRunLoop forever. Blocks the tap thread.
fn run_tap() {
    // An active tap requires Accessibility trust; poll until granted so a user
    // who approves the prompt later doesn't need to restart the app.
    while !crate::platform::macos::permissions::is_trusted() {
        thread::sleep(Duration::from_secs(2));
    }

    let tap = match CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::Default,
        vec![
            CGEventType::FlagsChanged,
            CGEventType::OtherMouseDown,
            CGEventType::OtherMouseUp,
        ],
        tap_callback,
    ) {
        Ok(tap) => tap,
        Err(_) => {
            eprintln!("[macos-input] failed to create event tap");
            return;
        }
    };

    // Leak the tap so it lives for the app's lifetime, and stash a pointer the
    // callback can use to re-enable it after an OS-triggered disable.
    let leaked: &'static CGEventTap<'static> = Box::leak(Box::new(tap));
    TAP_PTR.store(leaked as *const _ as *mut _, Ordering::SeqCst);

    let source = match leaked.mach_port.create_runloop_source(0) {
        Ok(source) => source,
        Err(_) => {
            eprintln!("[macos-input] failed to create runloop source");
            return;
        }
    };

    let current = CFRunLoop::get_current();
    unsafe {
        current.add_source(&source, kCFRunLoopCommonModes);
    }
    leaked.enable();
    CFRunLoop::run_current();
}

/// Re-enable the tap after the OS disabled it (callback stalled or user input).
fn rearm_tap() {
    let ptr = TAP_PTR.load(Ordering::SeqCst);
    if !ptr.is_null() {
        // Sound: only the tap thread (which owns the leaked, never-freed tap)
        // reaches this, from inside the tap's own callback.
        unsafe { (*ptr).enable() };
    }
}

/// The tap callback. Returning `None` passes the event through unchanged;
/// returning `Some(event)` substitutes it. To consume the bound mouse button we
/// return the event re-typed to `Null`, which the system discards.
fn tap_callback(_proxy: CGEventTapProxy, etype: CGEventType, event: &CGEvent) -> Option<CGEvent> {
    match etype {
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
            rearm_tap();
            return None;
        }
        _ => {}
    }

    // Ignore our own injected Cmd+V / Cmd+C so a bound modifier doesn't re-fire.
    if crate::injection::injecting::is_injecting() {
        return None;
    }

    match etype {
        CGEventType::FlagsChanged => {
            let target = MODIFIER_KEYCODE.load(Ordering::Relaxed);
            if target != 0 {
                let keycode =
                    event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u32;
                if keycode == target {
                    // The device-flag bit is set while the key is held, so its
                    // presence on this flagsChanged distinguishes press from
                    // release without tracking history.
                    let mask = MODIFIER_MASK.load(Ordering::Relaxed);
                    let down = event.get_flags().bits() & mask != 0;
                    if down {
                        if !MOD_DOWN.swap(true, Ordering::SeqCst) {
                            send(true);
                        }
                    } else if MOD_DOWN.swap(false, Ordering::SeqCst) {
                        send(false);
                    }
                }
            }
            None
        }
        CGEventType::OtherMouseDown | CGEventType::OtherMouseUp => {
            let target = MOUSE_BTN.load(Ordering::Relaxed);
            if target != 0 {
                let button =
                    event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER) as u32;
                if button == target {
                    let down = matches!(etype, CGEventType::OtherMouseDown);
                    if down {
                        if !MOUSE_DOWN.swap(true, Ordering::SeqCst) {
                            send(true);
                        }
                    } else if MOUSE_DOWN.swap(false, Ordering::SeqCst) {
                        send(false);
                    }
                    // Consume the bound button: re-type the event to Null so its
                    // normal click never reaches the app under the cursor.
                    let consumed = event.clone();
                    consumed.set_type(CGEventType::Null);
                    return Some(consumed);
                }
            }
            None
        }
        _ => None,
    }
}
