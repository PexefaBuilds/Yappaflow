# WORK_PLAN.md: live progress (read after CLAUDE.md)

Updated 2026-10-26. `PLAN.md` §0 stays the product source of truth; this file is "where we are right now".

## Done (all on `main`, pushed; 152 Rust tests pass on Windows, 148 on the Mac; clippy/fmt clean, `npm run build` passes; nothing acceptance-tested on screen)
- **Phases 1–8 built:** pipeline, cleanup + gate + app styles, pill/tray/toasts/settings/onboarding, dictionary/snippets/learning, command mode, Phase 6 app side (per-user NSIS, autostart, admin relaunch, clipboard restore, logs, data controls), Phase 7 cloud + local providers, Phase 8 macOS port (builds and tests on the Mac, never run with a GUI). API cost estimate on History.
- **Public:** https://github.com/PexefaBuilds/yappaflow (one squashed initial commit, noreply author; secret scanning, push protection, private vuln reporting, issue labels). Full local history only on local branch `pre-public-history` (never push).
- **Releases** (unsigned NSIS `.exe` + universal ad-hoc-signed `.dmg` + `SHA256SUMS`):
  - v0.9.0 (24 Oct): first public beta.
  - v0.9.1 (25 Oct): onboarding window fits the screen (780 px, capped to work area); footer below content; providers in 2 columns; mic test auto-allowed via `WEBVIEW_ARGS` (`--use-fake-ui-for-media-stream`, identical on every window + `tauri.conf.json`, test enforces it).
  - v0.9.2 (25 Oct):
    - **Signed auto-updates** (`updater.rs`, tauri-plugin-updater 2.14): check 60 s after launch, then every 6 h. Auto-install on = background download, then a sticky "ready · Restart now" toast; off = "available · Update". It refuses while a dictation is active. Settings › About › Updates has the version, Check for updates, the auto-install toggle and a Stable/Beta channel.
    - **Start with Windows** is the `start_with_windows` setting (default on), re-applied at every launch. Only `set_autostart` writes it.
    - **Window icon** follows the accent (`appicon.rs::set_taskbar_icon`, WM_SETICON ICON_BIG). The taskbar button still stayed azure, because Windows draws it from the shortcut; that's fixed only from v0.9.5 (shortcut recolouring, below).
  - v0.9.3 (25 Oct): first CI-signed release; the owner's 0.9.2 auto-updated to it. v0.9.4 (25 Oct): the owner's PC auto-updated to it (0.9.3 → 0.9.4 seen working).
  - **v0.9.5 (26 Oct, Latest):** dynamic cleanup limit, Paste now, cleanup failed/skipped notices, cursor context on by default, accent taskbar/Dock icon. The owner hasn't installed it yet.
- **macOS watchdog** (v0.9.3): llama-server is tied to the app with a pipe, not PID polling (PID reuse safe; unix test).
- **Models › On this PC speed line** (v0.9.3): each local model shows a rough time per 10 s of speech or per cleanup (`models::estimate_ms`, from `ref_ms` guesses, scaled by GPU and core count), replaced by the user's measured 30-day average after 3 dictations (`Storage::local_speeds`).
- **Context-aware cleanup** (v0.9.4, published 25 Oct; on by default since v0.9.5): "Use text near the cursor" in Settings › General. `context::text_before_cursor` (Windows UIA, macOS AX) runs at chord-down after the mic starts. `cleanup::cursor_tail` trims it to 500 chars, the prompt gets a `<before_cursor>` block, `cleanup::join_after` adds a leading space between words, and the skip gate never bypasses the model mid-sentence. Prompt v4, eval 92.5–94.3% (phase-2.md). Not tried on screen; which apps expose TextPattern is unknown.
- **Cleanup timing and pill** (v0.9.5, 26 Oct): cleanup's limit is dynamic (`cleanup::timeout_for`: 3 s + 1 s per 40 words, max 12 s), because 1.5 s timed out every cleanup on the owner's PC. While polishing, the pill shows elapsed time and a **Paste now** button (`pill_skip_cleanup` → `pipeline::SKIP_CLEANUP`). After a failure or skip it shows "Pasted · cleanup failed/skipped". "Use text near the cursor" is now on by default (`use_cursor_context`, a new key so 0.9.4's saved off doesn't stick).
- **Accent taskbar/Dock icon** (v0.9.5, 26 Oct): the taskbar button comes from the Start menu shortcut, so `appicon::apply` writes `<app data>/icons/<accent>.ico` and points our Start menu, desktop and pinned shortcuts at it (re-applied at every launch, so a reinstall's fresh shortcut gets fixed too). macOS sets the Dock icon from the same .ico. Built and unit-tested on both; not seen on screen yet.
- **Release pipeline:**
  - `release.yml`: draft job → notices → Windows + macOS builds. These sign updater packages when the `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)` secrets exist (via `tauri.updater.conf.json`). The `checksums` job builds `latest.json` with `scripts/make_latest_json.py` (self-test `scripts/test_make_latest_json.py`) plus `SHA256SUMS`.
  - `updater-beta.yml` copies each published release's `latest.json` to the `updater-beta` prerelease (the Beta channel).
  - The macOS leg runs on `macos-15` (v0.9.5): `macos-14` arm64 runners had no capacity (3 attempts never got a machine). If a Mac job is cancelled with "not acquired by Runner", that's GitHub capacity, not our code: `gh run rerun <id> --failed`.
  - v0.9.2 was signed by hand (`scripts/sign_release.md`). v0.9.3 is the first CI-signed release: its signatures for Windows and macOS (`.app.tar.gz`) name the key ID of the public key in `tauri.conf.json`. The macOS leg must build `--bundles app,dmg`, because dmg alone makes no updater package.

## Decisions and why
- **Updates:**
  - Updates never install without a click, and never mid-dictation (rule 3).
  - `latest.json` is built after both build legs finish, so they can't race.
  - Stable reads `releases/latest/download/latest.json` (skips prereleases); Beta reads `releases/download/updater-beta/latest.json`.
  - Updater key: `%USERPROFILE%\.tauri\yappaflow-updater.key` (+ `.password`, `.pub`). The public key is in `tauri.conf.json`. Losing the private key means installed copies can never update.
- **Releasing:** don't cut a release with no app change. v0.9.3 is the first CI-signed one and the first end-to-end update test.
- **Installer folder:** NSIS appends `\Yappaflow` to the folder the user picks. Kept as is: safer uninstall, and no custom NSIS template to maintain.
- **Rule 6 exceptions** (owner-approved, in CLAUDE.md): `lib.rs::elevate`, `llama_cpp.rs::tie_to_app`, `appicon.rs` (taskbar icon, shortcut icons, Dock icon).
- **Cleanup time limit:** dynamic, not unlimited (owner, 26 Oct). A stuck request still falls back to the raw text; the pill's Paste now lets the user stop waiting. Measured: gpt-6-luna takes 1–2 s from the owner's PC; at 1.5 s, 6 of 6 cleanups had timed out.
- **Cursor context:** on by default (owner, 26 Oct). Never read in password fields, Literal apps or command mode; never stored or logged.
- **Taskbar icon:** recolour our shortcuts (option A), not a separate window AppUserModelID (option B would split pinned and running buttons).
- **Release builds:** GitHub CI, not the owner's Mac (reproducible, signing stays in CI). The Mac over SSH is the fallback if CI's Mac runners are stuck.
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
1. **Owner checks v0.9.5 on screen** (Windows, then the Mac):
   - Cleanup now runs: "let's meet Thursday, actually Friday" → "Let's meet Friday."; the pill shows Polishing + timer + Paste now.
   - Paste now → "Pasted · cleanup skipped"; a failure → "Pasted · cleanup failed".
   - Taskbar button and Start menu icon follow the accent (may need the Settings window reopened); the Mac Dock icon too. If the taskbar stays azure, check `%APPDATA%\com.yappaflow.desktop\icons\` and the log for "couldn't recolour shortcut".
   - Text near the cursor: which apps expose it (Notepad, Outlook, browsers, Discord).
   - NSIS relaunches the app after an update; the Mac update works.
2. **Other on-screen checks:** About › Updates UI; History offers Retry for a row whose process died mid-STT.
3. **Phase 7 local-model leftovers:**
   - Settle item 7 (CUDA DLL size/licence, and whether the CUDA build runs without NVIDIA) before filling the llama-server pins in `release.yml`.
   - Pill "Warming up" state.
   - Calibrate the rough speed numbers (`ref_ms` in `resources/models.json`, shown on Models › On this PC) against real runs on the 2070 Super and a CPU-only laptop.
4. **Owner:** on-screen acceptance tests for Phases 1–8 (paste results into `docs/phases/phase-N.md`); rotate the OpenAI key; add a Groq key and run `stt_bench` / `cleanup_eval` to pick defaults.
5. **Owner:** back up the updater key offline. Remaining `docs/RELEASE_CHECKLIST.md` items: trademark/domains, Windows + Apple Developer ID signing, a required reviewer on the `release` environment.

## Open questions for the owner
- Deepgram sends data to training by default (`mip_opt_out` costs more), Gemini free keys may be human-reviewed: show per-provider privacy notes in Settings, and opt out of Deepgram by default? (Provider retention summary from 26 Oct; medium-confidence rows need checking on the providers' own pages before writing to `docs/providers.md`.)
- Default providers after the benchmarks.
- Very wide settings pages: cap the line length?
- Has the updater key been backed up offline?

## Ideas, undecided (owner will decide later; don't build without a go-ahead)
- **Real OpenAI spend on the cost panel** (26 Oct): today the History cost is a local estimate for this PC only (`usage.rs` × the local DB; no provider API is called), so it misses other devices and people sharing the key. OpenAI's Costs API (`/v1/organization/costs`, daily buckets, filterable by project/key) gives the real total, but needs a separate **Admin key** (`sk-admin-…`, Owner-only, Settings › Organization › Admin API Keys; read-only scope is enough). A normal or "unrestricted" project key can't read it. Sketch: an optional admin-key field (OS keychain) on the cost panel, real total shown next to the estimate, via a small billing hook on the provider seam (rule 4), not an OpenAI special case. Risk: a recent OpenAI forum report of the endpoint returning 404; check before building. Owner leaning: keep the estimate and use OpenAI's dashboard for now.

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
- **Dev mode:** `npm run tauri dev` needs port 1420 free. On the owner's PC, 1420 is often taken by another app ("Tandem"); for a browser preview use the gitignored `vite-alt` config (port 1431). The pill renders in the browser with a mocked `window.__TAURI_INTERNALS__`.
- **Mac copy of the repo** (`~/ShahidPersonalProjects/Yappaflow`) is a plain file copy, not a git checkout: sync the whole tree (tar over ssh, skip `target`) before `cargo test` there; use `-o ServerAliveInterval=30`, long builds drop the connection otherwise.
- **Owner's data on this PC:** `%APPDATA%\com.yappaflow.desktop\yappaflow.db` (History; read-only with `mode=ro`), log at `%LOCALAPPDATA%\com.yappaflow.desktop\logs\yappaflow.log`. `cleanup_ms` ≈ the limit in `timings` means cleanup timed out.
- **Eval scoring ignores case** unless a case is tagged `exact_case`; the score moves ~3 points run to run.
- **Release asset name:** the macOS updater asset is `*.app.tar.gz`. Hand-made ones need `COPYFILE_DISABLE=1 tar`.
- **Local STT on Windows** needs LLVM/libclang, CMake and a short `CARGO_TARGET_DIR`.
- **Unverified provider details** are listed in `docs/providers.md` › Phase 7 build checks.
- **The owner dictates messages:** expect transcription slips, and read for intent.
