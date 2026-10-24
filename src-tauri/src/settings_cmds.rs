//! Commands for the settings app (app styles, audio playback).

use crate::AppState;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

type Res<T> = Result<T, String>;

#[derive(Serialize)]
pub struct AppStyleRow {
    pub id: i64,
    pub match_kind: String,
    pub pattern: String,
    pub style: String,
}

#[tauri::command]
pub fn list_app_styles(state: State<'_, Arc<AppState>>) -> Res<Vec<AppStyleRow>> {
    let rows = state.db.list_app_styles().map_err(|e| format!("{e:#}"))?;
    Ok(rows.into_iter().map(|(id, match_kind, pattern, style)| AppStyleRow { id, match_kind, pattern, style }).collect())
}

#[tauri::command]
pub fn set_app_style(state: State<'_, Arc<AppState>>, id: i64, style: String) -> Res<()> {
    if !["casual", "formal", "neutral", "literal"].contains(&style.as_str()) {
        return Err(format!("Unknown style '{style}'"));
    }
    state.db.set_app_style(id, &style).map_err(|e| format!("{e:#}"))
}

/// Raw bytes of a dictation's saved audio, for the History player.
#[tauri::command]
pub fn read_audio(state: State<'_, Arc<AppState>>, id: i64) -> Result<tauri::ipc::Response, String> {
    let path = state.db.get(id).map_err(|e| format!("{e:#}"))?.and_then(|d| d.audio_path).ok_or("No audio kept for this dictation")?;
    std::fs::read(&path).map(tauri::ipc::Response::new).map_err(|e| format!("Couldn't read audio: {e}"))
}

#[tauri::command]
pub fn add_app_style(state: State<'_, Arc<AppState>>, match_kind: String, pattern: String, style: String) -> Res<()> {
    let pattern = pattern.trim().to_string();
    if pattern.is_empty() || ![crate::context::RULE_KIND.0, "title_contains"].contains(&match_kind.as_str()) {
        return Err("Enter an app name or title text".into());
    }
    if !["casual", "formal", "neutral", "literal"].contains(&style.as_str()) {
        return Err(format!("Unknown style '{style}'"));
    }
    state.db.add_app_style(&match_kind, &pattern, &style).map_err(|e| format!("{e:#}"))
}

#[derive(serde::Serialize)]
pub struct DefaultRule {
    pub id: String,
    pub text: String,
}

/// The built-in cleanup rules, for the "Writing rules" page.
#[tauri::command]
pub fn get_default_cleanup_rules() -> Vec<DefaultRule> {
    crate::cleanup::default_rules().into_iter().map(|r| DefaultRule { id: r.id, text: r.text }).collect()
}

/// While the Hotkeys dialog records a chord, pressing the current dictate chord must not
/// start a dictation. Expires on its own so a closed window can never leave dictation off.
static CAPTURE_UNTIL: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

pub fn capturing() -> bool {
    CAPTURE_UNTIL.load(std::sync::atomic::Ordering::Relaxed) > crate::storage::now_ms()
}

#[tauri::command]
pub fn set_hotkey_capture(on: bool) {
    let until = if on { crate::storage::now_ms() + 60_000 } else { 0 };
    CAPTURE_UNTIL.store(until, std::sync::atomic::Ordering::Relaxed);
}

// ── Dictionary and snippets (Phase 4) ──

/// Terms go into multipart fields and the cleanup prompt: `< >` and line breaks would break the request.
pub fn clean_term(s: &str, max: usize) -> String {
    s.chars().filter(|c| !matches!(c, '<' | '>' | '\r' | '\n')).take(max).collect::<String>().trim().to_string()
}

#[tauri::command]
pub fn list_terms(state: State<'_, Arc<AppState>>) -> Res<Vec<crate::storage::TermRow>> {
    state.db.list_terms().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn add_term(state: State<'_, Arc<AppState>>, term: String, sounds_like: String, learned: Option<bool>) -> Res<()> {
    let term = clean_term(&term, 50);
    if term.is_empty() {
        return Err("Enter a word or name".into());
    }
    let source = if learned == Some(true) { "learned" } else { "user" };
    state.db.add_term(&term, &clean_term(&sounds_like, 80), source).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn delete_term(state: State<'_, Arc<AppState>>, id: i64) -> Res<()> {
    state.db.delete_term(id).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn list_snippets(state: State<'_, Arc<AppState>>) -> Res<Vec<crate::storage::SnippetRow>> {
    state.db.list_snippets().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn save_snippet(state: State<'_, Arc<AppState>>, id: Option<i64>, trigger: String, expansion: String) -> Res<i64> {
    let trigger = trigger.trim();
    if crate::snippets::normalize(trigger).is_empty() || trigger.chars().count() > 60 {
        return Err("The trigger needs 1–60 characters of words".into());
    }
    if expansion.trim().is_empty() || expansion.chars().count() > 4000 {
        return Err("The text needs 1–4000 characters".into());
    }
    state.db.save_snippet(id, trigger, &expansion).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn delete_snippet(state: State<'_, Arc<AppState>>, id: i64) -> Res<()> {
    state.db.delete_snippet(id).map_err(|e| format!("{e:#}"))
}
