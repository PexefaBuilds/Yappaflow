//! Pure dictation session state machine. No OS calls, fully unit-tested.
//!
//! Inputs come from the hotkey hook (chord down/up, Esc, other key) plus a periodic
//! `Tick` from the controller loop. Outputs are `Action`s the controller executes.

use std::time::{Duration, Instant};

/// A press shorter than this counts as a "tap" (candidate for double-tap).
pub const TAP_MAX: Duration = Duration::from_millis(300);
/// The second tap must start within this window after the first tap's release.
pub const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(350);
/// Safety stop for runaway hands-free sessions.
pub const MAX_RECORDING: Duration = Duration::from_secs(6 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Hold,
    HandsFree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Recording {
        mode: Mode,
        since: Instant,
    },
    /// Chord was released quickly; mic keeps running while we wait for a second tap.
    PendingTap {
        started: Instant,
        released_at: Instant,
    },
    Processing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    ChordDown,
    ChordUp,
    Escape,
    OtherKeyDuringChord,
    /// The hands-free hotkey (only bound when double-tap is off): start, or stop and paste.
    HandsFreeKey,
    /// The command-mode hotkey. While the dictate chord is held it marks the take as a command;
    /// from idle it starts a hands-free command recording.
    CommandKey,
    Tick,
    ProcessingDone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    StartRecording,
    /// Discard the current recording and immediately start a fresh one.
    RestartRecording,
    EnteredHandsFree,
    /// The current recording is now a command (pill turns violet).
    EnteredCommand,
    StopAndProcess,
    Cancel,
}

#[derive(Debug)]
pub struct Session {
    state: State,
    /// This take is a command-mode edit, not a dictation.
    command: bool,
    /// Double-tap of the dictate chord enters hands-free. Off = the user bound a hands-free hotkey,
    /// so a quick tap is just a short dictation.
    pub double_tap: bool,
    /// The chord press that entered hands-free (second tap) is still held.
    second_tap_held: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self { state: State::Idle, command: false, double_tap: true, second_tap_held: false }
    }
}

impl Session {
    pub fn state(&self) -> State {
        self.state
    }

    /// True whenever Esc should be captured (we are recording or processing).
    pub fn is_active(&self) -> bool {
        !matches!(self.state, State::Idle)
    }

    pub fn reset(&mut self) {
        self.state = State::Idle;
    }

    pub fn is_command(&self) -> bool {
        self.command
    }

    fn record(&mut self, mode: Mode, since: Instant, command: bool) {
        self.state = State::Recording { mode, since };
        self.command = command;
        self.second_tap_held = false;
    }

    pub fn handle(&mut self, input: Input, now: Instant) -> Action {
        use Action as A;
        use Input as I;
        match (self.state, input) {
            // ── Idle ──────────────────────────────────────────────
            (State::Idle, I::ChordDown) => {
                self.record(Mode::Hold, now, false);
                A::StartRecording
            }
            (State::Idle, I::HandsFreeKey) => {
                self.record(Mode::HandsFree, now, false);
                A::StartRecording
            }
            (State::Idle, I::CommandKey) => {
                self.record(Mode::HandsFree, now, true);
                A::StartRecording
            }
            (State::Idle, _) => A::None,

            // ── Recording ─────────────────────────────────────────
            (State::Recording { mode: Mode::Hold, since }, I::ChordUp) => {
                // Commands never wait for a second tap: a short one just ends up empty.
                if self.command || !self.double_tap || now.duration_since(since) >= TAP_MAX {
                    self.state = State::Processing;
                    A::StopAndProcess
                } else {
                    self.state = State::PendingTap { started: since, released_at: now };
                    A::None
                }
            }
            (State::Recording { mode: Mode::Hold, .. }, I::CommandKey) => {
                if self.command {
                    return A::None;
                }
                self.command = true;
                A::EnteredCommand
            }
            (State::Recording { mode: Mode::Hold, since }, I::HandsFreeKey) => {
                self.state = State::Recording { mode: Mode::HandsFree, since };
                A::EnteredHandsFree
            }
            // Ctrl+Win+Alt pressed right after a quick tap: the second press is a held command, not hands-free.
            (State::Recording { mode: Mode::HandsFree, since }, I::CommandKey) if self.second_tap_held && !self.command => {
                self.second_tap_held = false;
                self.state = State::Recording { mode: Mode::Hold, since };
                self.command = true;
                A::EnteredCommand
            }
            (State::Recording { mode: Mode::HandsFree, .. }, I::ChordUp) => {
                self.second_tap_held = false;
                A::None
            }
            (State::Recording { mode: Mode::HandsFree, .. }, I::ChordDown | I::HandsFreeKey | I::CommandKey) => {
                self.state = State::Processing;
                A::StopAndProcess
            }
            (State::Recording { .. }, I::Escape | I::OtherKeyDuringChord) => {
                self.state = State::Idle;
                A::Cancel
            }
            (State::Recording { since, .. }, I::Tick) => {
                if now.duration_since(since) >= MAX_RECORDING {
                    self.state = State::Processing;
                    A::StopAndProcess
                } else {
                    A::None
                }
            }
            (State::Recording { .. }, _) => A::None,

            // ── PendingTap ────────────────────────────────────────
            (State::PendingTap { started, released_at }, I::ChordDown) => {
                if now.duration_since(released_at) <= DOUBLE_TAP_WINDOW {
                    // Keep the audio from the first tap: the user may already be talking.
                    self.state = State::Recording { mode: Mode::HandsFree, since: started };
                    self.second_tap_held = true;
                    A::EnteredHandsFree
                } else {
                    self.record(Mode::Hold, now, false);
                    A::RestartRecording
                }
            }
            (State::PendingTap { started, .. }, I::HandsFreeKey) => {
                self.state = State::Recording { mode: Mode::HandsFree, since: started };
                A::EnteredHandsFree
            }
            (State::PendingTap { .. }, I::CommandKey) => {
                self.record(Mode::HandsFree, now, true);
                A::RestartRecording
            }
            (State::PendingTap { released_at, .. }, I::Tick) => {
                if now.duration_since(released_at) > DOUBLE_TAP_WINDOW {
                    self.state = State::Idle;
                    A::Cancel
                } else {
                    A::None
                }
            }
            (State::PendingTap { .. }, I::Escape | I::OtherKeyDuringChord) => {
                self.state = State::Idle;
                A::Cancel
            }
            (State::PendingTap { .. }, _) => A::None,

            // ── Processing ────────────────────────────────────────
            (State::Processing, I::ProcessingDone) => {
                self.state = State::Idle;
                A::None
            }
            (State::Processing, I::Escape) => {
                self.state = State::Idle;
                A::Cancel
            }
            (State::Processing, _) => A::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn hold_to_talk() {
        let t0 = Instant::now();
        let mut s = Session::default();
        assert_eq!(s.handle(Input::ChordDown, t0), Action::StartRecording);
        assert_eq!(s.handle(Input::Tick, t0 + ms(500)), Action::None);
        assert_eq!(s.handle(Input::ChordUp, t0 + ms(1500)), Action::StopAndProcess);
        assert_eq!(s.state(), State::Processing);
        assert_eq!(s.handle(Input::ChordDown, t0 + ms(1600)), Action::None, "ignored while processing");
        assert_eq!(s.handle(Input::ProcessingDone, t0 + ms(2000)), Action::None);
        assert_eq!(s.state(), State::Idle);
    }

    #[test]
    fn single_short_tap_is_discarded() {
        let t0 = Instant::now();
        let mut s = Session::default();
        s.handle(Input::ChordDown, t0);
        assert_eq!(s.handle(Input::ChordUp, t0 + ms(120)), Action::None);
        assert!(matches!(s.state(), State::PendingTap { .. }));
        assert_eq!(s.handle(Input::Tick, t0 + ms(300)), Action::None);
        assert_eq!(s.handle(Input::Tick, t0 + ms(120 + 351)), Action::Cancel);
        assert_eq!(s.state(), State::Idle);
    }

    #[test]
    fn double_tap_enters_hands_free_and_tap_stops() {
        let t0 = Instant::now();
        let mut s = Session::default();
        s.handle(Input::ChordDown, t0);
        s.handle(Input::ChordUp, t0 + ms(100));
        assert_eq!(s.handle(Input::ChordDown, t0 + ms(250)), Action::EnteredHandsFree);
        assert_eq!(s.handle(Input::ChordUp, t0 + ms(330)), Action::None, "release ignored in hands-free");
        assert_eq!(s.handle(Input::Tick, t0 + ms(10_000)), Action::None);
        assert_eq!(s.handle(Input::ChordDown, t0 + ms(20_000)), Action::StopAndProcess);
        assert_eq!(s.handle(Input::ChordUp, t0 + ms(20_100)), Action::None);
    }

    #[test]
    fn late_second_press_restarts() {
        let t0 = Instant::now();
        let mut s = Session::default();
        s.handle(Input::ChordDown, t0);
        s.handle(Input::ChordUp, t0 + ms(100));
        // No tick arrived in time (e.g. busy thread), press comes late.
        assert_eq!(s.handle(Input::ChordDown, t0 + ms(100 + 500)), Action::RestartRecording);
        assert!(matches!(s.state(), State::Recording { mode: Mode::Hold, .. }));
    }

    #[test]
    fn escape_cancels_everywhere() {
        let t0 = Instant::now();
        let mut s = Session::default();
        assert_eq!(s.handle(Input::Escape, t0), Action::None, "idle Esc is not ours");
        s.handle(Input::ChordDown, t0);
        assert_eq!(s.handle(Input::Escape, t0 + ms(400)), Action::Cancel);
        assert_eq!(s.state(), State::Idle);

        s.handle(Input::ChordDown, t0);
        s.handle(Input::ChordUp, t0 + ms(800));
        assert_eq!(s.handle(Input::Escape, t0 + ms(900)), Action::Cancel, "cancel in-flight processing");
    }

    #[test]
    fn other_key_means_different_shortcut() {
        let t0 = Instant::now();
        let mut s = Session::default();
        s.handle(Input::ChordDown, t0);
        assert_eq!(s.handle(Input::OtherKeyDuringChord, t0 + ms(50)), Action::Cancel);
        assert_eq!(s.state(), State::Idle);
    }

    #[test]
    fn no_double_tap_quick_tap_is_a_short_dictation() {
        let t0 = Instant::now();
        let mut s = Session { double_tap: false, ..Default::default() };
        s.handle(Input::ChordDown, t0);
        assert_eq!(s.handle(Input::ChordUp, t0 + ms(100)), Action::StopAndProcess);
        s.handle(Input::ProcessingDone, t0 + ms(200));
        s.handle(Input::ChordDown, t0 + ms(300));
        s.handle(Input::ChordUp, t0 + ms(350));
        assert_eq!(s.handle(Input::ChordDown, t0 + ms(400)), Action::None, "no hands-free from a double tap");
    }

    #[test]
    fn hands_free_key_starts_and_either_key_stops() {
        let t0 = Instant::now();
        for stop in [Input::HandsFreeKey, Input::ChordDown] {
            let mut s = Session { double_tap: false, ..Default::default() };
            assert_eq!(s.handle(Input::HandsFreeKey, t0), Action::StartRecording);
            assert!(matches!(s.state(), State::Recording { mode: Mode::HandsFree, .. }));
            assert_eq!(s.handle(Input::ChordUp, t0 + ms(100)), Action::None);
            assert_eq!(s.handle(Input::Tick, t0 + ms(5000)), Action::None, "keeps listening");
            assert_eq!(s.handle(stop, t0 + ms(6000)), Action::StopAndProcess);
        }
        let mut s = Session { double_tap: false, ..Default::default() };
        s.handle(Input::HandsFreeKey, t0);
        assert_eq!(s.handle(Input::Escape, t0 + ms(900)), Action::Cancel, "Esc cancels hands-free");
        assert_eq!(s.state(), State::Idle);
    }

    #[test]
    fn double_tap_hands_free_escape_cancels() {
        let t0 = Instant::now();
        let mut s = Session::default();
        s.handle(Input::ChordDown, t0);
        s.handle(Input::ChordUp, t0 + ms(100));
        s.handle(Input::ChordDown, t0 + ms(200));
        assert_eq!(s.handle(Input::Escape, t0 + ms(3000)), Action::Cancel);
    }

    #[test]
    fn command_key_while_holding_marks_command_and_release_processes() {
        let t0 = Instant::now();
        let mut s = Session::default();
        s.handle(Input::ChordDown, t0);
        assert!(!s.is_command());
        assert_eq!(s.handle(Input::CommandKey, t0 + ms(20)), Action::EnteredCommand);
        assert_eq!(s.handle(Input::CommandKey, t0 + ms(30)), Action::None);
        assert!(s.is_command());
        assert_eq!(s.handle(Input::ChordUp, t0 + ms(150)), Action::StopAndProcess, "no double-tap wait for commands");
        s.handle(Input::ProcessingDone, t0 + ms(900));
        s.handle(Input::ChordDown, t0 + ms(1000));
        assert!(!s.is_command(), "next take is a plain dictation");
    }

    #[test]
    fn command_chord_right_after_a_quick_tap_is_a_command() {
        let t0 = Instant::now();
        let mut s = Session::default();
        s.handle(Input::ChordDown, t0);
        s.handle(Input::ChordUp, t0 + ms(100));
        assert_eq!(s.handle(Input::ChordDown, t0 + ms(250)), Action::EnteredHandsFree);
        assert_eq!(s.handle(Input::CommandKey, t0 + ms(250)), Action::EnteredCommand);
        assert!(s.is_command());
        assert_eq!(s.handle(Input::ChordUp, t0 + ms(2000)), Action::StopAndProcess, "release ends the command");
        // A real hands-free (second tap released) still stops on the command chord.
        let mut s = Session::default();
        s.handle(Input::ChordDown, t0);
        s.handle(Input::ChordUp, t0 + ms(100));
        s.handle(Input::ChordDown, t0 + ms(250));
        s.handle(Input::ChordUp, t0 + ms(330));
        assert_eq!(s.handle(Input::CommandKey, t0 + ms(5000)), Action::StopAndProcess);
        assert!(!s.is_command());
    }

    #[test]
    fn command_key_from_idle_is_hands_free_and_escape_cancels() {
        let t0 = Instant::now();
        let mut s = Session::default();
        assert_eq!(s.handle(Input::CommandKey, t0), Action::StartRecording);
        assert!(s.is_command());
        assert_eq!(s.handle(Input::ChordUp, t0 + ms(100)), Action::None);
        assert_eq!(s.handle(Input::CommandKey, t0 + ms(4000)), Action::StopAndProcess);
        let mut s = Session::default();
        s.handle(Input::CommandKey, t0);
        assert_eq!(s.handle(Input::ChordDown, t0 + ms(4000)), Action::StopAndProcess, "dictate chord stops it too");
        let mut s = Session::default();
        s.handle(Input::CommandKey, t0);
        assert_eq!(s.handle(Input::Escape, t0 + ms(500)), Action::Cancel);
    }

    #[test]
    fn hands_free_safety_stop() {
        let t0 = Instant::now();
        let mut s = Session::default();
        s.handle(Input::ChordDown, t0);
        s.handle(Input::ChordUp, t0 + ms(100));
        s.handle(Input::ChordDown, t0 + ms(200));
        assert_eq!(s.handle(Input::Tick, t0 + MAX_RECORDING + ms(1)), Action::StopAndProcess);
    }
}
