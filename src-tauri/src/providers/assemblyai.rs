//! AssemblyAI (async): POST {base}/v2/upload (raw bytes) → POST {base}/v2/transcript
//! → poll GET {base}/v2/transcript/{id} every 250 ms, hard timeout 30 s.
//! Auth: the raw key in `Authorization`, no "Bearer" (✓ assemblyai.com API reference,
//! checked 24 Oct 2026). Slow by design: good for Retry, not for live dictation.

use super::{http, vocab, AudioClip, SttOptions, SttProvider, Transcript};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(250);
/// 30 s, or longer for long hands-free clips (15 s + clip length).
fn deadline(clip_ms: u64) -> Duration {
    Duration::from_secs(30.max(15 + clip_ms / 1000))
}

pub struct AssemblyAiStt {
    http: reqwest::Client,
    base_url: String,
    model: String,
}

impl AssemblyAiStt {
    pub fn new(http: reqwest::Client, base_url: &str, model: &str) -> Self {
        Self { http, base_url: base_url.trim_end_matches('/').to_string(), model: model.to_string() }
    }
}

/// Transcript request. Auto-detect needs ≥ 15 s of audio, so the UI requires a fixed
/// language; if none reaches us we still ask for detection rather than fail.
/// `keyterms_prompt` is never combined with `prompt`.
pub fn request_body(audio_url: &str, model: &str, opts: &SttOptions) -> Value {
    let mut b = json!({ "audio_url": audio_url, "speech_models": [model] });
    match opts.languages.iter().map(|l| l.trim()).find(|l| !l.is_empty()) {
        Some(l) => b["language_code"] = json!(l),
        None => b["language_detection"] = json!(true),
    }
    let terms = vocab::ASSEMBLYAI.apply(&opts.keywords);
    if !terms.is_empty() {
        b["keyterms_prompt"] = json!(terms);
    }
    b
}

#[derive(Deserialize)]
struct Upload {
    upload_url: String,
}
#[derive(Deserialize)]
struct Job {
    id: String,
    status: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    language_code: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

const ID: &str = "assemblyai";

impl AssemblyAiStt {
    async fn send<T: serde::de::DeserializeOwned>(&self, rb: reqwest::RequestBuilder, key: Option<&str>, left: Duration) -> Result<T> {
        let resp = http::auth(rb, ID, key).timeout(left).send().await.context("network error calling assemblyai")?;
        http::check(ID, resp).await?.json().await.context("unexpected assemblyai response")
    }
}

#[async_trait]
impl SttProvider for AssemblyAiStt {
    fn id(&self) -> &'static str {
        ID
    }

    async fn prewarm(&self) {
        http::prewarm(&self.http, ID, &self.base_url, super::registry::key_test_url(ID, &self.base_url)).await;
    }

    async fn transcribe(&self, audio: &AudioClip, opts: &SttOptions) -> Result<Transcript> {
        let key = http::key_for(ID, &self.base_url)?;
        let key = key.as_deref();
        let start = Instant::now();
        let deadline = deadline(audio.duration_ms);
        let left = || deadline.checked_sub(start.elapsed()).filter(|d| !d.is_zero());
        let timed_out = || anyhow!("AssemblyAI timed out after {} s", deadline.as_secs());

        let up: Upload = self.send(self.http.post(format!("{}/v2/upload", self.base_url)).body(audio.bytes.clone()), key, deadline).await?;
        let body = request_body(&up.upload_url, &self.model, opts);
        let mut job: Job =
            self.send(self.http.post(format!("{}/v2/transcript", self.base_url)).json(&body), key, left().ok_or_else(timed_out)?).await?;
        loop {
            match job.status.as_str() {
                "completed" => {
                    return Ok(Transcript { text: job.text.unwrap_or_default().trim().to_string(), language: job.language_code })
                }
                "error" => bail!("AssemblyAI error: {}", job.error.unwrap_or_default()),
                _ => {}
            }
            let wait = left().ok_or_else(timed_out)?;
            tokio::time::sleep(POLL.min(wait)).await;
            let url = format!("{}/v2/transcript/{}", self.base_url, job.id);
            job = self.send(self.http.get(url), key, left().ok_or_else(timed_out)?).await?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_with_language_and_terms() {
        let o = SttOptions {
            languages: vec!["ar".into()],
            keywords: vec!["Yappaflow".into(), "a b c d e f g".into()],
            prompt: Some("x".into()),
        };
        let b = request_body("https://cdn/u", "universal-3-5-pro", &o);
        assert_eq!(
            b,
            json!({ "audio_url": "https://cdn/u", "speech_models": ["universal-3-5-pro"], "language_code": "ar", "keyterms_prompt": ["Yappaflow"] })
        );
        assert!(b.get("prompt").is_none());
    }

    #[test]
    fn deadline_grows_with_clip() {
        assert_eq!(deadline(5_000).as_secs(), 30);
        assert_eq!(deadline(120_000).as_secs(), 135);
    }

    #[test]
    fn body_auto_detect_without_terms() {
        let b = request_body("u", "universal-2", &SttOptions::default());
        assert_eq!(b, json!({ "audio_url": "u", "speech_models": ["universal-2"], "language_detection": true }));
    }
}
