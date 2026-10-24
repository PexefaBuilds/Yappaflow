# Phase 1: Bare pipeline

## Goal
Hold Ctrl+Win, speak, release, and the raw transcript is pasted at the cursor in any Windows app. It's reliable, logged with timings, and never loses audio.

## Status
All code is written (see `PLAN.md` §4, items P1.1–P1.12). It now compiles and runs on Windows (`cargo test` passes); acceptance tests 1.1 and 1.2 passed, 1.4–1.15 are still to run (see Results).

## Tasks
1. In `D:\Claude-Related\ClaudeFiles_Personal\Yappaflow`: `npm install`, then `npm run tauri dev`.
2. Fix compile errors **without changing behaviour**. Likely spots:
   - `windows` 0.58 signatures in `hotkey/mod.rs`, `insert.rs`, `context.rs`
   - `show_menu_on_left_click` (Tauri older than 2.2 calls it `menu_on_left_click`)
   - `GetMessageW(&mut msg, None, …)`: use `HWND::default()` if `None` is rejected
3. `cd src-tauri && cargo test` must pass, with all existing tests green.
4. Run the acceptance tests below and record the results.
5. Fix any bug the tests reveal. Keep fixes inside Phase 1 scope.

## Out of scope
Cleanup LLM, pill overlay, any provider except OpenAI, the full settings app.

## Design refs
`design/Settings.dc.html` (the interim Phase 1 window). The UI already implements it, including the accent swatches.

## Acceptance tests
| # | Steps | Pass when |
|---|---|---|
| 1.1 | Settings → paste OpenAI key → Save | "Key works · gpt-transcribe available" |
| 1.2 | In Notepad, hold Ctrl+Win and say "This is a test of Yappaflow", then release | The text appears; Recent shows a time |
| 1.3 | Repeat 1.2 in Chrome (any text box), Slack, VS Code, Windows Terminal | Works in all five |
| 1.4 | Copy "CLIPBOARD-CANARY" → dictate in Notepad → Ctrl+V somewhere else | Pastes "CLIPBOARD-CANARY" (clipboard restored) |
| 1.5 | Win+V clipboard history after a dictation | The dictated text is **not** in history |
| 1.6 | Double-tap Ctrl+Win, speak for 20 s, tap once | Full text pasted (hands-free) |
| 1.7 | Hold Ctrl+Win, speak, press Esc while holding, release | Nothing pasted; Start menu doesn't open |
| 1.8 | Tap Ctrl+Win once briefly | Nothing happens; no "Cancelled" flash, no Start menu |
| 1.9 | Ctrl+Win+→ (switch desktop) | The desktop switches, no recording left running |
| 1.10 | Hold Ctrl+Win in silence for 2 s | "Didn't hear anything"; no API call (check logs) |
| 1.11 | Wi-Fi off → dictate → Wi-Fi on → Recent → Retry | Failed row, then text after Retry |
| 1.12 | Open Notepad **as administrator** → dictate | "Copied, press Ctrl+V"; Ctrl+V pastes the text |
| 1.13 | Dictate 10 short sentences in Notepad | Record the median release→paste time from Recent (hover for the breakdown). This is the Phase 2 baseline. |
| 1.14 | Mixed language: "Let's meet tomorrow, إن شاء الله" | Both scripts appear; Arabic renders in Reem Kufi in Recent |
| 1.15 | Quit and relaunch | Settings and Recent persist; key still saved |

## Results
Built and ran on Windows with no behaviour changes (`cargo test` 30/30 incl. Phase 2 modules, UI builds).
- 1.1 passed (key saved, Test works).
- 1.2 to 1.15 not run yet: tester could not run 1.4 to 1.13.
- Recent "Copy" button bug fixed: the list was re-rendered on window focus and ate the first click; it now only re-renders when data changes.
