//! OpenAI-compatible file transcription: POST {base}/audio/transcriptions (multipart).
//! One client, one preset per provider (OpenAI, Groq, Mistral, Custom URL): the
//! preset holds the vocabulary strategy and which optional fields the server takes.
//! See docs/providers.md.
//!
//! `gpt-transcribe*` takes `languages[]` + `keywords[]` (never the singular `language`).
//! Every other model gets the singular `language`, with terms in the preset's field.

use super::vocab::{self, VocabStrategy};
use super::{http, AudioClip, SttOptions, SttProvider, Transcript};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use std::time::Duration;

/// What differs between OpenAI-compatible transcription servers.
#[derive(Debug)]
pub struct CompatPreset {
    pub id: &'static str,
    /// Used for models without `keywords[]` (all but gpt-transcribe).
    pub vocab: VocabStrategy,
    /// Send `response_format=json` (Mistral doesn't document the field).
    pub response_format: bool,
    /// Server takes a free-text `prompt`.
    pub prompt: bool,
}

pub const OPENAI: CompatPreset = CompatPreset { id: "openai", vocab: vocab::WHISPER_PROMPT, response_format: true, prompt: true };
pub const GROQ: CompatPreset = CompatPreset { id: "groq", vocab: vocab::WHISPER_PROMPT, response_format: true, prompt: true };
// UNVERIFIED: full OpenAI compatibility, and the multipart encoding of context_bias
// (sent as repeated fields). Mistral documents no `prompt` or `response_format`.
pub const MISTRAL: CompatPreset = CompatPreset { id: "mistral", vocab: vocab::MISTRAL, response_format: false, prompt: false };
pub const CUSTOM: CompatPreset = CompatPreset { id: "custom", vocab: vocab::WHISPER_PROMPT, response_format: true, prompt: true };

pub struct OpenAiCompatibleStt {
    preset: &'static CompatPreset,
    http: reqwest::Client,
    base_url: String,
    model: String,
}

impl OpenAiCompatibleStt {
    pub fn new(preset: &'static CompatPreset, http: reqwest::Client, base_url: &str, model: &str) -> Self {
        Self { preset, http, base_url: base_url.trim_end_matches('/').to_string(), model: model.to_string() }
    }
}

#[derive(Deserialize)]
struct LangCode {
    code: String,
}

#[derive(Deserialize)]
struct Resp {
    text: String,
    /// gpt-transcribe: `[{ "code": "en" }]`, empty when detection isn't confident.
    #[serde(default)]
    languages: Option<Vec<LangCode>>,
    /// Older models / some compatible servers (e.g. Mistral) return a single code.
    #[serde(default)]
    language: Option<String>,
}

#[async_trait]
impl SttProvider for OpenAiCompatibleStt {
    fn id(&self) -> &'static str {
        self.preset.id
    }

    /// Opens (or refreshes) the pooled TLS + HTTP/2 connection while the user talks,
    /// so the real upload skips DNS/TCP/TLS (~150–300 ms).
    async fn prewarm(&self) {
        http::prewarm(&self.http, self.preset.id, &self.base_url, format!("{}/models/{}", self.base_url, self.model)).await;
    }

    async fn transcribe(&self, audio: &AudioClip, opts: &SttOptions) -> Result<Transcript> {
        let id = self.preset.id;
        let key = http::key_for(id, &self.base_url)?;
        let part = Part::bytes(audio.bytes.clone()).file_name(audio.filename).mime_str(audio.mime)?;
        let form = form_fields(self.preset, &self.model, opts).into_iter().fold(Form::new().part("file", part), |f, (k, v)| f.text(k, v));

        // Generous timeout: long hands-free clips take longer; the audio is on disk anyway.
        let timeout = Duration::from_secs(15 + audio.duration_ms / 1000);
        let rb = self.http.post(format!("{}/audio/transcriptions", self.base_url));
        let resp = http::auth(rb, id, key.as_deref())
            .multipart(form)
            .timeout(timeout)
            .send()
            .await
            .with_context(|| format!("network error calling {id}"))?;
        let r: Resp = http::check(id, resp).await?.json().await.with_context(|| format!("unexpected {id} response"))?;
        let language = match r.languages {
            Some(list) => list.into_iter().next().map(|l| l.code),
            None => r.language,
        };
        Ok(Transcript { text: r.text.trim().to_string(), language })
    }
}

// ── Pure helpers (unit-tested below) ─────────────

/// `gpt-transcribe*` models take `languages[]` / `keywords[]` instead of `language`.
pub fn uses_list_params(model: &str) -> bool {
    model.starts_with("gpt-transcribe")
}

/// The multipart text fields after `file`, in send order.
pub fn form_fields(p: &CompatPreset, model: &str, opts: &SttOptions) -> Vec<(&'static str, String)> {
    let mut f = vec![("model", model.to_string())];
    if p.response_format {
        f.push(("response_format", "json".into()));
    }
    let languages: Vec<String> = opts.languages.iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    let prompt = opts.prompt.as_deref().filter(|p| !p.is_empty());

    if uses_list_params(model) {
        if let Some(pr) = prompt {
            f.push(("prompt", pr.to_string()));
        }
        // `languages[]` replaces `language`; never send both.
        f.extend(languages.into_iter().map(|l| ("languages[]", l)));
        f.extend(vocab::OPENAI_KEYWORDS.apply(&opts.keywords).into_iter().map(|k| ("keywords[]", k)));
        return f;
    }
    match p.vocab {
        VocabStrategy::Prompt { max_tokens } => {
            if let Some(pr) = vocab::fold_into_prompt(prompt, &p.vocab.apply(&opts.keywords), max_tokens) {
                f.push(("prompt", pr));
            }
        }
        v => {
            if let (true, Some(pr)) = (p.prompt, prompt.map(str::trim).filter(|p| !p.is_empty())) {
                f.push(("prompt", pr.to_string()));
            }
            f.extend(v.apply(&opts.keywords).into_iter().map(|k| ("context_bias", k)));
        }
    }
    // The singular field takes one code; extra entries are ignored.
    if let Some(l) = languages.into_iter().next() {
        f.push(("language", l));
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }
    fn opts(prompt: Option<&str>, langs: &[&str], kw: &[&str]) -> SttOptions {
        SttOptions { prompt: prompt.map(str::to_string), languages: s(langs), keywords: s(kw) }
    }
    fn f(v: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
        v.iter().map(|(k, x)| (*k, x.to_string())).collect()
    }

    #[test]
    fn model_switch() {
        assert!(uses_list_params("gpt-transcribe"));
        assert!(uses_list_params("gpt-transcribe-2026-10-01"));
        assert!(!uses_list_params("gpt-4o-transcribe"));
        assert!(!uses_list_params("gpt-4o-mini-transcribe"));
        assert!(!uses_list_params("whisper-large-v3-turbo"));
        assert!(!uses_list_params("voxtral-mini-2602"));
    }

    /// Regression: the exact fields OpenAI got in Phases 1–4.
    #[test]
    fn openai_fields_unchanged() {
        let o = opts(Some("ctx"), &[" en ", ""], &["<Yappa>flow", "", "Tauri\n"]);
        assert_eq!(
            form_fields(&OPENAI, "gpt-transcribe", &o),
            f(&[
                ("model", "gpt-transcribe"),
                ("response_format", "json"),
                ("prompt", "ctx"),
                ("languages[]", "en"),
                ("keywords[]", "Yappaflow"),
                ("keywords[]", "Tauri")
            ])
        );
        assert_eq!(
            form_fields(&OPENAI, "gpt-4o-transcribe", &o),
            f(&[
                ("model", "gpt-4o-transcribe"),
                ("response_format", "json"),
                ("prompt", "ctx\nVocabulary: Yappaflow, Tauri"),
                ("language", "en")
            ])
        );
        assert_eq!(
            form_fields(&OPENAI, "gpt-transcribe", &SttOptions::default()),
            f(&[("model", "gpt-transcribe"), ("response_format", "json")])
        );
        assert_eq!(
            form_fields(&OPENAI, "gpt-4o-mini-transcribe", &opts(None, &["ar", "en"], &[])),
            f(&[("model", "gpt-4o-mini-transcribe"), ("response_format", "json"), ("language", "ar")])
        );
    }

    #[test]
    fn groq_and_custom_fold_terms_into_prompt() {
        let o = opts(None, &["ar"], &["a", "b"]);
        for p in [&GROQ, &CUSTOM] {
            assert_eq!(
                form_fields(p, "whisper-large-v3-turbo", &o),
                f(&[("model", "whisper-large-v3-turbo"), ("response_format", "json"), ("prompt", "Vocabulary: a, b"), ("language", "ar")])
            );
        }
    }

    #[test]
    fn mistral_uses_context_bias_and_singular_language() {
        let o = opts(Some("ignored"), &["ar"], &["Yappa<flow>", "Tauri"]);
        assert_eq!(
            form_fields(&MISTRAL, "voxtral-mini-2602", &o),
            f(&[("model", "voxtral-mini-2602"), ("context_bias", "Yappaflow"), ("context_bias", "Tauri"), ("language", "ar")])
        );
        assert_eq!(form_fields(&MISTRAL, "voxtral-mini-2602", &SttOptions::default()), f(&[("model", "voxtral-mini-2602")]));
    }
}
