//! Tray: left click opens a custom popup window (tray.html); right click keeps a minimal native menu.
//! The popup takes focus (so Esc and Tab work) and hides on blur; `context::restore_focus` hands
//! focus back to the previously used window before Paste again / hands-free.

use crate::hotkey::ChordEvent;
use crate::insert::{self, InsertOptions, Outcome};
use crate::pill::CtlTx;
use crate::pipeline::CtlMsg;
use crate::{context, toast, AppState, APP_NAME};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const W: f64 = 412.0; // 380 menu + 16 shadow padding each side
const H: f64 = 600.0;

/// Epoch ms until which hotkeys are ignored (0 = not paused).
static PAUSED_UNTIL: AtomicI64 = AtomicI64::new(0);
/// When the popup last hid; a tray click right after a blur-hide must not reopen it.
static LAST_HIDE: AtomicI64 = AtomicI64::new(0);

fn now_ms() -> i64 {
    crate::storage::now_ms()
}

pub fn paused() -> bool {
    PAUSED_UNTIL.load(Ordering::Relaxed) > now_ms()
}

fn hide(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("tray") {
        if w.is_visible().unwrap_or(false) {
            LAST_HIDE.store(now_ms(), Ordering::Relaxed);
            let _ = w.hide();
        }
    }
}

/// Last tray click (physical px); the popup is re-placed from it whenever its size changes.
static CLICK: std::sync::Mutex<(f64, f64)> = std::sync::Mutex::new((0.0, 0.0));

/// Horizontally centred on the click, against the work-area edge nearest to it. Returns "top"/"bottom".
fn reposition(app: &AppHandle, w: &tauri::WebviewWindow) -> &'static str {
    let click = CLICK.lock().map(|c| *c).unwrap_or_default();
    let mut anchor = "bottom";
    if let Some(m) = app.monitor_from_point(click.0, click.1).ok().flatten() {
        let (wa, scale) = (m.work_area(), m.scale_factor());
        let size = w.outer_size().unwrap_or(tauri::PhysicalSize::new((W * scale) as u32, (H * scale) as u32));
        let (ww, hh) = (size.width as i32, size.height as i32);
        let min_x = wa.position.x;
        let max_x = (wa.position.x + wa.size.width as i32 - ww).max(min_x);
        let x = (click.0 as i32 - ww / 2).clamp(min_x, max_x);
        let y = if (click.1 as i32) < wa.position.y + wa.size.height as i32 / 2 {
            anchor = "top";
            wa.position.y
        } else {
            (wa.position.y + wa.size.height as i32 - hh).max(wa.position.y)
        };
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
    anchor
}

fn open(app: &AppHandle, click: PhysicalPosition<f64>) {
    let Some(w) = app.get_webview_window("tray") else { return };
    if w.is_visible().unwrap_or(false) {
        hide(app);
        return;
    }
    if now_ms() - LAST_HIDE.load(Ordering::Relaxed) < 250 {
        return;
    }
    if let Ok(mut c) = CLICK.lock() {
        *c = (click.x, click.y);
    }
    let anchor = reposition(app, &w);
    let _ = w.show();
    let _ = w.set_focus();
    let _ = app.emit_to("tray", "tray-open", anchor);
}

/// The page reports its content height so the window (and its click area) matches the menu.
#[tauri::command]
pub fn tray_fit(app: AppHandle, height: f64) {
    if let Some(w) = app.get_webview_window("tray") {
        let _ = w.set_size(tauri::LogicalSize::new(W, (height + 32.0).clamp(80.0, H)));
        reposition(&app, &w);
    }
}

pub fn build(app: &tauri::App) -> tauri::Result<()> {
    let h = app.handle();
    context::start_focus_tracker();

    let win = WebviewWindowBuilder::new(h, "tray", WebviewUrl::App("tray.html".into()))
        .additional_browser_args(crate::WEBVIEW_ARGS)
        .title("Yappaflow menu")
        .inner_size(W, H)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()?;
    let hh = h.clone();
    win.on_window_event(move |ev| {
        if let WindowEvent::Focused(false) = ev {
            hide(&hh);
        }
    });

    // Fallback right-click menu.
    let open_item = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", format!("Quit {APP_NAME}"), true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open_item, &sep, &quit])?;
    #[cfg(not(target_os = "macos"))]
    let Some(icon) = app.default_window_icon().cloned() else {
        return Err(tauri::Error::AssetNotFound("app icon".into()));
    };
    // macOS menu bar: a monochrome template image, so it follows the light/dark menu bar.
    #[cfg(target_os = "macos")]
    let icon = tauri::include_image!("icons/tray-dark-32.png");
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip(format!(
            "{APP_NAME}: hold {} to dictate",
            crate::hotkey_cmds::dictate_display(&app.state::<std::sync::Arc<crate::AppState>>().settings.read().unwrap())
        ))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| match ev.id.as_ref() {
            "settings" => crate::show_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, position, .. } = ev {
                open(tray.app_handle(), position);
            }
        })
        .build(app)?;
    Ok(())
}

#[derive(serde::Serialize)]
pub struct TrayState {
    paused_until: i64,
}

#[tauri::command]
pub fn tray_state() -> TrayState {
    TrayState { paused_until: PAUSED_UNTIL.load(Ordering::Relaxed).max(0) }
}

#[tauri::command]
pub fn tray_hide(app: AppHandle) {
    hide(&app);
}

#[tauri::command]
pub fn tray_pause(app: AppHandle, on: bool) -> i64 {
    let until = if on { now_ms() + 3_600_000 } else { 0 };
    PAUSED_UNTIL.store(until, Ordering::Relaxed);
    use crate::pipeline::{emit_status, Status};
    emit_status(&app, if on { Status::Paused { until_ms: until } } else { Status::Idle });
    until
}

#[tauri::command]
pub fn open_settings(app: AppHandle, section: Option<String>) {
    hide(&app);
    crate::show_settings(&app);
    // The settings UI may listen for this to jump to a page.
    let _ = app.emit_to("main", "open-settings", section);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

/// Give focus back to the window the user was in, then insert the text there.
#[tauri::command]
pub async fn tray_paste_again(app: AppHandle, state: State<'_, Arc<AppState>>, text: String) -> Result<(), String> {
    hide(&app);
    let s = state.settings.read().map_err(|e| e.to_string())?.clone();
    let opts = InsertOptions {
        mode: s.insert_mode,
        restore_clipboard: s.restore_clipboard,
        restore_delay: Duration::from_millis(s.restore_delay_ms),
    };
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        context::restore_focus();
        std::thread::sleep(Duration::from_millis(150));
        insert::insert_text(&text, &opts)
    })
    .await
    .map_err(|e| e.to_string())?;
    if matches!(outcome, Outcome::LeftOnClipboard(_)) {
        toast::admin_window(&app);
    }
    Ok(())
}

/// Start hands-free (a double tap of the chord) or stop it (one tap), as the pill's stop button does.
#[tauri::command]
pub async fn tray_hands_free(app: AppHandle, tx: State<'_, CtlTx>, on: bool) -> Result<(), String> {
    hide(&app);
    let tx = tx.0.lock().map_err(|e| e.to_string())?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if on {
            // Record in the user's window, not in this popup.
            context::restore_focus();
            std::thread::sleep(Duration::from_millis(150));
            // Works whichever way hands-free is set up (double-tap or its own hotkey).
            let _ = tx.send(CtlMsg::HandsFree);
        } else {
            let _ = tx.send(CtlMsg::Chord(ChordEvent::Down));
            let _ = tx.send(CtlMsg::Chord(ChordEvent::Up));
        }
    })
    .await
    .map_err(|e| e.to_string())
}
