## What and why

<!-- One or two sentences. Link the task row(s) from the phase doc, e.g. "Phase 1, tasks 1.3–1.4". -->

**Phase:** <!-- e.g. Phase 1: Bare pipeline (docs/phases/phase-1.md) -->

## Checklist

- [ ] This PR stays inside the phase above (anything new was discussed and agreed first)
- [ ] `npm run build` passes locally
- [ ] `cargo test` passes locally (in `src-tauri`)
- [ ] Prompt or provider changes: the cleanup eval (`docs/evals/cleanup.jsonl`, see CONTRIBUTING) passes; results pasted below
- [ ] Acceptance tests from `docs/phases/phase-N.md` run, results pasted below and into that file's **Results** section (timings included)
- [ ] No secrets: no API keys, `.env`, certificates or private keys; `gitleaks dir .` (or `gitleaks git .`) shows no leaks
- [ ] No full transcripts, audio or keys logged at info level
- [ ] Schema changes are a new step in `storage/migrations.rs` (old steps untouched)
- [ ] UI changes: screenshots attached and compared with the matching `design/*.dc.html`; colours come from `design/tokens.css` and the accent uses `var(--accent)`
- [ ] CI is green

## Acceptance test results

<!-- Paste the results here, e.g.
| # | Test | Result | Notes / timing |
|---|------|--------|----------------|
| 1 | Notepad, Chrome, Slack, VS Code, Terminal | Pass | ~1.2 s release → paste |
-->

## Screenshots (UI changes only)

<!-- Built app on the left, design/*.dc.html on the right. Name the design file. -->

| Built | Design (`design/....dc.html`) |
|---|---|
|  |  |
