//! Gemini cleanup: POST {base}/models/{model}:generateContent with a systemInstruction.
//! Thinking: "minimal" (the lowest level) is the default for gemini-3.1-flash-lite and
//! gemini-3.5-flash-lite (✓ ai.google.dev Gemini 3 + thinking docs, 24 Oct 2026), so we
//! send no thinking config. The generateContent field name for it is UNVERIFIED, and a
//! wrong field would fail every request.

use super::gemini_stt::response_text;
use super::openai_cleanup::{edit_parts, EDIT_TIMEOUT};
use super::{http, CleanupContext, CleanupProvider};
use crate::cleanup;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

const ID: &str = "gemini";

pub struct GeminiCleanup {
    http: reqwest::Client,
    base_url: String,
    model: String,
    timeout: Duration,
}

impl GeminiCleanup {
    pub fn new(http: reqwest::Client, base_url: &str, model: &str, timeout: Duration) -> Self {
        Self { http, base_url: base_url.trim_end_matches('/').to_string(), model: model.to_string(), timeout }
    }

    async fn complete(&self, system: String, user: String, max_out: u32, timeout: Duration) -> Result<String> {
        let key = http::key_for(ID, &self.base_url)?;
        let rb = self.http.post(format!("{}/models/{}:generateContent", self.base_url, self.model)).json(&body(system, user, max_out));
        let resp = http::auth(rb, ID, key.as_deref()).timeout(timeout).send().await.context("cleanup request failed")?;
        let v: Value = http::check(ID, resp).await?.json().await.context("unexpected cleanup response")?;
        Ok(response_text(&v))
    }
}

pub fn body(system: String, user: String, max_out: u32) -> Value {
    json!({
        "systemInstruction": { "parts": [{ "text": system }] },
        "contents": [{ "role": "user", "parts": [{ "text": user }] }],
        // +256: minimal thinking may still spend a few tokens against the output cap.
        "generationConfig": { "maxOutputTokens": max_out + 256 },
    })
}

#[async_trait]
impl CleanupProvider for GeminiCleanup {
    fn id(&self) -> &'static str {
        ID
    }

    async fn prewarm(&self) {
        http::prewarm(&self.http, ID, &self.base_url, super::registry::key_test_url(ID, &self.base_url)).await;
    }

    async fn clean(&self, text: &str, ctx: &CleanupContext) -> Result<String> {
        let system = cleanup::system_prompt(&ctx.tone, &ctx.disabled_rules, &ctx.custom_rules);
        let out = self.complete(system, cleanup::user_message(text, ctx), cleanup::max_output_tokens(text), self.timeout).await?;
        if cleanup::looks_like_answer(text, &out) {
            bail!("cleanup output looks like an answer, discarding");
        }
        Ok(out.trim().to_string())
    }

    async fn edit(&self, selection: Option<&str>, instruction: &str, ctx: &CleanupContext) -> Result<String> {
        let (system, user, max) = edit_parts(selection, instruction, ctx);
        let out = self.complete(system, user, max, self.timeout.max(EDIT_TIMEOUT)).await?;
        Ok(cleanup::strip_fences(&out, selection))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_body_uses_command_prompt() {
        let (sys, user, max) = edit_parts(Some("hola"), "translate to English", &CleanupContext::default());
        let b = body(sys, user, max);
        assert_eq!(b["systemInstruction"]["parts"][0]["text"], cleanup::COMMAND_SYSTEM);
        assert!(b["contents"][0]["parts"][0]["text"].as_str().unwrap().contains("<selection>\nhola\n</selection>"));
        assert_eq!(b["generationConfig"]["maxOutputTokens"], json!(cleanup::edit_max_tokens(Some("hola"), "translate to English") + 256));
    }

    #[test]
    fn body_shape() {
        let b = super::body("sys".into(), "usr".into(), 90);
        assert_eq!(b["systemInstruction"]["parts"][0]["text"], "sys");
        assert_eq!(b["contents"][0]["parts"][0]["text"], "usr");
        assert_eq!(b["generationConfig"]["maxOutputTokens"], 346);
        assert!(b["generationConfig"].get("thinkingConfig").is_none());
    }
}
