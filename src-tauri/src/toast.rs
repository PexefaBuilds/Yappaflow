//! Toasts: a small transparent window above the pill. Like the pill it never takes focus.
//! Rust decides *what* to say (`on_status`), toast.ts renders it, runs the 6 s timer and the actions.

use crate::insert::Outcome;
use crate::pipeline::Status;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const W: f64 = 372.0;
const H: f64 = 260.0;
/// Pill top edge (24 offset + ~35 pill) plus a gap, minus the toast's own shadow padding.
const ABOVE_BOTTOM: f64 = 48.0;

#[derive(Serialize, Clone)]
pub struct ToastAction {
    pub label: &'static str,
    /// retry | cloud_once | settings | dismiss | restart | update | add_term | skip_term (handled in toast.ts)
    pub kind: &'static str,
    pub arg: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct Toast {
    /// accent | warn | muted
    pub dot: &'static str,
    pub title: String,
    pub body: String,
    pub action: Option<ToastAction>,
    pub secondary: Option<ToastAction>,
}

fn act(label: &'static str, kind: &'static str, arg: Option<String>) -> Option<ToastAction> {
    Some(ToastAction { label, kind, arg })
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, "toast", WebviewUrl::App("toast.html".into()))
        .additional_browser_args(crate::WEBVIEW_ARGS)
        .title("Yappaflow toast")
        .inner_size(W, H)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .focused(false)
        .visible(false)
        .build()?;
    #[cfg(target_os = "macos")]
    if let Some(w) = app.get_webview_window("toast") {
        let _ = w.set_visible_on_all_workspaces(true);
    }
    Ok(())
}

fn place(app: &AppHandle, w: &WebviewWindow) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| w.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return };
    let (wa, scale) = (m.work_area(), m.scale_factor());
    let size = w.outer_size().unwrap_or(tauri::PhysicalSize::new((W * scale) as u32, (H * scale) as u32));
    let x = wa.position.x + (wa.size.width as i32 - size.width as i32) / 2;
    let y = wa.position.y + wa.size.height as i32 - size.height as i32 - (ABOVE_BOTTOM * scale) as i32;
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

pub fn show(app: &AppHandle, t: Toast) {
    let Some(w) = app.get_webview_window("toast") else { return };
    place(app, &w);
    let _ = w.set_ignore_cursor_events(false);
    let _ = w.show();
    let _ = app.emit_to("toast", "toast", t);
}

pub fn admin_window(app: &AppHandle) {
    show(
        app,
        Toast {
            dot: "warn",
            title: "Copied, press Ctrl+V".into(),
            body: "This window runs as admin, so Windows won't let us paste into it.".into(),
            action: act("How to fix", "settings", Some("advanced".into())),
            secondary: None,
        },
    );
}

/// macOS: text left on the clipboard (secure input, Accessibility off, keystrokes failed).
pub fn copied_mac(app: &AppHandle, reason: &str) {
    let (body, action) = match reason {
        "Accessibility is off" => (
            "Yappaflow needs Accessibility to paste for you.".to_string(),
            act("Open settings", "system_settings", Some("accessibility".into())),
        ),
        "secure input is on" => ("A password field or Secure Keyboard Entry is active, so we didn't type.".to_string(), None),
        r => (format!("We couldn't paste ({r})."), None),
    };
    show(app, Toast { dot: "warn", title: "Copied · press ⌘V".into(), body, action, secondary: None });
}

/// macOS: a permission was switched off while running; dictation falls back to the clipboard.
#[cfg(target_os = "macos")]
pub fn permission_revoked(app: &AppHandle, kind: &'static str, name: &str) {
    show(
        app,
        Toast {
            dot: "warn",
            title: format!("{name} is off"),
            body: "Yappaflow can't work fully without it. Dictations stay on the clipboard until it's back.".into(),
            action: act("Open settings", "system_settings", Some(kind.into())),
            secondary: None,
        },
    );
}

/// Command mode pasted over the selection; the app's own undo brings the original back.
pub fn replaced_selection(app: &AppHandle) {
    show(
        app,
        Toast {
            dot: "accent",
            title: "Replaced selection".into(),
            body: if cfg!(target_os = "macos") { "⌘Z to undo" } else { "Ctrl+Z to undo" }.into(),
            action: None,
            secondary: None,
        },
    );
}

/// Command mode refused before sending anything (password field, selection too long).
pub fn command_refused(app: &AppHandle, title: &str, body: &str) {
    show(app, Toast { dot: "warn", title: title.into(), body: body.into(), action: None, secondary: None });
}

/// Downloaded in the background ("Install updates automatically"); installs on Restart now.
pub fn update_ready(app: &AppHandle, version: &str, notes: &str) {
    let body = if notes.is_empty() { "Restart to finish updating." } else { notes };
    show(
        app,
        Toast {
            dot: "accent",
            title: format!("Yappaflow {version} is ready"),
            body: body.into(),
            action: act("Restart now", "restart", None),
            secondary: act("Later", "dismiss", None),
        },
    );
}

/// Automatic install is off: offer the update; Update downloads, installs and restarts.
pub fn update_available(app: &AppHandle, version: &str, notes: &str) {
    let body = if notes.is_empty() { "A new version is available." } else { notes };
    show(
        app,
        Toast {
            dot: "accent",
            title: format!("Yappaflow {version} is available"),
            body: body.into(),
            action: act("Update", "update", None),
            secondary: act("Later", "dismiss", None),
        },
    );
}

/// Error text comes from anyhow chains, so classification is by substring
/// (providers format as "{id} {status}: ...").
pub fn classify_error(message: &str, row_id: Option<i64>) -> Toast {
    let m = message.to_lowercase();
    let saved = if row_id.is_some() { " Your recording is saved." } else { "" };
    if m.contains("no api key") || m.contains("401") || m.contains("403") {
        let name = crate::secrets::KNOWN.iter().map(|id| crate::providers::registry::display_name(id)).find(|n| message.contains(n));
        let missing = m.contains("no api key");
        Toast {
            dot: "warn",
            title: match (missing, name) {
                (true, Some(n)) => format!("No {n} key"),
                (true, None) => "No API key".into(),
                (false, Some(n)) => format!("{n} rejected your key"),
                (false, None) => "Your API key was rejected".into(),
            },
            body: if missing {
                format!("Add one in Settings › Models.{saved}")
            } else {
                format!("It may be revoked or out of credit.{saved}")
            },
            action: act("Update key", "settings", Some("models".into())),
            secondary: None,
        }
    } else if m.starts_with("mic error") || m.contains("no audio captured") {
        Toast {
            dot: "warn",
            title: "Can't hear anything".into(),
            body: if cfg!(target_os = "macos") {
                "No microphone is connected, or macOS blocked access to it (System Settings › Privacy › Microphone)."
            } else {
                "No microphone is connected, or Windows blocked access to it."
            }
            .into(),
            action: act("Choose mic", "settings", Some("microphone".into())),
            secondary: None,
        }
    } else if is_local_model_error(&m) {
        // Only look up keys (keychain) on this path.
        let cloud = row_id.and_then(|_| crate::providers::registry::cloud_stt_with_key()).map(|(id, _)| id);
        local_model_toast(message, row_id, cloud)
    } else {
        let offline = ["error sending request", "dns", "connect", "timed out", "timeout"].iter().any(|k| m.contains(k));
        Toast {
            dot: "warn",
            title: if offline { "You're offline · audio saved" } else { "Transcription failed · audio saved" }.into(),
            body: "We kept the recording. Retry once the connection is back.".into(),
            action: row_id.and_then(|id| act("Retry", "retry", Some(id.to_string()))),
            secondary: None,
        }
    }
}

/// Errors from whisper_cpp.rs / models::installed_path (lowercased).
fn is_local_model_error(m: &str) -> bool {
    ["local model still downloading", "isn't downloaded yet", "can't load the speech model", "no local speech engine", "whisper.cpp failed"]
        .iter()
        .any(|k| m.contains(k))
}

/// Local speech model missing, still downloading or broken. "Use cloud once" is offered only
/// when a cloud key exists (`cloud`), and never runs on its own; the audio stays in History.
fn local_model_toast(message: &str, row_id: Option<i64>, cloud: Option<&str>) -> Toast {
    let downloading = message.to_lowercase().contains("still downloading");
    Toast {
        dot: "warn",
        title: if downloading { message.lines().next().unwrap_or(message).to_string() } else { "Local speech model unavailable".into() },
        body: if row_id.is_some() { "Your recording is saved in History.".into() } else { String::new() },
        action: match (row_id, cloud) {
            (Some(id), Some(_)) => act("Use cloud once", "cloud_once", Some(id.to_string())),
            _ => act("Open settings", "settings", Some("models".into())),
        },
        secondary: if cloud.is_some() && row_id.is_some() { act("Later", "dismiss", None) } else { None },
    }
}

pub fn on_status(app: &AppHandle, s: &Status) {
    match s {
        Status::Done { outcome: Outcome::LeftOnClipboard(r), .. } => {
            if cfg!(target_os = "macos") {
                copied_mac(app, r)
            } else {
                admin_window(app)
            }
        }
        Status::Error { message, row_id } => show(app, classify_error(message, *row_id)),
        _ => {}
    }
}

/// The page reports the card height so the window (and its click area) hugs the card.
#[tauri::command]
pub fn toast_fit(app: AppHandle, height: f64) {
    if let Some(w) = app.get_webview_window("toast") {
        let _ = w.set_size(tauri::LogicalSize::new(W, (height + 32.0).clamp(60.0, H)));
        place(&app, &w);
    }
}

#[tauri::command]
pub fn toast_hide(app: AppHandle) {
    if let Some(w) = app.get_webview_window("toast") {
        let _ = w.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies() {
        assert_eq!(classify_error("gpt-transcribe 401 Unauthorized: x", Some(3)).title, "Your API key was rejected");
        assert_eq!(classify_error("Deepgram 403 Forbidden: x", None).title, "Deepgram rejected your key");
        assert_eq!(classify_error("No API key set for Google Gemini. Open Settings.", None).title, "No Google Gemini key");
        assert_eq!(classify_error("Mic error: nope", None).title, "Can't hear anything");
        let t = classify_error("error sending request for url", Some(7));
        assert!(t.title.starts_with("You're offline"));
        assert_eq!(t.action.map(|a| a.arg), Some(Some("7".into())));
        assert!(classify_error("boom", None).action.is_none());
    }

    #[test]
    fn local_model_offers_cloud_once_only_with_a_key() {
        let msg = "Local model still downloading · 42%";
        assert!(is_local_model_error(&msg.to_lowercase()));
        let t = local_model_toast(msg, Some(5), Some("openai"));
        assert_eq!(t.title, msg);
        let a = t.action.unwrap();
        assert_eq!((a.kind, a.arg), ("cloud_once", Some("5".into())));
        assert_eq!(local_model_toast(msg, Some(5), None).action.unwrap().kind, "settings");
        assert_eq!(local_model_toast("whisper.cpp failed: x", None, Some("openai")).action.unwrap().kind, "settings");
    }
}
