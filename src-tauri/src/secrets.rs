//! Secure API-key storage backed by the OS credential store (Windows Credential
//! Manager via the `keyring` crate). Keys never touch the settings JSON.
//!
//! Multi-provider (Phase 2): each supported provider gets its own keychain
//! entry under one service name, addressed through the [`ProviderKey`] enum.
//! Groq deliberately keeps the exact account name the original single-slot
//! version always used (`groq_api_key`), so existing installs read their stored
//! key with zero migration - the legacy-read/write-through concern is moot by
//! construction.
//!
//! Reads are served from a small in-process cache so the per-session hot path
//! (transcription -> polish -> command mode) doesn't pay a keychain round-trip
//! on every call. Cached values live only in process memory - the same trust
//! domain as `reqwest` and every other holder of a decrypted key - and are
//! invalidated whenever the keychain value changes. They are never logged or
//! persisted anywhere outside the OS keychain.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use keyring::Entry;

const SERVICE: &str = "eve-dictation";

/// Which provider's credential an operation targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKey {
    Groq,
    OpenAi,
    OpenRouter,
    Anthropic,
    Deepgram,
}

impl ProviderKey {
    /// Parse the wire form used by the IPC commands and settings strings.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "groq" => Some(ProviderKey::Groq),
            "openai" => Some(ProviderKey::OpenAi),
            "openrouter" => Some(ProviderKey::OpenRouter),
            "anthropic" => Some(ProviderKey::Anthropic),
            "deepgram" => Some(ProviderKey::Deepgram),
            _ => None,
        }
    }

    /// Human label for error messages ("Set your OpenAI API key...").
    pub fn label(self) -> &'static str {
        match self {
            ProviderKey::Groq => "Groq",
            ProviderKey::OpenAi => "OpenAI",
            ProviderKey::OpenRouter => "OpenRouter",
            ProviderKey::Anthropic => "Anthropic",
            ProviderKey::Deepgram => "Deepgram",
        }
    }

    /// Stable OS-keychain account name for this provider.
    fn account(self) -> &'static str {
        match self {
            // Legacy compatibility: identical to the pre-multi-provider slot.
            ProviderKey::Groq => "groq_api_key",
            ProviderKey::OpenAi => "openai_api_key",
            ProviderKey::OpenRouter => "openrouter_api_key",
            ProviderKey::Anthropic => "anthropic_api_key",
            ProviderKey::Deepgram => "deepgram_api_key",
        }
    }
}

/// In-process mirror of the keychain entries, keyed by account name:
/// `Some(Some(key))` = known key, `Some(None)` = known absence, absent entry =
/// not read since startup/invalidation.
static CACHE: Mutex<Option<HashMap<&'static str, Option<String>>>> = Mutex::new(None);

fn cache() -> MutexGuard<'static, Option<HashMap<&'static str, Option<String>>>> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Snapshot one provider's cached value without holding the guard.
fn cached(account: &'static str) -> Option<Option<String>> {
    cache().as_ref().and_then(|m| m.get(account).cloned())
}

fn set_cache(account: &'static str, value: Option<String>) {
    cache()
        .get_or_insert_with(HashMap::new)
        .insert(account, value);
}

fn entry(p: ProviderKey) -> keyring::Result<Entry> {
    Entry::new(SERVICE, p.account())
}

pub fn set_provider_key(p: ProviderKey, key: &str) -> anyhow::Result<()> {
    entry(p)?.set_password(key)?;
    // Only after the keychain write succeeded do we adopt the new value.
    set_cache(p.account(), Some(key.to_string()));
    Ok(())
}

pub fn get_provider_key(p: ProviderKey) -> anyhow::Result<String> {
    if let Some(cached) = cached(p.account()) {
        return cached.ok_or_else(|| anyhow::anyhow!("Set your {} API key in Settings", p.label()));
    }
    match entry(p)?.get_password() {
        Ok(key) => {
            set_cache(p.account(), Some(key.clone()));
            Ok(key)
        }
        Err(keyring::Error::NoEntry) => {
            set_cache(p.account(), None);
            anyhow::bail!("Set your {} API key in Settings", p.label());
        }
        // Keychain itself failed (locked/unavailable). Don't cache the failure -
        // the next read retries the OS call.
        Err(e) => Err(e.into()),
    }
}

pub fn has_provider_key(p: ProviderKey) -> bool {
    if let Some(cached) = cached(p.account()) {
        return cached.is_some();
    }
    match entry(p).and_then(|e| e.get_password()) {
        Ok(key) => {
            set_cache(p.account(), Some(key));
            true
        }
        // Genuinely no credential stored — the only case that means "no key".
        Err(keyring::Error::NoEntry) => {
            set_cache(p.account(), None);
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

/// Fail-open variant of [`has_provider_key`] for callers that skip keyed work
/// when the answer is "no": `false` only when the key is KNOWN absent. A
/// transient keychain failure returns `true` so the caller does the keyed work
/// (e.g. still encodes the WAV) instead of silently skipping something a
/// fallback might need moments later.
pub fn has_provider_key_fail_open(p: ProviderKey) -> bool {
    if let Some(cached) = cached(p.account()) {
        return cached.is_some();
    }
    match entry(p).and_then(|e| e.get_password()) {
        Ok(key) => {
            set_cache(p.account(), Some(key));
            true
        }
        Err(keyring::Error::NoEntry) => {
            set_cache(p.account(), None);
            false
        }
        Err(e) => {
            eprintln!("[secrets] keychain unavailable while checking for API key: {e}");
            true
        }
    }
}

pub fn delete_provider_key(p: ProviderKey) -> anyhow::Result<()> {
    // Ignore "not found" so removing twice is harmless.
    if let Ok(e) = entry(p) {
        let _ = e.delete_credential();
    }
    set_cache(p.account(), None);
    Ok(())
}

// --- Groq legacy shims --------------------------------------------------------
// Kept so the existing transcription/pipeline hot path reads naturally. New
// code should use the per-provider variants.

pub fn set_api_key(key: &str) -> anyhow::Result<()> {
    set_provider_key(ProviderKey::Groq, key)
}

pub fn has_api_key() -> bool {
    has_provider_key(ProviderKey::Groq)
}

pub fn has_api_key_fail_open() -> bool {
    has_provider_key_fail_open(ProviderKey::Groq)
}

pub fn delete_api_key() -> anyhow::Result<()> {
    delete_provider_key(ProviderKey::Groq)
}
