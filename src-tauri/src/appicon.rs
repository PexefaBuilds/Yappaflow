//! Recolours the app icon to the chosen accent: every window's title-bar icon, its taskbar
//! and Alt+Tab icon (Windows: a separate "big" icon, see `set_taskbar_icon`) and the tray icon. The files in icons/accent/ are raw
//! RGBA renders of the icon in each accent, made from design/logo/*.svg.
//! The taskbar button itself is drawn from the app's shortcut, not the window, so Windows also
//! points our shortcuts (Start menu, desktop, pinned) at an accent .ico in the app data folder.
//! macOS sets the Dock icon. Limit: the .exe's embedded icon and the .app bundle stay azure.

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
    let icon = ico(&[(32, small), (128, big)]);
    #[cfg(windows)]
    if let Ok(dir) = app.path().app_data_dir() {
        // Shell COM and file IO: off the caller's (often the main) thread.
        let accent = accent.to_string();
        let _ = std::thread::Builder::new().name("shortcut-icon".into()).spawn(move || {
            let path = dir.join("icons").join(format!("{accent}.ico"));
            match std::fs::create_dir_all(dir.join("icons")).and_then(|_| std::fs::write(&path, &icon)) {
                Ok(()) => point_shortcuts_at(&path),
                Err(e) => log::warn!("accent icon not saved: {e}"),
            }
        });
    }
    #[cfg(target_os = "macos")]
    set_dock_icon(app, icon);
}

/// An .ico holding 32-bit BMP images (BGRA, bottom-up, empty AND mask) for each (size, RGBA).
/// Windows shortcuts and macOS NSImage both read it, so one encoder serves both.
pub fn ico(images: &[(u32, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    let mut bodies = Vec::new();
    for &(size, rgba) in images {
        let mask_row = size.div_ceil(32) * 4;
        let mut b = Vec::with_capacity((40 + size * size * 4 + mask_row * size) as usize);
        for v in [40u32, size, size * 2] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&1u16.to_le_bytes()); // planes
        b.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        b.extend_from_slice(&[0; 24]); // compression, image size, resolution, palette: all 0
        for row in rgba.chunks_exact(size as usize * 4).rev() {
            b.extend(row.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], p[3]]));
        }
        b.resize(b.len() + (mask_row * size) as usize, 0);
        let dim = if size >= 256 { 0 } else { size as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += b.len() as u32;
        bodies.push(b);
    }
    bodies.into_iter().for_each(|b| out.extend(b));
    out
}

/// Points every shortcut to this exe (Start menu, desktop, pinned to the taskbar) at `ico`, then
/// tells the shell to refresh. Owner-approved rule 6 exception (26 Oct). A desktop folder
/// redirected away from %USERPROFILE%\Desktop is skipped.
#[cfg(windows)]
fn point_shortcuts_at(ico: &std::path::Path) {
    use windows::core::{Interface, HSTRING, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READWRITE,
    };
    use windows::Win32::UI::Shell::{
        IShellLinkW, SHChangeNotify, ShellLink, SHCNE_ASSOCCHANGED, SHCNE_UPDATEITEM, SHCNF_IDLIST, SHCNF_PATHW,
    };
    let Ok(exe) = std::env::current_exe() else { return };
    let env = |k: &str| std::env::var_os(k).map(std::path::PathBuf::from);
    let dirs = [
        env("APPDATA").map(|d| d.join(r"Microsoft\Windows\Start Menu\Programs")),
        env("USERPROFILE").map(|d| d.join("Desktop")),
        env("APPDATA").map(|d| d.join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar")),
    ];
    let links: Vec<_> = dirs
        .into_iter()
        .flatten()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")))
        .collect();
    let ico_w = HSTRING::from(ico.as_os_str());
    let mut changed = false;
    // SAFETY: COM on this thread only; every interface is dropped before the thread ends.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        for lnk in links {
            let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else { return };
            let Ok(file) = link.cast::<IPersistFile>() else { continue };
            let lnk_w = HSTRING::from(lnk.as_os_str());
            if file.Load(&lnk_w, STGM_READWRITE).is_err() {
                continue;
            }
            let mut target = [0u16; 1024];
            if link.GetPath(&mut target, std::ptr::null_mut(), 0).is_err() {
                continue;
            }
            let target = String::from_utf16_lossy(&target[..target.iter().position(|&c| c == 0).unwrap_or(0)]);
            if !target.eq_ignore_ascii_case(&exe.to_string_lossy()) {
                continue;
            }
            if link.SetIconLocation(&ico_w, 0).is_ok() && file.Save(PCWSTR::null(), true).is_ok() {
                SHChangeNotify(SHCNE_UPDATEITEM, SHCNF_PATHW, Some(lnk_w.as_ptr() as _), None);
                changed = true;
            } else {
                log::warn!("couldn't recolour shortcut {}", lnk.display());
            }
        }
        if changed {
            SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None); // refresh the shell's icon cache
        }
    }
}

/// macOS: the Dock icon (NSApplication.applicationIconImage), from the same .ico. AppKit needs
/// the main thread. Owner-approved rule 6 exception (26 Oct).
#[cfg(target_os = "macos")]
fn set_dock_icon(app: &AppHandle, ico: Vec<u8>) {
    let _ = app.run_on_main_thread(move || {
        use objc2::{AllocAnyThread, MainThreadMarker};
        use objc2_app_kit::{NSApplication, NSImage};
        use objc2_foundation::NSData;
        let Some(mtm) = MainThreadMarker::new() else { return };
        let data = NSData::with_bytes(&ico);
        match NSImage::initWithData(NSImage::alloc(), &data) {
            // SAFETY: on the main thread (mtm), with a valid image.
            Some(img) => unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&img)) },
            None => log::warn!("Dock icon: couldn't read the accent icon"),
        }
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ico_has_a_directory_entry_per_image_pointing_at_its_bitmap() {
        let (big, small) = (ICONS[1].1, ICONS[1].2); // coral
        let f = ico(&[(32, small), (128, big)]);
        let u16_at = |i: usize| u16::from_le_bytes([f[i], f[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes([f[i], f[i + 1], f[i + 2], f[i + 3]]);
        assert_eq!((u16_at(2), u16_at(4)), (1, 2), "icon type, two images");
        for (n, size) in [(0usize, 32u32), (1, 128)] {
            let e = 6 + 16 * n;
            assert_eq!(f[e] as u32, size);
            let (len, off) = (u32_at(e + 8) as usize, u32_at(e + 12) as usize);
            assert_eq!(len as u32, 40 + size * size * 4 + size.div_ceil(32) * 4 * size);
            assert_eq!((u32_at(off), u32_at(off + 4), u32_at(off + 8)), (40, size, size * 2), "BITMAPINFOHEADER");
            assert!(off + len <= f.len());
        }
        // The top-left source pixel lands in the last pixel row (bottom-up), as BGRA.
        let last_row = u32_at(6 + 12) as usize + 40 + 31 * 32 * 4;
        assert_eq!(&f[last_row..last_row + 4], &[small[2], small[1], small[0], small[3]]);
    }
}
