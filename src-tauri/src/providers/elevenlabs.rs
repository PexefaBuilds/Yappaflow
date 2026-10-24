//! ElevenLabs Scribe: multipart POST {base}/speech-to-text, header `xi-api-key`,
//! model field `model_id` (✓ elevenlabs.io API reference, checked 24 Oct 2026).

use super::{http, vocab, AudioClip, SttOptions, SttProvider, Transcript};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use serde::Deserialize;
use std::time::Duration;

pub struct ElevenLabsStt {
    http: reqwest::Client,
    base_url: String,
    model: String,
}

impl ElevenLabsStt {
    pub fn new(http: reqwest::Client, base_url: &str, model: &str) -> Self {
        Self { http, base_url: base_url.trim_end_matches('/').to_string(), model: model.to_string() }
    }
}

/// Text fields after `file`. No `language_code` means auto-detect.
// UNVERIFIED: keyterms sent as repeated multipart fields; the docs list an array
// but don't show its form encoding.
pub fn form_fields(model: &str, opts: &SttOptions) -> Vec<(&'static str, String)> {
    let mut f = vec![("model_id", model.to_string())];
    if let Some(l) = opts.languages.iter().map(|l| l.trim()).find(|l| !l.is_empty()) {
        f.push(("language_code", l.to_string()));
    }
    f.extend(vocab::ELEVENLABS.apply(&opts.keywords).into_iter().map(|k| ("keyterms", k)));
    f
}

#[derive(Deserialize)]
struct Resp {
    text: String,
    #[serde(default)]
    language_code: Option<String>,
}

#[async_trait]
impl SttProvider for ElevenLabsStt {
    fn id(&self) -> &'static str {
        "elevenlabs"
    }

    async fn prewarm(&self) {
        http::prewarm(&self.http, self.id(), &self.base_url, super::registry::key_test_url(self.id(), &self.base_url)).await;
    }

    async fn transcribe(&self, audio: &AudioClip, opts: &SttOptions) -> Result<Transcript> {
        let key = http::key_for("elevenlabs", &self.base_url)?;
        let part = Part::bytes(audio.bytes.clone()).file_name(audio.filename).mime_str(audio.mime)?;
        let form = form_fields(&self.model, opts).into_iter().fold(Form::new().part("file", part), |f, (k, v)| f.text(k, v));
        let rb = self
            .http
            .post(format!("{}/speech-to-text", self.base_url))
            .multipart(form)
            .timeout(Duration::from_secs(15 + audio.duration_ms / 1000));
        let resp = http::auth(rb, "elevenlabs", key.as_deref()).send().await.context("network error calling elevenlabs")?;
        let r: Resp = http::check("elevenlabs", resp).await?.json().await.context("unexpected elevenlabs response")?;
        Ok(Transcript { text: r.text.trim().to_string(), language: r.language_code })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields() {
        let o = SttOptions { keywords: vec!["Yappaflow".into(), "a b c d e f".into()], ..Default::default() };
        assert_eq!(form_fields("scribe_v2", &o), vec![("model_id", "scribe_v2".into()), ("keyterms", "Yappaflow".into())]);
        let o = SttOptions { languages: vec!["ar".into()], ..Default::default() };
        assert_eq!(form_fields("scribe_v2", &o), vec![("model_id", "scribe_v2".into()), ("language_code", "ar".into())]);
    }
}
