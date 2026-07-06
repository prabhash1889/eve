//! Phase 2: focused-app context on macOS - bundle id / app name plus a
//! best-effort focused-window title, mapped to an `AppCategory` for Flow Styles
//! and history attribution (the macOS analogue of `active_window::resolve`).
//!
//! The app identity (bundle id, localized name) comes from `NSRunningApplication`
//! for the frontmost pid - reliable and permission-free. The window *title* needs
//! the Accessibility API (`AXUIElement`); it is best-effort and returns empty when
//! untrusted, in which case `classify` still works off the additive macOS
//! bundle-id lists.
//!
//! `#[allow(unused_unsafe)]`: whether a given objc2-app-kit accessor is generated
//! `unsafe` shifts between binding versions (same rationale as `focus.rs`).
//! The AX title path is finalized against real macOS/CI per the plan.

use core_foundation::base::{CFTypeRef, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use objc2_app_kit::NSRunningApplication;

use crate::context::active_window::{classify, AppContext};

/// Resolve the frontmost app (identified by `pid`) into an `AppContext`.
/// `process` carries the bundle identifier (e.g. `com.apple.Safari`) so the
/// additive macOS lists in `classify` match; `title` is the AX focused-window
/// title when Accessibility is granted, else empty.
pub fn resolve(pid: i32) -> AppContext {
    if pid <= 0 {
        return AppContext::unknown();
    }
    let process = app_identity(pid);
    let title = focused_window_title(pid).unwrap_or_default();
    let category = classify(&process, &title);
    AppContext {
        process,
        title,
        category,
    }
}

/// Bundle identifier of the app owning `pid`, falling back to its localized name
/// when a process has no bundle id (rare for GUI apps). Lowercased matching in
/// `classify` means the returned string's case doesn't matter.
#[allow(unused_unsafe)]
fn app_identity(pid: i32) -> String {
    unsafe {
        let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
            return String::new();
        };
        let bundle = app
            .bundleIdentifier()
            .map(|s| s.to_string())
            .unwrap_or_default();
        if !bundle.is_empty() {
            return bundle;
        }
        app.localizedName()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }
}

// AX (Accessibility) FFI for the focused-window title. Small extern block rather
// than a whole crate; success is `kAXErrorSuccess` (0). Values are +1-retained
// (Copy rule) and released via CFRelease / the CF wrapper's create rule.
type AXUIElementRef = *const std::ffi::c_void;
type AXError = i32;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn CFRelease(cf: CFTypeRef);
}

/// Best-effort title of the app's focused window via the Accessibility API.
/// Returns `None` when untrusted, the app has no focused window, or the window
/// exposes no title - callers treat that as "no title" and classify off the
/// bundle id alone.
fn focused_window_title(pid: i32) -> Option<String> {
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return None;
        }

        // app -> AXFocusedWindow.
        let focused_attr = CFString::from_static_string("AXFocusedWindow");
        let mut window: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(app, focused_attr.as_concrete_TypeRef(), &mut window);
        CFRelease(app as CFTypeRef);
        if err != 0 || window.is_null() {
            return None;
        }

        // window -> AXTitle (a CFString).
        let title_attr = CFString::from_static_string("AXTitle");
        let mut title_val: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(
            window as AXUIElementRef,
            title_attr.as_concrete_TypeRef(),
            &mut title_val,
        );
        CFRelease(window);
        if err != 0 || title_val.is_null() {
            return None;
        }

        // Take ownership of the +1-retained CFString and read it out.
        let cf = CFString::wrap_under_create_rule(title_val as CFStringRef);
        let title = cf.to_string();
        if title.is_empty() {
            None
        } else {
            Some(title)
        }
    }
}
