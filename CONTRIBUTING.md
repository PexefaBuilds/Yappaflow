# Contributing to Yappaflow

Thanks for helping. For anything bigger than a bug fix (a new feature, a UX change, a new dependency), open an issue first so we can agree on the approach before you build it.

## Build

Prerequisites (Windows): Rust with the MSVC toolchain, Visual Studio Build Tools 2022 with *Desktop development with C++*, Node.js 22 LTS or newer.

```powershell
npm install                     # once
npm run tauri dev               # run the app (hot-reloads the UI)
cd src-tauri; cargo test        # unit tests (pure modules run on any OS)
npm run build                   # type-check + build the UI (needed before a bare `cargo check`)
npm run tauri build             # NSIS installer → src-tauri/target/release/bundle/nsis/
```

Before you push, CI runs: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, `npm run build`, `cargo deny check licenses` and gitleaks.

## Secret scanning (please set this up)

We use [gitleaks](https://github.com/gitleaks/gitleaks) on every commit:

```powershell
pip install pre-commit          # or: pipx install pre-commit
pre-commit install              # runs gitleaks on staged changes before each commit
```

Never commit API keys, `.env` files, certificates or signing keys. Keys belong in the OS keychain only. If gitleaks flags a real false positive, add the narrowest possible entry to `.gitleaks.toml` and explain it in the PR.

## Ground rules

- **Clean room.** Build only from public documentation and our own designs. Never copy, decompile or inspect another dictation product's code, binaries, prompts, assets or UI, and never name or brand anything after one.
- **Never lose a dictation.** Audio is saved before any network call; every failure path leaves the text on the clipboard or a Retry in History.
- **The provider seam.** The pipeline talks only to the `SttProvider` and `CleanupProvider` traits. Don't special-case a provider in `pipeline.rs`.
- **The keyboard hook stays tiny.** `hotkey/mod.rs::hook_proc` must not allocate, lock, log or do IO.
- **OS-specific code** lives only in `hotkey/`, `insert.rs`, `context.rs` and `secrets.rs`, behind `#[cfg(...)]`.
- **Model IDs and endpoints are settings**, not constants scattered in code.
- **Never log** a key, audio, or a full transcript at info level.
- **Schema changes:** append a new step to `storage/migrations.rs`; never edit an old one.
- **Rust:** `anyhow` in app code, `Result<T, String>` from Tauri commands, no `unwrap()` on anything from the OS, the network or user input. Pure logic goes in its own module with `#[cfg(test)]` tests.

## Design tokens

Use the values in `design/tokens.css`, never ad-hoc colours. The accent is user-selectable, so always use `var(--accent)`. Accessibility: real `<button>`/`<input>`/`<label>`, visible focus rings, 4.5:1 text contrast, and `dir="auto"` on any user text.

## Prompt changes

The cleanup prompt lives in both `docs/prompts/cleanup.md` and `src-tauri/src/providers/prompts/cleanup.txt`; keep them identical. Any prompt change must pass the eval set in `docs/evals/cleanup.jsonl` (≥ 90% overall, 100% of `must_pass` and `injection` cases). It uses the key saved in the app:

```powershell
cd src-tauri; cargo test --features evals -- --ignored --nocapture
```

Paste the score in your PR.

## Adding a provider

1. Add verified facts to `docs/providers.md`: endpoint, model IDs, vocabulary parameter and limits, language caveats, price, and links to the sources. No source, no provider.
2. Reuse the OpenAI-compatible client with a preset if you can; otherwise add `providers/<id>.rs` implementing `SttProvider` or `CleanupProvider`.
3. Declare the vocabulary strategy, language style and (for cleanup) the reasoning-off patch as data.
4. Register it in `registry.rs` and add its models and price to the Models UI data. Never touch `pipeline.rs`.
5. Store the key via `secrets.rs` as `<id>_api_key`; never log it; map auth errors to the key toast.
6. Add unit tests for request building (no network), run the eval with your key, and paste the results in the PR.

## Commits and pull requests

- Small commits, one concern each, message in the imperative ("Add Groq STT provider").
- UI changes: include before/after screenshots.
- By contributing you agree your work is released under the MIT licence.
- Be kind: see [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
