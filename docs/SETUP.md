# Setup (Windows)

How to set up Yappaflow locally, push it to a private GitHub repo on your **personal** account, and get it running.

All commands are PowerShell. Run them in a normal (non-admin) window unless a step says otherwise.

---

## 1. Prerequisites

Install once. Close and reopen PowerShell after installing so `PATH` picks everything up.

```powershell
# Git
winget install --id Git.Git -e

# Rust (rustup), then make sure the MSVC toolchain is the default
winget install --id Rustlang.Rustup -e
rustup default stable-msvc

# Visual Studio 2022 Build Tools with the "Desktop development with C++" workload
# (MSVC compiler + Windows SDK). Run from an admin PowerShell; takes a while.
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"

# Node.js LTS (22 or newer)
winget install --id OpenJS.NodeJS.LTS -e

# GitHub CLI
winget install --id GitHub.cli -e

# gitleaks (secret scanner, see section 6)
winget install --id Gitleaks.Gitleaks -e
```

Check:

```powershell
git --version; rustc --version; cargo --version; node --version; npm --version; gh --version; gitleaks version
rustup show            # active toolchain should end in -x86_64-pc-windows-msvc
```

WebView2 is already part of Windows 10/11.

---

## 2. Where the project lives

The project lives locally at:

```
D:\Claude-Related\ClaudeFiles_Personal\Yappaflow
```

GitHub (a private repo on your personal account) is how it moves between your PCs, and it's your backup. Rust build output (`src-tauri\target`, several GB) stays inside the project and is git-ignored.

### On another PC

```powershell
New-Item -ItemType Directory -Force D:\Claude-Related\ClaudeFiles_Personal | Out-Null
cd D:\Claude-Related\ClaudeFiles_Personal
gh repo clone yappaflow Yappaflow
cd Yappaflow
npm install
```

**Switching PCs:** before you leave a PC, commit and push (`git add -A; git commit -m "…"; git push`). On the next PC, start with `git pull`.

### Sign in to GitHub with your personal account

```powershell
gh auth login          # GitHub.com → HTTPS → log in with a web browser
gh auth status
```

> **Check `gh auth status` shows your personal account, not a work one.** If both are logged in, the one marked *Active account: true* is used. Switch with `gh auth switch --user <your-personal-username>`. Then run `gh auth setup-git` so `git push` uses the same account.

### Use your GitHub noreply email (keeps your real address out of commit history)

Set it for **this repo only** (`git config` without `--global`), so your work identity elsewhere is untouched. Run this after `git init` below:

```powershell
git config user.name "Your Name"
git config user.email "<id>+<user>@users.noreply.github.com"
```

Your `<id>+<user>` address is shown at <https://github.com/settings/emails> (tick *Keep my email addresses private*). Or get it from the CLI:

```powershell
gh api user --jq '"\(.id)+\(.login)@users.noreply.github.com"'
```

### Create the repo and push

```powershell
cd D:\Claude-Related\ClaudeFiles_Personal\Yappaflow
git init -b main

# set user.name / user.email as above, then:
git config user.email          # confirm it's the noreply address

gitleaks dir .                 # must say "no leaks found" (section 6)

git add .
git status                     # sanity-check: no node_modules, target, dist, .env, *.key
git commit -m "Initial import of Yappaflow (Phase 1 code, plan, designs)"

gh repo create yappaflow --private --source . --push
```

The repo is created as `<your-personal-username>/yappaflow`, private. Keep it private until the Phase 9 open-source release.


---

## 3. First run

```powershell
cd D:\Claude-Related\ClaudeFiles_Personal\Yappaflow
npm install
npm run tauri dev
```

The code has **never been compiled**, so expect a few compile errors the first time. Fixing them is the first Phase 1 task (see section 5).

Unit tests:

```powershell
npm run build                                   # creates dist\, which Tauri needs before cargo builds
cargo test --manifest-path src-tauri/Cargo.toml
```

Once `npm install` has produced `package-lock.json`, commit it, then switch `npm install` to `npm ci` in `.github/workflows/ci.yml` (and drop the `cache-dependency-path` line).

---

## 4. Old copies on Google Drive

The copies in `G:\Shared drives\…\CustomTools\Yappaflow` and `…\CustomTools\YapAndFlow` are now out of date. Once the local project builds and is pushed to GitHub, delete them (or leave a `MOVED.txt` note) so nobody edits a stale copy.

---

## 5. Open it in Claude Code

```powershell
cd D:\Claude-Related\ClaudeFiles_Personal\Yappaflow
claude
```

`CLAUDE.md` in the repo root is picked up automatically, so Claude starts with the rules and current status.

Suggested first prompt:

> Read CLAUDE.md and docs/phases/phase-1.md, then get the app compiling on Windows without changing behaviour.

Do this on a `phase-1-build` branch (section 7).

---

## 6. Secret hygiene

API keys live only in Windows Credential Manager, never in files. `.gitignore` already excludes `.env*`, `*.pfx`, `*.p12`, `*.pem`, `*.key` (private keys, including the Tauri updater key; `*.key.pub` public keys are fine) and `/models/`.

Run gitleaks **before every push**:

```powershell
gitleaks dir .     # scans the working tree
gitleaks git .     # scans all commits
```

Both must report `no leaks found`. CI also runs gitleaks on every push and PR. `.gitleaks.toml` uses the default rules; add an allowlist entry there only for a proven false positive.

If a real key ever gets committed: **revoke it at the provider first**, then clean history. Rewriting history alone isn't enough once it has been pushed.

---

## 7. Branch and PR habit

One branch per phase, merged into `main` by PR:

```powershell
git switch main; git pull
git switch -c phase-1-build        # then phase-2-cleanup, phase-3-ui, ...

# work, small commits ("Fix windows-crate signature in insert.rs")
git push -u origin phase-1-build
gh pr create --fill                # the PR template has the checklist
gh pr checks --watch               # CI must pass
gh pr merge --squash --delete-branch
```

- **CI must be green before merging.** GitHub Free doesn't enforce branch protection on private repos, so this is a habit, not a lock. It can be enforced once the repo is public.
- Paste the phase's acceptance-test results into the PR and into `docs/phases/phase-N.md` → **Results** before calling a phase done.
- Attach screenshots for UI changes next to the matching `design/*.dc.html`.
