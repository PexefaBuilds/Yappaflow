# Release checklist (owner only)

Things only the owner can do before the repo goes public. Full context: `docs/phases/phase-9.md`. This is a checklist, not legal advice; ask a lawyer if anything is unclear.

## IP pre-flight (before anything is public)
- [x] Written confirmation from the employer (email is fine) that you personally hold the copyright and may publish under MIT on a personal account. Verbal OK received 24 Oct 2026. Keep the copy outside the repo.
- [ ] Re-read the employment contract's IP / moonlighting clause against that confirmation.
- [ ] Repo has no employer or client material: names, logos, internal URLs, credentials, client project references (`grep` per test 9.4).
- [ ] Decide which internal docs go public (`PLAN.md`, `CLAUDE.md`, `WORK_PLAN.md`, `design/`, `docs/`). No competitor names or branding anywhere.

## Placeholders to fill
- [x] `LICENSE`: replace `<OWNER NAME>` in the copyright line.
- [x] `CODE_OF_CONDUCT.md`: replace `<CONDUCT CONTACT EMAIL>`.
- [x] `SECURITY.md`: replace `<SECURITY CONTACT EMAIL>`, or delete the line and rely on private vulnerability reporting.
- [ ] `README.md`: download links and the GIF (TODOs).

## History
- [x] Publish a fresh squashed history (recommended, since the code started on a work drive), or audit every commit with `gitleaks git` and get 0 findings.
- [x] Commit author emails switched to your GitHub noreply address (`git config user.email <id>+<user>@users.noreply.github.com`).

## Name and trademark ("Yappaflow", classes 9 and 42)
Record findings and dates in phase-9.md Results.
- [ ] USPTO trademark search.
- [ ] EUIPO eSearch / TMview.
- [ ] WIPO Global Brand Database.
- [ ] UAE Ministry of Economy trademark register.
- [ ] Domains: `yappaflow.com`, `.app`, `.dev`.
- [x] GitHub: `PexefaBuilds/yappaflow` created; reserve the `yappaflow` org.
- [ ] winget ID `<Publisher>.Yappaflow` (search `microsoft/winget-pkgs`); Homebrew cask `yappaflow` (search `homebrew/homebrew-cask`).

## GitHub repo settings
- [x] Secret scanning **on**.
- [x] Push protection **on**.
- [x] Private vulnerability reporting **on** (Security tab).
- [ ] `release` environment created with you as required reviewer; signing secrets stored there, not at repo level.
- [x] Before going public: drop `continue-on-error` from the fmt and clippy steps in `.github/workflows/ci.yml` once they're green.

## Signing (secrets in the `release` environment, never in git)
- [ ] Windows code-signing: OV/EV certificate or Azure Trusted Signing (Phase 6). Configure `bundle.windows` in `tauri.conf.json` and uncomment the step in `release.yml`.
- [ ] Apple Developer ID certificate + notarisation API key (Phase 8): `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_API_ISSUER`, `APPLE_API_KEY` (+ the .p8).
- [ ] Tauri updater key: `npm run tauri signer generate -- -w <path outside the repo>`. Back up the private key and password offline; losing it means existing installs can't update. Store `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` as secrets; commit only the public key (in `tauri.conf.json`). Then set `includeUpdaterJson: true` in `release.yml`.

## First public release (v0.9.0 "public beta")
- [ ] Phases 1–8 acceptance tests pass on clean machines; results pasted.
- [x] `CHANGELOG.md` and release notes with known issues.
- [x] Version bumped in `tauri.conf.json`, `Cargo.toml`, `package.json`.
- [x] Release built by CI only; installers downloaded from the draft and smoke-tested; `SHA256SUMS` checked.
- [x] Repo switched to public; secret scanning and push protection confirmed on.
