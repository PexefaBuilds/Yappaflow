//! Anthropic Messages API cleanup: POST {base}/messages, headers `x-api-key` +
//! `anthropic-version` (see http.rs), `thinking: {type: "disabled"}`
//! (✓ platform.claude.com Messages reference, checked 24 Oct 2026).

use super::openai_cleanup::{edit_parts, EDIT_TIMEOUT};
use super::{http, CleanupContext, CleanupProvider};
use crate::cleanup;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

const ID: &str = "anthropic";

pub struct AnthropicCleanup {
    http: reqwest::Client,
    base_url: String,
    model: String,
    timeout: Duration,
}

impl AnthropicCleanup {
    pub fn new(http: reqwest::Client, base_url: &str, model: &str, timeout: Duration) -> Self {
        Self { http, base_url: base_url.trim_end_matches('/').to_string(), model: model.to_string(), timeout }
    }

    async fn complete(&self, system: String, user: String, max_out: u32, timeout: Duration) -> Result<String> {
        let key = http::key_for(ID, &self.base_url)?;
        let rb = self.http.post(format!("{}/messages", self.base_url)).json(&body(&self.model, system, user, max_out));
        let resp = http::auth(rb, ID, key.as_deref()).timeout(timeout).send().await.context("cleanup request failed")?;
        let v: Value = http::check(ID, resp).await?.json().await.context("unexpected cleanup response")?;
        Ok(response_text(&v))
    }
}

pub fn body(model: &str, system: String, user: String, max_out: u32) -> Value {
    json!({
        "model": model,
        "max_tokens": max_out,
        "thinking": { "type": "disabled" },
        "system": system,
        "messages": [{ "role": "user", "content": user }],
    })
}

/// Joins the `text` blocks of the response content.
fn response_text(v: &Value) -> String {
    v["content"]
        .as_array()
        .map(|c| c.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect())
        .unwrap_or_default()
}

#[async_trait]
impl CleanupProvider for AnthropicCleanup {
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
    fn body_disables_thinking() {
        let b = body("claude-haiku-5-5", "sys".into(), "usr".into(), 80);
        assert_eq!(b["thinking"], json!({ "type": "disabled" }));
        assert_eq!((b["system"].clone(), b["max_tokens"].clone()), (json!("sys"), json!(80)));
        assert_eq!(b["messages"], json!([{ "role": "user", "content": "usr" }]));
    }

    #[test]
    fn edit_body_uses_command_prompt() {
        let (sys, user, max) = edit_parts(None, "draft a thank-you note", &CleanupContext::default());
        let b = body("claude-haiku-5-5", sys, user, max);
        assert_eq!(b["system"], cleanup::COMMAND_SYSTEM);
        assert!(b["messages"][0]["content"].as_str().unwrap().contains("<instruction>\ndraft a thank-you note\n</instruction>"));
        assert_eq!(b["max_tokens"], json!(cleanup::edit_max_tokens(None, "draft a thank-you note")));
        assert_eq!(b["thinking"], json!({ "type": "disabled" }));
    }

    #[test]
    fn parses_text_blocks() {
        let v = json!({ "content": [{ "type": "text", "text": "Hello" }, { "type": "text", "text": " there." }] });
        assert_eq!(response_text(&v), "Hello there.");
    }
}
