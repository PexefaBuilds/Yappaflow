# Phase 3: Pill overlay, tray, full settings app, onboarding (cloud path)

## Goal
The app looks and feels like the designs. A non-focus-stealing pill shows state while you talk, a tray menu gives quick actions, and the sidebar settings app plus first-run onboarding replace the interim Phase 1 window.

## Scope
1. **Tokens first.** Import `design/tokens.css` into the UI. Replace every raw hex in `src/styles.css` with variables. Support `data-theme` (system/light/dark setting) and `data-accent`.
2. **Pill window** (second Tauri window, `label: "pill"`):
   - Transparent, no decorations, always on top, `skipTaskbar`, `focus: false`.
   - On Windows, add `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`. It must **never** take focus.
   - Click-through (`set_ignore_cursor_events(true)`), except the hands-free stop button and toast actions. Turn hit-testing on only while those are visible.
   - Position: bottom-centre of the monitor under the mouse cursor, 12 px above the work area (respect the taskbar).
   - States from `design/Main.dc.html` and `PillExtras.dc.html`: idle handle (optional) · listening · hands-free (timer + stop) · processing · polishing · done (0.9 s, then fade) · failed · copied (Ctrl+V) · warming up · paused · command (Phase 5).
   - Waveform driven by the existing `level` event (≈ 20 fps). Respect `prefers-reduced-motion`.
3. **Toasts** above the pill, from `design/Toasts.dc.html`: 6 s auto-hide, paused on hover, at most one action. Wire up the ones that apply now: offline/failed, key problem, no mic, admin window, update ready (stub).
4. **Tray** (`design/TrayMenu.dc.html`): a custom popup window, not the native menu. Contents: status line, last dictation with **Paste again** / **Copy**, hands-free toggle, microphone submenu, pause for 1 h, History, Settings…, Quit. A right-click on the tray icon may keep a minimal native menu as a fallback.
   - **Paste again** must give focus back to the previous window first. Record the foreground HWND before the popup opens.
5. **Settings app** with a left sidebar (`design/Sidebar.dc.html`), sections from the `Set*.dc.html` files. Build in this phase:
   - General
   - Hotkeys: rebinding with conflict detection; the chord tracker generalised to any 1–3 key chord
   - Microphone: live meter, 3 s record-and-playback test, gate slider, keep-ready, lower-other-audio. Ducking can be a stub if it's hard; note it.
   - Models (cloud, OpenAI only for now; other providers greyed "coming soon")
   - App styles (from Phase 2)
   - History: list + detail, search, timing breakdown, play audio, Copy / Retry
   - Advanced
   - About

   Dictionary and Snippets get placeholder pages ("coming in an update").
6. **Onboarding** (`Onb1`–`Onb5`, cloud path): shown on first run, re-runnable from About. The "On this PC" option is visible but disabled with "Coming soon" until Phase 7.
7. **Sounds:** soft start/stop ticks (General › sounds). Generate or record our own; don't download copyrighted sound packs.
8. **Settings window** remembers its size and position, closes to the tray, and single-instance focuses it.

## Out of scope
Local models, Mac, Dictionary/Snippets logic, Command mode.

## Acceptance tests
| # | Test | Pass when |
|---|---|---|
| 3.1 | Dictate into Notepad, Chrome, Slack, a full-screen video, a game in borderless mode | Pill appears; **the target keeps focus** (caret still blinking), text lands |
| 3.2 | Hands-free → click the pill's stop button | Text is pasted into the original window, not the pill |
| 3.3 | Two monitors: dictate with the mouse on monitor 2 | Pill on monitor 2 |
| 3.4 | Taskbar on the left or top | Pill still clear of the taskbar |
| 3.5 | Switch accent and theme in General | Every screen and the pill update live; contrast passes (axe or Lighthouse on the settings window: no contrast errors) |
| 3.6 | Keyboard only: Tab through every settings page | Visible focus on everything; Esc closes dialogs |
| 3.7 | Rebind dictate to Right Alt (hold) | Works; old chord no longer triggers; Start menu unaffected |
| 3.8 | Tray › Paste again while Outlook was focused | Text pasted into Outlook |
| 3.9 | Fresh install (delete app data) | Onboarding runs; after step 5 the first dictation works |
| 3.10 | Screen reader (Narrator) on onboarding | Every control has a sensible name |
| 3.11 | Compare each built screen with its `design/*.dc.html` side by side | Matches spacing, type and colours. List any deliberate deviation in Results. |

## Results
_(Not yet run. Built 24 Oct 2026; see WORK_PLAN.md for what is unverified on screen.)_

**3.5 contrast, static token check (24 Oct 2026, not the axe/Lighthouse run):** WCAG ratios computed for `--text`, `--text-2`, `--muted`, `--warn`, `--command` and every accent's `--accent-text` against `--bg`, `--sidebar`, `--surface`, `--raised`, `--seg-on` and `--pill-bg` in both themes, plus `--on-accent` on all 7 accents. All ≥ 4.5:1 except two light-theme pairs on `--sidebar`: `--warn` 4.47 and mint `--accent-text` 4.41. Neither is used for text on the sidebar (sidebar text is `--text`/`--muted`; accent there is only focus rings, which need 3:1), so there are no live failures. The axe run on the real window is still to do.
