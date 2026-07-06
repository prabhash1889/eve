//! Phase 3: the Linux/X11 OS-integration backend - the X11 sibling of the
//! Windows `hooks.rs` + focus code and the macOS `focus.rs`/`input.rs`.
//!
//! Two responsibilities, both gated behind `session() == X11` by the callers:
//!
//! - **Focus** ([`capture_frontmost`] / [`restore_focus`]): the `isize` handle is
//!   the X window id. Capture reads `_NET_ACTIVE_WINDOW` off the root; restore
//!   asks the window manager to re-focus via an EWMH `_NET_ACTIVE_WINDOW` client
//!   message (NOT `XSetInputFocus`, which bypasses the WM) and polls until the WM
//!   confirms - the same abort contract as the Windows/macOS `restore_focus`.
//! - **Triggers** ([`init`] / [`update_triggers`]): a bare modifier becomes an
//!   extra push-to-talk trigger via XI2 **raw** key events (observed on the root
//!   without a grab, so the key still reaches the focused app - the analogue of
//!   the macOS flagsChanged tap). A mouse button becomes a trigger via a passive
//!   `GrabButton`, which inherently CONSUMES the click (the analogue of the
//!   Windows `LRESULT(1)` / macOS re-type-to-Null). Both dispatch through the
//!   same channel-to-dispatcher pattern as `hooks.rs` into
//!   `hotkey::on_main_pressed/released`; our own synthetic Ctrl+V/Ctrl+C are
//!   dropped via the `injection::injecting` flag.
//!
//! Finalized against CI (ubuntu) + Linux hardware per the cross-platform plan's
//! verification model: the x11rb request/event ABI can only be exercised there.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use x11rb::connection::Connection;
use x11rb::protocol::xinput::{self, ConnectionExt as _, XIEventMask};
use x11rb::protocol::xproto::{
    AtomEnum, ButtonIndex, ClientMessageEvent, ConnectionExt as _, EventMask, GrabMode, ModMask,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use crate::config::Settings;
use crate::context::active_window::AppContext;
use crate::hotkey;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Focus capture / restore
// ---------------------------------------------------------------------------

/// Capture the active window (`_NET_ACTIVE_WINDOW`) + its resolved context. The
/// handle is the X window id; returns `(0, unknown)` when nothing can be read
/// (no WM EWMH support, or the connection failed).
pub fn capture_frontmost() -> (isize, AppContext) {
    match connect_and_capture() {
        Some((win, ctx)) => (win as isize, ctx),
        None => (0, AppContext::unknown()),
    }
}

fn connect_and_capture() -> Option<(u32, AppContext)> {
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots[screen].root;
    let net_active = intern(&conn, b"_NET_ACTIVE_WINDOW")?;
    let win = read_active_window(&conn, root, net_active)?;
    let pid = read_pid(&conn, win);
    let title = read_title(&conn, win);
    Some((win, super::context::resolve(pid, title)))
}

/// Re-focus the captured window via an EWMH `_NET_ACTIVE_WINDOW` client message
/// and poll until the WM confirms (~500 ms). Returns `false` when the handle is
/// null, the window is gone, or activation doesn't land - callers must not
/// paste/copy in that case.
pub fn restore_focus(handle: isize) -> bool {
    if handle == 0 {
        return false;
    }
    connect_and_restore(handle as u32).unwrap_or(false)
}

fn connect_and_restore(win: u32) -> Option<bool> {
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots[screen].root;
    let net_active = intern(&conn, b"_NET_ACTIVE_WINDOW")?;

    // Bail if the target window no longer exists - activating a stale id would
    // send the paste nowhere.
    if conn.get_window_attributes(win).ok()?.reply().is_err() {
        return Some(false);
    }

    // source indication 2 = "pager" / direct user action, which WMs honor even
    // when it means switching away from the currently active window. data[1] is a
    // timestamp; 0 (CurrentTime) is accepted here.
    let event = ClientMessageEvent::new(32, win, net_active, [2u32, 0, 0, 0, 0]);
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
        event,
    )
    .ok()?;
    conn.flush().ok()?;

    // Verify-by-reread - mirrors the Windows/macOS restore_focus success gate.
    for _ in 0..50 {
        thread::sleep(Duration::from_millis(10));
        if read_active_window(&conn, root, net_active) == Some(win) {
            return Some(true);
        }
    }
    Some(false)
}

/// Intern an atom, `None` when it doesn't exist (only_if_exists = true, so a
/// missing EWMH atom is a clean skip rather than an error).
fn intern(conn: &RustConnection, name: &[u8]) -> Option<u32> {
    let atom = conn.intern_atom(true, name).ok()?.reply().ok()?.atom;
    (atom != 0).then_some(atom)
}

/// The window id held in a `WINDOW`-typed property (`_NET_ACTIVE_WINDOW`).
fn read_active_window(conn: &RustConnection, target: u32, prop: u32) -> Option<u32> {
    let reply = conn
        .get_property(false, target, prop, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    // Bind before returning: `value32()` yields an iterator borrowing `reply`, so
    // the value must be pulled out (and the iterator dropped) before `reply` does.
    let win = reply.value32()?.next().filter(|w| *w != 0);
    win
}

/// `_NET_WM_PID` (CARDINAL) of the given window, when the app advertises it.
fn read_pid(conn: &RustConnection, win: u32) -> Option<u32> {
    let atom = intern(conn, b"_NET_WM_PID")?;
    let reply = conn
        .get_property(false, win, atom, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    // Bind before returning (same borrow reason as `read_active_window`).
    let pid = reply.value32()?.next();
    pid
}

/// The window caption: `_NET_WM_NAME` (UTF-8) first, falling back to the legacy
/// `WM_NAME`. Empty string when neither is set.
fn read_title(conn: &RustConnection, win: u32) -> String {
    intern(conn, b"_NET_WM_NAME")
        .and_then(|a| read_text(conn, win, a))
        .or_else(|| read_text(conn, win, u32::from(AtomEnum::WM_NAME)))
        .unwrap_or_default()
}

/// Read a text property as a lossy UTF-8 string (accepts any type: `_NET_WM_NAME`
/// is UTF8_STRING, `WM_NAME` is STRING/COMPOUND_TEXT). `None` when empty/unset.
fn read_text(conn: &RustConnection, win: u32, prop: u32) -> Option<String> {
    let reply = conn
        .get_property(false, win, prop, AtomEnum::ANY, 0, 1024)
        .ok()?
        .reply()
        .ok()?;
    if reply.value.is_empty() {
        return None;
    }
    let s = String::from_utf8_lossy(&reply.value);
    let s = s.trim_end_matches('\0').trim().to_string();
    (!s.is_empty()).then_some(s)
}

// ---------------------------------------------------------------------------
// Bare-modifier + mouse-button triggers
// ---------------------------------------------------------------------------

/// Keysym of the configured bare-modifier trigger (0 = none).
static MODIFIER_KEYSYM: AtomicU32 = AtomicU32::new(0);
/// X pointer button number of the configured mouse trigger (0 = none).
static MOUSE_BTN: AtomicU32 = AtomicU32::new(0);
/// Debounce: XI2 raw key-press auto-repeats while held.
static MOD_DOWN: AtomicBool = AtomicBool::new(false);
static MOUSE_DOWN: AtomicBool = AtomicBool::new(false);
/// Press(true)/release(false) events, drained by the dispatcher thread.
static TX: OnceLock<Sender<bool>> = OnceLock::new();

// XIAllMasterDevices - observe the merged master keyboard.
const XI_ALL_MASTER_DEVICES: u16 = 1;

/// Map a `Settings::modifier_trigger` id to its X11 keysym.
fn modifier_keysym(id: &str) -> u32 {
    match id {
        "left_shift" => 0xffe1,
        "right_shift" => 0xffe2,
        "left_ctrl" => 0xffe3,
        "right_ctrl" => 0xffe4,
        "left_alt" => 0xffe9,
        "right_alt" => 0xffea,
        _ => 0,
    }
}

/// Map a `Settings::mouse_trigger` id to its X pointer button number. Middle = 2;
/// the thumb "back"/"forward" buttons are 8/9 (4-7 are scroll).
fn mouse_button(id: &str) -> u8 {
    match id {
        "middle" => 2,
        "x1" => 8,
        "x2" => 9,
        _ => 0,
    }
}

/// Publish the configured triggers to the listener thread. Called at startup and
/// whenever settings are saved, so changes apply without a restart. The mouse
/// grab is reconciled by the listener when it next polls.
pub fn update_triggers(settings: &Settings) {
    MODIFIER_KEYSYM.store(modifier_keysym(&settings.modifier_trigger), Ordering::Relaxed);
    MOUSE_BTN.store(mouse_button(&settings.mouse_trigger) as u32, Ordering::Relaxed);
}

/// Start the dispatcher + XI2 listener threads. Call once from `setup` on an X11
/// session, after `AppState` is managed. Inert (no trigger fires, no button
/// grabbed) while nothing is configured.
pub fn init(app: AppHandle) {
    let (tx, rx) = channel::<bool>();
    if TX.set(tx).is_err() {
        return; // already initialized
    }

    // Dispatcher: same entry points as the global-shortcut handler, so activation
    // modes apply to trigger presses too.
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

    thread::spawn(run_triggers);
}

fn send(pressed: bool) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(pressed);
    }
}

/// The trigger listener. Owns its own X connection (grabs are connection-scoped,
/// so they must be issued from here) and its cached keyboard mapping.
fn run_triggers() {
    let mut listener = match TriggerListener::open() {
        Some(l) => l,
        None => {
            eprintln!("[linux-x11] failed to set up the trigger listener");
            return;
        }
    };

    loop {
        // Re-issue the button grab if the configured mouse trigger changed. A
        // short poll (rather than a blocking wait) lets us do this from the
        // connection-owning thread without needing to wake a blocked wait.
        listener.reconcile_mouse_grab();

        let mut got_event = false;
        while let Ok(Some(event)) = listener.conn.poll_for_event() {
            got_event = true;
            listener.handle_event(event);
        }
        if !got_event {
            thread::sleep(Duration::from_millis(16));
        }
    }
}

struct TriggerListener {
    conn: RustConnection,
    root: u32,
    /// Keyboard-mapping cache for keycode -> keysym resolution.
    min_keycode: u8,
    keysyms_per_keycode: usize,
    keysyms: Vec<u32>,
    /// The button currently passively grabbed (0 = none).
    grabbed: u8,
}

impl TriggerListener {
    fn open() -> Option<Self> {
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots[screen].root;

        // XI2 raw key events observe modifiers system-wide WITHOUT grabbing them,
        // so the key still reaches the focused app. Require XI 2.0.
        conn.xinput_xi_query_version(2, 0).ok()?.reply().ok()?;
        let mask = xinput::EventMask {
            deviceid: XI_ALL_MASTER_DEVICES,
            mask: vec![XIEventMask::RAW_KEY_PRESS | XIEventMask::RAW_KEY_RELEASE],
        };
        conn.xinput_xi_select_events(root, &[mask]).ok()?;

        // Cache the keyboard mapping so a raw keycode resolves to its keysym
        // (needed to tell left vs right modifiers apart - see the plan's note).
        let (min_keycode, count) = {
            let setup = conn.setup();
            (setup.min_keycode, setup.max_keycode - setup.min_keycode + 1)
        };
        let mapping = conn.get_keyboard_mapping(min_keycode, count).ok()?.reply().ok()?;
        conn.flush().ok()?;

        Some(Self {
            conn,
            root,
            min_keycode,
            keysyms_per_keycode: mapping.keysyms_per_keycode as usize,
            keysyms: mapping.keysyms,
            grabbed: 0,
        })
    }

    /// Grab/ungrab the pointer button so it matches the configured trigger. A
    /// passive grab consumes the click; when no button is configured we hold no
    /// grab, so normal middle/thumb clicks pass through untouched.
    fn reconcile_mouse_grab(&mut self) {
        let want = MOUSE_BTN.load(Ordering::Relaxed) as u8;
        if want == self.grabbed {
            return;
        }
        if self.grabbed != 0 {
            let _ = self
                .conn
                .ungrab_button(ButtonIndex::from(self.grabbed), self.root, ModMask::ANY);
        }
        if want != 0 {
            let _ = self.conn.grab_button(
                false, // owner_events: deliver to the grab window, not the app
                self.root,
                EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
                x11rb::NONE, // confine_to
                x11rb::NONE, // cursor
                ButtonIndex::from(want),
                ModMask::ANY,
            );
        }
        let _ = self.conn.flush();
        self.grabbed = want;
    }

    fn handle_event(&self, event: Event) {
        // Ignore our own injected Ctrl+V/Ctrl+C so a Ctrl-based trigger doesn't
        // re-fire on every paste (the analogue of the Windows LLKHF_INJECTED
        // check).
        if crate::injection::injecting::is_injecting() {
            return;
        }
        match event {
            Event::XinputRawKeyPress(e) => self.on_key(e.detail as u8, true),
            Event::XinputRawKeyRelease(e) => self.on_key(e.detail as u8, false),
            Event::ButtonPress(e) => self.on_button(e.detail, true),
            Event::ButtonRelease(e) => self.on_button(e.detail, false),
            _ => {}
        }
    }

    fn on_key(&self, keycode: u8, down: bool) {
        let target = MODIFIER_KEYSYM.load(Ordering::Relaxed);
        if target == 0 || !self.keycode_matches(keycode, target) {
            return;
        }
        if down {
            if !MOD_DOWN.swap(true, Ordering::SeqCst) {
                send(true);
            }
        } else if MOD_DOWN.swap(false, Ordering::SeqCst) {
            send(false);
        }
    }

    fn on_button(&self, button: u8, down: bool) {
        let target = MOUSE_BTN.load(Ordering::Relaxed) as u8;
        if target == 0 || button != target {
            return;
        }
        // The click is consumed by the passive grab itself (owner_events = false),
        // so it never reaches the app under the cursor.
        if down {
            if !MOUSE_DOWN.swap(true, Ordering::SeqCst) {
                send(true);
            }
        } else if MOUSE_DOWN.swap(false, Ordering::SeqCst) {
            send(false);
        }
    }

    /// Whether `keycode` produces the target modifier's keysym. Also accepts
    /// ISO_Level3_Shift for Right Alt, which is what AltGr emits on many layouts.
    fn keycode_matches(&self, keycode: u8, target: u32) -> bool {
        if keycode < self.min_keycode || self.keysyms_per_keycode == 0 {
            return false;
        }
        let base = (keycode - self.min_keycode) as usize * self.keysyms_per_keycode;
        let Some(&sym) = self.keysyms.get(base) else {
            return false;
        };
        sym == target || (target == 0xffea && sym == 0xfe03)
    }
}
