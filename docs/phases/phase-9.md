# Phase 9: Open-source release

## Goal
Yappaflow is public on the owner's personal GitHub under MIT: anyone can download a signed installer for Windows or macOS, or build it from a fresh clone, and can read exactly where their audio and text go. Nothing secret, nobody else's IP, and no unlicensed asset ships.

## Scope
1. **IP pre-flight (do this before anything is public).** This is a checklist, not legal advice; if anything is unclear, ask a lawyer.
   - [ ] The code was started on the owner's work drive. Get **written** confirmation (email is fine) from the employer that the owner personally holds the copyright and may publish it under MIT on a personal account. Keep a copy outside the repo.
   - [ ] Re-read the employment contract's IP / moonlighting clause against that confirmation.
   - [ ] Repo contains no employer or client material: names, logos, internal URLs, credentials, client project references.
   - [ ] Decide the copyright line in `LICENSE` (e.g. "Copyright (c) 2026 <owner's name>").
   - [ ] Decide which internal docs go public (`PLAN.md`, `CLAUDE.md`, `design/`, `docs/`). Public docs and the product must not name or brand themselves after any competitor (CLAUDE.md rule 1); keep a neutral clean-room statement.
   - [ ] Decide on history: publish a fresh squashed history (recommended, since it started on a work drive), or audit every commit with gitleaks. Commit author emails switched to the GitHub noreply address.
2. **Licences.**
   - `LICENSE`: MIT.
   - `THIRD_PARTY_NOTICES.md` generated in CI: Rust crates via `cargo about` (or `cargo-license`), npm via `license-checker`. Includes whisper.cpp (MIT), llama.cpp / `llama-server` sidecar (MIT), ggml (MIT), Tauri (MIT/Apache-2.0).
   - Fonts: Unbounded and Reem Kufi are **SIL OFL 1.1**. Ship `licenses/OFL.txt` (with both copyright lines) in the repo and inside the installers.
   - Model licences, shown in the catalogue and in About › Licences: OpenAI Whisper weights MIT; Qwen3.5 and Ministral 3 Apache-2.0. We link to Hugging Face rather than redistribute. verify: Gemma 4 licence before adding it to the catalogue.
   - verify: NVIDIA CUDA redistributable terms, if Phase 7 ships CUDA DLLs; add the required notice.
   - `cargo-deny` in CI with an allowlist (MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode, OFL-1.1, MPL-2.0 for unmodified deps); fail on GPL/AGPL/unknown.
3. **Docs at the repo root:**
   - `README.md` for users: what it does (GIF), download links, quick start (cloud key or local), supported providers, privacy summary linking PRIVACY.md, SmartScreen/Gatekeeper notes, build from source, licence.
   - `CONTRIBUTING.md`: build steps (from CLAUDE.md), commit style, provider plugin checklist (phase 7), "prompt changes must pass `docs/evals/cleanup.jsonl`", design tokens rule.
   - `CODE_OF_CONDUCT.md`: Contributor Covenant 2.1 with a contact address.
   - `SECURITY.md`: report privately via GitHub **Private vulnerability reporting** (Security Advisories) or a dedicated email; acknowledge within 7 days; scope includes key handling, updater signing, prompt injection that types unintended text. Never ask reporters to post keys.
   - `PRIVACY.md`, plain words and a table of every network destination:
     - your chosen STT provider: the audio clip + dictionary terms;
     - your chosen cleanup provider: the transcript, app style and dictionary terms (verify against code exactly what `CleanupContext` sends);
     - GitHub Releases: update check (app version; GitHub sees your IP);
     - Hugging Face: model downloads (local mode only).
     - Nothing else: no accounts, analytics or telemetry; crash upload off. Local data: recordings (newest 50), history in `yappaflow.db`, keys in the OS keychain; how to delete all of it.
4. **Secret scanning.**
   - Pre-commit: `gitleaks` via `.pre-commit-config.yaml` (`gitleaks protect --staged`); documented in CONTRIBUTING.
   - CI: `gitleaks` over the full history on every push and PR (verify: `gitleaks-action` needs a licence only for organisation accounts; otherwise run the binary).
   - `.gitleaks.toml` adds rules for provider key shapes (`sk-`, `sk-ant-`, `gsk_`, `AIza`, Deepgram/ElevenLabs/AssemblyAI patterns, Tauri updater key) and allowlists only test fixtures.
   - Turn on GitHub secret scanning and **push protection** on the repo.
5. **CI (GitHub Actions).**
   - PR workflow: `cargo fmt --check`, `clippy -D warnings`, `cargo test`, `npm run build`, `cargo-deny`, gitleaks, on `windows-latest` and `macos-14`.
   - Release workflow on tag `v*` with `tauri-apps/tauri-action`: Windows NSIS (signed, Phase 6) and macOS universal `.dmg` (signed + notarised, Phase 8); updater artifacts and `latest.json` signed with the Tauri updater key; `SHA256SUMS`; draft release for a human to publish.
   - Beta channel: tags `v*-beta.*` → prerelease + separate beta manifest.
   - Signing secrets live in a protected `release` environment with required reviewer; third-party actions pinned by commit SHA.
6. **Name and trademark check for "Yappaflow"** (classes 9 and 42):
   - USPTO trademark search, EUIPO eSearch / TMview, WIPO Global Brand Database, UAE Ministry of Economy register (owner is UAE-based). Look for identical and confusingly similar marks, including other "…flow" dictation products.
   - Domain (`yappaflow.com` / `.app` / `.dev`), GitHub (`<owner>/yappaflow` and reserve the `yappaflow` org), winget ID (`<Publisher>.Yappaflow`, search `microsoft/winget-pkgs`), Homebrew cask `yappaflow` (search `homebrew/homebrew-cask`).
   - Record findings and dates in Results. If there's a conflict, rename before launch (PLAN.md header lists what to change).
7. **Landing page:** one static page on GitHub Pages (`site/`), built with `design/tokens.css` and bundled fonts. Download buttons point at the latest release; privacy summary; links to source. No analytics, trackers or external fonts.
8. **Issue templates** (`.github/ISSUE_TEMPLATE/`): bug report (OS, version, cloud/local, provider + model, steps, redacted log; a bold "never paste your API key"), feature request, provider request (asks for a docs link per the plugin checklist). PR template with checks: evals run, no secrets, screenshots for UI changes. Labels: `provider`, `macos`, `windows`, `local-models`, `good first issue`.
9. **First public release checklist** (propose v0.9.0 "public beta"):
   - [ ] Phases 1–8 acceptance tests pass on clean machines; results pasted.
   - [ ] `CHANGELOG.md` and release notes with known issues.
   - [ ] README screenshots/GIF from the real app.
   - [ ] Version bumped in `tauri.conf.json`, `Cargo.toml`, `package.json`.
   - [ ] Release built by CI only, never locally; installers downloaded and smoke-tested from the release page.
   - [ ] Updater tested from the previous beta (Phase 6 test 6.2).
   - [ ] Repo switched to public; secret scanning + push protection confirmed on.
   - [ ] After release: winget submission (`wingetcreate`), Homebrew cask PR (verify the cask notability rules), landing page live.

## Out of scope
Monetisation, hosted services or accounts, telemetry, a Mac App Store or Microsoft Store listing, translations of the docs.

## Design refs
`SetAbout` (MIT line, Source code / Report a problem / Licences links, "No accounts, no analytics"), `design/tokens.css` for the landing page.

## Acceptance tests
| # | Test | Pass when |
|---|---|---|
| 9.1 | Employer sign-off (verbal OK received 24 Oct 2026; keep the written confirmation on file) | Written confirmation on file; IP checklist above fully ticked |
| 9.2 | Fresh clone on a clean Windows 11 VM and a clean Mac, following only README/CONTRIBUTING | Builds and runs with no undocumented step; `cargo test` passes |
| 9.3 | `gitleaks` over the full history, plus GitHub secret scanning | 0 findings |
| 9.4 | `grep` the repo for the employer's name, client names, internal paths, competitor names | 0 hits (the clean-room statement names no other product) |
| 9.5 | Download the Windows installer on a clean VM | Signed (publisher shown), no SmartScreen "unknown publisher" block |
| 9.6 | Download the `.dmg` on a clean Mac | Gatekeeper opens it without warning; notarised (`spctl`) |
| 9.7 | Proxy capture (mitmproxy or Fiddler) of: onboarding, 10 cloud dictations, 10 local dictations, update check, model download | Every host seen is listed in PRIVACY.md; nothing unlisted; no traffic at all during local dictations |
| 9.8 | `cargo-deny check licenses` and the notices job | Pass; `THIRD_PARTY_NOTICES.md` and `OFL.txt` present in both installers |
| 9.9 | About › Licences | Shows MIT, third-party notices, OFL, and the licence of each downloaded model |
| 9.10 | Push a test tag on a fork without secrets | Release workflow fails safely; no secret printed in logs |
| 9.11 | Open each issue template | Fields render; the key warning is visible |
| 9.12 | Name check | Results recorded with dates; no blocking conflict, or rename done |

## Results
_(sign-off date, name-check findings, proxy capture host list, release links)_

**24 Oct 2026: groundwork done, owner items pending.** Added LICENSE (copyright placeholder), README, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, PRIVACY (checked against `pipeline.rs`, `providers/*`, `cleanup.rs`, `learn.rs`), `licenses/OFL.txt`, `deny.toml`, gitleaks rules + pre-commit hook, CI matrix (Windows + macOS-14, fmt, clippy, cargo-deny, gitleaks) and a draft-only release workflow with signing commented out. None of it has run yet (no remote). Owner items: `docs/RELEASE_CHECKLIST.md`. Not started: THIRD_PARTY_NOTICES generation, issue/PR templates, landing page, About › Licences.
