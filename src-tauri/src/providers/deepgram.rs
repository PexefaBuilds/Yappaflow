//! Deepgram pre-recorded: POST {base}/listen with the raw audio body and options in
//! the query string. Auth `Authorization: Token <key>` (✓ developers.deepgram.com,
//! listen-pre-recorded reference, checked 24 Oct 2026).

use super::{http, vocab, AudioClip, SttOptions, SttProvider, Transcript};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

pub struct DeepgramStt {
    http: reqwest::Client,
    base_url: String,
    model: String,
}

impl DeepgramStt {
    pub fn new(http: reqwest::Client, base_url: &str, model: &str) -> Self {
        Self { http, base_url: base_url.trim_end_matches('/').to_string(), model: model.to_string() }
    }
}

/// Query params in send order. Arabic isn't in Deepgram's auto-detect list, so
/// `detect_language=true` only when the user left Language on auto (UI caveat).
pub fn query(model: &str, opts: &SttOptions) -> Vec<(&'static str, String)> {
    let mut q = vec![("model", model.to_string()), ("smart_format", "true".into())];
    match opts.languages.iter().map(|l| l.trim()).find(|l| !l.is_empty()) {
        Some(l) => q.push(("language", l.to_string())),
        None => q.push(("detect_language", "true".into())),
    }
    q.extend(vocab::DEEPGRAM.apply(&opts.keywords).into_iter().map(|k| ("keyterm", k)));
    q
}

#[derive(Deserialize)]
struct Resp {
    results: Results,
}
#[derive(Deserialize)]
struct Results {
    channels: Vec<Channel>,
}
#[derive(Deserialize)]
struct Channel {
    #[serde(default)]
    alternatives: Vec<Alt>,
    #[serde(default)]
    detected_language: Option<String>,
}
#[derive(Deserialize)]
struct Alt {
    transcript: String,
}

fn parse(r: Resp) -> Transcript {
    let ch = r.results.channels.into_iter().next();
    let language = ch.as_ref().and_then(|c| c.detected_language.clone());
    let text = ch.and_then(|c| c.alternatives.into_iter().next()).map(|a| a.transcript).unwrap_or_default();
    Transcript { text: text.trim().to_string(), language }
}

#[async_trait]
impl SttProvider for DeepgramStt {
    fn id(&self) -> &'static str {
        "deepgram"
    }

    async fn prewarm(&self) {
        http::prewarm(&self.http, self.id(), &self.base_url, super::registry::key_test_url(self.id(), &self.base_url)).await;
    }

    async fn transcribe(&self, audio: &AudioClip, opts: &SttOptions) -> Result<Transcript> {
        let key = http::key_for("deepgram", &self.base_url)?;
        let rb = self
            .http
            .post(format!("{}/listen", self.base_url))
            .query(&query(&self.model, opts))
            .header("Content-Type", audio.mime)
            .body(audio.bytes.clone())
            .timeout(Duration::from_secs(15 + audio.duration_ms / 1000));
        let resp = http::auth(rb, "deepgram", key.as_deref()).send().await.context("network error calling deepgram")?;
        let r: Resp = http::check("deepgram", resp).await?.json().await.context("unexpected deepgram response")?;
        Ok(parse(r))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_auto_and_fixed_language() {
        let o = SttOptions { keywords: vec!["Yappa flow".into(), "<x>".into()], ..Default::default() };
        let q = query("nova-3", &o);
        assert_eq!(q[0], ("model", "nova-3".into()));
        assert!(q.contains(&("detect_language", "true".into())));
        assert!(!q.iter().any(|(k, _)| *k == "language"));
        assert_eq!(q.iter().filter(|(k, _)| *k == "keyterm").count(), 2);
        let o = SttOptions { languages: vec!["ar".into()], ..Default::default() };
        let q = query("nova-3", &o);
        assert!(q.contains(&("language", "ar".into())) && !q.iter().any(|(k, _)| *k == "detect_language"));
    }

    #[test]
    fn parses_response() {
        let r: Resp =
            serde_json::from_str(r#"{"results":{"channels":[{"detected_language":"en","alternatives":[{"transcript":" Hi there. "}]}]}}"#)
                .unwrap();
        let t = parse(r);
        assert_eq!((t.text.as_str(), t.language.as_deref()), ("Hi there.", Some("en")));
    }
}
