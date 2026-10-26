//! The pill overlay: a second, transparent, always-on-top window that never takes focus,
//! so the cursor stays exactly where the text will land. Shown and hidden from `emit_status`.

use crate::hotkey::ChordEvent;
use crate::pipeline::{CtlMsg, Status};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc::Sender, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// Logical window size; the pill sits at the bottom with room above/below for its shadow.
const W: f64 = 300.0;
const H: f64 = 64.0;
const SHADOW_PAD: f64 = 14.0;
/// design/tokens.css --pill-offset; macOS sits 12 px above visibleFrame (spec 8 §8).
const OFFSET: f64 = if cfg!(target_os = "macos") { 12.0 } else { 24.0 };

/// Lets the pill's stop button reach the controller.
pub struct CtlTx(pub Mutex<Sender<CtlMsg>>);

/// Bumped on every status; a pending auto-hide only fires if nothing newer arrived.
static GEN: AtomicU64 = AtomicU64::new(0);

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let w = WebviewWindowBuilder::new(app, "pill", WebviewUrl::App("pill.html".into()))
        .additional_browser_args(crate::WEBVIEW_ARGS)
        .title("Yappaflow pill")
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
    mac_panel(&w);
    w.set_ignore_cursor_events(true)
}

#[cfg(target_os = "macos")]
tauri_nspanel::tauri_panel! {
    panel!(PillPanel {
        config: {
            can_become_key_window: false,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })
}

/// macOS: turn the pill into a non-activating NSPanel (clicking stop never takes focus from the
/// target app), on every Space and over other apps' full-screen windows.
#[cfg(target_os = "macos")]
fn mac_panel(w: &WebviewWindow) {
    use objc2_app_kit::{NSWindowCollectionBehavior as B, NSWindowStyleMask};
    use tauri_nspanel::WebviewWindowExt;
    match w.to_panel::<PillPanel<tauri::Wry>>() {
        Ok(p) => {
            if let Err(e) = p.set_style_mask(NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel) {
                log::warn!("pill panel style: {e}");
            }
            p.set_collection_behavior(B::CanJoinAllSpaces | B::FullScreenAuxiliary | B::Stationary | B::IgnoresCycle);
            p.set_level(25); // NSStatusWindowLevel: above full-screen apps' windows
        }
        Err(e) => {
            log::warn!("pill stays a normal window ({e}); it may take focus when clicked");
            let _ = w.set_visible_on_all_workspaces(true);
        }
    }
}

/// Command mode shows the spoken instruction in a bubble above the pill (CommandMode.dc.html).
const H_TALL: f64 = 150.0;
static TALL: AtomicBool = AtomicBool::new(false);
/// "Adding to dictionary" needs a wider window than the usual pill.
const W_WIDE: f64 = 440.0;
static WIDE: AtomicBool = AtomicBool::new(false);

fn width() -> f64 {
    if WIDE.load(Ordering::SeqCst) { W_WIDE } else { W }
}

/// Grow or shrink the (transparent) window so the bubble or the wide pill fits; the pill stays put.
fn fit(app: &AppHandle, w: &WebviewWindow, tall: bool, wide: bool) {
    let same_tall = TALL.swap(tall, Ordering::SeqCst) == tall;
    if WIDE.swap(wide, Ordering::SeqCst) == wide && same_tall {
        return;
    }
    let h = if tall { H_TALL } else { H };
    let _ = w.set_size(tauri::LogicalSize::new(width(), h));
    if w.is_visible().unwrap_or(false) {
        place_sized(app, w, Some(h));
    }
}

/// Bottom-centre of the work area of the monitor under the mouse, 24 px above the taskbar.
fn place(app: &AppHandle, w: &WebviewWindow) {
    place_sized(app, w, None);
}

/// `logical_h`: a size just requested with set_size (outer_size may not report it yet).
fn place_sized(app: &AppHandle, w: &WebviewWindow, logical_h: Option<f64>) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| w.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return };
    let (wa, scale) = (m.work_area(), m.scale_factor());
    // Use the real window size (it can differ from W×H on scaled displays).
    let size = match logical_h {
        Some(h) => tauri::PhysicalSize::new((width() * scale) as u32, (h * scale) as u32),
        None => w.outer_size().unwrap_or(tauri::PhysicalSize::new((width() * scale) as u32, (H * scale) as u32)),
    };
    let (ww, hh) = (size.width as i32, size.height as i32);
    let x = wa.position.x + (wa.size.width as i32 - ww) / 2;
    let bottom = wa.position.y + wa.size.height as i32;
    let y = (bottom - hh + ((SHADOW_PAD - OFFSET) * scale) as i32).min(bottom - hh);
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

fn hide_after(app: &AppHandle, gen: u64, d: Duration) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(d).await;
        if GEN.load(Ordering::SeqCst) == gen {
            if let Some(w) = app.get_webview_window("pill") {
                let _ = w.hide();
            }
        }
    });
}

pub fn on_status(app: &AppHandle, s: &Status) {
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let Some(w) = app.get_webview_window("pill") else { return };
    fit(app, &w, matches!(s, Status::Rewriting { .. }), false);
    let show = |clickable: bool| {
        if !w.is_visible().unwrap_or(false) {
            place(app, &w);
            let _ = w.show();
        }
        // Only the hands-free stop button takes clicks; everything else passes through.
        let _ = w.set_ignore_cursor_events(!clickable);
    };
    match s {
        Status::Listening { hands_free, .. } => show(*hands_free),
        Status::Processing | Status::Rewriting { .. } => show(false),
        Status::Polishing => show(true), // "Paste now" button
        Status::Done { cleanup_failed: true, .. } | Status::Done { cleanup_skipped: true, .. } => {
            show(false);
            hide_after(app, gen, Duration::from_millis(3000));
        }
        // The resting handle: click it to resume. Hides itself when the pause runs out.
        Status::Paused { until_ms } => {
            show(true);
            let left = (*until_ms - crate::storage::now_ms()).max(0) as u64;
            hide_after(app, gen, Duration::from_millis(left));
        }
        Status::Done { .. } => {
            show(false);
            hide_after(app, gen, Duration::from_millis(1200));
        }
        Status::Empty => {
            show(false);
            hide_after(app, gen, Duration::from_millis(1400));
        }
        Status::Error { .. } => {
            show(false);
            hide_after(app, gen, Duration::from_millis(5000));
        }
        Status::Idle | Status::Cancelled => {
            let _ = w.hide();
        }
    }
}

/// "Adding <term> to dictionary" with an Undo button for `ms`; the pill counts down, learn.rs commits.
pub fn show_learn(app: &AppHandle, term: &str, ms: u64) {
    use tauri::Emitter;
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let Some(w) = app.get_webview_window("pill") else { return };
    fit(app, &w, false, true);
    if !w.is_visible().unwrap_or(false) {
        place(app, &w);
        let _ = w.show();
    }
    let _ = w.set_ignore_cursor_events(false);
    let _ = app.emit_to("pill", "learn", serde_json::json!({ "term": term, "ms": ms }));
    hide_after(app, gen, Duration::from_millis(ms + 900));
}

/// Hide the learn pill now (Undo), unless a dictation has taken the pill over since.
pub fn hide_learn(app: &AppHandle) {
    if WIDE.load(Ordering::SeqCst) {
        if let Some(w) = app.get_webview_window("pill") {
            let _ = w.hide();
        }
    }
}

/// "Paste now" while Polishing: insert the raw transcript without waiting for cleanup.
#[tauri::command]
pub fn pill_skip_cleanup() {
    crate::pipeline::SKIP_CLEANUP.notify_waiters();
}

/// The hands-free stop button: same as tapping the hotkey again.
#[tauri::command]
pub fn pill_stop(tx: tauri::State<'_, CtlTx>) {
    if let Ok(tx) = tx.0.lock() {
        let _ = tx.send(CtlMsg::Chord(ChordEvent::Down));
        let _ = tx.send(CtlMsg::Chord(ChordEvent::Up));
    }
}
