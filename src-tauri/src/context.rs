//! Which app is the user dictating into? Foreground window → exe name + title.

use serde::Serialize;

/// App-style rules (Settings › Apps) match the exe on Windows and the bundle ID on macOS.
/// The other OS's kind stays in the table but is hidden and never matched.
#[cfg(not(target_os = "macos"))]
pub const RULE_KIND: (&str, &str) = ("exe", "bundle_id");
#[cfg(target_os = "macos")]
pub const RULE_KIND: (&str, &str) = ("bundle_id", "exe");

#[derive(Debug, Clone, Default, Serialize)]
pub struct AppContext {
    /// Lower-case file name, e.g. "slack.exe". macOS: the app's lower-case name, e.g. "slack".
    pub exe: String,
    pub title: String,
    /// macOS bundle identifier, e.g. "com.tinyspeck.slackmacgap". Empty on Windows.
    pub bundle_id: String,
}

#[cfg(windows)]
pub fn foreground() -> AppContext {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return AppContext::default();
        }
        let mut title_buf = [0u16; 512];
        let n = GetWindowTextW(hwnd, &mut title_buf);
        let title = String::from_utf16_lossy(&title_buf[..n.max(0) as usize]);

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let mut exe = String::new();
        if pid != 0 {
            if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut buf = [0u16; 1024];
                let mut len = buf.len() as u32;
                if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
                    let full = String::from_utf16_lossy(&buf[..len as usize]);
                    exe = full.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
                }
                let _ = CloseHandle(h);
            }
            // Elevated processes may refuse even limited access → exe stays "".
        }
        AppContext { exe, title, bundle_id: String::new() }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn foreground() -> AppContext {
    AppContext::default()
}

#[cfg(target_os = "macos")]
pub use mac::{focused_has_selection, focused_is_password, focused_text, foreground, restore_focus, start_focus_tracker};

// ── Previously focused window (for the tray popup's Paste again / hands-free) ──

#[cfg(windows)]
static LAST_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Polls the foreground window every 250 ms and remembers the last one that is not ours or the
/// taskbar. Needed because clicking the tray icon makes the taskbar the foreground window.
#[cfg(windows)]
pub fn start_focus_tracker() {
    use std::sync::atomic::Ordering;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId};
    const SHELL: [&str; 4] = ["Shell_TrayWnd", "Shell_SecondaryTrayWnd", "NotifyIconOverflowWindow", "TopLevelWindowForOverflowXamlIsland"];
    let _ = std::thread::Builder::new().name("focus-tracker".into()).spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_millis(250));
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                continue;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let mut buf = [0u16; 64];
            let n = GetClassNameW(hwnd, &mut buf);
            let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            if pid != GetCurrentProcessId() && !SHELL.contains(&class.as_str()) {
                LAST_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
            }
        }
    });
}

/// Bring the remembered window back to the foreground. Returns false if there is none.
#[cfg(windows)]
pub fn restore_focus() -> bool {
    use std::sync::atomic::Ordering;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{IsWindow, SetForegroundWindow};
    let h = LAST_HWND.load(Ordering::Relaxed);
    if h == 0 {
        return false;
    }
    let hwnd = HWND(h as *mut std::ffi::c_void);
    unsafe { IsWindow(hwnd).as_bool() && SetForegroundWindow(hwnd).as_bool() }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn start_focus_tracker() {}
#[cfg(not(any(windows, target_os = "macos")))]
pub fn restore_focus() -> bool {
    false
}

/// Text of the focused edit control via UI Automation (ValuePattern, else TextPattern document range,
/// capped at 20 000 chars). None for password fields, unreadable controls, or no UIA.
#[cfg(windows)]
fn focused_element() -> Option<windows::Win32::UI::Accessibility::IUIAutomationElement> {
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
    thread_local! {
        static UIA: Option<IUIAutomation> = unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED); // "already initialised" is fine
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
        };
    }
    UIA.with(|uia| unsafe { uia.as_ref()?.GetFocusedElement().ok() })
}

/// The focused control is a password box (UIA IsPassword). Command mode refuses to run there.
#[cfg(windows)]
pub fn focused_is_password() -> bool {
    focused_element().and_then(|el| unsafe { el.CurrentIsPassword().ok() }).is_some_and(|b| b.as_bool())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn focused_is_password() -> bool {
    false
}

/// Does the focused control have a non-empty text selection (UIA TextPattern)? None = it can't
/// say (no TextPattern), so the caller falls back to Ctrl+C and the clipboard.
#[cfg(windows)]
pub fn focused_has_selection() -> Option<bool> {
    use windows::Win32::UI::Accessibility::{IUIAutomationTextPattern, UIA_TextPatternId};
    let el = focused_element()?;
    unsafe {
        let tp = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId).ok()?;
        let ranges = tp.GetSelection().ok()?;
        for i in 0..ranges.Length().ok()? {
            if !ranges.GetElement(i).ok()?.GetText(1).ok()?.is_empty() {
                return Some(true);
            }
        }
        Some(false)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn focused_has_selection() -> Option<bool> {
    None
}

#[cfg(windows)]
pub fn focused_text() -> Option<String> {
    use windows::Win32::UI::Accessibility::{IUIAutomationTextPattern, IUIAutomationValuePattern, UIA_TextPatternId, UIA_ValuePatternId};
    const MAX: usize = 20_000;
    let el = focused_element()?;
    unsafe {
        if el.CurrentIsPassword().ok()?.as_bool() {
            return None;
        }
        if let Ok(vp) = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
            if let Ok(v) = vp.CurrentValue() {
                if !v.is_empty() {
                    return Some(v.to_string().chars().take(MAX).collect());
                }
            }
        }
        let tp = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId).ok()?;
        Some(tp.DocumentRange().ok()?.GetText(MAX as i32).ok()?.to_string())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn focused_text() -> Option<String> {
    None
}

// ── macOS: NSWorkspace for the app, the Accessibility (AX) API for window title and focused field ──

#[cfg(target_os = "macos")]
pub(crate) mod mac {
    use super::AppContext;
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
    use objc2_foundation::NSString;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicI32, Ordering};

    type Ref = *const c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> Ref;
        fn AXUIElementCreateApplication(pid: i32) -> Ref;
        fn AXUIElementCopyAttributeValue(el: Ref, attr: Ref, out: *mut Ref) -> i32;
        pub(crate) fn AXIsProcessTrusted() -> bool;
        pub(crate) fn AXIsProcessTrustedWithOptions(options: Ref) -> bool;
        pub(crate) static kAXTrustedCheckOptionPrompt: Ref;
    }
    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        /// Some app (password field, Terminal "Secure Keyboard Entry") asked macOS to hide keystrokes.
        pub(crate) fn IsSecureEventInputEnabled() -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(r: Ref);
        fn CFGetTypeID(r: Ref) -> usize;
        fn CFStringGetTypeID() -> usize;
    }

    /// An owned CF object, released on drop.
    struct Cf(Ref);
    impl Drop for Cf {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) }
            }
        }
    }

    fn attr(el: &Cf, name: &str) -> Option<Cf> {
        if el.0.is_null() {
            return None;
        }
        let key = NSString::from_str(name); // toll-free bridged to CFString
        let mut out: Ref = std::ptr::null();
        let err = unsafe { AXUIElementCopyAttributeValue(el.0, (&*key as *const NSString).cast(), &mut out) };
        (err == 0 && !out.is_null()).then(|| Cf(out))
    }

    fn string(v: &Cf) -> Option<String> {
        if unsafe { CFGetTypeID(v.0) != CFStringGetTypeID() } {
            return None;
        }
        Some(unsafe { &*(v.0 as *const NSString) }.to_string())
    }

    fn str_attr(el: &Cf, name: &str) -> Option<String> {
        attr(el, name).and_then(|v| string(&v))
    }

    fn focused() -> Option<Cf> {
        let sys = Cf(unsafe { AXUIElementCreateSystemWide() });
        attr(&sys, "AXFocusedUIElement")
    }

    // Every entry point runs on a background thread, so each drains its own autorelease pool.

    pub fn foreground() -> AppContext {
        autoreleasepool(|_| {
            let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() else { return AppContext::default() };
            let bundle_id = app.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default();
            let exe = app.localizedName().map(|s| s.to_string().to_lowercase()).unwrap_or_default();
            // Window title via AX (we hold Accessibility); never via screen-recording APIs.
            let ax = Cf(unsafe { AXUIElementCreateApplication(app.processIdentifier()) });
            let title = attr(&ax, "AXFocusedWindow").and_then(|w| str_attr(&w, "AXTitle")).unwrap_or_default();
            AppContext { exe, title, bundle_id }
        })
    }

    pub fn focused_is_password() -> bool {
        let secure = unsafe { IsSecureEventInputEnabled() };
        secure || autoreleasepool(|_| focused().is_some_and(|el| str_attr(&el, "AXSubrole").as_deref() == Some("AXSecureTextField")))
    }

    /// None = the control doesn't expose AXSelectedText; the caller falls back to ⌘C.
    pub fn focused_has_selection() -> Option<bool> {
        autoreleasepool(|_| str_attr(&focused()?, "AXSelectedText").map(|t| !t.is_empty()))
    }

    pub fn focused_text() -> Option<String> {
        const MAX: usize = 20_000;
        if focused_is_password() {
            return None;
        }
        autoreleasepool(|_| str_attr(&focused()?, "AXValue").map(|v| v.chars().take(MAX).collect()))
    }

    static LAST_PID: AtomicI32 = AtomicI32::new(0);

    /// Remembers the last frontmost app that isn't us, for the menu-bar popup's Paste again / hands-free.
    pub fn start_focus_tracker() {
        let me = std::process::id() as i32;
        let _ = std::thread::Builder::new().name("focus-tracker".into()).spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(250));
            autoreleasepool(|_| {
                if let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() {
                    let pid = app.processIdentifier();
                    if pid != me && pid > 0 {
                        LAST_PID.store(pid, Ordering::Relaxed);
                    }
                }
            });
        });
    }

    #[allow(deprecated)] // ActivateIgnoringOtherApps: still needed to take focus back from our popup.
    pub fn restore_focus() -> bool {
        let pid = LAST_PID.load(Ordering::Relaxed);
        if pid == 0 {
            return false;
        }
        let ok = autoreleasepool(|_| {
            NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
                .is_some_and(|app| app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps))
        });
        if !ok {
            return false;
        }
        // Activation is asynchronous; give the window a moment before keystrokes go to it.
        std::thread::sleep(std::time::Duration::from_millis(120));
        ok
    }
}

/// macOS privacy permissions (Microphone, Input Monitoring, Accessibility). Always granted elsewhere.
pub mod perms {
    use serde::Serialize;

    #[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
    pub struct Perms {
        pub microphone: bool,
        pub input_monitoring: bool,
        pub accessibility: bool,
        /// macOS: an MDM profile restricts the microphone; the user can't allow it.
        pub microphone_restricted: bool,
    }

    /// System Settings deep link for a permission id.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn pane_url(kind: &str) -> Option<&'static str> {
        Some(match kind {
            "microphone" => "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
            "input_monitoring" => "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent",
            "accessibility" => "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            "keyboard" => "x-apple.systempreferences:com.apple.Keyboard-Settings.extension",
            _ => return None,
        })
    }

    #[cfg(not(target_os = "macos"))]
    pub fn status() -> Perms {
        Perms { microphone: true, input_monitoring: true, accessibility: true, microphone_restricted: false }
    }
    #[cfg(not(target_os = "macos"))]
    pub fn request(_kind: &str) {}
    #[cfg(not(target_os = "macos"))]
    pub fn open_pane(_kind: &str) -> Result<(), String> {
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    pub fn fn_key_conflicts() -> bool {
        false
    }

    #[cfg(target_os = "macos")]
    pub use mac_impl::*;

    #[cfg(target_os = "macos")]
    mod mac_impl {
        use super::{pane_url, Perms};
        use crate::context::mac::{kAXTrustedCheckOptionPrompt, AXIsProcessTrusted, AXIsProcessTrustedWithOptions};
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, Bool};
        use objc2_foundation::{NSDictionary, NSNumber, NSString};

        #[link(name = "AVFoundation", kind = "framework")]
        extern "C" {
            static AVMediaTypeAudio: &'static NSString;
        }

        /// AVAuthorizationStatus: 0 not determined, 1 restricted, 2 denied, 3 authorized.
        fn mic_status() -> isize {
            let Some(cls) = AnyClass::get(c"AVCaptureDevice") else { return 0 };
            unsafe { msg_send![cls, authorizationStatusForMediaType: AVMediaTypeAudio] }
        }

        pub fn status() -> Perms {
            let mic = mic_status();
            Perms {
                microphone: mic == 3,
                input_monitoring: crate::hotkey::macos::input_monitoring_granted(),
                accessibility: unsafe { AXIsProcessTrusted() },
                microphone_restricted: mic == 1,
            }
        }

        /// Shows the system prompt where macOS has one (first time only); after that, Open settings.
        pub fn request(kind: &str) {
            objc2::rc::autoreleasepool(|_| request_in_pool(kind))
        }

        fn request_in_pool(kind: &str) {
            match kind {
                "microphone" if mic_status() == 0 => {
                    let Some(cls) = AnyClass::get(c"AVCaptureDevice") else { return };
                    let done = block2::RcBlock::new(|_granted: Bool| {});
                    let _: () = unsafe { msg_send![cls, requestAccessForMediaType: AVMediaTypeAudio, completionHandler: &*done] };
                }
                "input_monitoring" => {
                    crate::hotkey::macos::request_input_monitoring();
                }
                "accessibility" => unsafe {
                    let key: &NSString = &*(kAXTrustedCheckOptionPrompt as *const NSString);
                    let yes = NSNumber::new_bool(true);
                    let opts = NSDictionary::<NSString, NSNumber>::from_slices(&[key], &[&*yes]);
                    AXIsProcessTrustedWithOptions((&*opts as *const NSDictionary<NSString, NSNumber>).cast());
                },
                _ => {}
            }
        }

        pub fn open_pane(kind: &str) -> Result<(), String> {
            let url = pane_url(kind).ok_or_else(|| format!("Unknown settings page '{kind}'"))?;
            std::process::Command::new("/usr/bin/open").arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
        }

        pub fn fn_key_conflicts() -> bool {
            crate::hotkey::macos::fn_key_conflicts()
        }
    }
}
