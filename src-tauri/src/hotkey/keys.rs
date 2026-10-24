//! Pure key normalisation + chord tracking for a user-chosen 1-3 key chord (default Ctrl+Win).
//! The WinAPI hook (hotkey/mod.rs) feeds raw virtual-key codes in here.
//! Keys are normalised to a u8 "code": left/right modifiers collapse to one code, everything
//! else is its VK (all VKs fit in a byte). 0 = unused slot.

use std::sync::atomic::{AtomicU32, Ordering};

pub const CTRL: u8 = 0x11;
pub const SHIFT: u8 = 0x10;
pub const ALT: u8 = 0x12;
pub const WIN: u8 = 0x5B;
pub const ESC: u8 = 0x1B;
/// macOS only: fn (🌐) and right Option. Unassigned VKs, so Windows never produces them.
pub const FN: u8 = 0x0E;
pub const RALT: u8 = 0x0F;
/// Unassigned VK used as a "menu mask" so releasing Win doesn't open Start.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub const VK_MASK: u16 = 0xE8;

/// Collapse left/right variants. Anything unrecognised keeps its VK.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn normalize(vk: u32) -> u8 {
    match vk {
        0xA2 | 0xA3 => CTRL,
        0xA0 | 0xA1 => SHIFT,
        0xA4 | 0xA5 => ALT,
        0x5C => WIN,
        v if v < 256 => v as u8,
        _ => 0,
    }
}

pub fn is_modifier(c: u8) -> bool {
    matches!(c, CTRL | SHIFT | ALT | WIN | FN | RALT)
}

/// Default chords: dictate, command mode. Hold fn on a Mac (spec 8 §1), Ctrl+Win on Windows.
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_DICTATE: [u8; 3] = [CTRL, WIN, 0];
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_COMMAND: [u8; 3] = [CTRL, ALT, WIN];
#[cfg(target_os = "macos")]
pub const DEFAULT_DICTATE: [u8; 3] = [FN, 0, 0];
#[cfg(target_os = "macos")]
pub const DEFAULT_COMMAND: [u8; 3] = [CTRL, FN, 0];

/// Number of hotkey actions (see actions::ActionId).
pub const N_ACTIONS: usize = 6;

/// Active chord per action, packed 3 x u8 into an atomic so the hook never locks.
/// 0 = disabled. Index = ActionId as usize: dictate, hands_free, cancel, paste_last, toggle_pause, command_mode.
pub static CONFIG: [AtomicU32; N_ACTIONS] = [
    AtomicU32::new(pack(DEFAULT_DICTATE)),
    AtomicU32::new(0),
    AtomicU32::new(pack([ESC, 0, 0])),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(pack(DEFAULT_COMMAND)),
];

pub const fn pack(c: [u8; 3]) -> u32 {
    c[0] as u32 | (c[1] as u32) << 8 | (c[2] as u32) << 16
}
pub fn unpack(p: u32) -> [u8; 3] {
    [p as u8, (p >> 8) as u8, (p >> 16) as u8]
}
pub fn store_config(idx: usize, codes: &[u8]) {
    let mut c = [0u8; 3];
    c[..codes.len()].copy_from_slice(codes);
    CONFIG[idx].store(pack(c), Ordering::Relaxed);
}

#[cfg(not(target_os = "macos"))]
const MODS: &[(&str, u8)] = &[("Ctrl", CTRL), ("Shift", SHIFT), ("Alt", ALT), ("Win", WIN)];
#[cfg(target_os = "macos")]
const MODS: &[(&str, u8)] = &[("Control", CTRL), ("Shift", SHIFT), ("Option", ALT), ("Cmd", WIN), ("Fn", FN), ("RightOption", RALT)];

const NAMED: &[(&str, u8)] = &[
    ("Space", 0x20),
    ("Tab", 0x09),
    ("Enter", 0x0D),
    ("Backspace", 0x08),
    ("CapsLock", 0x14),
    ("Esc", ESC),
    ("Insert", 0x2D),
    ("Delete", 0x2E),
    ("Home", 0x24),
    ("End", 0x23),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
    ("PrintScreen", 0x2C),
    ("ScrollLock", 0x91),
    ("Pause", 0x13),
];

/// "ctrl" / "Control" / "F9" / "k" -> code.
pub fn parse_key(name: &str) -> Result<u8, String> {
    let n = name.trim();
    let l = n.to_ascii_lowercase();
    match l.as_str() {
        "control" => return Ok(CTRL),
        "windows" | "super" | "meta" | "cmd" => return Ok(WIN),
        "option" => return Ok(ALT),
        "escape" => return Ok(ESC),
        #[cfg(target_os = "macos")]
        "ctrl" => return Ok(CTRL),
        #[cfg(target_os = "macos")]
        "alt" => return Ok(ALT),
        #[cfg(target_os = "macos")]
        "win" | "command" => return Ok(WIN),
        #[cfg(target_os = "macos")]
        "function" | "globe" => return Ok(FN),
        #[cfg(target_os = "macos")]
        "ralt" | "rightalt" | "altright" => return Ok(RALT),
        _ => {}
    }
    if let Some((_, c)) = MODS.iter().chain(NAMED).find(|(k, _)| k.eq_ignore_ascii_case(n)) {
        return Ok(*c);
    }
    let b = l.as_bytes();
    if b.len() == 1 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) {
        return Ok(b[0].to_ascii_uppercase());
    }
    if let Some(num) = l.strip_prefix('f').and_then(|x| x.parse::<u8>().ok()) {
        if (1..=24).contains(&num) {
            return Ok(0x6F + num);
        }
    }
    Err(format!("Unknown key \"{n}\""))
}

pub fn key_name(c: u8) -> String {
    if let Some((k, _)) = MODS.iter().chain(NAMED).find(|(_, v)| *v == c) {
        return (*k).into();
    }
    match c {
        0x30..=0x39 | 0x41..=0x5A => (c as char).to_string(),
        0x70..=0x87 => format!("F{}", c - 0x6F),
        _ => format!("Key{c:#04x}"),
    }
}

/// Validate 1-3 distinct keys; returns codes in canonical order (modifiers first) .
pub fn parse_chord(names: &[String]) -> Result<Vec<u8>, String> {
    parse_chord_opt(names, false)
}

/// `allow_esc` is only true for the cancel action.
pub fn parse_chord_opt(names: &[String], allow_esc: bool) -> Result<Vec<u8>, String> {
    if names.is_empty() || names.len() > 3 {
        return Err("A hotkey needs 1 to 3 keys".into());
    }
    let mut v = Vec::new();
    for n in names {
        let c = parse_key(n)?;
        if c == ESC && !allow_esc {
            return Err("Esc is reserved for the Cancel hotkey".into());
        }
        if v.contains(&c) {
            return Err("Each key can only be used once".into());
        }
        v.push(c);
    }
    if allow_esc && v.len() == 1 && is_modifier(v[0]) {
        return Err("Cancel can't be a single modifier key; use Esc, another key, or a chord".into());
    }
    v.sort_by_key(|c| {
        (
            !is_modifier(*c),
            match *c {
                CTRL => 0,
                ALT => 1,
                SHIFT => 2,
                WIN => 3,
                o => o,
            },
        )
    });
    #[cfg(target_os = "macos")]
    if v.contains(&ALT) && v.contains(&RALT) {
        return Err("Use either Option or Right Option, not both".into());
    }
    Ok(v)
}

pub fn chord_names(codes: &[u8]) -> Vec<String> {
    codes.iter().filter(|c| **c != 0).map(|c| key_name(*c)).collect()
}

#[cfg(not(target_os = "macos"))]
pub fn chord_display(codes: &[u8]) -> String {
    chord_names(codes).join(" + ")
}

/// Mac glyphs: ⌃ ⌥ ⇧ ⌘ fn.
#[cfg(target_os = "macos")]
pub fn chord_display(codes: &[u8]) -> String {
    let glyph = |c: &u8| match *c {
        CTRL => "⌃".to_string(),
        ALT => "⌥".into(),
        SHIFT => "⇧".into(),
        WIN => "⌘".into(),
        FN => "fn".into(),
        RALT => "Right ⌥".into(),
        o => key_name(o),
    };
    codes.iter().filter(|c| **c != 0).map(glyph).collect::<Vec<_>>().join(" + ")
}

/// macOS clashes. Warnings, never errors.
#[cfg(target_os = "macos")]
pub fn conflicts(codes: &[u8]) -> Vec<String> {
    let has = |c: u8| codes.contains(&c);
    let with = |m: u8, k: u8| codes.len() == 2 && has(m) && has(k);
    let mods = codes.iter().filter(|c| is_modifier(**c)).count();
    let mut w = Vec::new();
    if mods == 0 && codes.len() == 1 && matches!(codes[0], 0x30..=0x39 | 0x41..=0x5A | 0x20 | 0x0D | 0x08 | 0x09) {
        w.push("A single plain key would trigger while you type. Add a modifier like ⌃ or ⌥.".into());
    }
    if with(WIN, 0x56) {
        w.push("⌘ + V is Paste. Yappaflow itself uses it to insert text.".into());
    }
    if with(WIN, 0x43) {
        w.push("⌘ + C is Copy.".into());
    }
    if with(WIN, 0x51) {
        w.push("⌘ + Q quits the app.".into());
    }
    if with(WIN, 0x20) {
        w.push("⌘ + Space opens Spotlight.".into());
    }
    if with(WIN, 0x09) {
        w.push("⌘ + Tab switches apps.".into());
    }
    if with(CTRL, 0x20) {
        w.push("⌃ + Space switches the input source.".into());
    }
    if codes.len() == 1 && (has(ALT) || has(WIN)) {
        w.push("Holding this key alone is common in other shortcuts; Right ⌥ or fn is safer.".into());
    }
    w
}

/// Human-readable clashes for a (validated) chord. Warnings, never errors.
#[cfg(not(target_os = "macos"))]
pub fn conflicts(codes: &[u8]) -> Vec<String> {
    let has = |c: u8| codes.contains(&c);
    let mods = codes.iter().filter(|c| is_modifier(**c)).count();
    let plain: Vec<u8> = codes.iter().copied().filter(|c| !is_modifier(*c)).collect();
    let mut w = Vec::new();
    if mods == 0 && plain.len() == 1 && matches!(plain[0], 0x30..=0x39 | 0x41..=0x5A | 0x20 | 0x0D | 0x08 | 0x09) {
        w.push("A single plain key would trigger while you type. Add a modifier like Ctrl or Alt.".into());
    }
    if codes.len() == 1 && has(WIN) {
        w.push("Win alone opens the Start menu.".into());
    }
    let two = |a: u8, b: u8| codes.len() == 2 && has(a) && has(b);
    let with = |m: &[u8], k: u8| has(k) && m.iter().all(|x| has(*x)) && codes.len() == m.len() + 1;
    if with(&[WIN], 0x4C) {
        w.push("Win + L locks the PC.".into());
    }
    if with(&[WIN], 0x44) {
        w.push("Win + D shows the desktop.".into());
    }
    if with(&[WIN], 0x45) {
        w.push("Win + E opens File Explorer.".into());
    }
    if with(&[WIN], 0x52) {
        w.push("Win + R opens Run.".into());
    }
    if with(&[WIN], 0x56) {
        w.push("Win + V opens clipboard history.".into());
    }
    if with(&[WIN], 0x20) {
        w.push("Win + Space switches the input language.".into());
    }
    if with(&[CTRL], 0x43) {
        w.push("Ctrl + C is Copy.".into());
    }
    if with(&[CTRL], 0x56) {
        w.push("Ctrl + V is Paste. Yappaflow itself uses it to insert text.".into());
    }
    if with(&[CTRL], 0x58) {
        w.push("Ctrl + X is Cut.".into());
    }
    if with(&[CTRL], 0x5A) {
        w.push("Ctrl + Z is Undo.".into());
    }
    if with(&[CTRL], 0x41) {
        w.push("Ctrl + A is Select all.".into());
    }
    if with(&[CTRL], 0x53) {
        w.push("Ctrl + S is Save.".into());
    }
    if two(ALT, 0x09) {
        w.push("Alt + Tab switches windows.".into());
    }
    if two(ALT, 0x73) {
        w.push("Alt + F4 closes the window.".into());
    }
    if two(CTRL, 0x09) {
        w.push("Ctrl + Tab switches tabs in many apps.".into());
    }
    if with(&[WIN], 0x09) {
        w.push("Win + Tab opens Task View.".into());
    }
    if codes.len() == 3 && has(CTRL) && has(ALT) && has(0x2E) {
        w.push("Ctrl + Alt + Del is the Windows security screen.".into());
    }
    if codes.len() == 3 && has(CTRL) && has(SHIFT) && has(0x1B) {
        w.push("Ctrl + Shift + Esc opens Task Manager.".into());
    }
    if two(CTRL, ALT) {
        w.push("Ctrl + Alt is AltGr on many keyboard layouts and can type characters.".into());
    }
    if with(&[ALT], 0x20) {
        w.push("Alt + Space opens the window menu.".into());
    }
    if has(0x7A) && codes.len() == 1 {
        w.push("F11 toggles full screen in most apps.".into());
    }
    if has(0x70) && codes.len() == 1 {
        w.push("F1 opens Help in most apps.".into());
    }
    w
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordEvent {
    Down,
    Up,
    /// A non-chord key was pressed while the chord was held (it's some other shortcut).
    Other,
    Escape,
}

#[derive(Debug)]
pub struct ChordTracker {
    chord: [u8; 3],
    /// Bit i set = chord[i] currently held.
    held: u8,
    chord_active: bool,
    /// Win took part in a chord since it went down -> mask its release.
    win_used: bool,
}

impl Default for ChordTracker {
    fn default() -> Self {
        Self::new(pack(DEFAULT_DICTATE))
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub event: Option<ChordEvent>,
    /// Inject VK_MASK before letting this key event through.
    pub mask_before: bool,
}

impl ChordTracker {
    /// Called when the chord is about to be replaced: close out an in-flight chord.
    pub fn flush(&mut self) -> Outcome {
        let mut out = Outcome::default();
        if self.chord_active {
            out.event = Some(ChordEvent::Up);
            out.mask_before = self.win_used;
        }
        out
    }

    pub fn new(packed: u32) -> Self {
        Self { chord: unpack(packed), held: 0, chord_active: false, win_used: false }
    }

    pub fn on_key(&mut self, code: u8, down: bool) -> Outcome {
        let mut out = Outcome::default();
        let idx = if code == 0 { None } else { self.chord.iter().position(|c| *c == code) };
        let full = self.chord.iter().filter(|c| **c != 0).count();
        let all = ((1u16 << full) - 1) as u8;
        match (idx, down) {
            (Some(i), true) => {
                self.held |= 1 << i;
                // Auto-repeat sends repeated downs; only the first transition counts.
                if self.held == all && !self.chord_active {
                    self.chord_active = true;
                    if self.chord.contains(&WIN) {
                        self.win_used = true;
                    }
                    out.event = Some(ChordEvent::Down);
                }
            }
            (Some(i), false) => {
                self.held &= !(1 << i);
                if code == WIN && self.win_used {
                    out.mask_before = true;
                    self.win_used = false;
                }
                if self.chord_active {
                    self.chord_active = false;
                    out.event = Some(ChordEvent::Up);
                }
            }
            (None, true) if code != SHIFT && self.chord_active => out.event = Some(ChordEvent::Other),
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(c: &[u8]) -> ChordTracker {
        let mut a = [0u8; 3];
        a[..c.len()].copy_from_slice(c);
        ChordTracker::new(pack(a))
    }
    const OTHER: u8 = 0x41;

    #[test]
    fn normalize_variants() {
        assert_eq!(normalize(0xA2), CTRL);
        assert_eq!(normalize(0x5C), WIN);
        assert_eq!(normalize(0xA5), ALT);
        assert_eq!(normalize(0x41), 0x41);
        assert_eq!(normalize(0x1B), ESC);
    }

    #[cfg(not(target_os = "macos"))] // Windows defaults and names (Ctrl + Win)
    #[test]
    fn ctrl_then_win() {
        let mut t = ChordTracker::default();
        assert_eq!(t.on_key(CTRL, true).event, None);
        assert_eq!(t.on_key(WIN, true).event, Some(ChordEvent::Down));
        assert_eq!(t.on_key(WIN, true).event, None);
        assert_eq!(t.on_key(CTRL, true).event, None);
        assert_eq!(t.on_key(CTRL, false), Outcome { event: Some(ChordEvent::Up), mask_before: false });
        assert_eq!(t.on_key(WIN, false), Outcome { event: None, mask_before: true });
    }

    #[cfg(not(target_os = "macos"))] // Windows defaults and names (Ctrl + Win)
    #[test]
    fn win_then_ctrl_release_win_first() {
        let mut t = ChordTracker::default();
        t.on_key(WIN, true);
        assert_eq!(t.on_key(CTRL, true).event, Some(ChordEvent::Down));
        assert_eq!(t.on_key(WIN, false), Outcome { event: Some(ChordEvent::Up), mask_before: true });
        assert_eq!(t.on_key(CTRL, false), Outcome::default());
    }

    #[test]
    fn plain_win_is_untouched() {
        let mut t = ChordTracker::default();
        t.on_key(WIN, true);
        assert_eq!(t.on_key(OTHER, true).event, None);
        assert_eq!(t.on_key(WIN, false), Outcome::default());
    }

    #[cfg(not(target_os = "macos"))] // Windows defaults and names (Ctrl + Win)
    #[test]
    fn tap_does_nothing_until_all_held_and_other_key_flagged() {
        let mut t = ChordTracker::default();
        t.on_key(CTRL, true);
        assert_eq!(t.on_key(CTRL, false), Outcome::default());
        t.on_key(CTRL, true);
        t.on_key(WIN, true);
        assert_eq!(t.on_key(OTHER, true).event, Some(ChordEvent::Other));
        assert_eq!(t.on_key(0x25, true).event, Some(ChordEvent::Other), "arrow passes through as Other");
        assert_eq!(t.on_key(SHIFT, true).event, None, "shift is harmless");
        assert_eq!(t.on_key(ALT, true).event, Some(ChordEvent::Other));
    }

    #[test]
    fn esc_is_cancel_only() {
        assert!(parse_chord(&["Esc".into()]).is_err());
        assert!(parse_chord_opt(&["Esc".into()], true).is_ok());
        assert_eq!(key_name(ESC), "Esc");
    }

    #[test]
    fn single_key_chord() {
        let mut t = tr(&[0x78]); // F9
        assert_eq!(t.on_key(0x78, true).event, Some(ChordEvent::Down));
        assert_eq!(t.on_key(0x78, true).event, None);
        assert_eq!(t.on_key(0x78, false).event, Some(ChordEvent::Up));
        assert_eq!(t.on_key(WIN, false), Outcome::default(), "no Win mask when Win not in chord");
    }

    #[test]
    fn three_key_chord_any_order() {
        let mut t = tr(&[CTRL, ALT, 0x4B]);
        assert_eq!(t.on_key(0x4B, true).event, None);
        assert_eq!(t.on_key(CTRL, true).event, None);
        assert_eq!(t.on_key(ALT, true).event, Some(ChordEvent::Down));
        assert_eq!(t.on_key(CTRL, false).event, Some(ChordEvent::Up));
        assert_eq!(t.on_key(CTRL, true).event, Some(ChordEvent::Down), "re-press while others held");
    }

    #[test]
    fn non_win_chord_has_no_mask_and_win_is_other() {
        let mut t = tr(&[CTRL, ALT]);
        t.on_key(CTRL, true);
        t.on_key(ALT, true);
        assert_eq!(t.on_key(WIN, true).event, Some(ChordEvent::Other));
        assert_eq!(t.on_key(WIN, false), Outcome::default());
    }

    #[cfg(not(target_os = "macos"))] // Windows defaults and names (Ctrl + Win)
    #[test]
    fn double_tap_sequence_down_up_down_up() {
        let mut t = ChordTracker::default();
        let mut evs = vec![];
        for _ in 0..2 {
            evs.extend(t.on_key(CTRL, true).event);
            evs.extend(t.on_key(WIN, true).event);
            evs.extend(t.on_key(WIN, false).event);
            evs.extend(t.on_key(CTRL, false).event);
        }
        assert_eq!(evs, vec![ChordEvent::Down, ChordEvent::Up, ChordEvent::Down, ChordEvent::Up]);
    }

    #[cfg(not(target_os = "macos"))] // Windows defaults and names (Ctrl + Win)
    #[test]
    fn parse_and_display() {
        let c = parse_chord(&["win".into(), "Control".into()]).unwrap();
        assert_eq!(c, vec![CTRL, WIN]);
        assert_eq!(chord_display(&c), "Ctrl + Win");
        assert_eq!(parse_key("f9"), Ok(0x78));
        assert_eq!(key_name(0x78), "F9");
        assert_eq!(parse_key("k"), Ok(0x4B));
        assert_eq!(key_name(0x4B), "K");
        assert_eq!(chord_display(&parse_chord(&["k".into(), "Alt".into(), "Ctrl".into()]).unwrap()), "Ctrl + Alt + K");
    }

    #[test]
    fn parse_rejects_bad() {
        assert!(parse_chord(&[]).is_err());
        assert!(parse_chord(&["a".into(), "b".into(), "c".into(), "d".into()]).is_err());
        assert!(parse_chord(&["ctrl".into(), "Control".into()]).is_err());
        assert!(parse_chord(&["wat".into()]).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_fn_default_names_and_glyphs() {
        let mut t = ChordTracker::default();
        assert_eq!(t.on_key(FN, true).event, Some(ChordEvent::Down));
        assert_eq!(t.on_key(0x25, true).event, Some(ChordEvent::Other), "fn + arrow cancels");
        assert_eq!(t.on_key(FN, false).event, Some(ChordEvent::Up));
        let c = parse_chord(&["Fn".into(), "Control".into()]).unwrap();
        assert_eq!(c, vec![CTRL, FN]);
        assert_eq!(chord_names(&c), vec!["Control", "Fn"]);
        assert_eq!(chord_display(&c), "⌃ + fn");
        assert_eq!(parse_key("RightOption"), Ok(RALT));
        assert_eq!(parse_key("Win"), Ok(WIN), "the web view's Meta name still parses");
        assert!(parse_chord(&["Option".into(), "RightOption".into()]).is_err());
        assert!(conflicts(&[WIN, 0x56])[0].contains("Paste"));
    }

    #[test]
    fn pack_roundtrip() {
        assert_eq!(unpack(pack([1, 2, 3])), [1, 2, 3]);
    }

    #[cfg(not(target_os = "macos"))] // Windows defaults and names (Ctrl + Win)
    #[test]
    fn conflict_warnings() {
        let w = |n: &[&str]| conflicts(&parse_chord(&n.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap());
        assert!(w(&["Win", "L"])[0].contains("locks"));
        assert!(w(&["Ctrl", "V"])[0].contains("Paste"));
        assert!(w(&["Ctrl", "C"])[0].contains("Copy"));
        assert!(w(&["Ctrl", "X"])[0].contains("Cut"));
        assert!(w(&["Ctrl", "Z"])[0].contains("Undo"));
        assert!(w(&["Alt", "Tab"])[0].contains("Alt + Tab"));
        assert!(w(&["Ctrl", "Alt", "Delete"])[0].contains("security"));
        assert!(w(&["K"])[0].contains("single plain key"));
        assert!(w(&["Ctrl", "Win"]).is_empty());
        assert!(w(&["F9"]).is_empty());
    }
}
