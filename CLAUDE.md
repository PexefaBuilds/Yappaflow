# CLAUDE.md: Yappaflow

Read this first in every session. It's the briefing for anyone working on this repo, human or Claude.

## What this is
Yappaflow is a hold-to-talk dictation app. Hold a hotkey, speak, release, and polished text appears at the cursor in any app.
- Windows first, macOS second.
- Users bring their own API key (OpenAI, Groq, Deepgram, …) or run local models (whisper.cpp / llama.cpp).
- Free and open source (MIT), public at https://github.com/PexefaBuilds/Yappaflow.

- **Plan & decisions:** `PLAN.md` (section 0 is the source of truth for product decisions)
- **Per-phase specs with acceptance tests:** `docs/phases/phase-N.md`
- **Designs:** `design/*.dc.html` (one file per screen; open any of them in a browser to view) and `design/tokens.css` / `design/tokens.md`
- **Decision history from the planning chat:** `docs/HANDOFF.md`
- **Verified provider facts:** `docs/providers.md`. Check it before writing provider code; never guess model IDs or parameter names.

## Current status
- Phases 1–8 are **built** and Phase 9 groundwork is done (155 unit tests pass on Windows). **No acceptance test has been run on screen**, so no phase box is checked yet. Prompts live in `docs/prompts/*.md` + `src-tauri/src/providers/prompts/*.txt` (keep identical; re-run the eval after any change).
- **Read `WORK_PLAN.md` next**: what's done, decisions, exact next steps, open questions, gotchas. Public repo: https://github.com/PexefaBuilds/Yappaflow (`main`); v0.9.6 is the latest release, v0.9.8 is mid-ship (see WORK_PLAN); installed copies update themselves (signed in CI, via `latest.json`).
- The next phase is set by the first unchecked box in `docs/phases/README.md`.

## Commands (Windows, PowerShell)
```powershell
npm install                     # once
npm run tauri dev               # run the app (hot-reloads the UI)
cd src-tauri; cargo test        # unit tests (pure modules run on any OS)
npm run build                   # type-check + build the UI (needed before a bare `cargo check`)
npm run tauri build             # NSIS installer → src-tauri/target/release/bundle/nsis/
```
Prerequisites: Rust (MSVC toolchain), VS Build Tools 2022 with "Desktop development with C++", Node 22 LTS or newer.

**The project lives locally at `D:\Claude-Related\ClaudeFiles_Personal\Yappaflow`.** Remote: `origin` = https://github.com/PexefaBuilds/Yappaflow (public). At the start of a session run `git pull` and `git status`; commit after each work chunk and push at the end. The pre-publish history is only on the local branch `pre-public-history` (never push it). Releases: bump the version in `package.json`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`, add `docs/release-notes/vX.Y.Z.md`, push a `vX.Y.Z` tag (`release.yml` makes the draft and third-party notices). Since v0.9.8 the owner wants the builds done locally (faster): Windows on this PC, the universal `.dmg` on the Mac over SSH, updater packages signed on this PC, then `latest.json` + `SHA256SUMS` (steps in WORK_PLAN gotchas); check the draft, then publish. Never launch installers from Claude's tools: use `explorer.exe` (see WORK_PLAN gotchas).

## Non-negotiable rules
1. **Clean room.** Never decompile, inspect, download or copy any other dictation product's binaries, assets, prompts or UI, and never use another product's name or branding. Build only from public docs and our own designs.
2. **No secrets in git.** No API keys, `.env`, code-signing certificates or updater private keys. Keys live only in the OS keychain (`src-tauri/src/secrets.rs`). Never log a key, a full transcript at info level, or audio.
3. **Never lose a dictation.** Audio is written to disk *before* any network call. Every failure path leaves the text on the clipboard or the audio in History with Retry.
4. **The provider seam is sacred.** The pipeline talks only to the `SttProvider` and `CleanupProvider` traits (`src-tauri/src/providers/mod.rs`). Add a provider by implementing a trait and registering it in `registry.rs`. Never special-case a provider in `pipeline.rs`.
5. **The hook callback stays tiny.** `hotkey/mod.rs::hook_proc` must not allocate, lock, log or do IO. Windows silently removes slow low-level hooks.
6. **OS-specific code lives in exactly four places:** `hotkey/`, `insert.rs`, `context.rs`, `secrets.rs`, each behind `#[cfg(...)]`. Everything else stays cross-platform. Owner-approved exceptions (keep them small, behind `#[cfg]`): `lib.rs::elevate` (admin relaunch + standard-account check) `providers/llama_cpp.rs::tie_to_app` (Job object / watchdog that kills llama-server with the app) and `appicon.rs` (`set_taskbar_icon`: WM_SETICON ICON_BIG; `point_shortcuts_at`: our .lnk shortcuts use the accent .ico, since the taskbar button is drawn from the shortcut; `set_dock_icon`: the macOS Dock icon).
7. **Model IDs and endpoints are settings, not constants scattered in code.**
8. **Match the designs.** Use the values in `design/tokens.css`, never ad-hoc colours. The accent is user-selectable (7 options, Azure default), so always use `var(--accent)` and never hard-code a hex.
9. **Accessibility:** real `<button>`/`<input>`/`<label>`, visible focus rings, 4.5:1 text contrast, `dir="auto"` on any user text (Arabic).

## Architecture in one breath
```
WH_KEYBOARD_LL thread → ChordTracker → controller thread (Session state machine)
  → Recorder (cpal thread) → resample 16 kHz → WAV/Opus → save to disk
  → SttProvider → CleanupProvider → insert (clipboard + Ctrl+V | Unicode typing)
  → SQLite log + Tauri events → web UI (settings window, later the pill overlay)
```
Key files: `session.rs` (pure state machine, unit-tested), `pipeline.rs` (orchestration), `providers/*`, `storage/*` (SQLite + migrations, append-only), `src/main.ts` (UI).

## Conventions
- **Rust:** `anyhow` in app code; return `Result<T, String>` from Tauri commands. No `unwrap()` on anything from the OS, the network or user input. Keep modules small; pure logic goes in its own module with `#[cfg(test)]` tests.
- **Schema changes:** append a new step to `storage/migrations.rs`. Never edit an old one.
- **UI:** vanilla TS + Vite for now. Fonts are Unbounded (Latin) and Reem Kufi (Arabic fallback) via `@fontsource`, bundled, so no network fonts.
- **Commits:** small, one concern each, message in the imperative ("Add Groq STT provider").
- **Before saying a phase is done:** run its acceptance test from `docs/phases/phase-N.md` and paste the results (timings included) into that file's "Results" section.

## Working style with the owner
The owner prefers to **discuss and plan before building**. For anything not already specified in the phase docs (a new feature, a UX change, a dependency, a provider), propose the approach and wait for a go-ahead. Fixing compile errors and bugs inside an agreed phase needs no approval.
