//! Updates from GitHub Releases via tauri-plugin-updater; every package is signature-checked
//! against the public key in tauri.conf.json (release.yml signs them).
//! Checks a minute after launch and every 6 h. With "Install updates automatically" on, the
//! update downloads in the background and a toast offers "Restart now"; otherwise it offers
//! "Update". Nothing installs without a click, so a dictation is never cut off.

use crate::AppState;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

const STABLE: &str = "https://github.com/PexefaBuilds/yappaflow/releases/latest/download/latest.json";
/// Every published release, prereleases too, is copied here by updater-beta.yml.
const BETA: &str = "https://github.com/PexefaBuilds/yappaflow/releases/download/updater-beta/latest.json";
const FIRST_CHECK: Duration = Duration::from_secs(60);
const EVERY: Duration = Duration::from_secs(6 * 3600);

/// The update the last check found, plus its bytes once downloaded.
#[derive(Default)]
pub struct Pending(Mutex<Option<(Update, Option<Vec<u8>>)>>);

pub fn spawn(app: AppHandle) {
    app.manage(Pending::default());
    // Dev builds would keep offering the published release.
    if cfg!(debug_assertions) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            if let Err(e) = background(&app).await {
                log::warn!("update check failed: {e:#}");
            }
            tokio::time::sleep(EVERY).await;
        }
    });
}

async fn find(app: &AppHandle) -> anyhow::Result<Option<Update>> {
    let beta = app.state::<Arc<AppState>>().settings.read().unwrap().update_channel == "beta";
    let url = if beta { BETA } else { STABLE };
    Ok(app.updater_builder().endpoints(vec![url.parse()?])?.build()?.check().await?)
}

async fn background(app: &AppHandle) -> anyhow::Result<()> {
    let Some(u) = find(app).await? else { return Ok(()) };
    let pending = app.state::<Pending>();
    // Offer each version once per run; "Later" means until the next launch.
    if pending.0.lock().unwrap().as_ref().is_some_and(|(p, _)| p.version == u.version) {
        return Ok(());
    }
    let auto = app.state::<Arc<AppState>>().settings.read().unwrap().auto_update;
    let bytes = if auto { Some(u.download(|_, _| {}, || {}).await?) } else { None };
    let (version, notes) = (u.version.clone(), summary(u.body.as_deref()));
    *pending.0.lock().unwrap() = Some((u, bytes));
    if auto {
        crate::toast::update_ready(app, &version, &notes);
    } else {
        crate::toast::update_available(app, &version, &notes);
    }
    Ok(())
}

/// First line of the release notes, short enough for a toast.
fn summary(body: Option<&str>) -> String {
    let line = body.and_then(|b| b.lines().map(str::trim).find(|l| !l.is_empty())).unwrap_or("");
    if line.chars().count() > 110 {
        format!("{}…", line.chars().take(109).collect::<String>())
    } else {
        line.into()
    }
}

/// About › Check for updates: the newer version, or None when this one is the latest.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<String>, String> {
    let u = find(&app).await.map_err(|e| format!("Couldn't check for updates: {e:#}"))?;
    let version = u.as_ref().map(|u| u.version.clone());
    if let Some(u) = u {
        let pending = app.state::<Pending>();
        let mut p = pending.0.lock().unwrap();
        // Keep an already-downloaded copy of the same version.
        if p.as_ref().map_or(true, |(old, _)| old.version != u.version) {
            *p = Some((u, None));
        }
    }
    Ok(version)
}

/// "Update" / "Restart now" (toast) and About's update button: download if needed, verify,
/// install and restart. On Windows the installer closes the app and starts the new version.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let taken = app.state::<Pending>().0.lock().unwrap().take();
    let (u, bytes) = taken.ok_or("No update is waiting. Check for updates first.")?;
    let bytes = match bytes {
        Some(b) => b,
        None => u.download(|_, _| {}, || {}).await.map_err(|e| format!("Download failed: {e}"))?,
    };
    u.install(&bytes).map_err(|e| format!("Couldn't install the update: {e}"))?;
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::summary;

    #[test]
    fn summary_is_the_first_non_empty_line_capped_for_a_toast() {
        assert_eq!(summary(Some("\n  Faster onboarding.\nMore text")), "Faster onboarding.");
        assert_eq!(summary(None), "");
        let long = "a".repeat(200);
        assert_eq!(summary(Some(&long)).chars().count(), 110);
    }
}
