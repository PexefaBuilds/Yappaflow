# Phase 6: Polish (installer, startup, updates, signing, robustness)

## Scope
1. **Installer:** NSIS, per-user install (no admin rights). It includes the WebView2 bootstrapper and the VC++ runtime if needed.
2. **Start with Windows** via `tauri-plugin-autostart`, on by default after onboarding.
3. **Auto-update** via `tauri-plugin-updater` with signed update manifests hosted on GitHub Releases.
   - The private signing key lives **only** in the CI secret store.
   - Stable and Beta channels (About page).
4. **Code signing:** Azure Trusted Signing (cheapest path for an individual) or an OV certificate. Without one, SmartScreen will warn; document that in the README until signing is set up.
5. **Admin windows:** opt-in "Work in admin windows" (Advanced) relaunches the app elevated via `ShellExecute runas`. Show a clear warning.
6. **Full clipboard restore:** preserve all formats (enumerate `EnumClipboardFormats` and copy each `HGLOBAL`) for files, HTML, RTF and images.
7. **Crash reporting:** a local crash log file. An optional upload is **off by default**; if added, it must never include audio, text or keys.
8. **Data controls** (Advanced): open data folder, export history (JSON/CSV), delete all history, recordings retention.
9. **Logs:** rotating log file, keys redacted, transcripts only at debug level.

## Acceptance tests
| # | Test | Pass when |
|---|---|---|
| 6.1 | Install on a clean Windows 11 VM without admin | Installs, launches, dictates |
| 6.2 | Publish v0.1.1 to the beta channel | A v0.1.0 beta install updates itself; the signature is verified |
| 6.3 | Copy a file in Explorer → dictate → Ctrl+V in Explorer | The file is pasted (all formats restored) |
| 6.4 | Admin mode on → dictate into an elevated PowerShell | Works |
| 6.5 | Delete all history | DB rows and WAVs are gone |
| 6.6 | grep the logs for "sk-" | No matches |

## Results
**App side built (2026-10-24); acceptance tests not run yet.** Unit tests pass (76, plus an ignored real-clipboard HTML+text snapshot test that passes on Windows).
- Installer: NSIS `installMode: currentUser`, WebView2 `downloadBootstrapper` (silent), VC++ runtime statically linked (`build.windows.staticVCRuntime`, the Tauri default made explicit), so no redistributable is needed.
- Start with Windows: `tauri-plugin-autostart`, switched on the first time onboarding finishes (release builds only), switch in General. Called through our own commands, so no JS package or capability entry.
- Admin mode (Advanced): `ShellExecuteW runas` relaunch after a confirm dialog; the new instance waits for the old one to exit (`--wait-pid`) so single-instance doesn't swallow it. While on, every start asks UAC.
- Clipboard: `insert::clip::Snapshot` copies every HGLOBAL format (skips GDI/metafile/private handles). If it can't snapshot, we type instead of pasting.
- Logs: `%LOCALAPPDATA%\com.yappaflow.desktop\logs\yappaflow.log`, 1 MB, one backup, keys and bearer tokens redacted, info by default. `crash.log` beside it (local only, no upload).
- Data (Advanced): open data/log folders, export JSON/CSV, delete all history (rows + VACUUM + every recording file).

**25 Oct 2026, item 3 (auto-update): built, not acceptance-tested.** `updater.rs` (tauri-plugin-updater 2.14): checks 60 s after launch and every 6 h against `releases/latest/download/latest.json` (Stable) or `releases/download/updater-beta/latest.json` (Beta, filled by `updater-beta.yml` for every published release). Auto-install on (default) = download in the background, then the "is ready · Restart now" toast; off = "is available · Update". Update toasts stay until answered; nothing installs without a click. About has the version status, Check for updates, the auto-install toggle and the channel. `release.yml` signs only when the key secrets exist; `scripts/make_latest_json.py` builds the manifest after both legs. Also: Start with Windows is a setting (default on) re-applied at launch, since a manual reinstall's uninstaller deletes the Run entry.

**Left for the owner:** back up the updater key offline, test 6.2 with the first CI-signed release (v0.9.3), code signing (Azure Trusted Signing or OV) and the SmartScreen note in the README. Run 6.1 and 6.3–6.6 on a real install.
