//! Global hotkey via a low-level keyboard hook (WH_KEYBOARD_LL) on a dedicated thread.
//!
//! The hook callback must return fast (Windows silently unhooks callbacks that exceed
//! LowLevelHooksTimeout, ~300 ms). It only classifies the key, updates the chord
//! tracker and posts an event to a channel. No allocation, no IO, no locks.

pub mod actions;
pub mod keys;
mod ring;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::thread::Thread;

pub use actions::HookEvent;
pub use keys::ChordEvent;

static RING: ring::Ring = ring::Ring::new();
static WAKER: OnceLock<Thread> = OnceLock::new();

/// Hook side: no allocation, no locks (unpark is a futex-style wake).
fn emit(ev: HookEvent) {
    if RING.push(ev.code()) {
        if let Some(t) = WAKER.get() {
            t.unpark();
        }
    }
}

/// Forwarder side: call from the thread that will drain events; blocks until one arrives.
pub fn next_event() -> HookEvent {
    let _ = WAKER.set(std::thread::current());
    loop {
        if let Some(ev) = RING.pop().and_then(HookEvent::from_code) {
            return ev;
        }
        std::thread::park_timeout(std::time::Duration::from_millis(100));
    }
}

/// Apply a validated chord live (empty = disabled); the hook picks it up on its next key event.
pub fn set_chord(action: actions::ActionId, codes: &[u8]) {
    keys::store_config(action.idx(), codes);
}

/// Set by the controller while a session is active, so the hook knows to swallow Esc.
pub static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_session_active(active: bool) {
    SESSION_ACTIVE.store(active, Ordering::Relaxed);
}

/// Settings › Hotkeys › Hands-free: true = double-tap the dictate chord, false = its own hotkey.
/// Read by the controller each loop; set by `hotkey_cmds::apply_all`.
pub static HANDS_FREE_DOUBLE_TAP: AtomicBool = AtomicBool::new(true);

#[cfg(windows)]
mod win {
    use super::actions::Hooks;
    use super::keys::VK_MASK;
    use super::*;
    use std::cell::RefCell;
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, HHOOK,
        KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    thread_local! {
        static HOOKS: RefCell<Hooks> = RefCell::new(Hooks::new());
    }

    fn send_mask_key() {
        let mk = |flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(VK_MASK), wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        };
        let inputs = [mk(KEYBD_EVENT_FLAGS(0)), mk(KEYEVENTF_KEYUP)];
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            // Ignore synthetic events (our own SendInput, other tools' macros).
            if kb.flags.0 & LLKHF_INJECTED.0 == 0 {
                let msg = wparam.0 as u32;
                let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
                let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
                if down || up {
                    let active = SESSION_ACTIVE.load(Ordering::Relaxed);
                    let out = HOOKS.with(|h| h.borrow_mut().on_vk(kb.vkCode, down, active));
                    if out.mask_before {
                        send_mask_key();
                    }
                    for ev in out.events.iter().flatten() {
                        emit(*ev);
                    }
                    if out.swallow {
                        return LRESULT(1);
                    }
                }
            }
        }
        CallNextHookEx(HHOOK::default(), code, wparam, lparam)
    }

    pub fn start() -> anyhow::Result<()> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        std::thread::Builder::new().name("hotkey-hook".into()).spawn(move || unsafe {
            let hmod: HINSTANCE = GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).unwrap_or_default();
            let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), hmod, 0) {
                Ok(h) => {
                    let _ = ready_tx.send(Ok(()));
                    h
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return;
                }
            };
            // A message loop is required for LL hooks to be called.
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let _ = UnhookWindowsHookEx(hook);
        })?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(anyhow::anyhow!("SetWindowsHookExW failed: {e}")),
            Err(_) => Err(anyhow::anyhow!("hook thread died")),
        }
    }
}

#[cfg(windows)]
pub use win::start;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use macos::start;

#[cfg(not(any(windows, target_os = "macos")))]
pub fn start() -> anyhow::Result<()> {
    log::warn!("global hotkey is only implemented on Windows and macOS");
    Ok(())
}
