//! Maps provider ids from settings to implementations. The settings dropdowns
//! are populated from the tables below, so adding a provider is: implement the
//! trait (or add a preset), add a row here, add a match arm.

use super::assemblyai::AssemblyAiStt;
use super::deepgram::DeepgramStt;
use super::elevenlabs::ElevenLabsStt;
use super::gemini_stt::GeminiStt;
use super::llama_cpp::LlamaCppCleanup;
use super::whisper_cpp::WhisperCppStt;
use super::{anthropic_cleanup::AnthropicCleanup, gemini_cleanup::GeminiCleanup};
use super::{openai_cleanup as chat, openai_cleanup::OpenAiCleanup, openai_stt as compat, openai_stt::OpenAiCompatibleStt};
use super::{passthrough::PassthroughCleanup, CleanupProvider, SttProvider};
use crate::settings::Settings;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

/// Model IDs come from docs/providers.md (verified Oct 2026). Don't guess new ones.
#[derive(Serialize, Clone)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub models: &'static [&'static str],
    pub available: bool,
    /// Caveat to show next to the provider in Settings.
    pub note: Option<&'static str>,
    /// Language caveat shown under the provider (Models page and onboarding).
    pub lang_note: Option<&'static str>,
    /// Auto-detect is unusable for dictation; the UI requires a fixed language.
    pub needs_language: bool,
    /// Model is free text (Custom URL), `models` are only suggestions.
    pub free_model: bool,
}

const fn p(id: &'static str, label: &'static str, models: &'static [&'static str], note: Option<&'static str>) -> ProviderInfo {
    ProviderInfo { id, label, models, available: true, note, lang_note: None, needs_language: false, free_model: false }
}
const fn lang(mut i: ProviderInfo, n: &'static str) -> ProviderInfo {
    i.lang_note = Some(n);
    i
}

const ARABIC_UNCONFIRMED: &str = "Arabic not confirmed by the provider.";

/// Providers that run on this PC: no key, no network.
pub const LOCAL: &[&str] = &["whisper_cpp", "llama_cpp"];
const LOCAL_STT_MODELS: &[&str] = &[
    "whisper-large-v3-turbo-q5_0",
    "whisper-large-v3-turbo-q8_0",
    "whisper-large-v3-q5_0",
    "whisper-medium-q5_0",
    "whisper-small-q5_1",
    "whisper-base-q5_1",
];
const LOCAL_CLEANUP_MODELS: &[&str] = &["qwen3.5-4b-q4_k_m", "ministral-3-3b-instruct-2512-q4_k_m", "qwen3.5-0.8b-q4_k_m"];

pub const STT_PROVIDERS: &[ProviderInfo] = &[
    p(
        "openai",
        "OpenAI (cloud)",
        &["gpt-transcribe", "gpt-4o-transcribe", "gpt-4o-mini-transcribe"],
        Some("Best with names and mixed languages; uses your dictionary as keyword hints."),
    ),
    lang(
        p(
            "groq",
            "Groq (cloud)",
            &["whisper-large-v3-turbo", "whisper-large-v3"],
            Some("Fastest and cheapest. Dictionary goes in as a short prompt (max 224 tokens). Clips under 10 s bill as 10 s."),
        ),
        ARABIC_UNCONFIRMED,
    ),
    lang(
        p("deepgram", "Deepgram (cloud)", &["nova-3"], Some("Uses your 50 most-used dictionary terms as key terms.")),
        "Arabic isn't auto-detected. Pick Arabic in General › Language to dictate in Arabic.",
    ),
    p("elevenlabs", "ElevenLabs (cloud)", &["scribe_v2"], Some("Strong multilingual auto-detect, including Arabic.")),
    ProviderInfo {
        needs_language: true,
        ..lang(
            // API reference lists universal-3-5-pro; models.md says universal-3-pro (UNVERIFIED, see providers.md).
            p("assemblyai", "AssemblyAI (cloud)", &["universal-3-5-pro", "universal-2"], Some("Slower (async), good for Retry.")),
            "Auto-detect needs 15 s or more of speech, so this provider needs a fixed language.",
        )
    },
    lang(
        p(
            "mistral",
            "Mistral (cloud)",
            &["voxtral-mini-2602", "voxtral-mini-transcribe-3-0"],
            Some("voxtral-mini-transcribe-3-0 is a preview model."),
        ),
        "Arabic confirmed for voxtral-mini-2602 only.",
    ),
    lang(
        p(
            "gemini",
            "Google Gemini (cloud)",
            &["gemini-3.5-flash-lite", "gemini-3.1-flash-lite"],
            Some("A general AI model listening to your audio. Very cheap."),
        ),
        ARABIC_UNCONFIRMED,
    ),
    ProviderInfo {
        free_model: true,
        ..p(
            "custom",
            "Custom URL (OpenAI-compatible)",
            &[],
            Some("For self-hosted Whisper servers. Type the model ID your server expects."),
        )
    },
    // Model ids = the stt entries of resources/models.json (unit-tested). Needs the local-stt build feature.
    ProviderInfo {
        available: cfg!(feature = "local-stt"),
        ..p("whisper_cpp", "On this PC · whisper.cpp", LOCAL_STT_MODELS, Some("Runs offline. Download models in Models › On this PC."))
    },
];

pub const CLEANUP_PROVIDERS: &[ProviderInfo] = &[
    p("none", "Off (raw transcript)", &[], None),
    p("openai", "OpenAI (cloud)", &["gpt-6-luna"], None),
    p("anthropic", "Anthropic (cloud)", &["claude-haiku-5-5"], None),
    p("gemini", "Google Gemini (cloud)", &["gemini-3.1-flash-lite"], None),
    p("groq", "Groq (cloud)", &["openai/gpt-oss-20b"], Some("Reasoning model: reasoning effort is set to low.")),
    p("mistral", "Mistral (cloud)", &["ministral-3b-2512"], None),
    ProviderInfo {
        free_model: true,
        ..p("openrouter", "OpenRouter (cloud)", &["openai/gpt-6-luna"], Some("Any OpenRouter model ID works."))
    },
    ProviderInfo {
        free_model: true,
        ..p("custom", "Custom URL (OpenAI-compatible)", &[], Some("Any OpenAI-compatible chat server. Type the model ID it expects."))
    },
    p(
        "llama_cpp",
        "On this PC · llama.cpp",
        LOCAL_CLEANUP_MODELS,
        Some("Runs offline through llama-server. Download models in Models › On this PC."),
    ),
];

/// Name used in error text and toasts ("Deepgram rejected your key").
pub fn display_name(id: &str) -> &'static str {
    match id {
        "openai" => "OpenAI",
        "groq" => "Groq",
        "mistral" => "Mistral",
        "deepgram" => "Deepgram",
        "elevenlabs" => "ElevenLabs",
        "assemblyai" => "AssemblyAI",
        "gemini" => "Google Gemini",
        "anthropic" => "Anthropic",
        "openrouter" => "OpenRouter",
        "custom" => "Custom URL",
        "whisper_cpp" => "whisper.cpp",
        "llama_cpp" => "llama.cpp",
        _ => "The provider",
    }
}

/// Official API base per provider (docs/providers.md). Overridable in settings.
pub fn default_base_url(id: &str) -> &'static str {
    match id {
        "openai" => "https://api.openai.com/v1",
        "groq" => "https://api.groq.com/openai/v1",
        "mistral" => "https://api.mistral.ai/v1",
        "deepgram" => "https://api.deepgram.com/v1",
        "elevenlabs" => "https://api.elevenlabs.io/v1",
        "assemblyai" => "https://api.assemblyai.com",
        "gemini" => "https://generativelanguage.googleapis.com/v1beta",
        "anthropic" => "https://api.anthropic.com/v1",
        "openrouter" => "https://openrouter.ai/api/v1",
        _ => "",
    }
}

pub fn base_url(s: &Settings, id: &str) -> String {
    match id {
        "openai" => s.openai_base_url.clone(),
        "groq" => s.groq_base_url.clone(),
        "custom" => s.custom_base_url.clone(),
        _ => s.base_urls.get(id).cloned().unwrap_or_else(|| default_base_url(id).to_string()),
    }
}

/// Cheapest authenticated GET for the Test button.
// UNVERIFIED as "cheapest call": Deepgram /projects, AssemblyAI transcript list,
// ElevenLabs /models, Anthropic /models (all documented auth'd GETs, none billed).
pub fn key_test_url(id: &str, base: &str) -> String {
    let base = base.trim_end_matches('/');
    match id {
        "deepgram" => format!("{base}/projects"),
        "assemblyai" => format!("{base}/v2/transcript?limit=1"),
        "openrouter" => format!("{base}/key"), // /models is public; /key needs a valid key
        _ => format!("{base}/models"),
    }
}

/// First cloud STT provider with a saved key, and its default model ("Use cloud once").
/// Custom URL is skipped: it has no default model.
pub fn cloud_stt_with_key() -> Option<(&'static str, &'static str)> {
    STT_PROVIDERS
        .iter()
        .filter(|p| p.available && !LOCAL.contains(&p.id) && !p.models.is_empty())
        .find(|p| crate::secrets::provider_key(p.id).is_some())
        .map(|p| (p.id, p.models[0]))
}

pub fn build_stt(s: &Settings, http: &reqwest::Client) -> Arc<dyn SttProvider> {
    let id = s.stt_provider.as_str();
    let (base, model, h) = (base_url(s, id), s.stt_model.as_str(), http.clone());
    match id {
        "whisper_cpp" => Arc::new(WhisperCppStt::new(model).unload_after_use(s.keep_models_loaded == "after_use")),
        "groq" => Arc::new(OpenAiCompatibleStt::new(&compat::GROQ, h, &base, model)),
        "mistral" => Arc::new(OpenAiCompatibleStt::new(&compat::MISTRAL, h, &base, model)),
        "custom" => Arc::new(OpenAiCompatibleStt::new(&compat::CUSTOM, h, &base, model)),
        "deepgram" => Arc::new(DeepgramStt::new(h, &base, model)),
        "elevenlabs" => Arc::new(ElevenLabsStt::new(h, &base, model)),
        "assemblyai" => Arc::new(AssemblyAiStt::new(h, &base, model)),
        "gemini" => Arc::new(GeminiStt::new(h, &base, model)),
        _ => Arc::new(OpenAiCompatibleStt::new(&compat::OPENAI, h, &base_url(s, "openai"), model)),
    }
}

pub fn build_cleanup(s: &Settings, http: &reqwest::Client) -> Arc<dyn CleanupProvider> {
    build_cleanup_with_timeout(s, http, chat::TIMEOUT)
}

/// Evals lift the 1.5 s cap: they judge prompt quality, not latency.
pub fn build_cleanup_with_timeout(s: &Settings, http: &reqwest::Client, t: Duration) -> Arc<dyn CleanupProvider> {
    let id = s.cleanup_provider.as_str();
    let (base, model, h) = (base_url(s, id), s.cleanup_model.as_str(), http.clone());
    let compat =
        |p: &'static chat::ChatPreset| -> Arc<dyn CleanupProvider> { Arc::new(OpenAiCleanup::new(p, http.clone(), &base, model, t)) };
    match id {
        "openai" => compat(&chat::OPENAI),
        "groq" => compat(&chat::GROQ),
        "mistral" => compat(&chat::MISTRAL),
        "openrouter" => compat(&chat::OPENROUTER),
        "custom" => compat(&chat::CUSTOM),
        "anthropic" => Arc::new(AnthropicCleanup::new(h, &base, model, t)),
        "gemini" => Arc::new(GeminiCleanup::new(h, &base, model, t)),
        "llama_cpp" => Arc::new(LlamaCppCleanup::from_settings(s, t)),
        _ => Arc::new(PassthroughCleanup),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_available_provider_builds_and_has_a_key_entry() {
        let http = reqwest::Client::new();
        for p in STT_PROVIDERS.iter().filter(|p| p.available) {
            let s = Settings { stt_provider: p.id.into(), ..Default::default() };
            assert_eq!(build_stt(&s, &http).id(), p.id);
            assert!(LOCAL.contains(&p.id) || crate::secrets::check(p.id).is_ok(), "{}", p.id);
            assert_ne!(display_name(p.id), "The provider");
        }
        for p in CLEANUP_PROVIDERS.iter().filter(|p| p.available) {
            let s = Settings { cleanup_provider: p.id.into(), ..Default::default() };
            assert_eq!(build_cleanup(&s, &http).id(), p.id);
            if p.id != "none" && !LOCAL.contains(&p.id) {
                assert!(crate::secrets::check(p.id).is_ok(), "{}", p.id);
            }
        }
    }

    #[test]
    fn local_model_lists_match_the_catalogue() {
        let ids =
            |kind: &str| crate::models::catalogue().models.iter().filter(|m| m.kind == kind).map(|m| m.id.as_str()).collect::<Vec<_>>();
        assert_eq!(LOCAL_STT_MODELS, ids("stt").as_slice());
        assert_eq!(LOCAL_CLEANUP_MODELS, ids("cleanup").as_slice());
    }

    #[test]
    fn base_urls_come_from_settings() {
        let mut s = Settings::default();
        assert_eq!(base_url(&s, "deepgram"), "https://api.deepgram.com/v1");
        s.base_urls.insert("deepgram".into(), "https://eu.example/v1".into());
        assert_eq!(base_url(&s, "deepgram"), "https://eu.example/v1");
        s.custom_base_url = "http://localhost:8000/v1".into();
        assert_eq!(base_url(&s, "custom"), "http://localhost:8000/v1");
        assert_eq!(base_url(&s, "openai"), s.openai_base_url);
    }

    #[test]
    fn key_test_urls() {
        assert_eq!(key_test_url("openai", "https://api.openai.com/v1/"), "https://api.openai.com/v1/models");
        assert_eq!(key_test_url("deepgram", "https://api.deepgram.com/v1"), "https://api.deepgram.com/v1/projects");
        assert_eq!(key_test_url("openrouter", "https://openrouter.ai/api/v1"), "https://openrouter.ai/api/v1/key");
        assert_eq!(key_test_url("assemblyai", "https://api.assemblyai.com"), "https://api.assemblyai.com/v2/transcript?limit=1");
    }
}
