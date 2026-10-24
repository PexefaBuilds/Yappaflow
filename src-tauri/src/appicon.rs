//! Recolours the app icon to the chosen accent: every window icon (which is also the
//! taskbar button and Alt+Tab icon) and the tray icon. The files in icons/accent/ are raw
//! RGBA renders of the icon in each accent, made from design/logo/*.svg.
//! Limit: the .exe's own embedded icon (pinned shortcut, installer) is static azure.

use tauri::image::Image;
use tauri::{AppHandle, Manager};

macro_rules! accent {
    ($id:literal) => {
        (
            $id,
            include_bytes!(concat!("../icons/accent/", $id, "-128.rgba")) as &[u8],
            include_bytes!(concat!("../icons/accent/", $id, "-32.rgba")) as &[u8],
        )
    };
}

const ICONS: &[(&str, &[u8], &[u8])] =
    &[accent!("azure"), accent!("coral"), accent!("tangerine"), accent!("sunflower"), accent!("mint"), accent!("violet"), accent!("chalk")];

pub fn apply(app: &AppHandle, accent: &str) {
    let (_, big, small) = ICONS.iter().find(|i| i.0 == accent).unwrap_or(&ICONS[0]);
    for (_, w) in app.webview_windows() {
        let _ = w.set_icon(Image::new(big, 128, 128));
    }
    // macOS keeps its monochrome template icon in the menu bar (tray.rs).
    if let Some(t) = app.tray_by_id("main").filter(|_| !cfg!(target_os = "macos")) {
        let _ = t.set_icon(Some(Image::new(small, 32, 32)));
    }
}
