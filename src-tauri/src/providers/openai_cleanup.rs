//! OpenAI-compatible cleanup via Chat Completions: OpenAI, Groq, Mistral, OpenRouter,
//! Custom URL and the local llama-server (llama_cpp.rs). Each preset carries its reasoning-off
//! switch as an `extra_body` patch, so no provider is special-cased in code
//! (docs/providers.md). Hard 1.5 s timeout: the pipeline falls back to the raw transcript.

use super::{http, CleanupContext, CleanupProvider};
use crate::cleanup;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

pub const TIMEOUT: Duration = Duration::from_millis(1500);
/// Command-mode rewrites can be long (up to 8 000 chars); Esc still cancels at any time.
pub const EDIT_TIMEOUT: Duration = Duration::from_secs(30);

/// System prompt, user message and output cap for a command-mode edit (same for every provider).
pub fn edit_parts(selection: Option<&str>, instruction: &str, ctx: &CleanupContext) -> (String, String, u32) {
    (
        cleanup::COMMAND_SYSTEM.to_string(),
        cleanup::command_user_message(selection, instruction, ctx),
        cleanup::edit_max_tokens(selection, instruction),
    )
}

#[derive(Debug)]
pub struct ChatPreset {
    pub id: &'static str,
    /// "max_completion_tokens" (OpenAI, Groq) or "max_tokens" (Mistral, OpenRouter, most servers).
    pub max_tokens_field: &'static str,
    /// Extra output tokens on top of the cap (reasoning tokens count against it).
    pub headroom: u32,
    /// JSON object merged into the request body.
    pub extra_body: &'static str,
}

pub const OPENAI: ChatPreset =
    ChatPreset { id: "openai", max_tokens_field: "max_completion_tokens", headroom: 0, extra_body: r#"{"reasoning_effort":"none"}"# };
/// gpt-oss has no "none"; reasoning goes in message.reasoning, which we drop.
pub const GROQ: ChatPreset = ChatPreset {
    id: "groq",
    max_tokens_field: "max_completion_tokens",
    headroom: 512,
    extra_body: r#"{"reasoning_effort":"low","include_reasoning":false}"#,
};
pub const MISTRAL: ChatPreset = ChatPreset { id: "mistral", max_tokens_field: "max_tokens", headroom: 0, extra_body: "{}" };
/// ✓ openrouter.ai/docs reasoning tokens: `reasoning.effort: "none"` disables reasoning.
pub const OPENROUTER: ChatPreset =
    ChatPreset { id: "openrouter", max_tokens_field: "max_tokens", headroom: 0, extra_body: r#"{"reasoning":{"effort":"none"}}"# };
pub const CUSTOM: ChatPreset = ChatPreset { id: "custom", max_tokens_field: "max_tokens", headroom: 0, extra_body: "{}" };

pub struct OpenAiCleanup {
    preset: &'static ChatPreset,
    http: reqwest::Client,
    base_url: String,
    model: String,
    timeout: Duration,
    /// Per-launch key of our own llama-server; skips the keychain and the https check.
    local_key: Option<String>,
}

impl OpenAiCleanup {
    pub fn new(preset: &'static ChatPreset, http: reqwest::Client, base_url: &str, model: &str, timeout: Duration) -> Self {
        Self { preset, http, base_url: base_url.trim_end_matches('/').to_string(), model: model.to_string(), timeout, local_key: None }
    }

    pub fn with_local_key(mut self, key: String) -> Self {
        self.local_key = Some(key);
        self
    }

    /// One chat turn, shared by cleanup and command-mode edits.
    async fn complete(&self, system: String, user: String, max_out: u32, timeout: Duration) -> Result<String> {
        let id = self.preset.id;
        let key = match &self.local_key {
            Some(k) => Some(k.clone()),
            None => http::key_for(id, &self.base_url)?,
        };
        let rb = self.http.post(format!("{}/chat/completions", self.base_url)).json(&body(self.preset, &self.model, system, user, max_out));
        let resp = http::auth(rb, id, key.as_deref()).timeout(timeout).send().await.context("cleanup request failed")?;
        let r: Resp = http::check(id, resp).await?.json().await.context("unexpected cleanup response")?;
        Ok(r.choices.into_iter().next().and_then(|c| c.message.content).unwrap_or_default())
    }
}

pub fn body(p: &ChatPreset, model: &str, system: String, user: String, max_out: u32) -> Value {
    let mut b = json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ],
    });
    b[p.max_tokens_field] = json!(max_out + p.headroom);
    if let (Some(obj), Ok(Value::Object(extra))) = (b.as_object_mut(), serde_json::from_str::<Value>(p.extra_body)) {
        obj.extend(extra);
    }
    b
}

#[derive(Deserialize)]
struct Resp {
    choices: Vec<Choice>,
}
#[derive(Deserialize)]
struct Choice {
    message: Msg,
}
#[derive(Deserialize)]
struct Msg {
    content: Option<String>,
}

#[async_trait]
impl CleanupProvider for OpenAiCleanup {
    fn id(&self) -> &'static str {
        self.preset.id
    }

    async fn prewarm(&self) {
        http::prewarm(&self.http, self.preset.id, &self.base_url, format!("{}/models/{}", self.base_url, self.model)).await;
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

    /// Regression: the exact body OpenAI got in Phases 2–4.
    #[test]
    fn openai_body_unchanged() {
        let b = body(&OPENAI, "gpt-6-luna", "sys".into(), "usr".into(), 100);
        let want = json!({
            "model": "gpt-6-luna", "reasoning_effort": "none", "max_completion_tokens": 100,
            "messages": [{ "role": "system", "content": "sys" }, { "role": "user", "content": "usr" }],
        });
        assert_eq!(b, want);
    }

    #[test]
    fn edit_body_uses_command_prompt() {
        let ctx = CleanupContext { app_exe: "outlook.exe".into(), ..Default::default() };
        let (sys, user, max) = edit_parts(Some("hi"), "make it formal", &ctx);
        let b = body(&OPENAI, "gpt-6-luna", sys, user, max);
        assert_eq!(b["messages"][0]["content"], cleanup::COMMAND_SYSTEM);
        let u = b["messages"][1]["content"].as_str().unwrap();
        assert!(u.contains("<instruction>\nmake it formal\n</instruction>") && u.contains("<selection>\nhi\n</selection>"));
        assert_eq!(b["max_completion_tokens"], json!(cleanup::edit_max_tokens(Some("hi"), "make it formal")));
        assert_eq!(b["reasoning_effort"], "none");
    }

    #[test]
    fn presets_patch_reasoning() {
        let b = body(&GROQ, "openai/gpt-oss-20b", "s".into(), "u".into(), 100);
        assert_eq!(
            (b["reasoning_effort"].clone(), b["include_reasoning"].clone(), b["max_completion_tokens"].clone()),
            (json!("low"), json!(false), json!(612))
        );
        let b = body(&OPENROUTER, "openai/gpt-6-luna", "s".into(), "u".into(), 100);
        assert_eq!((b["reasoning"].clone(), b["max_tokens"].clone()), (json!({ "effort": "none" }), json!(100)));
        for p in [&MISTRAL, &CUSTOM] {
            let b = body(p, "m", "s".into(), "u".into(), 100);
            assert_eq!(b.as_object().unwrap().len(), 3, "{p:?}");
            assert_eq!(b["max_tokens"], json!(100));
        }
        for p in [&OPENAI, &GROQ, &MISTRAL, &OPENROUTER, &CUSTOM] {
            assert!(serde_json::from_str::<Value>(p.extra_body).unwrap().is_object(), "{p:?}");
        }
    }
}
