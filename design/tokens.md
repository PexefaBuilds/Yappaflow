# Design tokens: how to use them

All values live in `design/tokens.css`. This page explains when to use which.

## Screens → files
| Screen | Design file | Size |
|---|---|---|
| Pill: core states (idle, listening, hands-free, processing, done, failed) | `Main.dc.html` | overlay |
| Pill: extra states (polishing, copied, command, warming up, paused, light taskbar) | `PillExtras.dc.html` | overlay |
| Onboarding 1–5 + local-model step + Mac permissions | `Onb1Welcome` … `Onb5Done`, `Onb2Local`, `OnbMacPerms` | 760 × 560 |
| Settings: sidebar component | `Sidebar.dc.html` | 240 wide |
| Settings: General, Hotkeys, Microphone, Models (cloud), Models (on this PC), App styles, Dictionary, Snippets, History, Advanced, About | `Set*.dc.html` | 1040 × 800 |
| Light variants | `SetGeneralLight`, `SetHistoryLight` (light History = `SetHistory` with `theme="light"`) | |
| Command mode, toasts, tray menu | `CommandMode`, `Toasts`, `TrayMenu` | |
| Accent options (reference) | `AccentOptions.dc.html` | |
| Phase 1 interim window (what's built now) | `Settings.dc.html` | 600 × 980 |

Open any `.dc.html` in a browser to see it. Copy is final unless it's in `[BRACKETS]`.

## Colour rules
- **Never hard-code a hex in UI code.** Use `var(--…)`.
- Accent **fills** (primary buttons, switch-on, pill bars, progress) use `--accent` with `--on-accent` text.
- Accent **text and thin strokes** (timings like "0.84 s", "Key works", links, selected borders) use `--accent-text`. In light mode this is the deep shade, which keeps 5:1 contrast on white.
- Errors and warnings use `--warn`. It turns red automatically when the accent is Coral or Tangerine.
- Command mode uses `--command` (violet; coral when the accent is Violet).
- Toasts are dark in both themes.

## Accents (user setting, default Azure)
| id | fill (`--accent`) | deep (light-mode text) |
|---|---|---|
| azure | #5AB0FF | #0B5FB3 |
| coral | #FF7A59 | #B23A1C |
| tangerine | #FF9A3D | #A84E00 |
| sunflower | #FFC94D | #8A5A00 |
| mint | #5EE3B0 | #0B7A50 |
| violet | #9A88FF | #5B47C9 |
| chalk | #ECEAE3 | #17171A |

Set it with `document.documentElement.dataset.accent = id`. Persist it in `Settings.accent` (Rust).

## Type
- Unbounded is wide. Keep body copy at 12px and help text at 10px, and never go below 9px.
- Section captions: `--text-h2`, uppercase, `letter-spacing: var(--tracking-caps)`, colour `--muted`.
- Numbers that change (timers, timings) use `font-variant-numeric: tabular-nums`.
- Any user text (transcripts, dictionary terms, snippets) gets `dir="auto"` so Arabic renders right-to-left with Reem Kufi.

## Components (as drawn)
- **Switch:** 42×24 track, 18px knob, 150ms. Off: `--line` track, `--muted` knob. On: `--accent` track, `--on-accent` knob. Light on: `--accent-deep` track, white knob.
- **Segmented control:** `--field` background with 3px padding; selected item `--seg-on`, weight 500.
- **Card row:** label (12px) plus help (10px, max 440px wide) on the left, control on the right, `--divider` between rows.
- **Pill:** 48px tall, `--pill-bg`, `--pill-shadow`, a 1px inset `--pill-ring`, 24px above the taskbar. States and copy are in `Main` and `PillExtras`.
- **Focus:** a 2px outline in `--accent-text`, offset 2px, on every interactive element.

## Motion
- Waveform bars follow the real mic level at about 20 fps. The 900ms loop in the designs is only an idle stand-in.
- Respect `prefers-reduced-motion`: no shimmer or wave loops, and use opacity changes only.
