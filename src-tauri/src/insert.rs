//! Put text at the cursor of whatever app has focus.
//! Primary: clipboard + Ctrl+V, then restore the user's clipboard.
//! Fallback: Unicode keystrokes. Last resort: leave text on the clipboard.

use crate::settings::InsertMode;
use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum Outcome {
    Pasted,
    Typed,
    /// Input was blocked (usually an elevated window). Text is on the clipboard.
    LeftOnClipboard(String),
}

/// Why text was left on the clipboard, in the words of each OS (shown in History and toasts).
#[cfg(not(target_os = "macos"))]
const BLOCKED_INPUT: &str = "input was blocked (elevated window?)";
#[cfg(not(target_os = "macos"))]
const BLOCKED_PASTE: &str = "paste keystroke was blocked (elevated window?)";
#[cfg(target_os = "macos")]
const BLOCKED_INPUT: &str = "macOS didn't accept our keystrokes";
#[cfg(target_os = "macos")]
const BLOCKED_PASTE: &str = "macOS didn't accept the ⌘V keystroke";

pub struct InsertOptions {
    pub mode: InsertMode,
    pub restore_clipboard: bool,
    pub restore_delay: Duration,
}

/// Blocking: call from `spawn_blocking`.
pub fn insert_text(text: &str, opts: &InsertOptions) -> Outcome {
    win::wait_modifiers_released(Duration::from_millis(500));
    // Secure input (password field, Terminal "Secure Keyboard Entry") or no Accessibility: post no keys.
    #[cfg(target_os = "macos")]
    if let Some(why) = win::blocked() {
        return leave_on_clipboard(text, why);
    }
    match opts.mode {
        InsertMode::Type => {
            if win::type_unicode(text) {
                return Outcome::Typed;
            }
            leave_on_clipboard(text, "keystrokes were blocked")
        }
        InsertMode::Paste => match paste(text, opts) {
            Ok(o) => o,
            Err(e) => {
                log::warn!("paste failed ({e}); falling back to typing");
                if win::type_unicode(text) {
                    Outcome::Typed
                } else {
                    leave_on_clipboard(text, BLOCKED_INPUT)
                }
            }
        },
    }
}

fn set_private(cb: &mut arboard::Clipboard, text: &str) -> Result<(), arboard::Error> {
    #[cfg(windows)]
    {
        use arboard::SetExtWindows;
        // Keep dictations out of Win+V history and cloud clipboard sync.
        cb.set().exclude_from_history().exclude_from_cloud().text(text)
    }
    #[cfg(target_os = "macos")]
    {
        // Transient + Concealed markers keep it out of clipboard managers' history.
        let _ = cb;
        if clip::set_private_text(text) {
            Ok(())
        } else {
            Err(arboard::Error::Unknown { description: "pasteboard write failed".into() })
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        cb.set_text(text)
    }
}

/// Terminals treat Ctrl+C without a selection as "interrupt the running program", so command
/// mode never sends it there.
pub fn is_terminal(exe: &str) -> bool {
    const TERMINALS: &[&str] =
        &["windowsterminal", "wt", "conhost", "cmd", "powershell", "pwsh", "wezterm", "wezterm-gui", "alacritty", "mintty"];
    let name = exe.trim().to_ascii_lowercase();
    TERMINALS.contains(&name.strip_suffix(".exe").unwrap_or(&name))
}

/// Puts the user's clipboard back on every exit path of `grab_selection`.
struct RestoreOnDrop(Option<clip::Snapshot>);
impl Drop for RestoreOnDrop {
    fn drop(&mut self) {
        if let Some(s) = self.0.take() {
            if s.is_empty() {
                // It was empty before: don't leave the copied selection behind.
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.clear();
                }
            } else {
                s.restore();
            }
        }
    }
}

/// Command mode: copy the focused app's selection without disturbing the user's clipboard.
/// Snapshots the clipboard, sends Ctrl+C, waits up to 150 ms for the clipboard sequence number to
/// change, reads the text, then restores the snapshot. None = nothing selected, a terminal, the
/// app ignored Ctrl+C, or the clipboard couldn't be snapshotted (then Ctrl+C is never sent).
/// `exe`: the target app (context::foreground). Blocking: call from `spawn_blocking`.
pub fn grab_selection(exe: &str) -> Option<String> {
    if is_terminal(exe) {
        return None;
    }
    // Editors copy the whole line on Ctrl+C with nothing selected; UIA tells us first when it can.
    if crate::context::focused_has_selection() == Some(false) {
        return None;
    }
    win::wait_modifiers_released(Duration::from_millis(500));
    let _guard = RestoreOnDrop(Some(clip::Snapshot::capture()?));
    let before = win::clipboard_seq();
    if !win::send_ctrl_c() {
        return None;
    }
    let t0 = Instant::now();
    while win::clipboard_seq() == before && t0.elapsed() < Duration::from_millis(150) {
        std::thread::sleep(Duration::from_millis(5));
    }
    if win::clipboard_seq() == before {
        // A slow app may still write the copy after we give up; wait so the restore lands last.
        std::thread::sleep(Duration::from_millis(150));
        return None;
    }
    let mut cb = arboard::Clipboard::new().ok()?;
    // The app may still be writing; one short retry covers a delayed render.
    let text = cb.get_text().ok().or_else(|| {
        std::thread::sleep(Duration::from_millis(20));
        cb.get_text().ok()
    });
    text.filter(|t| !t.is_empty())
}

fn paste(text: &str, opts: &InsertOptions) -> anyhow::Result<Outcome> {
    // Can't snapshot (clipboard busy or huge)? Err makes the caller type instead, so nothing is lost.
    let snapshot = if opts.restore_clipboard {
        Some(clip::Snapshot::capture().ok_or_else(|| anyhow::anyhow!("couldn't snapshot the clipboard"))?)
    } else {
        None
    };
    let mut cb = arboard::Clipboard::new()?;
    set_private(&mut cb, text)?;
    #[cfg(target_os = "macos")]
    let ours = win::clipboard_seq();
    if !win::send_ctrl_v() {
        // Text is already on the clipboard; the user can paste manually.
        return Ok(Outcome::LeftOnClipboard(BLOCKED_PASTE.into()));
    }

    if let Some(snap) = snapshot {
        // Apps read the clipboard asynchronously after Ctrl+V; give them time.
        std::thread::sleep(opts.restore_delay);
        // Only restore if nobody else changed the clipboard meanwhile.
        #[cfg(not(target_os = "macos"))]
        let still_ours = cb.get_text().ok().as_deref() == Some(text);
        #[cfg(target_os = "macos")]
        let still_ours = win::clipboard_seq() == ours;
        if still_ours {
            snap.restore();
        }
    }
    Ok(Outcome::Pasted)
}

fn leave_on_clipboard(text: &str, why: &str) -> Outcome {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = set_private(&mut cb, text);
    }
    Outcome::LeftOnClipboard(why.to_string())
}

pub fn copy_to_clipboard(text: &str) -> anyhow::Result<()> {
    let mut cb = arboard::Clipboard::new()?;
    cb.set_text(text)?;
    Ok(())
}

/// Save/restore of the whole clipboard around a paste: every format, so files, HTML, RTF and
/// images survive. `capture` returns None when the clipboard can't be read safely.
#[cfg(windows)]
pub(crate) mod clip {
    use std::time::Duration;
    use windows::core::w;
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

    /// shortcut: above this we don't snapshot (the caller types instead); raise if users hit it.
    const MAX_TOTAL: usize = 256 << 20;

    pub struct Snapshot(Vec<(u32, Vec<u8>)>);

    /// Formats whose handle is not an HGLOBAL: CF_BITMAP, CF_METAFILEPICT (holds an HMETAFILE),
    /// CF_PALETTE, CF_ENHMETAFILE, owner-display/DSP formats and the private/GDI-object ranges.
    /// Reading them as memory would be undefined, so they're skipped; Windows re-synthesises
    /// CF_BITMAP from CF_DIB (and the metafile formats from each other), so images still survive.
    fn hglobal_backed(f: u32) -> bool {
        !matches!(f, 2 | 3 | 9 | 14 | 0x80 | 0x82 | 0x83 | 0x8E | 0x200..=0x3FF)
    }

    struct Open;
    impl Open {
        fn new() -> Option<Self> {
            // The target app may still hold the clipboard right after Ctrl+V; retry briefly.
            for _ in 0..20 {
                if unsafe { OpenClipboard(None) }.is_ok() {
                    return Some(Open);
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            None
        }
    }
    impl Drop for Open {
        fn drop(&mut self) {
            let _ = unsafe { CloseClipboard() };
        }
    }

    fn alloc(bytes: &[u8]) -> Option<HGLOBAL> {
        let h = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }.ok()?;
        let p = unsafe { GlobalLock(h) } as *mut u8;
        if p.is_null() {
            let _ = unsafe { GlobalFree(h) };
            return None;
        }
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len()) };
        let _ = unsafe { GlobalUnlock(h) };
        Some(h)
    }

    fn free_all(hs: Vec<(u32, HGLOBAL)>) {
        for (_, h) in hs {
            let _ = unsafe { GlobalFree(h) };
        }
    }

    impl Snapshot {
        pub fn capture() -> Option<Self> {
            let _open = Open::new()?;
            let (mut formats, mut total, mut f) = (Vec::new(), 0usize, 0u32);
            // Office/Excel render formats on demand; asking for all of them can take seconds.
            let t0 = std::time::Instant::now();
            loop {
                if t0.elapsed() > std::time::Duration::from_millis(400) {
                    log::warn!("clipboard snapshot too slow; typing instead of pasting");
                    return None;
                }
                f = unsafe { EnumClipboardFormats(f) };
                if f == 0 {
                    break;
                }
                if !hglobal_backed(f) {
                    continue;
                }
                let Ok(h) = (unsafe { GetClipboardData(f) }) else { continue };
                let h = HGLOBAL(h.0);
                let size = unsafe { GlobalSize(h) };
                if size == 0 {
                    continue;
                }
                total += size;
                if total > MAX_TOTAL {
                    log::warn!("clipboard is over {} MB; typing instead of pasting", MAX_TOTAL >> 20);
                    return None;
                }
                let p = unsafe { GlobalLock(h) } as *const u8;
                if p.is_null() {
                    continue;
                }
                let bytes = unsafe { std::slice::from_raw_parts(p, size) }.to_vec();
                let _ = unsafe { GlobalUnlock(h) };
                formats.push((f, bytes));
            }
            Some(Self(formats))
        }

        pub fn is_empty(&self) -> bool {
            self.0.is_empty()
        }

        /// Puts every saved format back. Memory is allocated before the clipboard is emptied,
        /// so a failure leaves the dictation on the clipboard instead of nothing.
        pub fn restore(self) {
            // An empty clipboard before: leave the dictation there rather than clear it.
            if self.0.is_empty() {
                return;
            }
            let mut hs = Vec::with_capacity(self.0.len() + 2);
            for (f, bytes) in &self.0 {
                match alloc(bytes) {
                    Some(h) => hs.push((*f, h)),
                    None => return free_all(hs),
                }
            }
            // Keep the restored copy out of Win+V history and cloud sync (it's already there once).
            for name in [w!("CanIncludeInClipboardHistory"), w!("CanUploadToCloudClipboard")] {
                let f = unsafe { RegisterClipboardFormatW(name) };
                if f != 0 && !self.0.iter().any(|(g, _)| *g == f) {
                    match alloc(&0u32.to_ne_bytes()) {
                        Some(h) => hs.push((f, h)),
                        None => return free_all(hs),
                    }
                }
            }
            let Some(_open) = Open::new() else { return free_all(hs) };
            if unsafe { EmptyClipboard() }.is_err() {
                return free_all(hs);
            }
            for (f, h) in hs {
                // On success the system owns the memory; on failure it's still ours.
                if unsafe { SetClipboardData(f, HANDLE(h.0)) }.is_err() {
                    let _ = unsafe { GlobalFree(h) };
                }
            }
        }
    }
}

/// macOS: every NSPasteboard item with every type, so images, files and rich text survive.
#[cfg(target_os = "macos")]
pub(crate) mod clip {
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardTypeString, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSData, NSString};

    /// shortcut: above this we don't snapshot (the caller types instead); raise if users hit it.
    const MAX_TOTAL: usize = 256 << 20;
    /// nspasteboard.org markers: clipboard managers skip transient/concealed items.
    const MARKERS: [&str; 2] = ["org.nspasteboard.TransientType", "org.nspasteboard.ConcealedType"];

    pub struct Snapshot(Vec<Vec<(Retained<NSString>, Retained<NSData>)>>);

    // Called from worker threads: each entry point drains its own autorelease pool.
    impl Snapshot {
        pub fn capture() -> Option<Self> {
            autoreleasepool(|_| Self::read())
        }

        fn read() -> Option<Self> {
            let pb = NSPasteboard::generalPasteboard();
            let (mut items, mut total) = (Vec::new(), 0usize);
            for item in pb.pasteboardItems().map(|a| a.to_vec()).unwrap_or_default() {
                let mut kept = Vec::new();
                for t in item.types().to_vec() {
                    let Some(d) = item.dataForType(&t) else { continue };
                    total += d.length();
                    if total > MAX_TOTAL {
                        log::warn!("clipboard is over {} MB; typing instead of pasting", MAX_TOTAL >> 20);
                        return None;
                    }
                    kept.push((t, d));
                }
                items.push(kept);
            }
            Some(Self(items))
        }

        pub fn is_empty(&self) -> bool {
            self.0.is_empty()
        }

        pub fn restore(self) {
            if self.0.is_empty() {
                return;
            }
            autoreleasepool(|_| {
                let objs: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = self
                    .0
                    .iter()
                    .map(|types| {
                        let it = NSPasteboardItem::new();
                        for (t, d) in types {
                            it.setData_forType(d, t);
                        }
                        ProtocolObject::from_retained(it)
                    })
                    .collect();
                let pb = NSPasteboard::generalPasteboard();
                pb.clearContents();
                pb.writeObjects(&NSArray::from_retained_slice(&objs));
            })
        }
    }

    pub fn set_private_text(text: &str) -> bool {
        autoreleasepool(|_| {
            let pb = NSPasteboard::generalPasteboard();
            pb.clearContents();
            let ok = pb.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString });
            let empty = NSData::new();
            for m in MARKERS {
                pb.setData_forType(Some(&empty), &NSString::from_str(m));
            }
            ok
        })
    }

    pub fn change_count() -> u32 {
        autoreleasepool(|_| NSPasteboard::generalPasteboard().changeCount() as u32)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) mod clip {
    /// Text only on other platforms.
    pub struct Snapshot(Option<String>);
    impl Snapshot {
        pub fn capture() -> Option<Self> {
            Some(Self(arboard::Clipboard::new().ok()?.get_text().ok()))
        }
        pub fn is_empty(&self) -> bool {
            self.0.is_none()
        }
        pub fn restore(self) {
            if let (Some(t), Ok(mut cb)) = (self.0, arboard::Clipboard::new()) {
                let _ = cb.set_text(t);
            }
        }
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
        VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN, VK_SHIFT,
    };

    const VK_V: VIRTUAL_KEY = VIRTUAL_KEY(0x56);

    fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        }
    }

    fn send(inputs: &[INPUT]) -> bool {
        let n = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
        n as usize == inputs.len()
    }

    fn is_down(vk: VIRTUAL_KEY) -> bool {
        unsafe { (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0 }
    }

    /// If Win is still physically held, Ctrl+V becomes Win+Ctrl+V; wait for release.
    pub fn wait_modifiers_released(max: Duration) {
        let t0 = Instant::now();
        while t0.elapsed() < max && [VK_LWIN, VK_RWIN, VK_MENU, VK_SHIFT, VK_CONTROL].iter().any(|&k| is_down(k)) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn send_ctrl(k: VIRTUAL_KEY) -> bool {
        let none = KEYBD_EVENT_FLAGS(0);
        send(&[key(VK_CONTROL, 0, none), key(k, 0, none), key(k, 0, KEYEVENTF_KEYUP), key(VK_CONTROL, 0, KEYEVENTF_KEYUP)])
    }

    pub fn send_ctrl_v() -> bool {
        send_ctrl(VK_V)
    }

    pub fn send_ctrl_c() -> bool {
        send_ctrl(VIRTUAL_KEY(0x43))
    }

    pub fn clipboard_seq() -> u32 {
        unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() }
    }

    pub fn type_unicode(text: &str) -> bool {
        let mut inputs = Vec::with_capacity(text.len() * 2);
        for ch in text.chars() {
            match ch {
                '\r' => continue,
                // Shift+Enter: a line break everywhere, never "send" in chat apps (Slack, WhatsApp, Teams).
                '\n' => {
                    inputs.push(key(VK_SHIFT, 0, KEYBD_EVENT_FLAGS(0)));
                    inputs.push(key(VK_RETURN, 0, KEYBD_EVENT_FLAGS(0)));
                    inputs.push(key(VK_RETURN, 0, KEYEVENTF_KEYUP));
                    inputs.push(key(VK_SHIFT, 0, KEYEVENTF_KEYUP));
                }
                _ => {
                    let mut buf = [0u16; 2];
                    for unit in ch.encode_utf16(&mut buf).iter() {
                        inputs.push(key(VIRTUAL_KEY(0), *unit, KEYEVENTF_UNICODE));
                        inputs.push(key(VIRTUAL_KEY(0), *unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                    }
                }
            }
        }
        // Chunk so slow apps (Electron) don't drop characters.
        for chunk in inputs.chunks(64) {
            if !send(chunk) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(4));
        }
        true
    }
}

/// macOS keystrokes via CGEvent (needs Accessibility). Aliased to `win` so the shared code reads the same.
#[cfg(target_os = "macos")]
use mac as win;

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use crate::hotkey::macos::{FIELD_USER_DATA, OUR_EVENT};
    use std::ffi::c_void;

    type Ref = *mut c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceFlagsState(state: i32) -> u64;
        fn CGEventCreateKeyboardEvent(src: Ref, key: u16, down: bool) -> Ref;
        fn CGEventSetFlags(ev: Ref, flags: u64);
        fn CGEventSetIntegerValueField(ev: Ref, field: u32, v: i64);
        fn CGEventKeyboardSetUnicodeString(ev: Ref, len: usize, s: *const u16);
        fn CGEventPost(tap: u32, ev: Ref);
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(r: *const c_void);
    }

    const HID_TAP: u32 = 0; // kCGHIDEventTap
    const HID_STATE: i32 = 1; // kCGEventSourceStateHIDSystemState
    const CMD: u64 = 0x10_0000;
    const SHIFT: u64 = 0x2_0000;
    /// fn, ⌘, ⌥, ⇧, ⌃
    const HELD: u64 = 0x80_0000 | CMD | 0x8_0000 | SHIFT | 0x4_0000;
    const KEY_V: u16 = 9;
    const KEY_C: u16 = 8;
    const KEY_RETURN: u16 = 36;
    /// shortcut: 20 UTF-16 units per event as the spec suggests (true limit unverified); raise if apps cope.
    const CHUNK: usize = 20;

    pub fn blocked() -> Option<&'static str> {
        if unsafe { crate::context::mac::IsSecureEventInputEnabled() } {
            return Some("secure input is on");
        }
        if !unsafe { crate::context::mac::AXIsProcessTrusted() } {
            return Some("Accessibility is off");
        }
        None
    }

    /// If ⌘/⌥/fn is still physically held, our ⌘V would carry it; wait for release.
    pub fn wait_modifiers_released(max: Duration) {
        let t0 = Instant::now();
        while t0.elapsed() < max && unsafe { CGEventSourceFlagsState(HID_STATE) } & HELD != 0 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Key down + up, tagged so our own tap ignores it.
    fn post(key: u16, flags: u64, unicode: &[u16]) -> bool {
        for down in [true, false] {
            let ev = unsafe { CGEventCreateKeyboardEvent(std::ptr::null_mut(), key, down) };
            if ev.is_null() {
                return false;
            }
            unsafe {
                CGEventSetFlags(ev, flags);
                CGEventSetIntegerValueField(ev, FIELD_USER_DATA, OUR_EVENT);
                if !unicode.is_empty() {
                    CGEventKeyboardSetUnicodeString(ev, unicode.len(), unicode.as_ptr());
                }
                CGEventPost(HID_TAP, ev);
                CFRelease(ev);
            }
        }
        true
    }

    pub fn send_ctrl_v() -> bool {
        post(KEY_V, CMD, &[])
    }

    pub fn send_ctrl_c() -> bool {
        post(KEY_C, CMD, &[])
    }

    pub fn clipboard_seq() -> u32 {
        clip::change_count()
    }

    pub fn type_unicode(text: &str) -> bool {
        let mut buf: Vec<u16> = Vec::with_capacity(CHUNK + 1);
        // Keycode 0 carries the Unicode string. Caveat: it is kVK_ANSI_A, so apps that read raw
        // keycodes (games, remote desktops, some terminals/VMs) may see "a" instead of the text.
        let flush = |buf: &mut Vec<u16>| -> bool {
            let ok = buf.is_empty() || post(0, 0, buf);
            buf.clear();
            std::thread::sleep(Duration::from_millis(4));
            ok
        };
        for ch in text.chars() {
            match ch {
                '\r' => continue,
                // Shift+Return: a line break everywhere, never "send" in chat apps.
                '\n' => {
                    if !flush(&mut buf) || !post(KEY_RETURN, SHIFT, &[]) {
                        return false;
                    }
                }
                _ => {
                    let mut u = [0u16; 2];
                    let units = ch.encode_utf16(&mut u);
                    if buf.len() + units.len() > CHUNK && !flush(&mut buf) {
                        return false;
                    }
                    buf.extend_from_slice(units);
                }
            }
        }
        flush(&mut buf)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod win {
    use super::*;
    pub fn wait_modifiers_released(_max: Duration) {
        let _ = Instant::now();
    }
    pub fn send_ctrl_v() -> bool {
        false
    }
    pub fn send_ctrl_c() -> bool {
        false
    }
    pub fn clipboard_seq() -> u32 {
        0
    }
    pub fn type_unicode(_text: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn terminals_never_get_ctrl_c() {
        for t in ["WindowsTerminal.exe", "wt.exe", "cmd.exe", "pwsh.exe", "powershell.exe", "conhost.exe", "mintty.exe"] {
            assert!(super::is_terminal(t), "{t}");
        }
        assert!(!super::is_terminal("code.exe") && !super::is_terminal("notepad.exe") && !super::is_terminal(""));
    }
}

#[cfg(all(test, windows))]
mod clipboard_tests {
    /// Touches the real clipboard, so it only runs on request: cargo test -- --ignored clipboard
    #[test]
    #[ignore]
    fn copy_to_clipboard_roundtrip() {
        super::copy_to_clipboard("yappaflow-copy-test").unwrap();
        assert_eq!(arboard::Clipboard::new().unwrap().get_text().unwrap(), "yappaflow-copy-test");
    }

    /// cargo test -- --ignored snapshot
    #[test]
    #[ignore]
    fn snapshot_restores_html_and_text() {
        let users = super::clip::Snapshot::capture().unwrap();
        let mut cb = arboard::Clipboard::new().unwrap();
        cb.set_html("<b>yappaflow</b>", Some("yappaflow-before")).unwrap();
        let snap = super::clip::Snapshot::capture().unwrap();
        cb.set_text("dictation").unwrap();
        snap.restore();
        let (text, html) = (cb.get_text(), cb.get().html());
        users.restore();
        assert_eq!(text.unwrap(), "yappaflow-before");
        assert!(html.unwrap().contains("<b>yappaflow</b>"));
    }
}
