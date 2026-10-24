# Handoff: what was decided in the planning chat (23–24 Oct 2026)

This file replaces the planning conversation. Anyone (or any Claude session) picking up the project should read it once, after `CLAUDE.md`.

## The product
**Yappaflow:** hold a hotkey, speak, release, and polished text appears at the cursor in any app.
- Windows first, macOS second.
- Users bring their own API key for any of 8 cloud providers, or run local models.
- Free and open source (MIT), published later on the owner's **personal** GitHub. The employer has OK'd personal ownership and open-sourcing; keep the written confirmation on file.
- Original, clean-room work: no code, assets, prompts or naming from any other product.

## Decisions, in order
| Topic | Decision |
|---|---|
| Stack | Tauri 2 (Rust + web UI), tray app, WH_KEYBOARD_LL hook, cpal audio at 16 kHz, SQLite, OS keychain for keys |
| Architecture | `SttProvider` / `CleanupProvider` traits; providers chosen in settings; local models plug in later without refactoring |
| Default hotkey | Hold **Ctrl + Win**. Double-tap for hands-free. Esc cancels. Command mode **Ctrl + Win + Alt**. Mac: hold **fn/Globe**. |
| Default STT | OpenAI `gpt-transcribe` ($0.0045/min); keyword and language hints via `keywords[]` / `languages[]` |
| Cleanup LLM | OpenAI `gpt-6-luna`, reasoning off, 1.5 s timeout, raw-text fallback |
| Providers | OpenAI, Groq, Deepgram, ElevenLabs, AssemblyAI, Mistral, Gemini, plus custom OpenAI-compatible URL. Users pick one and paste its key. Details in `docs/providers.md`. |
| Local | whisper.cpp large-v3-turbo q5_0 (574 MB) + llama.cpp Qwen3.5-4B (3 GB) as the recommended pair; hardware detection picks the model |
| Team key / relay | **Dropped.** The app became public and bring-your-own-key, so there's no shared key and no relay server. |
| Name | **Yappaflow** (rejected along the way: Utter, Yap & Flow, and a few others). Do a full trademark and domain check before the public release. |
| Font | **Unbounded** for Latin, **Reem Kufi** for Arabic (automatic fallback) |
| Theme | Dark + light, following the OS |
| Accent | User picks from 7: **Azure (default)**, Coral, Tangerine, Sunflower, Mint, Violet, Chalk. Neon green was rejected; Rose was dropped. |
| Settings layout | Left sidebar, 11 sections (incl. Models split into Cloud and On this PC) |
| Logo | **One continuous stroke:** a sound wave that calms at the centre and becomes a line of text, fading from accent to white. Animated version: voice bars pulse once, fade out, then the stroke draws in (4.2 s loop, no cursor). |
| Static icon | The wave → line mark. Files are in `design/logo/` and `src-tauri/icons/` (incl. 16/32 px tray glyphs). |
| Where the code lives | `D:\Claude-Related\ClaudeFiles_Personal\Yappaflow`, synced between PCs via a private GitHub repo. Google Drive copies are obsolete. |

## Artifacts
- **Design canvas (all screens):** https://claude.ai/artifact/Po4WUmMyRGtRy2YYkAytYb
  - It's private to the owner's Claude account and can be opened from any chat or device. To revise it in a new chat, paste the link and ask for changes.
  - Pages: Accent options · Logo options (+ animation) · Pill · Onboarding · Settings app · Overlays & tray · Light theme
- **Source copies** of every canvas screen: `design/*.dc.html` (open any of them in a browser). Layout: `design/canvas.json`.

## State of the code
- Phase 1 (bare pipeline) is **written but has never been compiled**. The pure modules pass their unit tests: session 7, keys 5, resample 5, wav 1, keyword helpers 6.
- Two independent reviews found no definite compile errors. Expect small `windows`-crate or Tauri API fixes on the first build.
- The UI is the interim Phase 1 settings window, already using Unbounded/Reem Kufi, the accent picker and the new logo.

## Open questions (not blocking Phase 1)
- Phase 7: ship the CUDA runtime DLLs inside the installer, or as a separate download?
- Phase 8: needs a Mac and an Apple Developer account ($99/yr).
- Phase 9: publish with a fresh, squashed git history (recommended)? Full trademark check on the "…flow" part of the name.

## First prompt for Claude Code
> Read CLAUDE.md and docs/phases/phase-1.md, then get the app compiling on Windows without changing behaviour.
