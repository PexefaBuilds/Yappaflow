//! Commands callable from the web UI via `invoke(...)`.

use crate::providers::registry::{self, ProviderInfo};
use crate::settings::Settings;
use crate::storage::DictationRow;
use crate::{insert, pipeline, secrets, AppState};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tauri::State;

type Res<T> = Result<T, String>;
fn e<E: std::fmt::Display>(err: E) -> String {
    format!("{err:#}")
}

#[derive(Serialize)]
pub struct Options {
    pub stt_providers: &'static [ProviderInfo],
    pub cleanup_providers: &'static [ProviderInfo],
    pub hotkey: String,
    pub version: &'static str,
}

#[tauri::command]
pub fn get_options(state: State<'_, Arc<AppState>>) -> Options {
    Options {
        stt_providers: registry::STT_PROVIDERS,
        cleanup_providers: registry::CLEANUP_PROVIDERS,
        hotkey: crate::hotkey_cmds::info_from(&state.settings.read().unwrap().hotkey).display,
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, Arc<AppState>>) -> Settings {
    state.settings.read().unwrap().clone()
}

#[tauri::command]
pub fn save_settings(app: tauri::AppHandle, state: State<'_, Arc<AppState>>, mut settings: Settings) -> Res<()> {
    // Keys are sent to these URLs: reject plain http (except Custom URL on localhost/LAN).
    crate::providers::http::validate_urls(&settings)?;
    // Owned by set_autostart; the UI's copy may be stale.
    settings.start_with_windows = state.settings.read().unwrap().start_with_windows;
    state.db.save_settings(&settings).map_err(e)?;
    let accent_changed = state.settings.read().unwrap().accent != settings.accent;
    let accent = settings.accent.clone();
    *state.settings.write().unwrap() = settings;
    if accent_changed {
        crate::appicon::apply(&app, &accent);
    }
    Ok(())
}

#[tauri::command]
pub fn api_key_status() -> Option<String> {
    secrets::masked()
}

#[tauri::command]
pub fn set_api_key(key: String) -> Res<Option<String>> {
    secrets::set_api_key(&key).map_err(e)?;
    Ok(secrets::masked())
}

/// Checks the key and that the selected STT model is reachable.
#[tauri::command]
pub async fn test_api_key(state: State<'_, Arc<AppState>>) -> Res<String> {
    let key = secrets::api_key().ok_or("No API key saved")?;
    let (base, model) = {
        let s = state.settings.read().unwrap();
        (s.openai_base_url.trim_end_matches('/').to_string(), s.stt_model.clone())
    };
    let resp =
        state.http.get(format!("{base}/models/{model}")).bearer_auth(key).timeout(Duration::from_secs(10)).send().await.map_err(e)?;
    match resp.status().as_u16() {
        200 => Ok(format!("Key works · {model} available")),
        401 => Err("Key rejected (401)".into()),
        404 => Err(format!("Key works, but model '{model}' isn't available to it")),
        s => Err(format!("OpenAI returned {s}")),
    }
}

#[tauri::command]
pub fn set_provider_key(provider: String, key: String) -> Res<()> {
    secrets::check(&provider)?;
    secrets::set_provider_key(&provider, &key).map_err(e)
}

/// Masked key, or empty string when unset.
#[tauri::command]
pub fn provider_key_status(provider: String) -> Res<String> {
    secrets::check(&provider)?;
    Ok(secrets::provider_masked(&provider).unwrap_or_default())
}

/// Custom URL rule for the UI: Ok("") fine, Ok(warning) for unencrypted LAN, Err(reason).
#[tauri::command]
pub fn check_custom_url(url: String) -> Res<String> {
    let insecure = crate::providers::http::check_custom_url(&url)?;
    Ok(if insecure { "Unencrypted: allowed only because this is a localhost or LAN address.".into() } else { String::new() })
}

/// Cheapest authenticated call for the provider (registry::key_test_url). 401/403 map
/// to "<Provider> rejected your key".
#[tauri::command]
pub async fn test_provider_key(state: State<'_, Arc<AppState>>, provider: String) -> Res<String> {
    secrets::check(&provider)?;
    let base = registry::base_url(&state.settings.read().unwrap(), &provider);
    let key = crate::providers::http::key_for(&provider, &base).map_err(e)?;
    let rb = state.http.get(registry::key_test_url(&provider, &base)).timeout(Duration::from_secs(10));
    let resp = crate::providers::http::auth(rb, &provider, key.as_deref()).send().await.map_err(e)?;
    let name = registry::display_name(&provider);
    match resp.status().as_u16() {
        200..=299 => Ok(if key.is_some() { "Key works".into() } else { "Server reachable".into() }),
        c @ (401 | 403) => Err(format!("{name} rejected your key ({c})")),
        c => Err(format!("{name} returned {c}")),
    }
}

#[tauri::command]
pub async fn list_mics() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(pipeline::list_mics).await.unwrap_or_default()
}

#[tauri::command]
pub fn recent_dictations(state: State<'_, Arc<AppState>>, limit: Option<u32>) -> Res<Vec<DictationRow>> {
    state.db.recent(limit.unwrap_or(20)).map_err(e)
}

#[derive(Serialize)]
pub struct UsageEstimate {
    pub prices_checked: &'static str,
    pub version: &'static str,
    pub rows: Vec<crate::usage::RowCost>,
}

/// Per-dictation cost estimates since `since_ms`; the UI groups them by local month.
#[tauri::command]
pub fn usage_estimate(state: State<'_, Arc<AppState>>, since_ms: i64) -> Res<UsageEstimate> {
    let p = crate::usage::prices();
    let rows = state.db.usage_since(since_ms).map_err(e)?.iter().map(|r| p.estimate(r)).collect();
    Ok(UsageEstimate { prices_checked: &p.checked, version: env!("CARGO_PKG_VERSION"), rows })
}

#[tauri::command]
pub async fn retry_dictation(state: State<'_, Arc<AppState>>, id: i64) -> Res<DictationRow> {
    pipeline::retry(&state, id).await.map_err(e)
}

/// "Use cloud once" (local model missing or still downloading): re-transcribe this one recording
/// with the first cloud provider that has a key. Only ever on the user's click.
#[tauri::command]
pub async fn retry_dictation_cloud(state: State<'_, Arc<AppState>>, id: i64) -> Res<DictationRow> {
    let stt = crate::providers::registry::cloud_stt_with_key().ok_or("No cloud API key is saved.")?;
    pipeline::retry_with(&state, id, Some(stt)).await.map_err(e)
}

#[tauri::command]
pub fn copy_text(text: String) -> Res<()> {
    insert::copy_to_clipboard(&text).map_err(e)
}

#[tauri::command]
pub fn open_recordings_folder(state: State<'_, Arc<AppState>>) -> Res<()> {
    let dir = state.recordings_dir.clone();
    #[cfg(windows)]
    std::process::Command::new("explorer").arg(dir).spawn().map_err(e)?;
    #[cfg(not(windows))]
    let _ = dir;
    Ok(())
}

/// Optional real OpenAI spend since `since_ms`, using the admin key saved as "openai_admin".
#[tauri::command]
pub async fn openai_real_cost(state: State<'_, Arc<AppState>>, since_ms: i64) -> Res<crate::openai_costs::RealCost> {
    let admin = secrets::provider_key("openai_admin").ok_or("No OpenAI admin key is saved.")?;
    crate::openai_costs::since(&state.http, &admin, secrets::api_key().as_deref(), since_ms / 1000).await
}
