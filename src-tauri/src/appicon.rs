//! Recolours the app icon to the chosen accent: every window's title-bar icon, its taskbar
//! and Alt+Tab icon (Windows: a separate "big" icon, see `set_taskbar_icon`) and the tray icon. The files in icons/accent/ are raw
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
        #[cfg(windows)]
        set_taskbar_icon(&w, big, 128);
    }
    // macOS keeps its monochrome template icon in the menu bar (tray.rs).
    if let Some(t) = app.tray_by_id("main").filter(|_| !cfg!(target_os = "macos")) {
        let _ = t.set_icon(Some(Image::new(small, 32, 32)));
    }
}

/// Tauri's `set_icon` only sets ICON_SMALL (title bar); the taskbar and Alt+Tab use ICON_BIG,
/// which otherwise stays the exe's azure icon. Owner-approved rule 6 exception.
/// shortcut: the previous HICON isn't destroyed (a few KB per accent change), fine until accents change often.
#[cfg(windows)]
fn set_taskbar_icon(w: &tauri::WebviewWindow, rgba: &[u8], size: i32) {
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{CreateIcon, SendMessageW, ICON_BIG, WM_SETICON};
    let Ok(hwnd) = w.hwnd() else { return };
    // CreateIcon wants BGRA; an all-zero AND mask lets the alpha channel carry transparency.
    let bgra: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
    let mask = vec![0u8; (size * size / 8) as usize];
    // SAFETY: both buffers are size x size (32 bpp colour, 1 bpp mask) and outlive the call.
    let Ok(icon) = (unsafe { CreateIcon(HINSTANCE::default(), size, size, 1, 32, mask.as_ptr(), bgra.as_ptr()) }) else { return };
    // SAFETY: plain message to our own window; Windows copies nothing, the HICON stays alive.
    unsafe { SendMessageW(HWND(hwnd.0 as _), WM_SETICON, WPARAM(ICON_BIG as usize), LPARAM(icon.0 as isize)) };
}
