//! First-run onboarding window (label "onboarding", page onboarding.html).

use crate::AppState;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

pub fn show(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("onboarding") {
        // Restart at step 1 each time it is re-run.
        let _ = w.eval("location.reload()");
        let _ = w.show();
        let _ = w.unminimize();
        return w.set_focus();
    }
    WebviewWindowBuilder::new(app, "onboarding", WebviewUrl::App("onboarding.html".into()))
        .additional_browser_args(crate::WEBVIEW_ARGS)
        .title("Welcome to Yappaflow")
        .inner_size(960.0, fit_height(app))
        .center()
        .build()?;
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        let accent = state.settings.read().map(|s| s.accent.clone()).unwrap_or_default();
        crate::appicon::apply(app, &accent);
    }
    Ok(())
}

/// Tall enough for the Connect step without scrolling; smaller screens scroll instead.
fn fit_height(app: &AppHandle) -> f64 {
    const WANT: f64 = 780.0;
    let monitor = app.primary_monitor().ok().flatten();
    monitor.map_or(WANT, |m| (m.work_area().size.height as f64 / m.scale_factor() - 48.0).clamp(540.0, WANT))
}

#[tauri::command]
pub fn show_onboarding(app: AppHandle) -> Result<(), String> {
    show(&app).map_err(|e| e.to_string())
}

/// Marks onboarding done and closes it. The UI saves its choices (provider, mic, typing style) first.
#[tauri::command]
pub fn finish_onboarding(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
    finish(&app, &state)
}

/// Shared by the Finish button and closing the window with X.
pub fn finish(app: &AppHandle, state: &AppState) -> Result<(), String> {
    mark_onboarded(state)?;
    if let Some(w) = app.get_webview_window("onboarding") {
        let _ = w.hide();
    }
    Ok(())
}

/// Read-modify-write under one write lock so concurrent saves can't be lost.
/// Returns true when this call is the one that marked it.
pub fn mark_onboarded(state: &AppState) -> Result<bool, String> {
    let mut g = state.settings.write().unwrap();
    if g.onboarded {
        return Ok(false);
    }
    let mut s = g.clone();
    s.onboarded = true;
    state.db.save_settings(&s).map_err(|e| format!("{e:#}"))?;
    *g = s;
    Ok(true)
}
