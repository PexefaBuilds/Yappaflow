//! Settings commands about the app itself: start with Windows, admin mode, data folders, wiping history.

use crate::AppState;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

type Res<T> = Result<T, String>;

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Res<bool> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, state: State<'_, Arc<AppState>>, on: bool) -> Res<()> {
    let l = app.autolaunch();
    if on { l.enable() } else { l.disable() }.map_err(|e| e.to_string())?;
    let mut g = state.settings.write().unwrap();
    let mut s = g.clone();
    s.start_with_windows = on;
    state.db.save_settings(&s).map_err(|e| format!("{e:#}"))?;
    *g = s;
    Ok(())
}

/// Startup: put the Run entry back if the setting is on and something (a reinstall) removed it.
/// Not in dev builds: it would register the debug exe to run at login.
pub fn restore_autostart(app: &AppHandle, on: bool) {
    let l = app.autolaunch();
    if on && !cfg!(debug_assertions) && !l.is_enabled().unwrap_or(true) {
        if let Err(e) = l.enable() {
            log::warn!("couldn't re-enable start with Windows: {e}");
        }
    }
}

/// macOS permission ticks for onboarding (polled every second). Always all-true on Windows.
#[tauri::command]
pub fn permission_status() -> crate::context::perms::Perms {
    crate::context::perms::status()
}

/// Shows the macOS prompt for one permission (where macOS has one) and opens its System Settings page.
#[tauri::command]
pub fn request_permission(kind: String) -> Res<()> {
    crate::context::perms::request(&kind);
    crate::context::perms::open_pane(&kind)
}

#[tauri::command]
pub fn open_system_settings(kind: String) -> Res<()> {
    crate::context::perms::open_pane(&kind)
}

/// macOS: 🌐/fn is bound to something (emoji picker, input source, dictation) that fires alongside us.
#[tauri::command]
pub fn fn_key_conflict() -> bool {
    crate::context::perms::fn_key_conflicts()
}

/// Input Monitoring may only take effect after a relaunch (spec 8 §6, unverified).
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}

/// Saves the switch; turning it on relaunches elevated (UAC) and exits this instance.
/// A declined UAC prompt switches it back off and returns an error.
#[tauri::command]
pub fn set_admin_mode(app: AppHandle, state: State<'_, Arc<AppState>>, on: bool) -> Res<()> {
    save_admin_mode(&state, on)?;
    #[cfg(windows)]
    if on && !crate::elevate::is_elevated() {
        if let Err(e) = crate::elevate::relaunch_elevated() {
            save_admin_mode(&state, false)?;
            return Err(format!("{e}, so admin mode stays off."));
        }
        app.exit(0);
    }
    let _ = app;
    Ok(())
}

#[tauri::command]
pub fn is_elevated() -> bool {
    #[cfg(windows)]
    return crate::elevate::is_elevated();
    #[cfg(not(windows))]
    false
}

/// A standard (non-admin) Windows account: admin mode would run as another account, so it's refused.
#[tauri::command]
pub fn is_standard_user() -> bool {
    #[cfg(windows)]
    return crate::elevate::is_standard_user();
    #[cfg(not(windows))]
    false
}

fn save_admin_mode(state: &AppState, on: bool) -> Res<()> {
    let mut g = state.settings.write().unwrap();
    let mut s = g.clone();
    s.admin_mode = on;
    state.db.save_settings(&s).map_err(|e| format!("{e:#}"))?;
    *g = s;
    Ok(())
}

/// Opens the data ("data") or log ("logs") folder in Explorer.
#[tauri::command]
pub fn open_app_folder(app: AppHandle, which: String) -> Res<()> {
    let p = app.path();
    let dir = match which.as_str() {
        "data" => p.app_data_dir(),
        "logs" => p.app_log_dir(),
        w => return Err(format!("unknown folder '{w}'")),
    }
    .map_err(|e| e.to_string())?;
    #[cfg(windows)]
    std::process::Command::new("explorer").arg(dir).spawn().map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    let _ = dir;
    Ok(())
}

/// Text of a bundled licence file for About › Licences: "notices" (THIRD_PARTY_NOTICES.md, made in CI)
/// or "fonts" (OFL.txt). Shown in-app, so no file association or opener is needed.
#[tauri::command]
pub fn read_licence(app: AppHandle, which: String) -> Res<String> {
    let file = match which.as_str() {
        "notices" => "THIRD_PARTY_NOTICES.md",
        "fonts" => "OFL.txt",
        w => return Err(format!("unknown licence '{w}'")),
    };
    let path = app.path().resource_dir().map_err(|e| e.to_string())?.join("licenses").join(file);
    std::fs::read_to_string(&path).map_err(|e| format!("Couldn't read {file}: {e}"))
}

/// Deletes every dictation row and its recording. Refused mid-dictation; one that starts meanwhile
/// keeps its row and audio (see `delete_all_dictations`). The VACUUM runs off the UI thread.
#[tauri::command]
pub async fn delete_all_history(app: AppHandle, state: State<'_, Arc<AppState>>) -> Res<()> {
    if crate::hotkey::SESSION_ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("A dictation is in progress. Try again when it's finished.".into());
    }
    let st = state.inner().clone();
    let failed = tauri::async_runtime::spawn_blocking(move || -> Res<usize> {
        let files = st.db.delete_all_dictations().map_err(|e| format!("{e:#}"))?;
        Ok(files.iter().filter(|p| std::fs::remove_file(p).is_err_and(|e| e.kind() != std::io::ErrorKind::NotFound)).count())
    })
    .await
    .map_err(|e| e.to_string())??;
    let _ = tauri::Emitter::emit(&app, "dictation-updated", 0); // History page reloads
    if failed > 0 {
        return Err(format!("History deleted, but {failed} recording(s) couldn't be removed (in use?)."));
    }
    Ok(())
}
