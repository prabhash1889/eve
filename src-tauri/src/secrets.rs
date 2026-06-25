//! Secure API-key storage backed by the OS credential store (Windows Credential
//! Manager via the `keyring` crate). The key never touches the settings JSON.

use keyring::Entry;

const SERVICE: &str = "eve-dictation";
const ACCOUNT: &str = "groq_api_key";

fn entry() -> keyring::Result<Entry> {
    Entry::new(SERVICE, ACCOUNT)
}

pub fn set_api_key(key: &str) -> anyhow::Result<()> {
    entry()?.set_password(key)?;
    Ok(())
}

pub fn get_api_key() -> anyhow::Result<String> {
    Ok(entry()?.get_password()?)
}

pub fn has_api_key() -> bool {
    matches!(entry().and_then(|e| e.get_password()), Ok(_))
}

pub fn delete_api_key() -> anyhow::Result<()> {
    // Ignore "not found" so removing twice is harmless.
    if let Ok(e) = entry() {
        let _ = e.delete_credential();
    }
    Ok(())
}
