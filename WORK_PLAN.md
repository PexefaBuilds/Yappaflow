# WORK_PLAN.md: live progress (read after CLAUDE.md)

Updated 2026-10-24. `PLAN.md` §0 stays the product source of truth; this file is "where we are right now".

## Done (all on `main`; 141 Rust tests pass, `npm run build` passes; nothing acceptance-tested on screen)
- **Phases 1–4** built: pipeline, cleanup + gate + app styles, pill/tray/toasts/settings/onboarding, Writing rules, dictionary + snippets + learn-from-corrections (`learn.rs`, UIA), Polishing/Paused pill states, pill text kept sharp (only `::before` morphs), pill 24 px above taskbar.
- **Phase 5:** command mode (Ctrl+Win+Alt; fn+⌃ on Mac), selection grab via clipboard Snapshot + UIA selection check, never Ctrl+C in terminals, `CleanupProvider::edit()`, command prompt + 15-case eval (not run). Hands-free trigger setting `hands_free_double_tap` (default double-tap dictate, or its own shortcut).
- **Phase 6 (app side):** per-user NSIS, Start with Windows, admin-mode relaunch, all-format clipboard restore (400 ms cap), rotating redacted log + `crash.log`, data controls. Typed line breaks = Shift+Enter.
- **Phase 7:** cloud providers (OpenAI/Groq/Mistral/Custom URL presets, Deepgram, ElevenLabs, AssemblyAI, Gemini; cleanup Anthropic/Gemini/OpenRouter/Mistral), keys only over https or a validated custom URL, redirects off. Local: catalogue `resources/models.json` (HF-pinned + sha256), resumable downloader, hardware pick, llama-server sidecar (127.0.0.1, per-launch key, no proxy), `WhisperCppStt` behind feature `local-stt` (not buildable on this PC).
- **Phase 8:** macOS port (CGEventTap, AX context, NSPasteboard, Keychain, perms onboarding, menu-bar icon, tauri-nspanel pill, migration v4 `bundle_id`). Builds, links, 127 tests pass on the Mac; never run with a GUI.
- **Phase 9 groundwork:** README, PRIVACY, SECURITY, CONTRIBUTING, CODE_OF_CONDUCT, LICENSE (`<OWNER NAME>`), OFL, gitleaks, CI, draft-only Windows release workflow, `deny.toml`, `docs/RELEASE_CHECKLIST.md`.
- **API usage estimate:** History page "Estimated API cost this month" + 12-month table (`usage.rs`, `resources/prices.json` from providers.md only, migration v5). Copy says Yappaflow is free.
- **Local-models groundwork** (merged 24 Oct, reviewed + fixed): VRAM budget check, keep-loaded setting (`models/keep.rs`), "Use cloud once" toast, CI `local-engines` job, release steps that bundle a pinned llama-server (skipped while `LLAMA_TAG`/hashes are empty). Not done: pill "Warming up" state, low-priority start-up warm-up, free-VRAM check (uses dedicated VRAM).
- **Polish** (merged 24 Oct): admin mode refused on standard accounts (stale setting cleared at startup), delete-all in one transaction that spares an in-flight dictation, About › Licences, issue/PR templates, cargo-about `THIRD_PARTY_NOTICES` job (none of the release/CI steps have run yet). Command-edit Retry was dropped (it lost the selection; phase-5 says no Retry for edits).
- Identifier is `com.yappaflow.desktop`; `lib.rs` moves the old `com.yappaflow.app` roaming data folder once (done on the owner's PC).

## Decisions and why
- rustfmt: `src-tauri/rustfmt.toml` (max_width 140, small heuristics Max). CI fmt + clippy (`-D warnings`) now block on Windows; the macOS leg stays non-blocking. The Mac has no clippy/rustfmt components (`cargo check` there is clean).
- Parallel work = agents in git worktrees (`.claude/worktrees/`, gitignored); every branch gets a separate `opus-reviewer` pass before merge.
- Cleanup gate skips the LLM unless needed (saves ~1.1 s); STT default `gpt-transcribe` (fastest in bench). Prompt changes need an eval re-run; `docs/prompts/*.md` and `providers/prompts/*.txt` must stay identical.
- Hook rules: atomics + lock-free ring, never alloc/lock/log in `hook_proc` or the Mac tap callback.
- Owner rule: **install nothing on the owner's PC** for local models; native local builds go in CI. Mac has rustup (`~/.cargo`) and Node 22 (`~/.local/node`, PATH in `~/.zprofile`), installed with the owner's OK; Mac files only under `~/ShahidPersonalProjects/Yappaflow/`.
- Windows/Mac signing, updater key, Apple Developer ID, GitHub repo: owner only.
- Never type or store API keys: the owner enters them in Settings › Models.

## Current task
None in flight; no open branches or worktrees.

## Next steps (in order)
1. Before filling the llama-server pins in `release.yml`: settle Phase 7 item 7 (CUDA DLL size/licence; the only Windows build bundled is CUDA, so check it runs without NVIDIA). Also open: `docs/providers.md` lacks the llama-server flags / `-ngl 0` fallback; macOS watchdog trusts PID liveness (PID reuse); create repo labels `bug`, `enhancement`, `provider` for the issue forms.
2. Owner: on-screen acceptance tests for Phases 1–8 (lists in `docs/phases/phase-N.md`; paste results there). Evals 4.5/5.5 cost cents with the saved key.
3. Owner: rotate the OpenAI key (old one was pasted in chat); add a Groq key; run `stt_bench` / `cleanup_eval` with `STT_PROVIDER`/`EVAL_PROVIDER` and pick defaults.
4. Owner: release items in `docs/RELEASE_CHECKLIST.md` (licence name, contacts, employer sign-off, trademark, repo, signing, updater key). Test builds on request: unsigned NSIS here, `.dmg` on the Mac.

## Open questions for the owner
- Git repo URL (no off-machine backup yet). Default providers after benchmarks. Very wide settings pages: cap line length?

## Gotchas
- `cargo` isn't on PATH in new shells: Git Bash `export PATH="$PATH:$HOME/.cargo/bin"`. Owner's terminal is **PowerShell**: give PowerShell syntax (e.g. `Remove-Item -Recurse -Force`), not bash.
- Python heredocs in Git Bash: don't add `< /dev/null` after `<<'EOF'` (it replaces the heredoc). Writing a script file to the scratchpad and running `python -I file.py` is reliable.
- Claude Code's safety check blocks `rm -rf` on drive-root folders (e.g. `D:\x`); ask the owner.
- `npm run tauri dev` auto-rebuilds on Rust edits; port 1420 must be free.
- Every new `Settings` field needs a serde default; schema changes are new migration steps (last is v5).
- `capabilities/default.json` must keep listing main, pill, onboarding, toast, tray.
- Rule 6 has two owner-approved exceptions (see CLAUDE.md): `lib.rs::elevate`, `llama_cpp.rs::tie_to_app`.
- Worktree `node_modules`: link with a junction to the main one; remove it with `cmd /c rmdir` (never `Remove-Item -Recurse`) before `git worktree remove`.
- Typing mode sends Shift+Enter for newlines. Command mode: apps without UIA TextPattern still get Ctrl+C on an empty selection.
- Local STT on Windows needs LLVM/libclang + `LIBCLANG_PATH`, CMake on PATH, and a short `CARGO_TARGET_DIR` (long worktree paths break MSBuild).
- Unverified provider details are listed in `docs/providers.md` › Phase 7 build checks (AssemblyAI model ID, Mistral/ElevenLabs list encoding, Gemini thinking field).
- The owner dictates messages: expect transcription slips; read for intent.
