//! Global hotkey on macOS: a CGEventTap (session, head insert, active so Esc can be swallowed)
//! on its own thread with a CFRunLoop. Like the Windows `hook_proc`, the callback only maps the
//! keycode, updates the chord tracker and pushes to the lock-free ring: no allocation, locks, logs or IO.
//! Needs Accessibility (active tap) and Input Monitoring; until both are granted the tap can't be
//! created, so the thread retries every second instead of failing app start-up.
//! Unlike Windows (which skips LLKHF_INJECTED), synthetic keystrokes from other tools (Keyboard
//! Maestro, BetterTouchTool, remote desktops) DO reach this tap and can trigger the chord; only our
//! own events are skipped, via the user-data tag. macOS has no reliable "injected" flag to filter on.

use super::actions::Hooks;
use super::keys::{ALT, CTRL, ESC, FN, RALT, SHIFT, WIN};
use super::*;
use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, AtomicU32};

type Ref = *mut c_void;
type TapCallback = unsafe extern "C" fn(Ref, u32, Ref, *mut c_void) -> Ref;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, cb: TapCallback, info: *mut c_void) -> Ref;
    fn CGEventTapEnable(tap: Ref, enable: bool);
    fn CGEventTapIsEnabled(tap: Ref) -> bool;
    fn CGEventGetIntegerValueField(ev: Ref, field: u32) -> i64;
    fn CGEventGetFlags(ev: Ref) -> u64;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFMachPortCreateRunLoopSource(alloc: Ref, port: Ref, order: isize) -> Ref;
    fn CFMachPortInvalidate(port: Ref);
    fn CFRunLoopGetCurrent() -> Ref;
    fn CFRunLoopAddSource(rl: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRemoveSource(rl: Ref, source: Ref, mode: Ref);
    fn CFRunLoopWakeUp(rl: Ref);
    fn CFRunLoopRun();
    fn CFRelease(r: *const c_void);
    fn CFRetain(r: *const c_void) -> Ref;
    static kCFRunLoopCommonModes: Ref;
}

const SESSION_TAP: u32 = 1; // kCGSessionEventTap
const HEAD_INSERT: u32 = 0; // kCGHeadInsertEventTap
const TAP_DEFAULT: u32 = 0; // kCGEventTapOptionDefault (active)
const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS_CHANGED: u32 = 12;
const DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
const FIELD_KEYCODE: u32 = 9; // kCGKeyboardEventKeycode
/// kCGEventSourceUserData: events Yappaflow posts (insert.rs) carry this, and the tap ignores them.
pub const FIELD_USER_DATA: u32 = 42;
pub const OUR_EVENT: i64 = 0x5961_7070; // "Yapp"

static TAP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
/// Times macOS disabled the tap (timeout / user input) and we turned it back on. Logged by the watchdog.
static REENABLES: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static HOOKS: RefCell<Hooks> = RefCell::new(Hooks::new());
}

/// Mac virtual keycode (Carbon kVK_*, ANSI positions) → the Windows-VK-style code keys.rs uses.
/// 0 = unmapped; the tracker still treats it as "another key" while a chord is held.
const MAP: [u8; 128] = {
    let mut m = [0u8; 128];
    let letters = b"ASDFHGZXCV\0BQWERYT";
    let mut i = 0;
    while i < letters.len() {
        m[i] = letters[i];
        i += 1;
    }
    m[18] = b'1';
    m[19] = b'2';
    m[20] = b'3';
    m[21] = b'4';
    m[22] = b'6';
    m[23] = b'5';
    m[24] = 0xBB;
    m[25] = b'9';
    m[26] = b'7';
    m[27] = 0xBD;
    m[28] = b'8';
    m[29] = b'0';
    m[30] = 0xDD;
    m[31] = b'O';
    m[32] = b'U';
    m[33] = 0xDB;
    m[34] = b'I';
    m[35] = b'P';
    m[36] = 0x0D;
    m[37] = b'L';
    m[38] = b'J';
    m[39] = 0xDE;
    m[40] = b'K';
    m[41] = 0xBA;
    m[42] = 0xDC;
    m[43] = 0xBC;
    m[44] = 0xBF;
    m[45] = b'N';
    m[46] = b'M';
    m[47] = 0xBE;
    m[48] = 0x09;
    m[49] = 0x20;
    m[50] = 0xC0;
    m[51] = 0x08;
    m[53] = ESC;
    m[57] = 0x14;
    // F-keys
    m[122] = 0x70;
    m[120] = 0x71;
    m[99] = 0x72;
    m[118] = 0x73;
    m[96] = 0x74;
    m[97] = 0x75;
    m[98] = 0x76;
    m[100] = 0x77;
    m[101] = 0x78;
    m[109] = 0x79;
    m[103] = 0x7A;
    m[111] = 0x7B;
    m[105] = 0x7C;
    m[107] = 0x7D;
    m[113] = 0x7E;
    m[106] = 0x7F;
    m[64] = 0x80;
    m[79] = 0x81;
    m[80] = 0x82;
    m[90] = 0x83;
    // Navigation
    m[114] = 0x2D;
    m[115] = 0x24;
    m[116] = 0x21;
    m[117] = 0x2E;
    m[119] = 0x23;
    m[121] = 0x22;
    m[123] = 0x25;
    m[124] = 0x27;
    m[125] = 0x28;
    m[126] = 0x26;
    m
};

/// Modifier keycode → (our code, device-dependent flag bit that says "this exact key is down").
fn modifier(keycode: i64) -> Option<(u8, u64)> {
    Some(match keycode {
        55 => (WIN, 0x08), // left ⌘  (NX_DEVICELCMDKEYMASK)
        54 => (WIN, 0x10), // right ⌘
        56 => (SHIFT, 0x02),
        60 => (SHIFT, 0x04),
        59 => (CTRL, 0x01),
        62 => (CTRL, 0x2000),
        58 => (ALT, 0x20),     // left ⌥
        61 => (RALT, 0x40),    // right ⌥ has its own code (fallback chord "Right ⌥")
        63 => (FN, 0x80_0000), // fn / 🌐 (kCGEventFlagMaskSecondaryFn)
        _ => return None,
    })
}

unsafe extern "C" fn tap_cb(_proxy: Ref, ty: u32, ev: Ref, _info: *mut c_void) -> Ref {
    if ty == DISABLED_BY_TIMEOUT || ty == DISABLED_BY_USER_INPUT {
        let tap = TAP.load(Ordering::Relaxed);
        if !tap.is_null() {
            CGEventTapEnable(tap, true);
            REENABLES.fetch_add(1, Ordering::Relaxed);
        }
        return ev;
    }
    if CGEventGetIntegerValueField(ev, FIELD_USER_DATA) == OUR_EVENT {
        return ev;
    }
    let keycode = CGEventGetIntegerValueField(ev, FIELD_KEYCODE);
    let (code, down) = match ty {
        FLAGS_CHANGED => match modifier(keycode) {
            Some((c, bit)) => (c, CGEventGetFlags(ev) & bit != 0),
            None => return ev, // caps lock etc.
        },
        KEY_DOWN | KEY_UP => (MAP.get(keycode as usize).copied().unwrap_or(0), ty == KEY_DOWN),
        _ => return ev,
    };
    // Right ⌥ is always RALT, so a chord with plain ⌥ needs the left Option key.
    let active = SESSION_ACTIVE.load(Ordering::Relaxed);
    let out = HOOKS.with(|h| h.borrow_mut().on_code(code, down, active));
    for e in out.events.iter().flatten() {
        emit(*e);
    }
    if out.swallow {
        return std::ptr::null_mut();
    }
    ev
}

pub fn input_monitoring_granted() -> bool {
    unsafe { CGPreflightListenEventAccess() }
}

/// Shows the system prompt once; afterwards the user has to flip the switch in System Settings.
pub fn request_input_monitoring() -> bool {
    unsafe { CGRequestListenEventAccess() }
}

/// The tap thread's run loop (retained) and the current tap's run-loop source.
static RUN_LOOP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static SRC: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

unsafe fn create_tap() -> Ref {
    let mask = (1u64 << KEY_DOWN) | (1u64 << KEY_UP) | (1u64 << FLAGS_CHANGED);
    CGEventTapCreate(SESSION_TAP, HEAD_INSERT, TAP_DEFAULT, mask, tap_cb, std::ptr::null_mut())
}

/// Puts `tap` on the tap thread's run loop and retires the previous tap, if any.
/// CFRunLoop add/remove/wake are thread-safe, so the watchdog may call this too.
unsafe fn install(tap: Ref) {
    let rl = RUN_LOOP.load(Ordering::Acquire);
    let src = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
    CFRunLoopAddSource(rl, src, kCFRunLoopCommonModes);
    let old_src = SRC.swap(src, Ordering::AcqRel);
    let old_tap = TAP.swap(tap, Ordering::AcqRel);
    CGEventTapEnable(tap, true);
    if !old_src.is_null() {
        CFRunLoopRemoveSource(rl, old_src, kCFRunLoopCommonModes);
        CFRelease(old_src);
    }
    if !old_tap.is_null() {
        CFMachPortInvalidate(old_tap);
        CFRelease(old_tap);
    }
    CFRunLoopWakeUp(rl);
}

pub fn start() -> anyhow::Result<()> {
    std::thread::Builder::new().name("hotkey-tap".into()).spawn(|| unsafe {
        RUN_LOOP.store(CFRetain(CFRunLoopGetCurrent()), Ordering::Release);
        let mut warned = false;
        loop {
            let t = create_tap();
            if !t.is_null() {
                install(t);
                break;
            }
            if !warned {
                log::warn!("event tap not allowed yet (Accessibility / Input Monitoring); retrying every second");
                warned = true;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        log::info!("event tap installed");
        CFRunLoopRun();
    })?;
    // Watchdog: log re-enables; a tap that stays off is re-enabled, and if that doesn't stick
    // (e.g. after a permission flip) it is replaced by a fresh one.
    std::thread::Builder::new().name("hotkey-watchdog".into()).spawn(|| {
        let mut seen = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let n = REENABLES.load(Ordering::Relaxed);
            if n != seen {
                log::warn!("event tap was disabled by macOS and re-enabled ({n} times so far)");
                seen = n;
            }
            let tap = TAP.load(Ordering::Acquire);
            if tap.is_null() || unsafe { CGEventTapIsEnabled(tap) } {
                continue;
            }
            unsafe { CGEventTapEnable(tap, true) };
            if unsafe { CGEventTapIsEnabled(tap) } {
                continue;
            }
            let fresh = unsafe { create_tap() };
            if fresh.is_null() {
                continue; // no permission right now; try again next round
            }
            unsafe { install(fresh) };
            log::warn!("event tap stayed disabled; replaced it with a new one");
        }
    })?;
    Ok(())
}

/// `defaults read com.apple.HIToolbox AppleFnUsageType`. Values seen in macOS: 0 Do Nothing,
/// 1 Change Input Source, 2 Show Emoji & Symbols, 3 Start Dictation. Missing key = macOS default
/// (Change Input Source / Emoji, depending on the keyboard), so it counts as a conflict.
pub fn fn_key_conflicts() -> bool {
    let out = std::process::Command::new("/usr/bin/defaults").args(["read", "com.apple.HIToolbox", "AppleFnUsageType"]).output();
    !matches!(out, Ok(o) if o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "0")
}

#[cfg(test)]
mod tests {
    #[test]
    fn map_basics() {
        assert_eq!(super::MAP[0], b'A');
        assert_eq!(super::MAP[9], b'V');
        assert_eq!(super::MAP[10], 0);
        assert_eq!(super::MAP[17], b'T');
        assert_eq!(super::MAP[53], super::ESC);
        assert_eq!(super::MAP[123], 0x25);
    }
}
