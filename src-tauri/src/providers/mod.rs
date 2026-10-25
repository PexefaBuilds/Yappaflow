//! Provider interfaces. Everything above this layer (pipeline, UI) talks only to
//! these traits, so a local whisper.cpp / llama.cpp backend can be added later by
//! implementing them and registering an id in `registry.rs`.

pub mod anthropic_cleanup;
pub mod assemblyai;
pub mod deepgram;
pub mod elevenlabs;
pub mod gemini_cleanup;
pub mod gemini_stt;
pub mod http;
pub mod llama_cpp;
pub mod openai_cleanup;
pub mod openai_stt;
pub mod passthrough;
pub mod registry;
pub mod vocab;
pub mod whisper_cpp;

use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;

/// One utterance, already resampled to 16 kHz mono.
pub struct AudioClip {
    /// Encoded container for cloud APIs (WAV now, Opus in Phase 2).
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub filename: &'static str,
    /// Raw 16 kHz PCM for local engines (whisper.cpp wants samples, not files).
    #[cfg_attr(not(feature = "local-stt"), allow(dead_code))]
    pub pcm16k: Vec<i16>,
    pub duration_ms: u64,
}

#[derive(Default, Clone)]
pub struct SttOptions {
    /// Free-form context prompt.
    pub prompt: Option<String>,
    /// ISO-639-1 hints; empty = auto-detect. Providers with a single-language
    /// parameter use the first entry.
    pub languages: Vec<String>,
    /// Vocabulary terms (dictionary, Phase 4). Each provider maps these to its own
    /// parameter (`keywords[]`, `keyterm`, …) or folds them into the prompt.
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Transcript {
    pub text: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CleanupContext {
    pub app_exe: String,
    pub window_title: String,
    /// "casual" | "formal" | "neutral" | "literal" (Phase 2 app profiles)
    pub tone: String,
    pub dictionary: Vec<String>,
    /// Default rule ids switched off, and the user's own extra rules (see cleanup::system_prompt).
    pub disabled_rules: Vec<String>,
    pub custom_rules: Vec<String>,
    /// Text just before the cursor (cleanup::cursor_tail); empty when off. Never stored or logged.
    pub before_cursor: String,
}

#[async_trait]
pub trait SttProvider: Send + Sync {
    fn id(&self) -> &'static str;
    /// Called on chord-down while the user is still speaking: open TLS, load model, …
    async fn prewarm(&self) {}
    async fn transcribe(&self, audio: &AudioClip, opts: &SttOptions) -> Result<Transcript>;
}

#[async_trait]
pub trait CleanupProvider: Send + Sync {
    #[allow(dead_code)] // mirrors SttProvider::id; every provider names itself
    fn id(&self) -> &'static str;
    async fn prewarm(&self) {}
    async fn clean(&self, text: &str, ctx: &CleanupContext) -> Result<String>;
    /// Command mode: apply the spoken `instruction` to `selection` (None = write new text at the
    /// cursor). Returns the replacement text only. The selection is data, never instructions.
    async fn edit(&self, _selection: Option<&str>, _instruction: &str, _ctx: &CleanupContext) -> Result<String> {
        anyhow::bail!("Command mode needs a cleanup model. Choose one in Settings › Models.")
    }
}
