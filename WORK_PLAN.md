# WORK_PLAN.md: live progress (read after CLAUDE.md)

Updated 2026-10-25. `PLAN.md` §0 stays the product source of truth; this file is "where we are right now".

## Done (all on `main`, pushed; 144 Rust tests pass, clippy/fmt clean, `npm run build` passes; nothing acceptance-tested on screen)
- **Phases 1–8 built:** pipeline, cleanup + gate + app styles, pill/tray/toasts/settings/onboarding, dictionary/snippets/learning, command mode, Phase 6 app side (per-user NSIS, autostart, admin relaunch, clipboard restore, logs, data controls), Phase 7 cloud + local providers, Phase 8 macOS port (builds and tests on the Mac, never run with a GUI). API cost estimate on History.
- **Public:** https://github.com/PexefaBuilds/yappaflow (one squashed initial commit, noreply author; secret scanning, push protection, private vuln reporting, issue labels). Full local history only on local branch `pre-public-history` (never push).
- **Releases** (unsigned NSIS `.exe` + universal ad-hoc-signed `.dmg` + `SHA256SUMS`):
  - v0.9.0 (24 Oct): first public beta.
  - v0.9.1 (25 Oct): onboarding window fits the screen (780 px, capped to work area); footer below content; providers in 2 columns; mic test auto-allowed via `WEBVIEW_ARGS` (`--use-fake-ui-for-media-stream`, identical on every window + `tauri.conf.json`, test enforces it).
  - **v0.9.2 (25 Oct, Latest, installed on the owner's PC):**
    - **Signed auto-updates** (`updater.rs`, tauri-plugin-updater 2.14): check 60 s after launch, then every 6 h. Auto-install on = background download, then a sticky "ready · Restart now" toast; off = "available · Update". It refuses while a dictation is active. Settings › About › Updates has the version, Check for updates, the auto-install toggle and a Stable/Beta channel.
    - **Start with Windows** is the `start_with_windows` setting (default on), re-applied at every launch. Only `set_autostart` writes it.
    - **Taskbar/Alt+Tab icon** follows the accent (`appicon.rs::set_taskbar_icon`, WM_SETICON ICON_BIG).
- **Models › On this PC speed line** (v0.9.3): each local model shows a rough time per 10 s of speech or per cleanup (`models::estimate_ms`, from `ref_ms` guesses, scaled by GPU and core count), replaced by the user's measured 30-day average after 3 dictations (`Storage::local_speeds`).
- **Context-aware cleanup** (v0.9.4, published 25 Oct): opt-in "Use text near the cursor" in Settings › General. `context::text_before_cursor` (Windows UIA, macOS AX) runs at chord-down after the mic starts. `cleanup::cursor_tail` trims it to 500 chars, the prompt gets a `<before_cursor>` block, `cleanup::join_after` adds a leading space between words, and the skip gate never bypasses the model mid-sentence. Prompt v4, eval 92.5–94.3% (phase-2.md). Not tried on screen; which apps expose TextPattern is unknown.
- **Cleanup timing and pill** (unreleased, 26 Oct): cleanup's limit is dynamic (`cleanup::timeout_for`: 3 s + 1 s per 40 words, max 12 s), because 1.5 s timed out every cleanup on the owner's PC. While polishing, the pill shows elapsed time and a **Paste now** button (`pill_skip_cleanup` → `pipeline::SKIP_CLEANUP`). After a failure or skip it shows "Pasted · cleanup failed/skipped". "Use text near the cursor" is now on by default (`use_cursor_context`, a new key so 0.9.4's saved off doesn't stick).
- **Accent taskbar/Dock icon** (unreleased, 26 Oct): the taskbar button comes from the Start menu shortcut, so `appicon::apply` writes `<app data>/icons/<accent>.ico` and points our Start menu, desktop and pinned shortcuts at it (re-applied at every launch, so a reinstall's fresh shortcut gets fixed too). macOS sets the Dock icon from the same .ico. Built and unit-tested on both; not seen on screen yet.
- **Release pipeline:**
  - `release.yml`: draft job → notices → Windows + macOS builds. These sign updater packages when the `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)` secrets exist (via `tauri.updater.conf.json`). The `checksums` job builds `latest.json` with `scripts/make_latest_json.py` (self-test `scripts/test_make_latest_json.py`) plus `SHA256SUMS`.
  - `updater-beta.yml` copies each published release's `latest.json` to the `updater-beta` prerelease (the Beta channel).
  - v0.9.2 was signed by hand (`scripts/sign_release.md`). v0.9.3 is the first CI-signed release: its signatures for Windows and macOS (`.app.tar.gz`) name the key ID of the public key in `tauri.conf.json`. The macOS leg must build `--bundles app,dmg`, because dmg alone makes no updater package.

## Decisions and why
- **Updates:**
  - Updates never install without a click, and never mid-dictation (rule 3).
  - `latest.json` is built after both build legs finish, so they can't race.
  - Stable reads `releases/latest/download/latest.json` (skips prereleases); Beta reads `releases/download/updater-beta/latest.json`.
  - Updater key: `%USERPROFILE%\.tauri\yappaflow-updater.key` (+ `.password`, `.pub`). The public key is in `tauri.conf.json`. Losing the private key means installed copies can never update.
- **Releasing:** don't cut a release with no app change. v0.9.3 is the first CI-signed one and the first end-to-end update test.
- **Installer folder:** NSIS appends `\Yappaflow` to the folder the user picks. Kept as is: safer uninstall, and no custom NSIS template to maintain.
- **Rule 6 exceptions** (owner-approved, in CLAUDE.md): `lib.rs::elevate`, `llama_cpp.rs::tie_to_app`, `appicon.rs::set_taskbar_icon`.
- **Command-edit Retry was dropped:** it lost the selection, and phase-5 says edits get no Retry.
- **Code style and CI:** rustfmt `max_width 140`. CI fmt + clippy `-D warnings` block on Windows; the macOS leg stays non-blocking.
- **Engineering conventions:**
  - Parallel work goes in git worktrees, and every branch gets an `opus-reviewer` pass before merge.
  - Cleanup gate skips the LLM unless needed. Prompt changes need an eval re-run, and `docs/prompts/*.md` must stay identical to `providers/prompts/*.txt`.
- **Owner rules:**
  - Install nothing on the owner's PC for local models; native local builds go in CI.
  - Never type or store API keys.
  - Signing certificates and the Apple Developer ID are the owner's to handle.

## Current task
None in flight. Working tree clean (`.claude/launch.json`, the vite preview config, is gitignored).

## Next steps (in order)
1. **v0.9.3 (published 25 Oct, Latest; Stable and Beta manifests both serve it):**
   - Confirm the installed 0.9.2 shows the update toast and updates. Also check that NSIS relaunches the app after the passive install, and that the macOS update works.
2. **On-screen checks of v0.9.2:**
   - The taskbar icon matches the accent on all Windows versions (32 bpp `CreateIcon` alpha; `CreateIconIndirect` with a DIB section is the fallback).
   - About › Updates UI.
   - History offers Retry for a row whose process died mid-STT.
3. **Phase 7 local-model leftovers:**
   - Settle item 7 (CUDA DLL size/licence, and whether the CUDA build runs without NVIDIA) before filling the llama-server pins in `release.yml`.
   - Pill "Warming up" state.
   - Calibrate the rough speed numbers (`ref_ms` in `resources/models.json`, shown on Models › On this PC) against real runs on the 2070 Super and a CPU-only laptop.
4. **Owner:** on-screen acceptance tests for Phases 1–8 (paste results into `docs/phases/phase-N.md`); rotate the OpenAI key; add a Groq key and run `stt_bench` / `cleanup_eval` to pick defaults.
5. **Owner:** back up the updater key offline. Remaining `docs/RELEASE_CHECKLIST.md` items: trademark/domains, Windows + Apple Developer ID signing, a required reviewer on the `release` environment.

## Open questions for the owner
- Default providers after the benchmarks.
- Very wide settings pages: cap the line length?
- Has the updater key been backed up offline?

## Gotchas
- **Never launch an installer or the app from a Claude tool directly.**
  - Claude is an MSIX app, so new `%LOCALAPPDATA%` writes go into `%LOCALAPPDATA%\Packages\Claude_*\LocalCache`, and the registry goes through an overlay. Deleted keys can look missing there.
  - Start them with `explorer.exe <path>`. For a silent install, use an Explorer-launched `.cmd` running `setup.exe /S`.
  - Read the real registry with `reg query` inside an Explorer-launched `.cmd`.
- **`gh` logins are separate:** Claude's `gh` login lives in the sandbox; the owner's PowerShell has its own (`gh auth login`, done 25 Oct).
- **GitHub secrets:** the permission classifier blocks Claude from setting them. The owner runs those commands.
- **Owner's terminal is PowerShell:** give PowerShell syntax (`Start-Process`, not `start ""`).
- **cargo PATH:** cargo isn't on PATH in new shells. In Git Bash: `export PATH="$PATH:$HOME/.cargo/bin"`.
- **Chained shell commands:** in `a && b`, a failed `a` doesn't stop later `;` commands even with `set -e`. Check each step before tagging; a bad chain once tagged a release without its fixes.
- **Python scripts:** write them to the scratchpad and run `python -I file.py`. Heredocs and `printf` with `\U` paths break in Git Bash.
- **Settings and schema:** every new `Settings` field needs a serde default (struct-level `#[serde(default)]`). Schema changes are new migration steps (last is v5). `capabilities/default.json` must keep main, pill, onboarding, toast and tray.
- **WebView args:** WebView2 windows share one environment. Any new window must use `crate::WEBVIEW_ARGS`, or window creation fails.
- **Dev mode:** `npm run tauri dev` needs port 1420 free.
- **Release asset name:** the macOS updater asset is `*.app.tar.gz`. Hand-made ones need `COPYFILE_DISABLE=1 tar`.
- **Local STT on Windows** needs LLVM/libclang, CMake and a short `CARGO_TARGET_DIR`.
- **Unverified provider details** are listed in `docs/providers.md` › Phase 7 build checks.
- **The owner dictates messages:** expect transcription slips, and read for intent.
