//! API keys live only in the operating system's secure credential store.

use anyhow::{anyhow, Result};

const SERVICE: &str = "computer-agent";

pub fn set(provider_id: &str, key: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, provider_id)?;
    entry.set_password(key).map_err(|e| anyhow!("Could not save key to secure storage: {e}"))
}

pub fn get(provider_id: &str) -> Option<String> {
    keyring::Entry::new(SERVICE, provider_id).ok()?.get_password().ok()
}

pub fn delete(provider_id: &str) {
    if let Ok(e) = keyring::Entry::new(SERVICE, provider_id) {
        let _ = e.delete_credential();
    }
}
