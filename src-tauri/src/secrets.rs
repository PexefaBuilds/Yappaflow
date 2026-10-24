//! API keys in Windows Credential Manager / the macOS Keychain (keyring crate: windows-native and
//! apple-native backends; service "yappaflow", account "<provider>_api_key"), one entry per provider id.
//! Unsigned macOS dev builds may ask for Keychain access again after each rebuild; that's expected.
//! Cached in memory after the first read; never written to SQLite, logs, or the UI.

use crate::APP_ID;
use std::collections::HashMap;
use std::sync::RwLock;

static CACHE: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);

/// OpenAI keeps its original entry name so existing installs keep working.
fn account(provider: &str) -> String {
    if provider == "openai" {
        "openai_api_key".into()
    } else {
        format!("{provider}_api_key")
    }
}

/// Only these ids may become Credential Manager entry names.
pub const KNOWN: &[&str] =
    &["openai", "groq", "mistral", "deepgram", "elevenlabs", "assemblyai", "gemini", "anthropic", "openrouter", "custom"];

pub fn check(provider: &str) -> Result<(), String> {
    if KNOWN.contains(&provider) {
        Ok(())
    } else {
        Err(format!("Unknown provider '{provider}'"))
    }
}

fn entry(provider: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(APP_ID, &account(provider))
}

pub fn provider_key(provider: &str) -> Option<String> {
    if let Some(k) = CACHE.read().unwrap().as_ref().and_then(|m| m.get(provider)) {
        return Some(k.clone());
    }
    // Only a found key is cached, so a transient Credential Manager failure retries next time.
    let v = entry(provider).and_then(|e| e.get_password()).ok().filter(|k| !k.is_empty())?;
    CACHE.write().unwrap().get_or_insert_with(HashMap::new).insert(provider.to_string(), v.clone());
    Some(v)
}

pub fn set_provider_key(provider: &str, key: &str) -> anyhow::Result<()> {
    let key = key.trim();
    let e = entry(provider)?;
    if key.is_empty() {
        let _ = e.delete_credential();
        if let Some(m) = CACHE.write().unwrap().as_mut() {
            m.remove(provider);
        }
    } else {
        e.set_password(key)?;
        CACHE.write().unwrap().get_or_insert_with(HashMap::new).insert(provider.to_string(), key.to_string());
    }
    Ok(())
}

/// Last 4 chars only, e.g. "…1234".
pub fn mask(key: &str) -> String {
    let tail: String = key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    format!("…{tail}")
}

pub fn provider_masked(provider: &str) -> Option<String> {
    provider_key(provider).map(|k| mask(&k))
}

// OpenAI shorthands used by the existing code.
pub fn api_key() -> Option<String> {
    provider_key("openai")
}

pub fn set_api_key(key: &str) -> anyhow::Result<()> {
    set_provider_key("openai", key)
}

/// "sk-…abcd" for display.
pub fn masked() -> Option<String> {
    api_key().map(|k| format!("sk-{}", mask(&k)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accounts_and_mask() {
        assert_eq!(account("openai"), "openai_api_key");
        assert!(check("groq").is_ok() && check("../x").is_err());
        assert_eq!(account("groq"), "groq_api_key");
        assert_eq!(mask("gsk_abcdef1234"), "…1234");
        assert_eq!(mask("ab"), "…ab");
    }
}
