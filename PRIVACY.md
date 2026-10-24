# Privacy

Yappaflow has no servers, no accounts, no analytics and no telemetry. It talks only to the providers you choose, using your own API key. This page lists every place the app sends data and exactly what goes there.

## Network destinations

| Destination | When | What is sent |
|---|---|---|
| **Your speech-to-text provider** (OpenAI `api.openai.com` or Groq `api.groq.com`, or the base URL you set) | Each dictation | The audio clip; the model name; your language setting, if you set one; your dictionary terms (as a vocabulary list, or folded into the prompt for models without one). Your API key, as the auth header. |
| **Your cleanup provider** (same hosts) | Each dictation, unless cleanup is Off or the text needs no cleanup | The transcript; the name of the app you're dictating into (its `.exe` name) and the first 80 characters of its window title, so the tone fits the app; your writing style for that app; your dictionary terms (up to 100, with any "sounds like" hints); your custom writing rules and which built-in rules you turned off. Your API key. |
| **Both providers, connection warm-up** | When you start holding the hotkey | A request for the model's info (`GET /models/<model>`) with your API key, to open the connection early. No audio or text. |
| **Both providers, key test** | When you save a key | A small test request with that key. |

Each provider handles your data under its own privacy policy and retention terms. Check them before you choose one.

**Window titles can contain private things** (an email subject, a document name). Only the first 80 characters are sent, only to your cleanup provider, and only when cleanup runs. Set cleanup to **Off** to send nothing but audio.

Planned, not in this version yet. This table will be updated when they ship:

| Destination | What will be sent |
|---|---|
| GitHub Releases (update check) | A request for the latest version manifest. GitHub sees your IP address and the app version. |
| Hugging Face (local models only) | Model file downloads you start. Hugging Face sees your IP address. Local dictation itself makes no network calls. |

Nothing else. Crash reports are not uploaded.

## What stays on your computer

- **Recordings:** your newest 50 clips (WAV), saved before any upload so a dictation is never lost. Older clips are deleted automatically. You can change the number in Settings.
- **History database** (`yappaflow.db`): for each dictation, the app name and window title, the raw transcript and the final text, timings, status and errors. Also your settings, per-app styles, dictionary and snippets.
- **API keys:** in the OS keychain (Windows Credential Manager, under `yappaflow`). Never written to the database, a file or the logs.
- **Learning from corrections:** for 15 seconds after Yappaflow pastes, it reads the focused text field to see whether you corrected one of its words, so it can suggest a dictionary entry. Only the words it inserted are compared. Nothing read from the field is logged or sent anywhere, and nothing is stored except the corrected word: kept in memory, and saved to your dictionary only if you accept the suggestion.
- **Logs:** never contain your API key or audio. Transcripts are not logged at the normal log level.

## Deleting your data

1. Quit Yappaflow from the tray.
2. Delete the folders `%APPDATA%\com.yappaflow.desktop\` (history database, recordings, settings) and `%LOCALAPPDATA%\com.yappaflow.desktop\` (the settings window's web cache).
3. Remove your keys: either clear the key field in Settings › Models and save, or open **Credential Manager → Windows Credentials** and remove the entries under `yappaflow`.
4. Uninstall the app from Windows Settings › Apps.

To delete what a provider has stored, use that provider's own dashboard or contact them.

## Questions

Open an issue, or for anything sensitive see [SECURITY.md](SECURITY.md).
