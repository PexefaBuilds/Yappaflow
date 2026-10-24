# Phase 8: macOS

## Goal
Yappaflow works on a Mac the way it does on Windows: hold **fn (🌐)**, speak, release, and text lands in any app. It lives in the menu bar, asks for its three permissions clearly, and installs from a signed, notarised universal `.dmg` without Gatekeeper warnings.

**You need a Mac to develop this phase** (permissions, event taps and signing can't be tested elsewhere). CI can build and sign on GitHub's `macos-14` runners; pure modules (`session.rs`, providers, storage) keep running `cargo test` on any OS. Only the four OS folders change (CLAUDE.md rule 6).

## Scope
1. **`hotkey/macos.rs`: CGEventTap** on its own thread with a `CFRunLoop`.
   - Session tap, head insert, mask = `flagsChanged | keyDown | keyUp`. fn is `flagsChanged` with keycode 63 (`kVK_Function`) and `kCGEventFlagMaskSecondaryFn`.
   - Same rule as `hook_proc`: the callback only classifies and posts to the channel. No allocation, locks, logs or IO.
   - Handle `kCGEventTapDisabledByTimeout` / `ByUserInput` by calling `CGEventTapEnable(tap, true)` and counting re-enables in the log.
   - Default chord: **hold fn**. Double-tap fn = hands-free (existing state machine). fn + another key (fn+arrows = Home/End etc.) cancels, as "other key while held" already does. Esc cancels and is swallowed (needs an active tap, so Accessibility).
   - Fallback chords offered in Hotkeys: Right ⌥ (hold), ⌃⌥ (hold). Command mode: propose **fn + ⌃** (owner to confirm).
2. **fn / 🌐 conflict.** macOS can bind 🌐 to Show Emoji & Symbols, Change Input Source or Start Dictation, which fires alongside us.
   - Detect the current setting: `defaults read com.apple.HIToolbox AppleFnUsageType` (verify the value mapping; expected 0 = Do Nothing).
   - If it isn't "Do Nothing", onboarding and Settings › Hotkeys show: "Set **Press 🌐 key to: Do Nothing** in System Settings › Keyboard" with an **Open Keyboard settings** button. We never write the system default ourselves.
   - verify: whether Apple Dictation's shortcut ("Press 🎤/fn twice") also collides with our double-tap; if so, add it to the same notice.
3. **`insert.rs` (macOS, behind `#[cfg(target_os = "macos")]`):**
   - Wait (≤ 500 ms) until fn/⌘/⌥/⇧ are physically up (`CGEventSourceFlagsState`).
   - Save every `NSPasteboard.general` item and type; write our text plus the `org.nspasteboard.TransientType` and `org.nspasteboard.ConcealedType` markers (clipboard managers skip these, like Windows' history exclusion).
   - Post ⌘V: `CGEventCreateKeyboardEvent` keycode 9 (`kVK_ANSI_V`) with `kCGEventFlagMaskCommand`, to `kCGHIDEventTap`.
   - Restore after 250 ms **only if** `changeCount` is still ours.
   - **Fallback** (per-app "type" list, or paste failed): Unicode typing with `CGEventKeyboardSetUnicodeString` in chunks (verify the max UTF-16 units per event; start at 20).
   - **Secure input** (`IsSecureEventInputEnabled()`, e.g. password fields, Terminal "Secure Keyboard Entry"): don't post keys. Leave the text on the clipboard and show "Copied · press ⌘V".
4. **`context.rs` (macOS):** `NSWorkspace.shared.frontmostApplication` → `bundleIdentifier` + `localizedName`, captured at chord-down. Window title (for `title_contains` rules such as "Gmail") via the Accessibility API (`AXFocusedWindow` → `AXTitle`), since we already hold Accessibility; never via screen-recording APIs.
5. **`secrets.rs` (macOS):** Keychain via `keyring`'s apple-native backend, service `yappaflow`, user `<provider>_api_key`. Unsigned dev builds may re-prompt for Keychain access after each rebuild; that's expected.
6. **Permissions onboarding** (`OnbMacPerms`, step 3 of 5) for Microphone, Input Monitoring and Accessibility:
   - Each row: why we need it, an **Open settings** deep link, and a live tick. Poll every 1 s while the step is visible.
   - Microphone: `AVCaptureDevice.authorizationStatus(for: .audio)`; request on button press; `NSMicrophoneUsageDescription` in Info.plist.
   - Input Monitoring: `CGPreflightListenEventAccess()` / `CGRequestListenEventAccess()`. Accessibility: `AXIsProcessTrusted()` / `AXIsProcessTrustedWithOptions` (prompt).
   - Deep links: `x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone`, `…?Privacy_ListenEvent`, `…?Privacy_Accessibility`. verify on macOS 14, 15 and 26.
   - verify: whether Input Monitoring needs an app relaunch to take effect. If so, show **Restart Yappaflow** once it's granted.
   - Continue is disabled until all three are granted. If one is revoked later, show a toast with **Open settings** and fall back to clipboard-only.
7. **Menu bar instead of tray:** Tauri tray icon = `NSStatusItem`, monochrome **template** image so it follows light/dark. The custom `TrayMenu` popup opens under the status item. App runs as an accessory app (`LSUIElement`, no Dock icon); the Settings window still opens normally.
8. **Pill:** bottom-centre of the screen under the mouse, 12 px above `NSScreen.visibleFrame` (which excludes the Dock, whatever its side, and the menu bar). Must never take focus: non-activating panel, all Spaces, over full-screen apps. verify: whether Tauri 2 window options suffice or we need an `NSPanel` (e.g. the community `tauri-nspanel` plugin; check its licence and upkeep).
9. **Open at login** via `tauri-plugin-autostart`, on by default after onboarding. verify: whether it uses `SMAppService` (macOS 13+, shows in Login Items) or a LaunchAgent; prefer `SMAppService`.
10. **App-style defaults remapped to bundle IDs.** Migration adds `match_kind = 'bundle_id'` and seeds:
    - casual: `com.tinyspeck.slackmacgap`, `com.hnc.Discord`, `ru.keepcoder.Telegram`, `net.whatsapp.WhatsApp`, `com.apple.MobileSMS`
    - formal: `com.microsoft.Outlook`, `com.apple.mail`, `com.microsoft.Word`, title contains "Gmail"
    - literal: `com.microsoft.VSCode`, `com.apple.Terminal`, `com.googlecode.iterm2`, `dev.warp.Warp-Stable`, `com.mitchellh.ghostty`, Cursor
    - verify each ID with `osascript -e 'id of app "Name"'` on a real install (Cursor's is unusual).
11. **UI strings per OS:** ⌘V not Ctrl+V, "Open at login", "Keychain" instead of "Credential Manager", models folder `~/Library/Application Support/Yappaflow/models`.
12. **Local models on Mac:** whisper-rs with Metal; `llama-server` sidecar built for Metal. Intel Macs use CPU (recommendation table in Phase 7).
13. **Build, sign, notarise:**
    - Universal binary: `rustup target add aarch64-apple-darwin x86_64-apple-darwin`, `npm run tauri build -- --target universal-apple-darwin`. The `llama-server` sidecar must be universal too (`lipo`).
    - Apple Developer Program ($99/yr), **Developer ID Application** certificate, hardened runtime, entitlement `com.apple.security.device.audio-input`. Not sandboxed (event taps + Accessibility rule out the Mac App Store).
    - Sign the sidecar with the same Team ID (nested code), then the app.
    - Notarise with `xcrun notarytool submit --wait` using an App Store Connect API key, then `xcrun stapler staple`. Tauri's bundler does this when `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY`, `APPLE_API_ISSUER` are set.
    - Output: `.dmg` plus the `.app.tar.gz` + signature for the updater (`darwin-universal` in the manifest). Certificates and keys live only in CI secrets.
14. **Minimum macOS:** propose 13 Ventura (owner to confirm); test on the newest release too.

## Out of scope
Mac App Store, Intel-GPU acceleration, iOS, touch-bar or Shortcuts integration, Windows changes beyond shared refactors.

## Design refs
`OnbMacPerms` (permission step), `TrayMenu` (as a menu-bar popup), `Main` / `PillExtras` (pill, "Copied · press ⌘V"), `Toasts` (permission revoked, secure input), `SetHotkeys` (fn default + 🌐 notice), `SetApps` (bundle IDs instead of `.exe`), `SetGeneral` ("Open at login").

## Acceptance tests
| # | Test | Pass when |
|---|---|---|
| 8.1 | Fresh Mac user account, install the `.dmg` downloaded via a browser (quarantine set) | Opens with no Gatekeeper warning; `spctl -a -vvv` says "Notarized Developer ID"; `xcrun stapler validate` passes |
| 8.2 | `lipo -archs` on the app binary and sidecar; run on Apple Silicon and an Intel Mac | Both list `x86_64 arm64`; both machines dictate |
| 8.3 | Onboarding permissions: grant each in System Settings | Each tick appears within 2 s; Continue enables only when all 3 are on |
| 8.4 | Hold fn, dictate into TextEdit, Notes, Safari, Slack, Mail, VS Code, Terminal, iTerm2 | Text lands; target keeps focus; no emoji picker, no input-source switch |
| 8.5 | 🌐 set to "Show Emoji & Symbols" | Notice with deep link shown in onboarding and Hotkeys |
| 8.6 | fn + ← (Home) in a text field | No dictation starts; Home works |
| 8.7 | Same sentence in Slack vs Mail vs VS Code | Casual / formal / literal, as on Windows |
| 8.8 | Dictate into a password field / Terminal with Secure Keyboard Entry on | No keystrokes posted; "Copied · press ⌘V" shown |
| 8.9 | Copy an image in Preview → dictate → ⌘V in Preview | Image restored (clipboard restore) |
| 8.10 | Pill with Dock bottom, left, auto-hide; external monitor; full-screen Safari | Pill above the Dock on the screen under the mouse, visible over full screen, never takes focus |
| 8.11 | Save a key, inspect Keychain Access and `yappaflow.db` | Key in Keychain under `yappaflow`; not in the DB or logs |
| 8.12 | Revoke Accessibility while running | Toast with Open settings; dictation falls back to clipboard; no crash |
| 8.13 | 30 min idle + sleep/wake, then dictate | Tap still alive (re-enable count logged if it was disabled) |
| 8.14 | Apple Silicon (M1/M2 16 GB) local mode, 20 × 5 s clips | p50 ≤ 1.2 s, p90 ≤ 1.8 s (record per-stage) |
| 8.15 | Reboot with Open at login on | Menu-bar icon appears; first dictation works |
| 8.16 | Run all Windows acceptance tests that apply (1.x–7.x) on Mac | Pass, or deviations listed in Results |

## Results
_(macOS versions tested, latency table, signing/notarisation logs, deviations)_

### Built, not acceptance-tested (24 Oct 2026)
Items 1–11 and 13's build config are written. **Built on the Mac, never run there** (no GUI session over SSH, permissions can't be granted).

Mac build results (`ssh mac`, macOS 27.0.1, Apple Silicon, Rust 1.99.1, Node 22; synced to `~/ShahidPersonalProjects/Yappaflow`):
- `npm ci && npm run build`: pass.
- `cargo build` (debug): pass, links (AVFoundation, Carbon, ApplicationServices, CoreGraphics resolved).
- `cargo test --lib`: **127 passed, 0 failed**. Tests that assume the Windows defaults (Ctrl + Win names) are `cfg(not(macos))`; Mac equivalents check fn / fn + ⌃ defaults, glyph display and Right ⌥.
- `npm run tauri build -- --bundles app`: pass, `Yappaflow.app` 8.7 MiB, arm64. Info.plist has `LSUIElement = true`, `NSMicrophoneUsageDescription`, `LSMinimumSystemVersion = 13.0`. Signature is ad-hoc (linker-signed), so the entitlements file isn't applied until a Developer ID identity is set.
- Universal build (`npm run build:mac -- --bundles app`): pass, 18.0 MiB, `lipo -archs` → `x86_64 arm64`. Tauri warned that the old identifier `com.yappaflow.app` ended in `.app`; changed to `com.yappaflow.desktop` on 24 Oct 2026 (the app moves the old roaming data folder on first launch).
Windows: `npm run build` and `cargo test --lib` (132 pass) are green; Windows code paths are unchanged (new code is behind `cfg(target_os = "macos")` or `cfg!`).

What's where: `hotkey/macos.rs` (CGEventTap, raw CoreGraphics FFI, keycode→VK table, fn = code 0x0E, Right ⌥ = 0x0F, re-enable counter + watchdog), `context.rs` (`mac` module: NSWorkspace, AX title/value/selection/secure field, focus tracker; `perms` module: mic/Input Monitoring/Accessibility status, prompts, deep links), `insert.rs` (NSPasteboard snapshot of every item/type, transient+concealed markers, ⌘V to `kCGHIDEventTap`, changeCount restore check, Unicode typing in 20-unit chunks with Shift+Return, secure input / no Accessibility → clipboard + "Copied · press ⌘V"), `secrets.rs` (keyring `apple-native`), migration v4 (`bundle_id` kind via table rebuild, Mac seeds), `tauri.macos.conf.json` + `Info.plist` + `entitlements.plist`, `npm run build:mac` (universal). Defaults on Mac: dictate = hold fn, command mode = fn + ⌃ (owner confirmed). Review fixes: Cocoa calls on worker threads run in `autoreleasepool`; the watchdog replaces an event tap that stays disabled; Mac-worded "left on clipboard" reasons; the permissions step has Skip (with a warning) and explains a restricted microphone; step numbers follow the screen order. Known limit: other tools' synthetic keystrokes reach the tap (macOS has no reliable injected flag), only our own are tagged and skipped. Menu bar: template icon from `icons/tray-dark-32.png`, `ActivationPolicy::Accessory` + `LSUIElement`.

Spec "verify" items:
| Item | Status |
|---|---|
| §2 `AppleFnUsageType` value mapping | **Open.** Coded as 0 = Do Nothing, anything else or missing = conflict. On the test Mac the key is absent (`defaults read` → "Could not find key"), i.e. the factory default, which shows the notice. 1/2/3 meanings are from memory, unconfirmed. |
| §2 Apple Dictation "press fn twice" vs our double-tap | **Open.** Not checked; notice text doesn't mention it yet. |
| §3 max UTF-16 units per `CGEventKeyboardSetUnicodeString` | **Open.** Using 20 (`CHUNK` in insert.rs, marked `shortcut:`). |
| §6 deep links on 14/15/26 | **Open.** URLs as in the spec; untested. Keyboard notice uses `x-apple.systempreferences:com.apple.Keyboard-Settings.extension` (unverified). |
| §6 Input Monitoring needs relaunch? | **Open.** The tap thread retries creation every second, so it may work without one; onboarding shows **Restart Yappaflow** when Input Monitoring flips to granted, just in case. |
| §8 Tauri window options vs NSPanel | **Decided (owner): NSPanel.** `tauri-nspanel` 2.1.1 (crates.io; MIT OR Apache-2.0; needs tauri ≥ 2.8.5, we have 2.12; enables tauri's `macos-private-api` feature on macOS only). The pill is converted with `to_panel`, style `Borderless | NonactivatingPanel`, collection `CanJoinAllSpaces | FullScreenAuxiliary | Stationary | IgnoresCycle`, level 25 (status). If conversion fails it falls back to a normal window and logs it. The toast stays a Tauri window with `set_visible_on_all_workspaces`. Behaviour (no focus steal, over full-screen) **untested on screen**. |
| §9 autostart: SMAppService or LaunchAgent | **Confirmed (source):** `tauri-plugin-autostart` 2.7.0 offers only `MacosLauncher::LaunchAgent` / `AppleScript` (no SMAppService); we use LaunchAgent (plist in `~/Library/LaunchAgents`). |
| §10 bundle IDs | **Confirmed on the test Mac with `osascript 'id of app'`:** Telegram `ru.keepcoder.Telegram`, WhatsApp `net.whatsapp.WhatsApp`, Messages `com.apple.MobileSMS`, Outlook `com.microsoft.Outlook`, Mail `com.apple.mail`, Word `com.microsoft.Word`, Terminal `com.apple.Terminal`. **Open** (apps not installed): Slack, Discord, VS Code, iTerm2, Warp, Ghostty, and Cursor (seeded as `com.todesktop.230313mzl4w4u92`, from memory). |
| §8 pill offset / visibleFrame | **Confirmed (source):** Tauri's `Monitor::work_area()` on macOS is `NSScreen.visibleFrame` (tauri-runtime-wry 2.12.3 `monitor/macos.rs`); pill offset is 12 px on Mac. |

Deviations / limits: Right ⌥ is its own key, so a chord with plain ⌥ needs the left Option key. fn can't be recorded from the web view, so the hotkey dialog offers Mac preset buttons (fn, Right ⌥, ⌃ ⌥, fn ⌃). `exe` rules are hidden and unused on Mac (and `bundle_id` rules on Windows). Signing/notarisation is not done; `signingIdentity` is `null` with no secrets. Acceptance tests 8.1–8.16 not run.

**24 Oct 2026, item 12 (local models on Mac): groundwork built, never compiled with Metal.** New cargo feature `metal` = `local-stt` + `whisper-rs/metal`; `models::GPU_BUILD` is true for `cuda` or `metal`, so whisper asks for the GPU and the catalogue's Apple Silicon row (≥ 16 GB RAM → large-v3-turbo + Qwen3.5-4B, "Metal") applies in a metal build. The VRAM budget treats 2/3 of RAM as Metal's share. llama-server: the release workflow fetches the official macOS arm64 and x64 zips, checks pinned sha256s, joins them with `lipo` into `binaries/llama-server-universal-apple-darwin` (Tauri `externalBin`, lands in `Contents/MacOS/llama-server`, where `llama_cpp.rs` looks) and puts the lipo'd dylibs beside it via `bundle.macOS.files`. It is skipped while the TODO tag/hashes are empty, and the macOS release leg itself is still commented out (needs signing). UNVERIFIED: that the release binaries load their dylibs from `@loader_path`; nested signing of the sidecar. CI's `local-engines` job builds `--features local-stt,metal` on macos-14 (non-blocking until it has passed once). Default-feature `cargo test --lib` passes on the Mac.
