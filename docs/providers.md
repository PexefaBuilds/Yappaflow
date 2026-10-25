# Provider reference (verified 24 Oct 2026)

Check this page before writing provider code. Tags: **✓** confirmed from official docs; **UNVERIFIED** means confirm before relying on it. Re-verify prices before a release, because they change. Prices are mirrored in `src-tauri/resources/prices.json` (History cost estimate): update both and its `checked` date.

## Speech to text

### OpenAI: default cloud provider ✓
- **Endpoint:** `POST https://api.openai.com/v1/audio/transcriptions` (multipart), `Authorization: Bearer`
- **Models:** `gpt-transcribe` ($0.0045/min, default) · `gpt-4o-transcribe` ($0.006/min) · `gpt-4o-mini-transcribe` ($0.003/min)
- **`gpt-transcribe` parameters:**
  - `keywords[]`: repeated multipart field, one per term. A term containing `<`, `>`, CR or LF makes the **whole request fail**, so strip those characters. No documented maximum count; we cap at 100.
  - `languages[]`: repeated, ISO 639-1 (plus `zh-cn` / `zh-tw` / `zh-hk`). **Replaces `language`. Never send both.**
  - `prompt`: free-text context.
  - Response: `{ "text": "...", "languages": [{ "code": "en" }] }`. `languages` is `[]` when detection isn't confident.
  - `stream=true` → SSE events `transcript.text.delta` … `transcript.text.done` (the done event carries `languages`).
- **Older models (`gpt-4o-*`):** use the singular `language` + `prompt`. No `keywords`.
- Sources: [API ref](https://developers.openai.com/api/reference/resources/audio/subresources/transcriptions/methods/create/index.html) · [STT guide](https://developers.openai.com/api/docs/guides/speech-to-text?lang=curl) · [pricing](https://developers.openai.com/api/docs/pricing)

### Groq ✓
- **Endpoint:** OpenAI-compatible, `https://api.groq.com/openai/v1/audio/transcriptions`. Reuse the OpenAI provider with a different base URL and key.
- **Models:** `whisper-large-v3-turbo` ($0.04/hour, the cheapest and fastest) · `whisper-large-v3` ($0.111/hour)
- **Vocabulary:** none. Pass terms through `prompt` (max 224 tokens).
- **Streaming:** not documented.
- **Language:** `language` (ISO-639-1) is optional and improves accuracy/latency. Groq's docs do not describe auto-detect, Arabic or mixed-language handling (**UNVERIFIED**).
- **Request:** formats flac, mp3, mp4, mpeg, mpga, m4a, ogg, wav, webm; max 25 MB free / 100 MB dev tier; defaults `response_format=json`, `temperature=0`, `timestamp_granularities=segment` (need no setting). Clips under 10 s are billed as 10 s. Speed ~216x real time (turbo).
- **Free-tier limits (Rate Limits page, table labelled "base"/Developer, plan label ambiguous):** both Whisper models 20 RPM, 2K RPD, 7.2K audio-sec/hour, 28.8K audio-sec/day. `openai/gpt-oss-20b`: 30 RPM, 1K RPD, 8K TPM, 200K TPD. HTTP 429 sets `retry-after`. Source: [rate limits](https://console.groq.com/docs/rate-limits)
- Sources: [docs](https://console.groq.com/docs/speech-to-text) · [models](https://console.groq.com/docs/models)

### Deepgram ✓
- **Endpoint:** own REST API (`POST /v1/listen`, raw audio body). Not OpenAI-compatible.
- **Model:** `nova-3`. Pre-recorded costs $0.0043/min mono, $0.0052/min multilingual (promo pricing). Keyterm add-on $0.0013/min.
- **Vocabulary:** `keyterm`, repeated in the query string. ≤ 500 tokens in total, ideally 20–50 terms.
- **Language:** `detect_language=true` works on pre-recorded only. ⚠ **Arabic is not in the auto-detect list** and not in `multi` code-switching. Arabic works only when the user sets `ar`/`ar-AE` explicitly. Show this caveat in the UI.
- Sources: [pricing](https://deepgram.com/pricing) · [keyterm](https://developers.deepgram.com/docs/keyterm) · [language detection](https://developers.deepgram.com/docs/language-detection) · [languages](https://developers.deepgram.com/docs/models-languages-overview)

### ElevenLabs ✓
- **Endpoint:** `POST https://api.elevenlabs.io/v1/speech-to-text` (multipart, header `xi-api-key`)
- **Model:** `scribe_v2`. $0.22/hour, plus $0.05/hour with keyterms.
- **Vocabulary:** `keyterms`, up to 1,000 terms, each under 50 characters and ≤ 5 words.
- **Language:** auto when `language_code` is null. Response includes `language_code` and `language_probability`. Arabic is in the "Good" tier.
- **Realtime:** separate WebSocket product ($0.39/hour); the realtime model ID is **UNVERIFIED**.
- Sources: [API](https://elevenlabs.io/docs/api-reference/speech-to-text/convert) · [capabilities](https://elevenlabs.io/docs/capabilities/speech-to-text) · [pricing](https://elevenlabs.io/pricing/api)

### AssemblyAI ✓ (with caveats)
- **Endpoint:** async. `POST https://api.assemblyai.com/v2/transcript`, then poll; upload first with `/v2/upload`. Polling adds latency, so it's a poor fit for dictation and better suited to "Retry with another model".
- **Models:** set via `speech_models`. The page says `universal-3-5-pro` but models.md says `universal-3-pro`, so **verify the ID** (still conflicting on 24 Oct 2026; see Phase 7 build checks). $0.21/hour (+$0.05 for keyterms). `universal-2` is $0.15/hour.
- **Vocabulary:** `keyterms_prompt`, ≤ 1,000 terms of ≤ 6 words. Can't be combined with `prompt` on U-3.5 Pro.
- **Language:** ⚠ `language_detection` needs **≥ 15 s of audio**, so short clips must use a fixed language.
- Sources: [pricing](https://www.assemblyai.com/pricing) · [U-3.5 Pro](https://assemblyai.com/docs/pre-recorded-audio/universal-3-5-pro) · [models](https://www.assemblyai.com/llms/models.md)

### Mistral ✓
- **Endpoint:** `POST https://api.mistral.ai/v1/audio/transcriptions`. Same path as OpenAI; full compatibility **UNVERIFIED**, so test the OpenAI provider against it first.
- **Models:** `voxtral-mini-2602` (Transcribe 2, GA, $0.003/min) · `voxtral-mini-transcribe-3-0` (preview since 20 Oct 2026, $0.0035/min)
- **Vocabulary:** `context_bias`, an array of strings.
- **Language:** optional `language`; the response includes `language`. Arabic is confirmed for v2 only.
- Sources: [endpoint](https://docs.mistral.ai/api/endpoint/audio/transcriptions) · [capability](https://docs.mistral.ai/capabilities/audio_transcription) · [v2 model](https://docs.mistral.ai/models/voxtral-mini-transcribe-26-02) · [v3 model](https://docs.mistral.ai/models/voxtral-mini-transcribe-3-0)

### Google Gemini ✓ (an LLM with audio input, not a dedicated STT API)
- **Endpoint:** `generateContent` with inline audio plus an instruction prompt ("Transcribe exactly…").
- **Models:** `gemini-3.5-flash-lite` (audio $0.30/1M tokens) or `gemini-3.1-flash-lite` ($0.50/1M). Audio is 32 tokens/s, so ≈ **$0.0006–0.001/min** plus output tokens.
- Could transcribe and clean in a single call (future idea).
- **Language:** detection is prompt-driven; the Arabic language list is **UNVERIFIED**.
- Sources: [audio](https://ai.google.dev/gemini-api/docs/audio) · [pricing](https://ai.google.dev/gemini-api/docs/pricing)

### Custom URL
Any OpenAI-compatible `/v1/audio/transcriptions` server (self-hosted faster-whisper, LocalAI, etc.). It's the OpenAI provider with a user base URL, optional key and a free-text model ID.

## Cleanup LLMs

| Provider | Model ID | Price per 1M tokens (in / out) | Turning reasoning off | Endpoint |
|---|---|---|---|---|
| OpenAI (default) ✓ | `gpt-6-luna` | $0.10 / $0.50 | Responses: `reasoning: {effort: "none"}` · Chat: `reasoning_effort: "none"` | `/v1/responses` or `/v1/chat/completions` |
| Anthropic ✓ | `claude-haiku-5-5` | $0.10 / $0.50 (≤ 100K prompt) | `thinking: {type: "disabled"}` (effort ≤ high) | Messages API |
| Google ✓ | `gemini-3.1-flash-lite` | $0.25 / $1.50 | Default is `minimal` (lowest) ✓; send nothing (see Phase 7 build checks) | `generateContent` |
| Groq ✓ | `openai/gpt-oss-20b` | $0.075 / $0.30 | Reasoning model: `reasoning_effort: "low"` (low/medium/high; no "none"), `include_reasoning: false` | OpenAI-compatible |
| Mistral | `ministral-3b-2512` | "$0.10/M" (split **UNVERIFIED**) | n/a | `/v1/chat/completions` |
| OpenRouter ✓ | `openai/gpt-6-luna` (or any) | $0.10 / $0.50 | as OpenAI | `https://openrouter.ai/api/v1/chat/completions` |

Groq `openai/gpt-oss-20b` verified (Oct 2026): ~1,000 tok/s, $0.075 in / $0.30 out per 1M, 131K context. `reasoning_effort` accepts low|medium|high on Chat Completions; `reasoning_format` is not supported for gpt-oss; `include_reasoning` (default true) controls whether reasoning is returned in `message.reasoning`, separate from `message.content`. Reasoning tokens count toward `max_completion_tokens` (default 1024), so we add 512 headroom. Sources: [reasoning](https://console.groq.com/docs/reasoning) · [model page](https://console.groq.com/docs/model/openai/gpt-oss-20b)

Sources: [OpenAI gpt-6-luna](https://developers.openai.com/api/docs/models/gpt-6-luna) · [Anthropic pricing](https://platform.claude.com/docs/en/about-claude/pricing) · [Haiku 5.5](https://platform.claude.com/docs/en/models/haiku-5-5/overview) · [Gemini pricing](https://ai.google.dev/gemini-api/docs/pricing) · [Groq models](https://console.groq.com/docs/models) · [Ministral 3B](https://docs.mistral.ai/models/ministral-3-3b-25-12) · [OpenRouter](https://openrouter.ai/openai/gpt-6-luna)

## Local models (Phase 7)

### whisper.cpp (speech to text) ✓
Files are from [huggingface.co/ggerganov/whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp/tree/main).

| Model | f16 | q8_0 | q5 | Use for |
|---|---|---|---|---|
| large-v3-turbo | 1.62 GB | 874 MB | **574 MB (q5_0)** | GPU default. q5_0 is the recommended download. |
| large-v3 | 3.1 GB | n/a | 1.08 GB (q5_0) | Best accuracy, slower |
| medium | 1.53 GB | 823 MB | 539 MB | |
| small | 488 MB | 264 MB | 190 MB (q5_1) | CPU-only laptops |
| base | 148 MB | 82 MB | 60 MB (q5_1) | Very slow PCs |

### llama.cpp (cleanup); all Apache-2.0, free to redistribute ✓

| Slot | Model | Q4_K_M | Source | Notes |
|---|---|---|---|---|
| Recommended (~4B) | **Qwen3.5-4B** | 3.01 GB | [bartowski GGUF](https://huggingface.co/bartowski/Qwen_Qwen3.5-4B-GGUF) | 201 languages (Arabic not explicitly named: **UNVERIFIED**, so test with the eval set). Thinking is **on** by default; pass `chat_template_kwargs: {"enable_thinking": false}`. |
| Fast (~3B) | **Ministral 3 3B Instruct 2512** | 2.15 GB | [official GGUF](https://huggingface.co/mistralai/Ministral-3-3B-Instruct-2512-GGUF) | Arabic is explicitly listed. Temperature < 0.1. |
| CPU-only (~1B) | **Qwen3.5-0.8B** | 0.58 GB | [bartowski GGUF](https://huggingface.co/bartowski/Qwen_Qwen3.5-0.8B-GGUF) | Fillers and punctuation only; same thinking flag as above |
| Alternative | Gemma 4 E4B-it | 5.41 GB | [bartowski GGUF](https://huggingface.co/bartowski/google_gemma-4-E4B-it-GGUF) | Too big for 8 GB alongside whisper large |

**Budget on an 8 GB GPU:** turbo q5_0 (0.6 GB) + Qwen3.5-4B Q4_K_M (3.0 GB) + context is about 4.5 GB. ✓ Fits.

Pick the local cleanup model by running `docs/evals/cleanup.jsonl` on all three; the eval score decides.

**How we launch llama-server** (`providers/llama_cpp.rs::args`):

| Flag | Value | Why |
|---|---|---|
| `-m` | path to the installed `.gguf` | the model |
| `--host` | `127.0.0.1` | loopback only, never the LAN |
| `--port` | a free port, picked per launch | a new one on the retry |
| `--api-key` | random per-launch key | other local processes can't use the server |
| `--jinja` | | makes the chat template honour `chat_template_kwargs` (the thinking switch) |
| `-c` | `4096` | context size; plenty for one dictation plus the prompt |
| `-ngl` | `999`, or `0` | all layers on the GPU, or none |

- **`-ngl 0` fallback:** when the VRAM budget (`models::budget_for_settings`) says the chosen models don't fit (`fit != "ok"`), we launch with `-ngl 0`, so everything runs on the CPU. It is slower but works. The log says "CPU only: models don't fit in graphics memory".
- **Ready check:** we poll `GET /health` until it returns OK (the model is loaded). If the server exits early we retry once on a new port, then give up and paste the raw text.
- **Lifetime:** started on chord-down (prewarm), restarted if it died, killed on app exit or model change, and tied to the app (Job object on Windows, watchdog on macOS).

## Phase 7 build checks (24 Oct 2026)
What the cloud-provider code relies on, checked against official docs while building. Code comments carry the same tags.

**Confirmed ✓**
- **Deepgram:** `POST https://api.deepgram.com/v1/listen`, `Authorization: Token <key>`; `keyterm` repeated (Nova-3 only), `detect_language`, `language`, `model`; transcript at `results.channels[0].alternatives[0].transcript`, language at `results.channels[0].detected_language`. `smart_format=true` adds punctuation. Sources: [listen pre-recorded](https://developers.deepgram.com/reference/speech-to-text/listen-pre-recorded.md) · [smart format](https://developers.deepgram.com/docs/smart-format)
- **ElevenLabs:** model field is `model_id`; `language_code`, `keyterms`; header `xi-api-key`; response `text`, `language_code`. Source: [convert](https://elevenlabs.io/docs/api-reference/speech-to-text/convert)
- **AssemblyAI:** raw key in `authorization` (no "Bearer"); `speech_models` is an array; `keyterms_prompt` ≤ 1,000 phrases (U-3.5 Pro) / 200 (U-2), ≤ 6 words; `language_code` and `language_detection` are mutually exclusive; status `queued|processing|completed|error`. Source: [submit transcript](https://www.assemblyai.com/docs/api-reference/transcripts/submit)
- **Mistral STT:** `Authorization: Bearer`; fields `model`, `file`, `language`, `context_bias` (array of strings); response `text`, `language`. No `prompt` or `response_format` documented, so we don't send them. Mistral chat uses `max_tokens` (no `max_completion_tokens`). Sources: [transcriptions](https://docs.mistral.ai/api/endpoint/audio/transcriptions) · [chat](https://docs.mistral.ai/api/endpoint/chat)
- **Gemini:** key header `x-goog-api-key`; request ≤ 20 MB total incl. inline audio; `audio/wav` supported; response text at `candidates[0].content.parts[].text`. Sources: [audio](https://ai.google.dev/gemini-api/docs/audio) · [generateContent](https://ai.google.dev/api/generate-content)
- **Anthropic:** `x-api-key` + `anthropic-version: 2023-06-01`; `thinking: {"type": "disabled"}` is a valid config. Source: [Messages](https://platform.claude.com/docs/en/api/messages)
- **OpenRouter:** `reasoning: {"effort": "none"}` disables reasoning (top-level `reasoning_effort` isn't documented); `GET /api/v1/key` checks a key (`/models` is public). Sources: [reasoning tokens](https://openrouter.ai/docs/use-cases/reasoning-tokens) · [limits](https://openrouter.ai/docs/api-reference/limits)

**Still UNVERIFIED**
- **AssemblyAI model ID:** the API reference lists `universal-3-5-pro` / `universal-2` for `speech_models` (default `["universal-3-5-pro","universal-2"]`), but models.md says `universal-3-pro` is the pre-recorded API value and `universal-3-5-pro` is the realtime model. We ship `universal-3-5-pro` (API reference wins); the 7.9 matrix run settles it.
- **Mistral STT:** full OpenAI compatibility, and how `context_bias` is encoded in multipart (we send repeated `context_bias` fields). Any limit on the number of terms (we cap at 100).
- **ElevenLabs:** multipart encoding of `keyterms` (we send repeated fields).
- **Gemini thinking field on generateContent:** the docs show `thinking_level` only for the Interactions API, so the generateContent field name is unconfirmed and we send none. We don't need it: `minimal` (the lowest level, "does not guarantee that thinking is off") is the **default** for `gemini-3.1-flash-lite` ✓ ([Gemini 3](https://ai.google.dev/gemini-api/docs/gemini-3)) and `gemini-3.5-flash-lite` ✓ ([thinking](https://ai.google.dev/gemini-api/docs/thinking)). Cleanup adds 256 output tokens of headroom. Check reasoning latency in test 7.10.
- **Key test endpoints** (cheapest authenticated GET, not billed): Deepgram `GET /v1/projects`, AssemblyAI `GET /v2/transcript?limit=1`, ElevenLabs `GET /v1/models`, Anthropic `GET /v1/models`, Gemini `GET /v1beta/models`.

## Gotchas
- `gpt-transcribe`: `languages[]`, never `language`. Strip `< > \r \n` from keywords.
- Deepgram can't auto-detect Arabic: the UI shows a caveat (pick Arabic in General › Language), but auto stays allowed for other languages. AssemblyAI can't auto-detect on clips under 15 s, so the UI requires a fixed language for it.
- Reasoning-off switches differ per provider (see the table). Forgetting one adds seconds of latency.
