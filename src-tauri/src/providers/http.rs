//! Request plumbing shared by every cloud provider: which header carries the key,
//! error text the toast can classify, and the Custom URL safety check.

use super::registry::display_name;
use crate::secrets;
use anyhow::{anyhow, bail, Result};
use reqwest::{RequestBuilder, Response};
use std::net::IpAddr;

/// ✓ platform.claude.com Messages API example, checked 24 Oct 2026.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// The stored key for `id`. A Custom URL server may need none.
pub fn key(id: &str) -> Result<Option<String>> {
    match secrets::provider_key(id) {
        Some(k) => Ok(Some(k)),
        None if id == "custom" => Ok(None),
        None => Err(anyhow!("No API key set for {}. Open Settings.", display_name(id))),
    }
}

/// The key for `id`, but only once `base` is safe to send it to: Custom URL follows
/// `check_custom_url`, every other provider needs https. Call before any request.
pub fn key_for(id: &str, base: &str) -> Result<Option<String>> {
    if id == "custom" {
        check_custom_url(base).map_err(anyhow::Error::msg)?;
    } else {
        require_https(display_name(id), base).map_err(anyhow::Error::msg)?;
    }
    key(id)
}

pub fn require_https(what: &str, url: &str) -> Result<(), String> {
    match reqwest::Url::parse(url.trim()) {
        Ok(u) if u.scheme() == "https" && u.host_str().is_some() => Ok(()),
        _ => Err(format!("{what} URL must start with https://")),
    }
}

/// Every endpoint in settings, checked at the save_settings boundary.
pub fn validate_urls(s: &crate::settings::Settings) -> Result<(), String> {
    require_https("OpenAI", &s.openai_base_url)?;
    require_https("Groq", &s.groq_base_url)?;
    for (id, url) in &s.base_urls {
        require_https(display_name(id), url)?;
    }
    if !s.custom_base_url.trim().is_empty() {
        check_custom_url(&s.custom_base_url)?;
    }
    Ok(())
}

/// Opens the pooled TLS connection with a cheap authenticated GET while the user talks,
/// so the real request doesn't spend its budget on DNS/TCP/TLS. Errors are ignored.
pub async fn prewarm(client: &reqwest::Client, id: &str, base: &str, url: String) {
    let Ok(key) = key_for(id, base) else { return };
    let _ = auth(client.get(url), id, key.as_deref()).timeout(std::time::Duration::from_secs(5)).send().await;
}

/// Adds the provider's auth header (docs/providers.md). No key, no header.
pub fn auth(rb: RequestBuilder, id: &str, key: Option<&str>) -> RequestBuilder {
    let Some(k) = key else { return rb };
    match id {
        "deepgram" => rb.header("Authorization", format!("Token {k}")),
        "elevenlabs" => rb.header("xi-api-key", k),
        "assemblyai" => rb.header("Authorization", k),
        "gemini" => rb.header("x-goog-api-key", k),
        "anthropic" => rb.header("x-api-key", k).header("anthropic-version", ANTHROPIC_VERSION),
        _ => rb.bearer_auth(k),
    }
}

/// Turns a non-2xx response into "{Name} {status}: {snippet}". toast::classify_error
/// keys off the status code and the name; the body never contains our key.
pub async fn check(id: &str, resp: Response) -> Result<Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let snippet: String = resp.text().await.unwrap_or_default().chars().take(300).collect();
    bail!("{} {status}: {snippet}", display_name(id))
}

/// Custom URL rule: https anywhere; plain http only to localhost / LAN.
/// Ok(true) means "allowed but unencrypted" (the UI warns).
pub fn check_custom_url(url: &str) -> Result<bool, String> {
    let u = reqwest::Url::parse(url.trim()).map_err(|_| "Enter a full URL, e.g. http://localhost:8000/v1".to_string())?;
    match u.scheme() {
        "https" => Ok(false),
        "http" if is_local(&u) => Ok(true),
        "http" => Err("Plain http:// is only allowed for localhost or LAN addresses. Use https://.".into()),
        _ => Err("The URL must start with https:// (or http:// for a local server).".into()),
    }
}

fn is_local(u: &reqwest::Url) -> bool {
    let Some(h) = u.host_str() else { return false };
    match h.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        // Loopback, unique-local fc00::/7, link-local fe80::/10.
        Ok(IpAddr::V6(ip)) => ip.is_loopback() || (ip.segments()[0] & 0xfe00) == 0xfc00 || (ip.segments()[0] & 0xffc0) == 0xfe80,
        Err(_) => h == "localhost" || h.ends_with(".localhost") || h.ends_with(".local") || h.ends_with(".lan"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_urls_need_https() {
        let mut s = crate::settings::Settings::default();
        assert!(validate_urls(&s).is_ok());
        s.custom_base_url = "http://localhost:8000/v1".into();
        assert!(validate_urls(&s).is_ok());
        s.custom_base_url = "http://example.com/v1".into();
        assert!(validate_urls(&s).is_err());
        s.custom_base_url.clear();
        s.base_urls.insert("deepgram".into(), "http://api.deepgram.com/v1".into());
        assert!(validate_urls(&s).unwrap_err().contains("Deepgram"));
        s.base_urls.clear();
        s.groq_base_url = "http://localhost/v1".into();
        assert!(validate_urls(&s).is_err());
        // The key is never fetched for an unsafe base.
        assert!(key_for("custom", "http://example.com/v1").is_err());
        assert!(key_for("openai", "http://api.openai.com/v1").is_err());
    }

    #[test]
    fn custom_url_rules() {
        assert_eq!(check_custom_url("https://example.com/v1"), Ok(false));
        for ok in [
            "http://localhost:8000/v1",
            "http://127.0.0.1:8080/v1",
            "http://192.168.1.5/v1",
            "http://10.0.0.2/v1",
            "http://[::1]:9000/v1",
            "http://box.local/v1",
        ] {
            assert_eq!(check_custom_url(ok), Ok(true), "{ok}");
        }
        for bad in ["http://example.com/v1", "http://8.8.8.8/v1", "ftp://localhost/", "localhost:8000", ""] {
            assert!(check_custom_url(bad).is_err(), "{bad}");
        }
    }
}
