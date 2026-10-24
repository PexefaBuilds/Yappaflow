# Phase 5: Command mode

## Goal
Select text, hold **Ctrl + Win + Alt**, say how to change it ("make it friendlier", "translate to Arabic", "turn this into bullet points"), and the selection is replaced. With nothing selected, it writes new text at the cursor ("draft a polite no to Friday's meeting").

## Scope
1. **Chord:** extend `ChordTracker` with a second chord. Alt pressed while Ctrl+Win is held switches the session to Command (today Alt counts as "other key" and cancels, so change that). Make it configurable in Hotkeys.
2. **Get the selection:** save the clipboard → send Ctrl+C → wait ≤ 150 ms for the clipboard sequence number (`GetClipboardSequenceNumber`) to change → read the text → restore the clipboard. If nothing changed, there is no selection.
3. **New trait method** `CleanupProvider::edit(selection: Option<&str>, instruction: &str, ctx) -> Result<String>`, with a separate prompt in `docs/prompts/command.md` (write it in this phase, plus a 15-case eval at `docs/evals/command.jsonl`). The output is the replacement text only.
4. **Replace:** paste the result over the still-active selection. Show the "Replaced selection · Ctrl+Z to undo" toast.
5. **Pill:** violet command state (`PillExtras` #09). Show the live transcript above the pill (`CommandMode.dc.html`).
6. **Safety:**
   - Selections over 8,000 characters → toast "Selection too long" and abort.
   - Don't run in password fields (UIA `IsPassword`).

## Acceptance tests
| # | Test | Pass when |
|---|---|---|
| 5.1 | Select a long sentence in Outlook → "make it shorter and friendlier" | Replaced; Ctrl+Z restores the original |
| 5.2 | Select English → "translate to Arabic" | Arabic text; renders RTL in Outlook and Word |
| 5.3 | No selection in Notepad → "write a two-line out-of-office message" | Text appears at the cursor |
| 5.4 | Select code in VS Code → "add comments" | Code plus comments, nothing else (no markdown fences) |
| 5.5 | Command eval set | ≥ 90% |
| 5.6 | Focus a password box | Nothing happens; notice shown |

## Results
**Status: Built, not acceptance-tested.** `npm run build` and `cargo test --lib` pass (unit tests only). None of 5.1–5.6 has been run on screen, and the command eval (5.5) has not been run (it costs money; the owner runs it).

What was built:
- **Chord.** Command mode defaults to **Ctrl + Win + Alt**. When the command chord contains the whole dictate chord, its extra key no longer counts as "other key" (no cancel); the command slot fires and the held dictation becomes a command (`hotkey/actions.rs`, `Hooks::on_key`). Pressing all three at once gives Down then Command. A rebound command chord that does not contain the dictate chord starts a hands-free command recording; the next press of it or of the dictate chord stops it, Esc cancels. Settings › Hotkeys no longer warns that Ctrl + Win + Alt "overlaps" dictate.
- **Session** (`session.rs`): new inputs `CommandKey` and `HandsFreeKey`, a `command` flag per take, action `EnteredCommand`. A held command never waits for a double tap on release.
- **Hands-free setting (owner request).** New `Settings.hands_free_double_tap` (default true, so old settings load). Settings › Hotkeys › Hands-free has a choice: "Double-tap dictate" (default) or "Its own shortcut" (the recorder; double-tap is then off and a quick tap of the dictate chord is a short dictation). The hook only sees the hands-free chord when double-tap is off (`hotkey_cmds::live_codes`); the controller reads `hotkey::HANDS_FREE_DOUBLE_TAP` (an atomic). Tray › Hands-free now sends a direct hands-free start, so it works in both modes. Esc cancels every hands-free path (unit-tested).
- **Selection grab** (`insert::grab_selection`): waits for modifiers to be released, saves the clipboard, sends Ctrl+C, waits ≤ 150 ms on `GetClipboardSequenceNumber`, reads the text and restores the clipboard (privately; clears it if it was empty). No change = no selection.
- **Provider seam:** `CleanupProvider::edit(selection, instruction, ctx)`. The default (passthrough "none") errors with "Command mode needs a cleanup model". OpenAI and Groq share the implementation (`openai_cleanup.rs`, 30 s timeout, larger token cap). Prompt in `docs/prompts/command.md`, embedded copy `providers/prompts/command.txt` (a unit test checks they match). The model's ``` fence is stripped if the selection had none; long dashes are stripped.
- **Pipeline** (`pipeline::process_command`): password check (UIA `IsPassword`, `context::focused_is_password`) → selection grab → over 8,000 chars refuses → audio saved and row inserted (mode `command`) before any network call → STT → `Rewriting { instruction }` status → edit → paste over the selection → "Replaced selection · Ctrl+Z to undo" toast (only when there was a selection). On an edit error the selection is left untouched and the instruction stays in History.
- **Pill:** violet command state (sparkle badge, `--command` bars and ring; coral under the Violet accent via tokens) for hold and hands-free (with timer + stop). While the edit runs, the pill says "Rewriting" and the transcribed instruction is shown in a bubble above it (the window grows to fit, then shrinks back). Sidebar status card handles "Command" and "Rewriting…".
- **Eval:** `docs/evals/command.jsonl` (15 cases, 3 injection) and runner `src-tauri/src/command_evals.rs` behind the `evals` feature: `cargo test --features evals command_eval -- --ignored --nocapture`. An offline test checks the file parses.

Unverified / known limits:
- Everything on screen: the pill and bubble look, the window resize timing, the toast text, Alt not opening app menus after the chord, the Win mask.
- The bubble shows the instruction after STT, not live (no streaming STT yet).
- Ctrl+C side effects, mitigated: Ctrl+C is never sent to terminals (Windows Terminal, conhost, cmd, PowerShell, WezTerm, Alacritty, mintty), and is skipped when UIA TextPattern reports an empty selection. Apps without TextPattern still get Ctrl+C, so an editor that copies the whole line on an empty selection could still be hit there. The clipboard is snapshotted (all formats) and restored on every path. The app's own copy may land in Win+V history.
- The password check runs after the instruction is recorded. The WAV and History row are saved first (local only), then the row is marked "Command refused" and the notice shows.
- Alt release after the command chord is masked like Win (unverified against Office KeyTips).
- An edit failure shows the error with no Retry (Retry re-transcribes as a dictation, which would be wrong here).
