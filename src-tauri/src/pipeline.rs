//! The controller: owns the session state machine and the recorder, reacts to hotkey
//! events, and runs each dictation (resample → save → STT → cleanup → insert) as an
//! async task it can cancel.

use crate::audio::{self, resample, wav, Captured, Recorder};
use crate::cleanup;
use crate::context::{self, AppContext};
use crate::hotkey::{self, ChordEvent};
use crate::insert::{self, InsertOptions, Outcome};
use crate::providers::{registry, AudioClip, CleanupContext, SttOptions};
use crate::session::{Action, Input, Mode, Session, State};
use crate::settings::Settings;
use crate::snippets;
use crate::storage::{now_ms, DictationRow, NewDictation, TermRow, Timings};
use crate::usage;
use crate::AppState;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter};

pub enum CtlMsg {
    Chord(ChordEvent),
    /// The hands-free hotkey (or Tray › Hands-free).
    HandsFree,
    /// The command-mode hotkey.
    Command,
    ProcessingDone(u64),
    /// The pill's "Rerun" after a cancel.
    Rerun,
}

/// How long a cancelled recording can still be rerun (the pill shows it for 2.5 s).
const RERUN_WINDOW: Duration = Duration::from_secs(4);

/// The pill's "Paste now" while Polishing: stop waiting for cleanup and insert the raw text.
/// notify_waiters (not notify_one) so a late click can't skip the next dictation's cleanup.
pub static SKIP_CLEANUP: tokio::sync::Notify = tokio::sync::Notify::const_new();

#[derive(Serialize, Clone)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    Idle,
    /// `command`: this take edits the selection (violet pill).
    Listening {
        hands_free: bool,
        command: bool,
    },
    Processing,
    /// The LLM cleanup call is running (only when the skip gate lets it).
    Polishing,
    /// Command mode: the edit call is running; `instruction` is what the user said.
    Rewriting {
        instruction: String,
    },
    /// Hotkeys paused until this epoch ms (Tray › Pause).
    Paused {
        until_ms: i64,
    },
    Done {
        text: String,
        total_ms: u64,
        outcome: Outcome,
        /// Cleanup errored or ran out of time, so the raw transcript was inserted.
        cleanup_failed: bool,
        /// The user pressed "Paste now" on the pill, so the raw transcript was inserted.
        cleanup_skipped: bool,
    },
    /// Nothing audible was recorded; nothing sent.
    Empty,
    /// `can_rerun`: the audio was kept, so the pill offers Rerun for a moment.
    Cancelled {
        can_rerun: bool,
    },
    /// `row_id`: the saved recording, so the UI can offer Retry.
    Error {
        message: String,
        row_id: Option<i64>,
    },
}

pub fn emit_status(app: &AppHandle, s: Status) {
    crate::pill::on_status(app, &s);
    crate::toast::on_status(app, &s);
    let _ = app.emit("status", s);
}

pub fn spawn(app: AppHandle, state: Arc<AppState>, tx: Sender<CtlMsg>, rx: Receiver<CtlMsg>) -> Result<()> {
    let level_app = app.clone();
    let rec = Recorder::spawn(Arc::new(move |peak: f32| {
        let _ = level_app.emit("level", peak);
    }))?;
    std::thread::Builder::new().name("controller".into()).spawn(move || {
        let mut c = Controller { app, state, tx, rec, session: Session::default(), seq: 0, ctx: None, task: None, cancelled: None, in_flight: None };
        c.run(rx);
    })?;
    Ok(())
}

struct Controller {
    app: AppHandle,
    state: Arc<AppState>,
    tx: Sender<CtlMsg>,
    rec: Recorder,
    session: Session,
    /// Increments per dictation; stale ProcessingDone messages are ignored.
    seq: u64,
    ctx: Option<AppContext>,
    task: Option<JoinHandle<()>>,
    /// The last cancelled recording, its context and mode, kept briefly for Rerun.
    cancelled: Option<(Captured, AppContext, &'static str, Instant)>,
    /// A copy of the recording being processed, so a cancel mid-transcription can offer Rerun too.
    in_flight: Option<(Captured, AppContext, &'static str)>,
}

impl Controller {
    fn run(&mut self, rx: Receiver<CtlMsg>) {
        loop {
            self.session.double_tap = hotkey::HANDS_FREE_DOUBLE_TAP.load(std::sync::atomic::Ordering::Relaxed);
            let (input, now) = match rx.recv_timeout(Duration::from_millis(30)) {
                // Tray › Pause: ignore new starts only; Up/Escape/stop must still get through.
                Ok(CtlMsg::Chord(ChordEvent::Down) | CtlMsg::HandsFree | CtlMsg::Command)
                    if (crate::tray::paused() || crate::settings_cmds::capturing()) && !self.session.is_active() =>
                {
                    continue
                }
                Ok(CtlMsg::HandsFree) => (Input::HandsFreeKey, Instant::now()),
                Ok(CtlMsg::Command) => (Input::CommandKey, Instant::now()),
                Ok(CtlMsg::Chord(ev)) => (
                    match ev {
                        ChordEvent::Down => Input::ChordDown,
                        ChordEvent::Up => Input::ChordUp,
                        ChordEvent::Escape => Input::Escape,
                        ChordEvent::Other => Input::OtherKeyDuringChord,
                    },
                    Instant::now(),
                ),
                Ok(CtlMsg::Rerun) => (Input::Rerun, Instant::now()),
                Ok(CtlMsg::ProcessingDone(id)) => {
                    if id != self.seq {
                        continue;
                    }
                    self.task = None;
                    self.in_flight = None;
                    (Input::ProcessingDone, Instant::now())
                }
                Err(RecvTimeoutError::Timeout) => (Input::Tick, Instant::now()),
                Err(RecvTimeoutError::Disconnected) => break,
            };
            // Remember the mode before the transition: StopAndProcess leaves Recording.
            let mode = match self.session.state() {
                _ if self.session.is_command() => "command",
                State::Recording { mode: Mode::HandsFree, .. } => "hands_free",
                _ => "hold",
            };
            // A lone short tap that times out is an accident, not a user cancel: stay quiet.
            let quiet = input == Input::Tick && matches!(self.session.state(), State::PendingTap { .. });
            let action = self.session.handle(input, now);
            self.execute(action, mode, now, quiet);
            hotkey::set_session_active(self.session.is_active());
        }
    }

    fn execute(&mut self, action: Action, mode: &'static str, now: Instant, quiet: bool) {
        match action {
            Action::None => {}
            Action::StartRecording => self.start_recording(),
            Action::RestartRecording => {
                self.rec.cancel();
                self.start_recording();
            }
            Action::EnteredHandsFree | Action::EnteredCommand => self.emit_listening(),
            Action::Cancel => {
                // A user cancel keeps the audio for a moment, so Rerun can use it: the live
                // recording, or the copy of the one being transcribed.
                let kept = if quiet {
                    self.rec.cancel();
                    None
                } else if self.task.is_some() {
                    self.in_flight.take()
                } else {
                    let ctx = self.ctx.take().unwrap_or_default();
                    self.rec.stop().filter(|c| !c.samples.is_empty()).map(|c| (c, ctx, mode))
                };
                if let Some(t) = self.task.take() {
                    t.abort();
                }
                self.in_flight = None;
                self.seq += 1;
                let can_rerun = kept.is_some();
                self.cancelled = kept.map(|(c, ctx, mode)| (c, ctx, mode, now));
                emit_status(&self.app, if quiet { Status::Idle } else { Status::Cancelled { can_rerun } });
            }
            Action::Rerun => match self.cancelled.take().filter(|k| now.duration_since(k.3) < RERUN_WINDOW) {
                Some((cap, ctx, mode, _)) => self.process_captured(Some(cap), ctx, mode, now),
                None => {
                    self.session.reset();
                }
            },
            Action::StopAndProcess => {
                let captured = self.rec.stop();
                let ctx = self.ctx.take().unwrap_or_default();
                self.process_captured(captured, ctx, mode, now);
            }
        }
    }

    fn process_captured(&mut self, captured: Option<Captured>, ctx: AppContext, mode: &'static str, now: Instant) {
        self.in_flight = captured.as_ref().map(|c| (c.clone(), ctx.clone(), mode));
        self.seq += 1;
        let id = self.seq;
        let (app, state, tx) = (self.app.clone(), self.state.clone(), self.tx.clone());
        emit_status(&app, Status::Processing);
        self.task = Some(tauri::async_runtime::spawn(async move {
            // Sends ProcessingDone on every exit path (panic, abort included), so the
            // controller can never get stuck in Processing. Stale ids are ignored.
            let _done = DoneGuard(tx, id);
            match captured {
                Some(cap) => {
                    let run = if mode == "command" {
                        process_command(&app, &state, cap, ctx, now).await
                    } else {
                        process(&app, &state, cap, ctx, mode, now).await
                    };
                    if let Err(e) = run {
                        log::error!("dictation failed: {e:#}");
                        emit_status(&app, Status::Error { message: format!("{e:#}"), row_id: None });
                    }
                }
                None => emit_status(&app, Status::Error { message: "No audio captured".into(), row_id: None }),
            }
        }));
    }

    fn start_recording(&mut self) {
        self.cancelled = None;
        // Capture the target app first, before our UI could ever take focus.
        self.ctx = Some(context::foreground());
        let s = self.state.settings.read().unwrap().clone();
        let mic = Some(s.mic_device.clone()).filter(|m| !m.is_empty());
        if let Err(e) = self.rec.start(mic) {
            self.session.reset();
            emit_status(&self.app, Status::Error { message: format!("Mic error: {e:#}"), row_id: None });
            return;
        }
        self.emit_listening();
        // Warm the connection while the user talks.
        let stt = registry::build_stt(&s, &self.state.http);
        let clean = registry::build_cleanup(&s, &self.state.http);
        tauri::async_runtime::spawn(async move { stt.prewarm().await });
        tauri::async_runtime::spawn(async move { clean.prewarm().await });
        // Read after the mic is running, so a slow UI Automation call can't cost the first words.
        if s.use_cursor_context && !self.session.is_command() {
            if let (Some(ctx), Some(before)) = (self.ctx.as_mut(), context::text_before_cursor(cleanup::CURSOR_CHARS)) {
                ctx.before_cursor = cleanup::cursor_tail(&before);
            }
        }
    }

    fn emit_listening(&self) {
        let hands_free = matches!(self.session.state(), State::Recording { mode: Mode::HandsFree, .. });
        emit_status(&self.app, Status::Listening { hands_free, command: self.session.is_command() });
    }
}

struct DoneGuard(Sender<CtlMsg>, u64);
impl Drop for DoneGuard {
    fn drop(&mut self) {
        let _ = self.0.send(CtlMsg::ProcessingDone(self.1));
    }
}

/// The `language` setting is one code (empty = auto-detect); providers take a list.
/// `terms`: the dictionary, most-used first (providers cap the count).
fn stt_options(s: &Settings, terms: &[TermRow]) -> SttOptions {
    let lang = s.language.trim();
    let languages = if lang.is_empty() { Vec::new() } else { vec![lang.to_string()] };
    SttOptions { languages, keywords: terms.iter().map(|t| t.term.clone()).collect(), ..Default::default() }
}

fn hint_terms(state: &AppState) -> Vec<TermRow> {
    let mut terms = state.db.list_terms().unwrap_or_default();
    terms.sort_by_key(|t| std::cmp::Reverse(t.uses));
    terms
}

/// A "sounds like" phrase was heard, so the cleanup model must run to swap in the right spelling.
fn heard_sounds_like(terms: &[TermRow], text: &str) -> bool {
    let said = format!(" {} ", snippets::normalize(text));
    terms.iter().any(|t| {
        let sl = snippets::normalize(&t.sounds_like);
        !sl.is_empty() && said.contains(&format!(" {sl} "))
    })
}

/// Resample to 16 kHz, drop silent clips, encode. None = nothing audible.
fn prepare(cap: Captured, silence_dbfs: f32) -> Option<AudioClip> {
    let pcm = resample::to_16k(&cap.samples, cap.sample_rate);
    if !resample::has_speech(&pcm, silence_dbfs, 250) {
        return None;
    }
    let pcm16k = resample::to_i16(&pcm);
    let duration_ms = resample::duration_ms(pcm16k.len());
    let bytes = wav::encode(&pcm16k, resample::TARGET_RATE);
    Some(AudioClip { bytes, mime: "audio/wav", filename: "audio.wav", pcm16k, duration_ms })
}

async fn process(app: &AppHandle, state: &AppState, cap: Captured, ctx: AppContext, mode: &str, released: Instant) -> Result<()> {
    let s = state.settings.read().unwrap().clone();

    let Some(clip) = prepare(cap, s.silence_threshold_dbfs) else {
        emit_status(app, Status::Empty);
        return Ok(());
    };

    // Never lose a dictation: audio hits the disk before any network call.
    let path = state.recordings_dir.join(format!("{}.wav", now_ms()));
    std::fs::write(&path, &clip.bytes).context("saving recording")?;
    let row_id = state.db.insert_dictation(&NewDictation {
        app_exe: &ctx.exe,
        window_title: &ctx.title,
        mode,
        duration_ms: clip.duration_ms,
        audio_path: &path.to_string_lossy(),
        stt_provider: &s.stt_provider,
        stt_model: &s.stt_model,
    })?;
    let mut t = Timings { prepare_ms: released.elapsed().as_millis() as u64, ..Default::default() };

    // ── STT ──
    let stt = registry::build_stt(&s, &state.http);
    let terms = hint_terms(state);
    let opts = stt_options(&s, &terms);
    let t0 = Instant::now();
    let transcript = match stt.transcribe(&clip, &opts).await {
        Ok(tr) => tr,
        Err(e) => {
            let _ = state.db.fail_dictation(row_id, &format!("{e:#}"));
            let _ = app.emit("dictation-updated", row_id);
            log::error!("dictation #{row_id} failed: {e:#}");
            emit_status(app, Status::Error { message: format!("{e:#}"), row_id: Some(row_id) });
            return Ok(());
        }
    };
    t.stt_ms = t0.elapsed().as_millis() as u64;

    // ── Snippets: a trigger at the end (or alone) adds its text exactly; only the words before it get cleaned. ──
    let snips = state.db.list_snippets().unwrap_or_default();
    let hit = snippets::find(&transcript.text, &snips);
    let body = hit.as_ref().map_or(transcript.text.as_str(), |h| h.prefix.as_str());

    // ── Cleanup. Errors fall back to the raw text. ──
    // The limit grows with the dictation; the pill's "Paste now" skips the wait (SKIP_CLEANUP).
    let cleanup = registry::build_cleanup_with_timeout(&s, &state.http, cleanup::timeout_for(body));
    let tone = cleanup::resolve_style(&state.db.style_rules(), &ctx.exe, &ctx.title, &ctx.bundle_id);
    // Code editors and terminals (Literal) never get the text near the cursor.
    let before_cursor = if tone == "literal" { String::new() } else { ctx.before_cursor.clone() };
    let tone = format!("{tone}|{}", s.typing_style); // cleanup::system_prompt splits it
    let cctx = CleanupContext {
        before_cursor: before_cursor.clone(),
        app_exe: ctx.exe.clone(),
        window_title: ctx.title.clone(),
        tone,
        disabled_rules: s.disabled_rules.clone(),
        custom_rules: s.custom_rules.clone(),
        dictionary: terms
            .iter()
            .take(100)
            .map(|t| if t.sounds_like.is_empty() { t.term.clone() } else { format!("{} (sounds like \"{}\")", t.term, t.sounds_like) })
            .collect(),
    };
    let t1 = Instant::now();
    let skip =
        s.typing_style == "full" && s.custom_rules.is_empty() && !heard_sounds_like(&terms, body) && !cleanup::mid_sentence(&before_cursor);
    let (mut cleanup_failed, mut cleanup_skipped) = (false, false);
    let cleaned = if body.trim().is_empty() {
        String::new()
    } else if skip && cleanup::should_skip(body) {
        body.to_string()
    } else {
        let skipped = SKIP_CLEANUP.notified(); // registered before the pill can show its button
        emit_status(app, Status::Polishing);
        tokio::select! {
            r = cleanup.clean(body, &cctx) => match r {
                Ok(c) => {
                    let _ = state.db.set_cleanup_usage(row_id, &s.cleanup_provider, &s.cleanup_model, usage::clean_tokens(body, &cctx, &c));
                    c
                }
                Err(e) => {
                    log::warn!("cleanup failed, using raw transcript: {e:#}");
                    cleanup_failed = true;
                    body.to_string()
                }
            },
            _ = skipped => {
                log::info!("cleanup skipped from the pill");
                cleanup_skipped = true;
                body.to_string()
            }
        }
    };
    t.cleanup_ms = t1.elapsed().as_millis() as u64;
    let cleaned = cleanup::join_after(&before_cursor, &cleanup::strip_long_dashes(&cleaned));
    let final_text = match &hit {
        Some(h) => {
            state.db.bump_snippet(h.id);
            if cleaned.trim().is_empty() {
                h.expansion.to_string()
            } else {
                format!("{}\n{}", cleaned.trim_end(), h.expansion)
            }
        }
        None => cleaned,
    };

    if final_text.trim().is_empty() {
        state.db.finish_dictation(row_id, &transcript.text, "", &t, "empty")?;
        emit_status(app, Status::Empty);
        return Ok(());
    }

    // ── Insert ──
    let iopts = InsertOptions {
        mode: s.insert_mode,
        restore_clipboard: s.restore_clipboard,
        restore_delay: Duration::from_millis(s.restore_delay_ms),
    };
    let t2 = Instant::now();
    let text = final_text.clone();
    let outcome =
        tauri::async_runtime::spawn_blocking(move || insert::insert_text(&text, &iopts)).await.map_err(|e| anyhow!("insert task: {e}"))?;
    t.insert_ms = t2.elapsed().as_millis() as u64;
    // Text is visible ~when the paste keystroke lands, before the clipboard-restore wait.
    let restore_wait = if outcome == Outcome::Pasted && s.restore_clipboard { s.restore_delay_ms.min(t.insert_ms) } else { 0 };
    t.total_ms = (released.elapsed().as_millis() as u64).saturating_sub(restore_wait);

    let status = if matches!(outcome, Outcome::LeftOnClipboard(_)) { "clipboard" } else { "done" };
    state.db.finish_dictation(row_id, &transcript.text, &final_text, &t, status)?;
    log::info!(
        "dictation #{row_id} [{}] total {} ms (prep {} / stt {} / clean {} / insert {})",
        ctx.exe,
        t.total_ms,
        t.prepare_ms,
        t.stt_ms,
        t.cleanup_ms,
        t.insert_ms
    );
    state.db.bump_terms(&final_text);
    if !matches!(outcome, Outcome::LeftOnClipboard(_)) {
        crate::learn::watch(app.clone(), final_text.clone());
    }
    emit_status(app, Status::Done { text: final_text, total_ms: t.total_ms, outcome, cleanup_failed, cleanup_skipped });
    let _ = app.emit("dictation-updated", row_id);

    for p in state.db.take_old_audio(s.keep_recordings).unwrap_or_default() {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

/// Command mode: transcribe the spoken instruction, apply it to the selection (or write new text
/// when nothing is selected), and paste the result over the still-active selection.
async fn process_command(app: &AppHandle, state: &AppState, cap: Captured, ctx: AppContext, released: Instant) -> Result<()> {
    let s = state.settings.read().unwrap().clone();
    let Some(clip) = prepare(cap, s.silence_threshold_dbfs) else {
        emit_status(app, Status::Empty);
        return Ok(());
    };

    // Never lose a dictation: audio hits the disk (and History) first, even for a refusal.
    let path = state.recordings_dir.join(format!("{}.wav", now_ms()));
    std::fs::write(&path, &clip.bytes).context("saving recording")?;
    let row_id = state.db.insert_dictation(&NewDictation {
        app_exe: &ctx.exe,
        window_title: &ctx.title,
        mode: "command",
        duration_ms: clip.duration_ms,
        audio_path: &path.to_string_lossy(),
        stt_provider: &s.stt_provider,
        stt_model: &s.stt_model,
    })?;
    let mut t = Timings { prepare_ms: released.elapsed().as_millis() as u64, ..Default::default() };

    // Local checks before anything is sent; the selection grab needs the target app still focused.
    let exe = ctx.exe.clone();
    let grabbed = tauri::async_runtime::spawn_blocking(move || {
        if context::focused_is_password() {
            return Err(());
        }
        Ok(insert::grab_selection(&exe))
    })
    .await
    .map_err(|e| anyhow!("selection task: {e}"))?;
    let refuse = |title: &str, body: &str| -> Result<()> {
        state.db.fail_dictation(row_id, &format!("Command refused: {title}"))?;
        let _ = app.emit("dictation-updated", row_id);
        emit_status(app, Status::Idle);
        crate::toast::command_refused(app, title, body);
        Ok(())
    };
    let selection = match grabbed {
        Err(()) => return refuse("Not in password fields", "Command mode doesn't read or change passwords."),
        Ok(sel) => sel,
    };
    if selection.as_ref().is_some_and(|x| x.chars().count() > cleanup::MAX_SELECTION) {
        return refuse("Selection too long", "Command mode works on up to 8,000 characters. Select less and try again.");
    }

    let stt = registry::build_stt(&s, &state.http);
    let t0 = Instant::now();
    let instruction = match stt.transcribe(&clip, &stt_options(&s, &hint_terms(state))).await {
        Ok(tr) => tr.text,
        Err(e) => {
            let _ = state.db.fail_dictation(row_id, &format!("{e:#}"));
            let _ = app.emit("dictation-updated", row_id);
            emit_status(app, Status::Error { message: format!("{e:#}"), row_id: Some(row_id) });
            return Ok(());
        }
    };
    t.stt_ms = t0.elapsed().as_millis() as u64;
    if instruction.trim().is_empty() {
        state.db.finish_dictation(row_id, "", "", &t, "empty")?;
        emit_status(app, Status::Empty);
        return Ok(());
    }

    emit_status(app, Status::Rewriting { instruction: instruction.clone() });
    let cctx = CleanupContext { app_exe: ctx.exe.clone(), window_title: ctx.title.clone(), ..Default::default() };
    let t1 = Instant::now();
    let edited = match registry::build_cleanup(&s, &state.http).edit(selection.as_deref(), &instruction, &cctx).await {
        Ok(out) => {
            let tokens = usage::edit_tokens(selection.as_deref(), &instruction, &cctx, &out);
            let _ = state.db.set_cleanup_usage(row_id, &s.cleanup_provider, &s.cleanup_model, tokens);
            cleanup::strip_long_dashes(&out)
        }
        Err(e) => {
            // The selection is untouched; the instruction stays in History.
            let msg = format!("Command failed: {e:#}");
            state.db.finish_dictation(row_id, &instruction, "", &t, "failed")?;
            let _ = state.db.fail_dictation(row_id, &msg);
            let _ = app.emit("dictation-updated", row_id);
            emit_status(app, Status::Error { message: msg, row_id: None });
            return Ok(());
        }
    };
    t.cleanup_ms = t1.elapsed().as_millis() as u64;
    if edited.trim().is_empty() {
        state.db.finish_dictation(row_id, &instruction, "", &t, "empty")?;
        emit_status(app, Status::Empty);
        return Ok(());
    }

    let iopts = InsertOptions {
        mode: s.insert_mode,
        restore_clipboard: s.restore_clipboard,
        restore_delay: Duration::from_millis(s.restore_delay_ms),
    };
    let t2 = Instant::now();
    let text = edited.clone();
    let outcome =
        tauri::async_runtime::spawn_blocking(move || insert::insert_text(&text, &iopts)).await.map_err(|e| anyhow!("insert task: {e}"))?;
    t.insert_ms = t2.elapsed().as_millis() as u64;
    let restore_wait = if outcome == Outcome::Pasted && s.restore_clipboard { s.restore_delay_ms.min(t.insert_ms) } else { 0 };
    t.total_ms = (released.elapsed().as_millis() as u64).saturating_sub(restore_wait);

    let left = matches!(outcome, Outcome::LeftOnClipboard(_));
    state.db.finish_dictation(row_id, &instruction, &edited, &t, if left { "clipboard" } else { "done" })?;
    log::info!(
        "command #{row_id} [{}] total {} ms (stt {} / edit {} / insert {}), selection {} chars",
        ctx.exe,
        t.total_ms,
        t.stt_ms,
        t.cleanup_ms,
        t.insert_ms,
        selection.as_ref().map_or(0, |x| x.chars().count())
    );
    emit_status(app, Status::Done { text: edited, total_ms: t.total_ms, outcome, cleanup_failed: false, cleanup_skipped: false });
    if selection.is_some() && !left {
        crate::toast::replaced_selection(app);
    }
    let _ = app.emit("dictation-updated", row_id);
    Ok(())
}

/// Re-transcribe a saved recording (after a network failure, or with another model).
/// The result is stored and returned; the UI offers Copy rather than pasting blindly.
pub async fn retry(state: &AppState, id: i64) -> Result<DictationRow> {
    retry_with(state, id, None).await
}

/// `stt`: (provider, model) to use this once instead of the selected one ("Use cloud once").
pub async fn retry_with(state: &AppState, id: i64, stt: Option<(&str, &str)>) -> Result<DictationRow> {
    let row = state.db.get(id)?.ok_or_else(|| anyhow!("dictation {id} not found"))?;
    let path = row.audio_path.clone().ok_or_else(|| anyhow!("audio for #{id} was pruned"))?;
    let bytes = std::fs::read(&path).with_context(|| format!("reading {path}"))?;
    let (pcm16k, _rate) = wav::decode(&bytes).ok_or_else(|| anyhow!("corrupt recording"))?;
    let duration_ms = resample::duration_ms(pcm16k.len());
    let clip = AudioClip { bytes, mime: "audio/wav", filename: "audio.wav", pcm16k, duration_ms };

    let mut s = state.settings.read().unwrap().clone();
    if let Some((p, m)) = stt {
        (s.stt_provider, s.stt_model) = (p.to_string(), m.to_string());
    }
    let stt = registry::build_stt(&s, &state.http);
    let opts = stt_options(&s, &hint_terms(state));
    let t0 = Instant::now();
    match stt.transcribe(&clip, &opts).await {
        Ok(tr) => {
            let _ = state.db.set_stt_model(id, &s.stt_provider, &s.stt_model); // the estimate prices the model that ran last
            let t = Timings { stt_ms: t0.elapsed().as_millis() as u64, ..Default::default() };
            let snips = state.db.list_snippets().unwrap_or_default();
            let text = match snippets::find(&tr.text, &snips) {
                Some(h) if h.prefix.is_empty() => h.expansion.to_string(),
                Some(h) => format!("{}\n{}", h.prefix, h.expansion),
                None => tr.text.clone(),
            };
            state.db.finish_dictation(id, &tr.text, &text, &t, "retried")?;
        }
        Err(e) => {
            state.db.fail_dictation(id, &format!("{e:#}"))?;
        }
    }
    state.db.get(id)?.ok_or_else(|| anyhow!("dictation vanished"))
}

pub fn list_mics() -> Vec<String> {
    audio::list_input_devices()
}
