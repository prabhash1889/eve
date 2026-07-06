//! Parity A3/A4: Windows low-level keyboard + mouse hooks that turn a bare
//! modifier key (e.g. Right Alt) or a mouse button (middle / X1 / X2) into an
//! additional record trigger. The global-shortcut plugin can't express either.
//!
//! Design constraints:
//! - Hook procs must return fast - Windows silently removes a low-level hook
//!   that exceeds its timeout (~300 ms). The procs only flip atomics and push
//!   onto a channel; a dedicated dispatcher thread runs the actual handlers.
//! - The bound mouse button is CONSUMED (the proc returns 1) so its normal
//!   click never reaches the app under the cursor. Bare modifiers are NOT
//!   consumed - swallowing Alt/Ctrl/Shift would break the rest of the OS.
//! - Injected events (our own `SendInput` Ctrl+V during paste) are ignored,
//!   otherwise binding Left Ctrl would re-trigger recording on every paste.

#![cfg(windows)]

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;

use tauri::{AppHandle, Manager};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT, LLKHF_INJECTED,
    LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use crate::config::Settings;
use crate::hotkey;
use crate::state::AppState;

/// Virtual-key code of the configured bare-modifier trigger (0 = none).
static MODIFIER_VK: AtomicU32 = AtomicU32::new(0);
/// Configured mouse trigger: 0 = none, 1 = middle, 2 = X1 (back), 3 = X2.
static MOUSE_BTN: AtomicU32 = AtomicU32::new(0);
/// Debounce: the OS auto-repeats key-down while a key is held.
static MOD_DOWN: AtomicBool = AtomicBool::new(false);
static MOUSE_DOWN: AtomicBool = AtomicBool::new(false);
/// Press(true)/release(false) events, drained by the dispatcher thread.
static TX: OnceLock<Sender<bool>> = OnceLock::new();

/// Map the `Settings::modifier_trigger` id to a Windows virtual-key code.
fn modifier_vk(id: &str) -> u32 {
    match id {
        "left_shift" => 0xA0,  // VK_LSHIFT
        "right_shift" => 0xA1, // VK_RSHIFT
        "left_ctrl" => 0xA2,   // VK_LCONTROL
        "right_ctrl" => 0xA3,  // VK_RCONTROL
        "left_alt" => 0xA4,    // VK_LMENU
        "right_alt" => 0xA5,   // VK_RMENU
        _ => 0,
    }
}

fn mouse_btn(id: &str) -> u32 {
    match id {
        "middle" => 1,
        "x1" => 2,
        "x2" => 3,
        _ => 0,
    }
}

/// Publish the configured triggers to the hook procs. Called at startup and
/// whenever settings are saved, so changes apply without a restart.
pub fn update_triggers(settings: &Settings) {
    MODIFIER_VK.store(modifier_vk(&settings.modifier_trigger), Ordering::Relaxed);
    MOUSE_BTN.store(mouse_btn(&settings.mouse_trigger), Ordering::Relaxed);
}

/// Install both hooks and start the dispatcher. Call once from `setup`, after
/// `AppState` is managed. The hooks stay installed for the app's lifetime and
/// are inert (immediate pass-through) while no trigger is configured.
pub fn init(app: AppHandle) {
    let (tx, rx) = channel::<bool>();
    if TX.set(tx).is_err() {
        return; // already initialized
    }

    // Dispatcher: runs the same press/release entry points as the
    // global-shortcut handler, so activation modes apply to hook triggers too.
    std::thread::spawn(move || {
        while let Ok(pressed) = rx.recv() {
            let st = app.state::<AppState>();
            if pressed {
                hotkey::on_main_pressed(&app, &st);
            } else {
                hotkey::on_main_released(&app, &st);
            }
        }
    });

    // Hook thread: low-level hooks require a message pump on their thread.
    std::thread::spawn(|| unsafe {
        let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0);
        let ms = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0);
        if kb.is_err() && ms.is_err() {
            eprintln!("[hooks] failed to install low-level hooks");
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
    });
}

fn send(pressed: bool) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(pressed);
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let target = MODIFIER_VK.load(Ordering::Relaxed);
        if target != 0 {
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let injected = kb.flags.0 & LLKHF_INJECTED.0 != 0;
            if !injected && kb.vkCode == target {
                match wparam.0 as u32 {
                    WM_KEYDOWN | WM_SYSKEYDOWN => {
                        if !MOD_DOWN.swap(true, Ordering::SeqCst) {
                            send(true);
                        }
                    }
                    WM_KEYUP | WM_SYSKEYUP => {
                        if MOD_DOWN.swap(false, Ordering::SeqCst) {
                            send(false);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let target = MOUSE_BTN.load(Ordering::Relaxed);
        if target != 0 {
            let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let injected = ms.flags & LLMHF_INJECTED != 0;
            // High word of mouseData identifies which X button for WM_XBUTTON*.
            let xbtn = (ms.mouseData >> 16) as u16 as u32;
            let msg = wparam.0 as u32;
            let matched = !injected
                && match msg {
                    WM_MBUTTONDOWN | WM_MBUTTONUP => target == 1,
                    WM_XBUTTONDOWN | WM_XBUTTONUP => {
                        (target == 2 && xbtn == 1) || (target == 3 && xbtn == 2)
                    }
                    _ => false,
                };
            if matched {
                let down = matches!(msg, WM_MBUTTONDOWN | WM_XBUTTONDOWN);
                if down {
                    if !MOUSE_DOWN.swap(true, Ordering::SeqCst) {
                        send(true);
                    }
                } else if MOUSE_DOWN.swap(false, Ordering::SeqCst) {
                    send(false);
                }
                // Consume the bound button so the click never reaches the app
                // under the cursor.
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}
