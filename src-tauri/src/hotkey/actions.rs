//! Named hotkey actions, their defaults, cross-action conflict checks, and the multi-chord
//! state machine the hook drives. Pure logic: no OS calls, no locks, no allocation per key.

use super::keys::{self, normalize, unpack, ChordEvent, ChordTracker, CONFIG, ESC, N_ACTIONS, WIN};
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionId {
    Dictate = 0,
    HandsFree = 1,
    Cancel = 2,
    PasteLast = 3,
    TogglePause = 4,
    /// Edit the selection by voice. When its chord extends the dictate chord (default
    /// Ctrl+Win+Alt), the extra key turns a held dictation into a command instead of cancelling.
    CommandMode = 5,
}

pub const ALL: [ActionId; N_ACTIONS] =
    [ActionId::Dictate, ActionId::HandsFree, ActionId::Cancel, ActionId::PasteLast, ActionId::TogglePause, ActionId::CommandMode];

impl ActionId {
    pub fn name(self) -> &'static str {
        match self {
            Self::Dictate => "dictate",
            Self::HandsFree => "hands_free",
            Self::Cancel => "cancel",
            Self::PasteLast => "paste_last",
            Self::TogglePause => "toggle_pause",
            Self::CommandMode => "command_mode",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Dictate => "Dictate (hold to talk)",
            Self::HandsFree => "Hands-free mode",
            Self::Cancel => "Cancel dictation",
            Self::PasteLast => "Paste last dictation",
            Self::TogglePause => "Pause / resume Yappaflow",
            Self::CommandMode => "Command mode",
        }
    }
    pub fn parse(s: &str) -> Result<Self, String> {
        ALL.iter().copied().find(|a| a.name() == s).ok_or_else(|| format!("Unknown hotkey action \"{s}\""))
    }
    /// Empty = disabled by default.
    pub fn default_codes(self) -> Vec<u8> {
        match self {
            Self::Dictate => keys::DEFAULT_DICTATE.into_iter().filter(|c| *c != 0).collect(),
            Self::Cancel => vec![ESC],
            Self::CommandMode => keys::DEFAULT_COMMAND.into_iter().filter(|c| *c != 0).collect(),
            _ => vec![],
        }
    }
    pub fn idx(self) -> usize {
        self as usize
    }
}

/// Resolve the saved chord for an action; invalid or missing values fall back to the default.
/// `dictate` is the legacy `Settings.hotkey`; the rest come from `Settings.hotkeys`.
pub fn resolve(a: ActionId, dictate: &[String], others: &BTreeMap<String, Vec<String>>) -> Vec<u8> {
    let names = if a == ActionId::Dictate { Some(dictate) } else { others.get(a.name()).map(|v| v.as_slice()) };
    match names {
        Some(n) if n.is_empty() && a != ActionId::Dictate => vec![],
        Some(n) => keys::parse_chord_opt(n, a == ActionId::Cancel).unwrap_or_else(|_| a.default_codes()),
        None => a.default_codes(),
    }
}

/// Warnings for `codes` as the chord of `me`, given everyone else's resolved chords.
pub fn action_conflicts(me: ActionId, codes: &[u8], all: &[(ActionId, Vec<u8>)]) -> Vec<String> {
    let mut w = if me == ActionId::Cancel { vec![] } else { keys::conflicts(codes) };
    if codes.is_empty() {
        return w;
    }
    for (other, oc) in all {
        if *other == me || oc.is_empty() {
            continue;
        }
        let sub = codes.iter().all(|c| oc.contains(c));
        let sup = oc.iter().all(|c| codes.contains(c));
        let (disp, lbl) = (keys::chord_display(oc), other.label());
        // Command extending dictate is the intended design (hold dictate, add a key), not a clash.
        let pair = |a: ActionId, b: ActionId| me == a && *other == b;
        if !(sub && sup)
            && ((pair(ActionId::CommandMode, ActionId::Dictate) && sup) || (pair(ActionId::Dictate, ActionId::CommandMode) && sub))
        {
            continue;
        }
        if sub && sup {
            w.push(format!("Already used by \"{lbl}\" ({disp})."));
        } else if sub || sup {
            w.push(format!("Overlaps \"{lbl}\" ({disp}): one chord contains the other, so both would fire together."));
        }
    }
    w
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    HandsFree,
    PasteLast,
    TogglePause,
    CommandMode,
}

/// What the hook thread sends to the forwarder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    Chord(ChordEvent),
    Action(Action),
}

impl HookEvent {
    /// Non-zero single-byte encoding for the lock-free event ring.
    pub fn code(self) -> u8 {
        match self {
            Self::Chord(ChordEvent::Down) => 1,
            Self::Chord(ChordEvent::Up) => 2,
            Self::Chord(ChordEvent::Other) => 3,
            Self::Chord(ChordEvent::Escape) => 4,
            Self::Action(Action::HandsFree) => 5,
            Self::Action(Action::PasteLast) => 6,
            Self::Action(Action::TogglePause) => 7,
            Self::Action(Action::CommandMode) => 8,
        }
    }
    pub fn from_code(c: u8) -> Option<Self> {
        Some(match c {
            1 => Self::Chord(ChordEvent::Down),
            2 => Self::Chord(ChordEvent::Up),
            3 => Self::Chord(ChordEvent::Other),
            4 => Self::Chord(ChordEvent::Escape),
            5 => Self::Action(Action::HandsFree),
            6 => Self::Action(Action::PasteLast),
            7 => Self::Action(Action::TogglePause),
            8 => Self::Action(Action::CommandMode),
            _ => return None,
        })
    }
}

/// Fire-on-press chord for every action except dictate.
#[derive(Debug, Default, Clone, Copy)]
struct Slot {
    chord: [u8; 3],
    held: u8,
    active: bool,
    win_used: bool,
    /// Alt took part in a fired chord: mask its release so menu bars / Office KeyTips stay shut.
    alt_used: bool,
}

impl Slot {
    fn new(p: u32) -> Self {
        Self { chord: unpack(p), ..Default::default() }
    }
    fn len(&self) -> usize {
        self.chord.iter().filter(|c| **c != 0).count()
    }
    /// -> (fired, mask_win_release)
    fn on_key(&mut self, code: u8, down: bool) -> (bool, bool) {
        let n = self.len();
        let Some(i) = (if code == 0 || n == 0 { None } else { self.chord.iter().position(|c| *c == code) }) else {
            return (false, false);
        };
        if down {
            self.held |= 1 << i;
            if self.held == ((1u16 << n) - 1) as u8 && !self.active {
                self.active = true;
                self.win_used = self.chord.contains(&WIN);
                self.alt_used = self.chord.contains(&keys::ALT);
                return (true, false);
            }
            (false, false)
        } else {
            self.held &= !(1 << i);
            self.active = false;
            let m = (code == WIN && self.win_used) || (code == keys::ALT && self.alt_used);
            if code == WIN {
                self.win_used = false;
            }
            if code == keys::ALT {
                self.alt_used = false;
            }
            (false, m)
        }
    }
}

#[derive(Debug, Default)]
pub struct HookOut {
    pub events: [Option<HookEvent>; N_ACTIONS + 2],
    pub mask_before: bool,
    /// Do not pass this key event on to the focused app.
    pub swallow: bool,
}

pub struct Hooks {
    cfg: [u32; N_ACTIONS],
    dictate: ChordTracker,
    /// Index = ActionId - 1.
    slots: [Slot; N_ACTIONS - 1],
    /// Cancel key is physically down / its down was swallowed (so its up must be too).
    cancel_down: bool,
    cancel_swallowed: bool,
}

impl Hooks {
    pub fn new() -> Self {
        let mut cfg = [0u32; N_ACTIONS];
        for (i, c) in cfg.iter_mut().enumerate() {
            *c = CONFIG[i].load(Ordering::Relaxed);
        }
        let mut slots = [Slot::default(); N_ACTIONS - 1];
        for (i, s) in slots.iter_mut().enumerate() {
            *s = Slot::new(cfg[i + 1]);
        }
        Self { cfg, dictate: ChordTracker::new(cfg[0]), slots, cancel_down: false, cancel_swallowed: false }
    }

    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn on_vk(&mut self, vk: u32, down: bool, session_active: bool) -> HookOut {
        self.on_code(normalize(vk), down, session_active)
    }

    /// Like `on_vk` for an already-normalised code (the macOS tap maps its own keycodes).
    pub fn on_code(&mut self, code: u8, down: bool, session_active: bool) -> HookOut {
        let mut new = self.cfg;
        let mut changed = false;
        for (i, n) in new.iter_mut().enumerate() {
            *n = CONFIG[i].load(Ordering::Relaxed);
            changed |= *n != self.cfg[i];
        }
        let mut pre = if changed { self.apply_cfg(new) } else { HookOut::default() };
        let out = self.on_key(code, down, session_active);
        // Flushed events (at most 1) go first.
        let n = pre.events.iter().flatten().count();
        for (slot, e) in pre.events[n..].iter_mut().zip(out.events.iter().flatten()) {
            *slot = Some(*e);
        }
        pre.mask_before |= out.mask_before;
        pre.swallow = out.swallow;
        pre
    }

    /// Swap in new chords. An in-flight chord is closed out first (Up event, Win mask) so the
    /// session never waits for a release the new tracker can't see.
    pub fn apply_cfg(&mut self, new: [u32; N_ACTIONS]) -> HookOut {
        let mut out = HookOut::default();
        for (i, p) in new.iter().enumerate() {
            if *p == self.cfg[i] {
                continue;
            }
            self.cfg[i] = *p;
            if i == 0 {
                let f = self.dictate.flush();
                out.mask_before |= f.mask_before;
                if let Some(e) = f.event {
                    out.events[0] = Some(HookEvent::Chord(e));
                }
                self.dictate = ChordTracker::new(*p);
            } else {
                out.mask_before |= self.slots[i - 1].win_used || self.slots[i - 1].alt_used;
                self.slots[i - 1] = Slot::new(*p);
            }
        }
        out
    }

    pub fn on_key(&mut self, code: u8, down: bool, session_active: bool) -> HookOut {
        let mut out = HookOut::default();
        let mut n = 0;
        let cancel = self.slots[ActionId::Cancel.idx() - 1];
        let is_cancel_key = cancel.chord.contains(&code) && code != 0;
        // The extra key of a command chord that contains the whole dictate chord switches the
        // held dictation to Command (its slot fires below) instead of cancelling it.
        let cmd = self.slots[ActionId::CommandMode.idx() - 1].chord;
        let extends = code != 0 && cmd.contains(&code) && unpack(self.cfg[0]).iter().all(|c| *c == 0 || cmd.contains(c));

        let d = self.dictate.on_key(code, down);
        out.mask_before = d.mask_before;
        match d.event {
            // The cancel key itself reports as Escape below, not as "other shortcut".
            Some(ChordEvent::Other) if is_cancel_key || extends => {}
            Some(e) => {
                out.events[n] = Some(HookEvent::Chord(e));
                n += 1;
            }
            None => {}
        }
        for a in &ALL[1..] {
            let (fired, mask) = self.slots[a.idx() - 1].on_key(code, down);
            out.mask_before |= mask;
            if fired {
                out.events[n] = Some(match a {
                    ActionId::Cancel => HookEvent::Chord(ChordEvent::Escape),
                    ActionId::HandsFree => HookEvent::Action(Action::HandsFree),
                    ActionId::PasteLast => HookEvent::Action(Action::PasteLast),
                    ActionId::TogglePause => HookEvent::Action(Action::TogglePause),
                    _ => HookEvent::Action(Action::CommandMode),
                });
                n += 1;
            }
        }
        // A single-key cancel chord is swallowed while a dictation is active, so Esc doesn't
        // also reach the app underneath.
        // Decided on the FIRST down only (auto-repeats follow it), and the up always matches.
        if cancel.len() == 1 && is_cancel_key {
            if down {
                if !self.cancel_down {
                    self.cancel_down = true;
                    self.cancel_swallowed = session_active;
                }
                out.swallow = self.cancel_swallowed;
            } else {
                out.swallow = self.cancel_swallowed;
                self.cancel_down = false;
                self.cancel_swallowed = false;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keys::{ALT, CTRL};

    fn p(c: &[u8]) -> u32 {
        let mut a = [0u8; 3];
        a[..c.len()].copy_from_slice(c);
        keys::pack(a)
    }
    /// Hooks with an explicit config (bypasses the global atomics).
    fn hooks(cfg: [&[u8]; N_ACTIONS]) -> Hooks {
        let mut h = Hooks::new();
        for (c, s) in h.cfg.iter_mut().zip(cfg) {
            *c = p(s);
        }
        h.dictate = ChordTracker::new(h.cfg[0]);
        for i in 1..N_ACTIONS {
            h.slots[i - 1] = Slot::new(h.cfg[i]);
        }
        h
    }
    fn defaults() -> Hooks {
        hooks([&[CTRL, WIN], &[], &[ESC], &[], &[], &[]])
    }
    fn evs(o: &HookOut) -> Vec<HookEvent> {
        o.events.iter().flatten().copied().collect()
    }

    #[test]
    fn default_esc_cancels_and_is_swallowed_only_when_active() {
        let mut h = defaults();
        let o = h.on_key(ESC, true, false);
        assert_eq!(evs(&o), vec![HookEvent::Chord(ChordEvent::Escape)]);
        assert!(!o.swallow);
        h.on_key(ESC, false, false);
        let o = h.on_key(ESC, true, true);
        assert!(o.swallow);
        assert!(h.on_key(ESC, false, false).swallow, "matching key-up swallowed too");
        assert!(!h.on_key(ESC, false, false).swallow);
    }

    #[test]
    fn swallow_decided_on_first_down_so_no_stuck_key() {
        let mut h = defaults();
        // Down passed while idle; session starts mid-hold: repeat and up must pass too.
        assert!(!h.on_key(ESC, true, false).swallow);
        assert!(!h.on_key(ESC, true, true).swallow, "auto-repeat follows the first down");
        assert!(!h.on_key(ESC, false, true).swallow, "up passes because down passed");
        // Down swallowed while active: repeats and up swallowed even if session ended.
        assert!(h.on_key(ESC, true, true).swallow);
        assert!(h.on_key(ESC, true, false).swallow);
        assert!(h.on_key(ESC, false, false).swallow);
        assert!(!h.on_key(ESC, false, false).swallow);
    }

    #[test]
    fn config_change_mid_hold_flushes_up_and_masks_win() {
        let mut h = defaults();
        h.on_key(CTRL, true, false);
        h.on_key(WIN, true, false);
        let mut cfg = h.cfg;
        cfg[0] = p(&[0x78]);
        let o = h.apply_cfg(cfg);
        assert_eq!(evs(&o), vec![HookEvent::Chord(ChordEvent::Up)]);
        assert!(o.mask_before);
        // idle replacement emits nothing
        let mut cfg2 = h.cfg;
        cfg2[0] = p(&[CTRL, WIN]);
        let o = h.apply_cfg(cfg2);
        assert!(evs(&o).is_empty() && !o.mask_before);
        // a held secondary Win chord is masked when replaced
        let mut h = hooks([&[0x78], &[], &[ESC], &[WIN, 0x56], &[], &[]]);
        h.on_key(WIN, true, false);
        h.on_key(0x56, true, false);
        let mut cfg3 = h.cfg;
        cfg3[3] = 0;
        assert!(h.apply_cfg(cfg3).mask_before);
    }

    #[test]
    fn cancel_single_modifier_rejected() {
        for m in ["Ctrl", "Alt", "Shift", "Win"] {
            assert!(keys::parse_chord_opt(&[m.to_string()], true).is_err());
        }
        assert!(keys::parse_chord_opt(&["Ctrl".into(), "X".into()], true).is_ok());
        assert!(keys::parse_chord_opt(&["F8".into()], true).is_ok());
    }

    #[test]
    fn event_codes_roundtrip() {
        for c in 1..=8u8 {
            assert_eq!(HookEvent::from_code(c).unwrap().code(), c);
        }
        assert!(HookEvent::from_code(0).is_none() && HookEvent::from_code(9).is_none());
    }

    #[test]
    fn esc_during_chord_is_one_escape_not_other() {
        let mut h = defaults();
        h.on_key(CTRL, true, false);
        h.on_key(WIN, true, false);
        assert_eq!(evs(&h.on_key(ESC, true, true)), vec![HookEvent::Chord(ChordEvent::Escape)]);
    }

    #[test]
    fn cancel_rebound_to_chord_and_esc_becomes_ordinary() {
        let mut h = hooks([&[CTRL, WIN], &[], &[CTRL, 0x58], &[], &[], &[]]);
        assert!(evs(&h.on_key(ESC, true, true)).is_empty());
        assert!(!h.on_key(ESC, true, true).swallow);
        h.on_key(CTRL, true, true);
        assert_eq!(evs(&h.on_key(0x58, true, true)), vec![HookEvent::Chord(ChordEvent::Escape)]);
    }

    #[test]
    fn cancel_disabled_never_fires() {
        let mut h = hooks([&[CTRL, WIN], &[], &[], &[], &[], &[]]);
        assert!(evs(&h.on_key(ESC, true, false)).is_empty());
    }

    #[test]
    fn dictate_still_works_and_other_actions_dont_leak() {
        let mut h = defaults();
        h.on_key(CTRL, true, false);
        assert_eq!(evs(&h.on_key(WIN, true, false)), vec![HookEvent::Chord(ChordEvent::Down)]);
        let o = h.on_key(WIN, false, false);
        assert!(o.mask_before);
        assert_eq!(evs(&o), vec![HookEvent::Chord(ChordEvent::Up)]);
    }

    #[test]
    fn optional_action_fires_once_per_press_and_rearms() {
        let mut h = hooks([&[CTRL, WIN], &[], &[ESC], &[CTRL, ALT, 0x56], &[0x7B], &[]]);
        h.on_key(CTRL, true, false);
        h.on_key(0x56, true, false);
        assert_eq!(evs(&h.on_key(ALT, true, false)), vec![HookEvent::Action(Action::PasteLast)]);
        assert!(evs(&h.on_key(ALT, true, false)).is_empty(), "auto-repeat");
        h.on_key(ALT, false, false);
        assert_eq!(evs(&h.on_key(ALT, true, false)), vec![HookEvent::Action(Action::PasteLast)]);
        assert_eq!(evs(&h.on_key(0x7B, true, false)), vec![HookEvent::Action(Action::TogglePause)]);
    }

    #[test]
    fn hands_free_own_chord() {
        let mut h = hooks([&[CTRL, WIN], &[CTRL, ALT], &[ESC], &[], &[], &[]]);
        h.on_key(CTRL, true, false);
        assert_eq!(evs(&h.on_key(ALT, true, false)), vec![HookEvent::Action(Action::HandsFree)]);
    }

    #[test]
    fn alt_while_dictating_switches_to_command_not_cancel() {
        let mut h = hooks([&[CTRL, WIN], &[], &[ESC], &[], &[], &[CTRL, ALT, WIN]]);
        h.on_key(CTRL, true, false);
        assert_eq!(evs(&h.on_key(WIN, true, false)), vec![HookEvent::Chord(ChordEvent::Down)]);
        assert_eq!(evs(&h.on_key(ALT, true, true)), vec![HookEvent::Action(Action::CommandMode)]);
        assert!(evs(&h.on_key(ALT, true, true)).is_empty(), "auto-repeat");
        let up = h.on_key(ALT, false, true);
        assert!(evs(&up).is_empty());
        assert!(up.mask_before, "Alt release masked so menus/KeyTips don't open");
        assert_eq!(evs(&h.on_key(CTRL, false, true)), vec![HookEvent::Chord(ChordEvent::Up)]);
        // A key outside the command chord still cancels.
        h.on_key(CTRL, true, false);
        assert_eq!(evs(&h.on_key(0x41, true, true)), vec![HookEvent::Chord(ChordEvent::Other)]);
    }

    #[test]
    fn all_three_at_once_gives_down_then_command() {
        let mut h = hooks([&[CTRL, WIN], &[], &[ESC], &[], &[], &[CTRL, ALT, WIN]]);
        h.on_key(CTRL, true, false);
        h.on_key(ALT, true, false);
        assert_eq!(evs(&h.on_key(WIN, true, false)), vec![HookEvent::Chord(ChordEvent::Down), HookEvent::Action(Action::CommandMode)]);
    }

    #[test]
    fn non_superset_command_chord_is_its_own_press_and_alt_cancels_again() {
        let mut h = hooks([&[CTRL, WIN], &[], &[ESC], &[], &[], &[0x77]]);
        assert_eq!(evs(&h.on_key(0x77, true, false)), vec![HookEvent::Action(Action::CommandMode)]);
        h.on_key(0x77, false, true);
        h.on_key(CTRL, true, true);
        h.on_key(WIN, true, true);
        assert_eq!(evs(&h.on_key(ALT, true, true)), vec![HookEvent::Chord(ChordEvent::Other)]);
    }

    #[test]
    fn hands_free_shortcut_fires_its_action() {
        let mut h = hooks([&[CTRL, WIN], &[0x78], &[ESC], &[], &[], &[CTRL, ALT, WIN]]);
        assert_eq!(evs(&h.on_key(0x78, true, false)), vec![HookEvent::Action(Action::HandsFree)]);
        h.on_key(0x78, false, true);
        // With no hands-free chord bound (double-tap mode), nothing but the dictate chord fires.
        let mut h = hooks([&[CTRL, WIN], &[], &[ESC], &[], &[], &[CTRL, ALT, WIN]]);
        assert!(evs(&h.on_key(0x78, true, false)).is_empty());
    }

    #[cfg(not(target_os = "macos"))] // Windows defaults and names (Ctrl + Win)
    #[test]
    fn command_extending_dictate_is_not_a_conflict() {
        let all = vec![(ActionId::Dictate, vec![CTRL, WIN]), (ActionId::CommandMode, vec![CTRL, ALT, WIN])];
        assert!(action_conflicts(ActionId::CommandMode, &[CTRL, ALT, WIN], &all).is_empty());
        assert!(action_conflicts(ActionId::Dictate, &[CTRL, WIN], &all).is_empty());
        assert!(!action_conflicts(ActionId::CommandMode, &[CTRL, WIN], &all).is_empty(), "identical still clashes");
        assert_eq!(ActionId::CommandMode.default_codes(), vec![CTRL, ALT, WIN]);
    }

    #[test]
    fn win_based_secondary_chord_masks_start_menu() {
        let mut h = hooks([&[0x78], &[], &[ESC], &[WIN, 0x56], &[], &[]]);
        h.on_key(WIN, true, false);
        h.on_key(0x56, true, false);
        assert!(h.on_key(WIN, false, false).mask_before);
    }

    #[cfg(not(target_os = "macos"))] // Windows defaults and names (Ctrl + Win)
    #[test]
    fn resolve_defaults_and_fallbacks() {
        let none = BTreeMap::new();
        let d: Vec<String> = vec!["Ctrl".into(), "Win".into()];
        assert_eq!(resolve(ActionId::Dictate, &d, &none), vec![CTRL, WIN]);
        assert_eq!(resolve(ActionId::Cancel, &d, &none), vec![ESC]);
        assert!(resolve(ActionId::PasteLast, &d, &none).is_empty());
        assert_eq!(resolve(ActionId::Dictate, &["bogus".into()], &none), vec![CTRL, WIN], "invalid -> default");
        let mut m = BTreeMap::new();
        m.insert("cancel".to_string(), Vec::<String>::new());
        m.insert("paste_last".to_string(), vec!["f12".to_string()]);
        m.insert("toggle_pause".to_string(), vec!["nope".to_string()]);
        assert!(resolve(ActionId::Cancel, &d, &m).is_empty(), "explicit empty disables");
        assert_eq!(resolve(ActionId::PasteLast, &d, &m), vec![0x7B]);
        assert!(resolve(ActionId::TogglePause, &d, &m).is_empty());
        assert_eq!(resolve(ActionId::Dictate, &[], &none), vec![CTRL, WIN], "empty dictate -> default");
    }

    #[test]
    fn cross_action_conflicts() {
        let all = vec![(ActionId::Dictate, vec![CTRL, WIN]), (ActionId::Cancel, vec![ESC]), (ActionId::PasteLast, vec![])];
        let w = action_conflicts(ActionId::HandsFree, &[CTRL, WIN], &all);
        assert!(w.iter().any(|m| m.contains("Already used")));
        let w = action_conflicts(ActionId::HandsFree, &[CTRL, WIN, 0x20], &all);
        assert!(w.iter().any(|m| m.contains("Overlaps")), "superset");
        let w = action_conflicts(ActionId::PasteLast, &[CTRL], &all);
        assert!(w.iter().any(|m| m.contains("Overlaps")), "subset");
        let w = action_conflicts(ActionId::PasteLast, &[0x7B], &all);
        assert!(w.is_empty());
        assert!(action_conflicts(ActionId::Cancel, &[ESC], &all).is_empty(), "own chord ignored");
        assert!(action_conflicts(ActionId::PasteLast, &[], &all).is_empty());
    }

    #[test]
    fn action_names_roundtrip() {
        for a in ALL {
            assert_eq!(ActionId::parse(a.name()).unwrap(), a);
        }
        assert!(ActionId::parse("x").is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_defaults_are_fn_and_fn_ctrl() {
        assert_eq!(ActionId::Dictate.default_codes(), vec![keys::FN]);
        assert_eq!(ActionId::CommandMode.default_codes(), vec![CTRL, keys::FN]);
        let all = [(ActionId::Dictate, vec![keys::FN]), (ActionId::CommandMode, vec![CTRL, keys::FN])];
        assert!(action_conflicts(ActionId::CommandMode, &all[1].1, &all).is_empty(), "command extends dictate");
    }
}
