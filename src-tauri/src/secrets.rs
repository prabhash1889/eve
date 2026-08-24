//! Secure API-key storage backed by the OS credential store (Windows Credential
//! Manager via the `keyring` crate). The key never touches the settings JSON.
//!
//! Reads are served from a small in-process cache so the per-session hot path
//! (transcription → polish → command mode) doesn't pay a keychain round-trip on
//! every call. The cached value lives only in process memory - the same trust
//! domain as `reqwest` and every other holder of the decrypted key - and is
//! invalidated whenever the keychain value changes. It is never logged or
//! persisted anywhere outside the OS keychain.

use std::sync::{Mutex, MutexGuard};

use keyring::Entry;

const SERVICE: &str = "eve-dictation";
const ACCOUNT: &str = "groq_api_key";

/// In-process mirror of the keychain entry: `Some(Some(key))` = known key,
/// `Some(None)` = known absence, `None` = not read since startup/invalidation.
static CACHE: Mutex<Option<Option<String>>> = Mutex::new(None);

fn cache() -> MutexGuard<'static, Option<Option<String>>> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

fn entry() -> keyring::Result<Entry> {
    Entry::new(SERVICE, ACCOUNT)
}

pub fn set_api_key(key: &str) -> anyhow::Result<()> {
    entry()?.set_password(key)?;
    // Only after the keychain write succeeded do we adopt the new value.
    *cache() = Some(Some(key.to_string()));
    Ok(())
}

pub fn get_api_key() -> anyhow::Result<String> {
    if let Some(cached) = cache().clone() {
        return cached.ok_or_else(|| anyhow::anyhow!("Set your Groq API key in Settings"));
    }
    match entry()?.get_password() {
        Ok(key) => {
            *cache() = Some(Some(key.clone()));
            Ok(key)
        }
        Err(keyring::Error::NoEntry) => {
            *cache() = Some(None);
            anyhow::bail!("Set your Groq API key in Settings");
        }
        // Keychain itself failed (locked/unavailable). Don't cache the failure -
        // the next read retries the OS call.
        Err(e) => Err(e.into()),
    }
}

pub fn has_api_key() -> bool {
    if let Some(cached) = cache().clone() {
        return cached.is_some();
    }
    match entry().and_then(|e| e.get_password()) {
        Ok(key) => {
            *cache() = Some(Some(key));
            true
        }
        // Genuinely no credential stored — the only case that means "no key".
        Err(keyring::Error::NoEntry) => {
            *cache() = Some(None);
            false
        }
        // The keychain itself is unavailable/locked/erroring. Don't silently
        // treat this as "no key" — log it so a real OS failure is visible rather
        // than masquerading as an un-onboarded user.
        Err(e) => {
            eprintln!("[secrets] keychain unavailable while checking for API key: {e}");
            false
        }
    }
}

/// Fail-open variant of [`has_api_key`] for callers that skip Groq-keyed work
/// when the answer is "no": `false` only when the key is KNOWN absent. A
/// transient keychain failure returns `true` so the caller does the keyed work
/// (e.g. still encodes the WAV) instead of silently skipping something a
/// fallback might need moments later.
pub fn has_api_key_fail_open() -> bool {
    if let Some(cached) = cache().clone() {
        return cached.is_some();
    }
    match entry().and_then(|e| e.get_password()) {
        Ok(key) => {
            *cache() = Some(Some(key));
            true
        }
        Err(keyring::Error::NoEntry) => {
            *cache() = Some(None);
            false
        }
        Err(e) => {
            eprintln!("[secrets] keychain unavailable while checking for API key: {e}");
            true
        }
    }
}

pub fn delete_api_key() -> anyhow::Result<()> {    // Ignore "not found" so removing twice is harmless.
    if let Ok(e) = entry() {
        let _ = e.delete_credential();
    }
    *cache() = Some(None);
    Ok(())
}
