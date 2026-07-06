//! macOS Accessibility (AX) trust checks for the Phase 2 event tap.
//!
//! Creating an *active* CGEventTap - one that can consume the bound mouse button
//! and observe modifier keys system-wide - requires the app to be trusted for
//! Accessibility. These wrap the two AX trust calls with a small `extern "C"`
//! block (no extra crate): [`is_trusted`] polls the current state (used by the
//! input thread's retry loop and the onboarding banner), [`prompt_trust`] opens
//! the system prompt that deep-links to System Settings -> Privacy & Security ->
//! Accessibility.
//!
//! Finalized against real macOS/CI per the cross-platform plan's verification
//! model (the AX symbol/ABI details can only be exercised on hardware).

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// Whether Eve is currently trusted for Accessibility. Cheap and side-effect
/// free, so it is safe to poll (the input thread does so while waiting for the
/// user to grant access).
pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Trigger the system Accessibility prompt and return the trust state afterward.
/// Idempotent: a no-op returning `true` when the app is already trusted, so it is
/// safe to call from a "Grant access" button repeatedly.
pub fn prompt_trust() -> bool {
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let value = CFBoolean::true_value();
        let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef())
    }
}
