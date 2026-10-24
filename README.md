# Yappaflow

Hold a hotkey, speak, let go. Polished text appears at the cursor in any app.

- **Bring your own key.** Use your own OpenAI, Groq, Deepgram, Gemini, Anthropic… account, or run local models (build from source). No Yappaflow account, no subscription.
- **Works everywhere you type.** Editors, browsers, chat apps, terminals.
- **Cleans up as you go.** Removes filler words, fixes punctuation, keeps your dictionary terms spelled right. Turn it off for the raw transcript.
- **Never loses a dictation.** Audio is saved before anything is uploaded; failures leave your text on the clipboard or a Retry button in History.
- **Free and open source** under the MIT licence.

Windows 10/11 and macOS 13+.

## Download

Get the latest version from **[Releases](https://github.com/PexefaBuilds/yappaflow/releases/latest)**:

- **Windows:** `Yappaflow_<version>_x64-setup.exe` (per-user, no admin rights needed)
- **macOS:** `Yappaflow_<version>_universal.dmg` (Apple Silicon and Intel)

**Windows SmartScreen:** releases are not code-signed yet, so Windows may show "Windows protected your PC". Click **More info → Run anyway** only if you downloaded the installer from this repository's Releases page. You can compare its hash with `SHA256SUMS` on the same page. Signed installers are planned.

**macOS Gatekeeper:** the beta isn't signed or notarised yet. After dragging Yappaflow to Applications, the first launch is blocked. Open **System Settings → Privacy & Security**, click **Open Anyway** and confirm.

## Quick start

1. Install and launch Yappaflow. Onboarding opens on first run.
2. Paste an API key from your provider (OpenAI or Groq). It's tested and then stored in your OS keychain, never on disk in plain text.
3. Click into any text field, hold **Ctrl + Win** (macOS: **fn**), speak, and release.

| Action | How |
|---|---|
| Dictate | Hold **Ctrl + Win** (macOS: **fn**), speak, release |
| Hands-free | Double-tap **Ctrl + Win**, speak as long as you like, tap again to finish |
| Cancel | **Esc** while listening or transcribing |
| Settings | Tray icon, or launch the app again |
| Lost text? | Settings › History: Copy or Retry |

Hotkeys can be rebound in Settings.

## Providers

| | Speech-to-text | Cleanup |
|---|---|---|
| OpenAI, Groq, Mistral | Yes | Yes |
| Deepgram, ElevenLabs, AssemblyAI | Yes | |
| Gemini | Yes | Yes |
| Anthropic, OpenRouter | | Yes |
| Custom OpenAI-compatible URL | Yes | Yes |
| Off (raw transcript) | | Yes |
| Local (whisper.cpp / llama.cpp) | Build from source | Build from source |

## Privacy

Your audio goes only to the speech-to-text provider you pick, and the transcript only to the cleanup provider you pick. There are no Yappaflow servers, accounts, analytics or telemetry. [PRIVACY.md](PRIVACY.md) lists every network destination and exactly what is sent.

## Build from source

Prerequisites (Windows, one time):

1. **Rust** with the MSVC toolchain: <https://rustup.rs>
2. **Visual Studio Build Tools 2022** with the *Desktop development with C++* workload
3. **Node.js 22 LTS** or newer
4. **WebView2**, already on Windows 10/11

```powershell
npm install                     # once
npm run tauri dev               # run the app (hot-reloads the UI)
cd src-tauri; cargo test        # unit tests
npm run build                   # type-check + build the UI (needed before a bare `cargo check`)
npm run tauri build             # NSIS installer → src-tauri/target/release/bundle/nsis/
```

The first build takes a few minutes. See [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

### Local models

Release builds are the easy path: they ship the local engines when they're enabled. A plain source build has none: speech on this PC needs a cargo feature, and local cleanup needs a `llama-server` binary.

- **Speech (whisper.cpp, in-process):** build with `--features local-stt` (CPU), `cuda` (NVIDIA, also needs the CUDA Toolkit) or `metal` (Apple Silicon). Building it needs:
  - **LLVM** (libclang, for bindgen). On Windows set `LIBCLANG_PATH` to its `bin` folder, e.g. `C:\Program Files\LLVM\bin`. macOS: Xcode's command line tools are enough.
  - **CMake** on `PATH` (the copy inside VS Build Tools works: `…\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin`).
  - On Windows, a short target dir (`$env:CARGO_TARGET_DIR="C:\t"`), because MSBuild hits the 260-character path limit in deep folders.

  ```powershell
  npm run tauri dev -- --features local-stt
  ```
- **Cleanup (llama.cpp):** download `llama-server` from the official [llama.cpp releases](https://github.com/ggml-org/llama.cpp/releases) (CUDA or CPU build on Windows, macOS build on Mac) and put it, with its DLLs/dylibs, next to the Yappaflow executable, or set its path in Settings › Models › On this PC.
- Models download from Settings › Models › On this PC (pinned, sha256-checked).

## Where your data lives (Windows)

`%APPDATA%\com.yappaflow.desktop\`: `yappaflow.db` (history, dictionary, settings) and `recordings\` (the newest 50 clips). API keys are in Windows Credential Manager under `yappaflow`. See [PRIVACY.md](PRIVACY.md) for how to delete it all.

## Licence

MIT, see [LICENSE](LICENSE). The bundled fonts (Unbounded, Reem Kufi) are under the SIL Open Font License 1.1, see [licenses/OFL.txt](licenses/OFL.txt).

Yappaflow is an independent clean-room project, built only from public provider documentation and its own designs.
