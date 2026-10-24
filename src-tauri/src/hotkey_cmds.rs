//! Hotkey commands for the settings UI, plus the non-dictation hotkey actions.

use crate::hotkey::actions::{self, Action, ActionId, ALL};
use crate::hotkey::keys as k;
use crate::pipeline::CtlMsg;
use crate::settings::Settings;
use crate::{insert, AppState};
use serde::Serialize;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, State};

#[derive(Serialize)]
pub struct HotkeyInfo {
    /// e.g. "Ctrl + Win"
    pub display: String,
    pub keys: Vec<String>,
}

#[derive(Serialize)]
pub struct HotkeyBinding {
    pub action: String,
    pub label: String,
    pub keys: Vec<String>,
    pub display: String,
    pub default_keys: Vec<String>,
    pub enabled: bool,
}

pub fn info_from(names: &[String]) -> HotkeyInfo {
    let codes = k::parse_chord(names).unwrap_or_else(|_| vec![k::CTRL, k::WIN]);
    HotkeyInfo { display: k::chord_display(&codes), keys: k::chord_names(&codes) }
}

fn codes_of(s: &Settings, a: ActionId) -> Vec<u8> {
    actions::resolve(a, &s.hotkey, &s.hotkeys)
}

/// Dictate chord display for tooltips and logs.
pub fn dictate_display(s: &Settings) -> String {
    k::chord_display(&codes_of(s, ActionId::Dictate))
}

/// The chord the hook should listen for: the hands-free chord is only live when double-tap is off.
fn live_codes(s: &Settings, a: ActionId) -> Vec<u8> {
    if a == ActionId::HandsFree && s.hands_free_double_tap {
        return vec![];
    }
    codes_of(s, a)
}

/// Push every saved chord into the hook's atomics (invalid saved values fall back to defaults).
pub fn apply_all(s: &Settings) {
    for a in ALL {
        crate::hotkey::set_chord(a, &live_codes(s, a));
    }
    crate::hotkey::HANDS_FREE_DOUBLE_TAP.store(s.hands_free_double_tap, std::sync::atomic::Ordering::Relaxed);
}

fn all_chords(s: &Settings) -> Vec<(ActionId, Vec<u8>)> {
    ALL.iter().map(|a| (*a, live_codes(s, *a))).collect()
}

fn binding(s: &Settings, a: ActionId) -> HotkeyBinding {
    let c = codes_of(s, a);
    HotkeyBinding {
        action: a.name().into(),
        label: a.label().into(),
        keys: k::chord_names(&c),
        display: k::chord_display(&c),
        default_keys: k::chord_names(&a.default_codes()),
        enabled: !c.is_empty(),
    }
}

fn validate(a: ActionId, names: &[String]) -> Result<Vec<u8>, String> {
    if names.is_empty() {
        return if a == ActionId::Dictate { Err("Dictate needs a hotkey".into()) } else { Ok(vec![]) };
    }
    k::parse_chord_opt(names, a == ActionId::Cancel)
}

fn store(s: &mut Settings, a: ActionId, codes: &[u8]) {
    let names = k::chord_names(codes);
    if a == ActionId::Dictate {
        s.hotkey = names;
    } else {
        s.hotkeys.insert(a.name().into(), names);
    }
}

fn set_for(state: &AppState, app: Option<&AppHandle>, a: ActionId, codes: Vec<u8>) -> Result<(), String> {
    let mut s = state.settings.write().unwrap();
    store(&mut s, a, &codes);
    state.db.save_settings(&s).map_err(|e| format!("{e:#}"))?;
    crate::hotkey::set_chord(a, &live_codes(&s, a));
    if a == ActionId::Dictate {
        if let Some(t) = app.and_then(|h| h.tray_by_id("main")) {
            let _ = t.set_tooltip(Some(format!("{}: hold {} to dictate", crate::APP_NAME, k::chord_display(&codes))));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn get_hotkey(state: State<'_, Arc<AppState>>) -> Result<HotkeyInfo, String> {
    Ok(info_from(&state.settings.read().unwrap().hotkey))
}

#[tauri::command]
pub fn set_hotkey(app: AppHandle, state: State<'_, Arc<AppState>>, keys: Vec<String>) -> Result<(), String> {
    let codes = validate(ActionId::Dictate, &keys)?;
    set_for(&state, Some(&app), ActionId::Dictate, codes)
}

#[tauri::command]
pub fn check_hotkey_conflict(state: State<'_, Arc<AppState>>, keys: Vec<String>) -> Result<Vec<String>, String> {
    check_for(&state, ActionId::Dictate, &keys)
}

#[tauri::command]
pub fn get_hotkeys(state: State<'_, Arc<AppState>>) -> Result<Vec<HotkeyBinding>, String> {
    let s = state.settings.read().unwrap();
    Ok(ALL.iter().map(|a| binding(&s, *a)).collect())
}

#[tauri::command]
pub fn set_hotkey_for(app: AppHandle, state: State<'_, Arc<AppState>>, action: String, keys: Vec<String>) -> Result<(), String> {
    let a = ActionId::parse(&action)?;
    let codes = validate(a, &keys)?;
    set_for(&state, Some(&app), a, codes)
}

#[tauri::command]
pub fn reset_hotkey(app: AppHandle, state: State<'_, Arc<AppState>>, action: String) -> Result<(), String> {
    let a = ActionId::parse(&action)?;
    set_for(&state, Some(&app), a, a.default_codes())
}

/// Settings › Hotkeys › Hands-free: double-tap the dictate chord, or use its own hotkey.
#[tauri::command]
pub fn set_hands_free_double_tap(state: State<'_, Arc<AppState>>, on: bool) -> Result<(), String> {
    let mut s = state.settings.write().unwrap();
    s.hands_free_double_tap = on;
    state.db.save_settings(&s).map_err(|e| format!("{e:#}"))?;
    apply_all(&s);
    Ok(())
}

fn check_for(state: &AppState, a: ActionId, keys: &[String]) -> Result<Vec<String>, String> {
    let codes = validate(a, keys)?;
    let all = all_chords(&state.settings.read().unwrap());
    Ok(actions::action_conflicts(a, &codes, &all))
}

#[tauri::command]
pub fn check_hotkey_conflict_for(state: State<'_, Arc<AppState>>, action: String, keys: Vec<String>) -> Result<Vec<String>, String> {
    check_for(&state, ActionId::parse(&action)?, &keys)
}

/// Runs on the forwarder thread (never the hook thread).
pub fn run_action(app: &AppHandle, state: &Arc<AppState>, ctl: &Sender<CtlMsg>, a: Action) {
    match a {
        Action::HandsFree => {
            let _ = ctl.send(CtlMsg::HandsFree);
        }
        Action::CommandMode => {
            let _ = ctl.send(CtlMsg::Command);
        }
        Action::TogglePause => {
            crate::tray::tray_pause(app.clone(), !crate::tray::paused());
        }
        Action::PasteLast if !crate::tray::paused() => {
            let state = state.clone();
            std::thread::spawn(move || {
                let text = state.db.recent(10).ok().and_then(|r| r.into_iter().find_map(|d| d.final_text.filter(|t| !t.is_empty())));
                let Some(text) = text else { return };
                let s = state.settings.read().unwrap().clone();
                let opts = insert::InsertOptions {
                    mode: s.insert_mode,
                    restore_clipboard: s.restore_clipboard,
                    restore_delay: Duration::from_millis(s.restore_delay_ms),
                };
                let _ = insert::insert_text(&text, &opts);
            });
        }
        Action::PasteLast => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_resolve_roundtrip() {
        let mut s = Settings::default();
        store(&mut s, ActionId::Cancel, &[]);
        store(&mut s, ActionId::PasteLast, &[k::CTRL, 0x7B]);
        store(&mut s, ActionId::Dictate, &[0x78]);
        assert!(codes_of(&s, ActionId::Cancel).is_empty());
        assert_eq!(codes_of(&s, ActionId::PasteLast), vec![k::CTRL, 0x7B]);
        assert_eq!(dictate_display(&s), "F9");
        assert!(!binding(&s, ActionId::Cancel).enabled);
        assert_eq!(binding(&s, ActionId::Cancel).default_keys, vec!["Esc".to_string()]);
    }

    #[test]
    fn validate_rules() {
        assert!(validate(ActionId::Dictate, &[]).is_err());
        assert_eq!(validate(ActionId::PasteLast, &[]), Ok(vec![]));
        assert!(validate(ActionId::PasteLast, &["Esc".into()]).is_err());
        assert!(validate(ActionId::Cancel, &["Esc".into()]).is_ok());
    }

    #[test]
    fn hands_free_chord_live_only_without_double_tap() {
        let mut s = Settings::default();
        store(&mut s, ActionId::HandsFree, &[0x78]);
        assert!(live_codes(&s, ActionId::HandsFree).is_empty(), "double-tap is the default");
        assert_eq!(binding(&s, ActionId::HandsFree).keys, vec!["F9".to_string()], "saved chord still shown");
        s.hands_free_double_tap = false;
        assert_eq!(live_codes(&s, ActionId::HandsFree), vec![0x78]);
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(old.hands_free_double_tap, "old settings load with double-tap on");
    }
}
