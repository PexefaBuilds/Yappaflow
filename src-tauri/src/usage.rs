//! API cost estimate from local History and resources/prices.json. Pure: no IO, no network.
//! Yappaflow itself is free; this only estimates what the user's providers may bill.

use crate::cleanup;
use crate::providers::{registry::STT_PROVIDERS, vocab::approx_tokens, CleanupContext};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct Prices {
    pub checked: String,
    free_providers: Vec<String>,
    stt: Vec<SttPrice>,
    cleanup: Vec<TokenPrice>,
}

#[derive(Deserialize)]
struct SttPrice {
    provider: String,
    model: String,
    /// "minute" | "hour" | "1m_audio_tokens" (Gemini: 32 audio tokens per second)
    unit: String,
    price: f64,
    /// Shorter clips are billed as this long (Groq: 10 s).
    #[serde(default)]
    min_seconds: f64,
}

#[derive(Deserialize)]
struct TokenPrice {
    provider: String,
    model: String,
    /// USD per 1M tokens.
    input: f64,
    output: f64,
}

pub fn prices() -> &'static Prices {
    static P: OnceLock<Prices> = OnceLock::new();
    P.get_or_init(|| serde_json::from_str(include_str!("../resources/prices.json")).expect("resources/prices.json is valid (unit-tested)"))
}

/// One dictation as stored. `None` providers = recorded before v5 (provider not known).
pub struct UsageRow {
    pub created_at: i64,
    pub duration_ms: i64,
    pub stt_provider: Option<String>,
    pub stt_model: String,
    pub cleanup_provider: Option<String>,
    pub cleanup_model: String,
    pub tokens_in: i64,
    pub tokens_out: i64,
}

#[derive(Serialize, Debug)]
pub struct RowCost {
    pub created_at: i64,
    pub duration_ms: i64,
    pub stt_provider: String,
    /// None = price unknown.
    pub stt_usd: Option<f64>,
    /// Empty = no cleanup call (skipped, off, or before v5 when `cleanup_usd` is None).
    pub cleanup_provider: String,
    pub cleanup_usd: Option<f64>,
}

impl Prices {
    fn free(&self, provider: &str) -> bool {
        self.free_providers.iter().any(|p| p == provider)
    }

    /// Old rows have no provider: match the model alone (STT model ids are unique per provider).
    pub fn stt_usd(&self, provider: Option<&str>, model: &str, duration_ms: i64) -> Option<f64> {
        if provider.is_some_and(|p| self.free(p)) {
            return Some(0.0);
        }
        // Old rows: a local model id (whisper.cpp) is free even without a provider.
        if provider.is_none() && STT_PROVIDERS.iter().any(|i| self.free(i.id) && i.models.contains(&model)) {
            return Some(0.0);
        }
        let p = self.stt.iter().find(|p| p.model == model && provider.map_or(true, |id| id == p.provider))?;
        let secs = (duration_ms.max(0) as f64 / 1000.0).max(p.min_seconds);
        match p.unit.as_str() {
            "minute" => Some(secs / 60.0 * p.price),
            "hour" => Some(secs / 3600.0 * p.price),
            "1m_audio_tokens" => Some(secs * 32.0 / 1e6 * p.price),
            _ => None,
        }
    }

    /// `provider` None = recorded before v5 (unknown); "" = no cleanup call was made.
    pub fn cleanup_usd(&self, provider: Option<&str>, model: &str, tokens_in: i64, tokens_out: i64) -> Option<f64> {
        let provider = provider?;
        if provider.is_empty() || self.free(provider) {
            return Some(0.0);
        }
        let p = self.cleanup.iter().find(|p| p.provider == provider && p.model == model)?;
        Some((tokens_in.max(0) as f64 * p.input + tokens_out.max(0) as f64 * p.output) / 1e6)
    }

    pub fn estimate(&self, r: &UsageRow) -> RowCost {
        let stt_provider = r
            .stt_provider
            .clone()
            .unwrap_or_else(|| self.stt.iter().find(|p| p.model == r.stt_model).map_or_else(String::new, |p| p.provider.clone()));
        RowCost {
            created_at: r.created_at,
            duration_ms: r.duration_ms,
            stt_usd: self.stt_usd(r.stt_provider.as_deref(), &r.stt_model, r.duration_ms),
            stt_provider,
            cleanup_provider: r.cleanup_provider.clone().unwrap_or_default(),
            cleanup_usd: self.cleanup_usd(r.cleanup_provider.as_deref(), &r.cleanup_model, r.tokens_in, r.tokens_out),
        }
    }
}

// shortcut: tokens are estimated from the prompt text (conservative), not read from the
// provider's response; reasoning tokens are not counted. Parse `usage` if estimates drift.
pub fn clean_tokens(text: &str, ctx: &CleanupContext, out: &str) -> (i64, i64) {
    let system = cleanup::system_prompt(&ctx.tone, &ctx.disabled_rules, &ctx.custom_rules);
    ((approx_tokens(&system) + approx_tokens(&cleanup::user_message(text, ctx))) as i64, approx_tokens(out) as i64)
}

pub fn edit_tokens(selection: Option<&str>, instruction: &str, ctx: &CleanupContext, out: &str) -> (i64, i64) {
    let user = cleanup::command_user_message(selection, instruction, ctx);
    ((approx_tokens(cleanup::COMMAND_SYSTEM) + approx_tokens(&user)) as i64, approx_tokens(out) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::registry::CLEANUP_PROVIDERS;

    fn close(a: Option<f64>, b: f64) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-9)
    }

    #[test]
    fn every_price_matches_a_registered_model() {
        let p = prices();
        for s in &p.stt {
            assert!(STT_PROVIDERS.iter().any(|i| i.id == s.provider && i.models.contains(&s.model.as_str())), "{}/{}", s.provider, s.model);
            assert!(["minute", "hour", "1m_audio_tokens"].contains(&s.unit.as_str()));
        }
        for c in &p.cleanup {
            assert!(
                CLEANUP_PROVIDERS.iter().any(|i| i.id == c.provider && i.models.contains(&c.model.as_str())),
                "{}/{}",
                c.provider,
                c.model
            );
        }
    }

    #[test]
    fn stt_billing_rules() {
        let p = prices();
        // OpenAI $0.0045/min, pro rata: 30 s = half a minute.
        assert!(close(p.stt_usd(Some("openai"), "gpt-transcribe", 30_000), 0.00225));
        // Groq $0.04/hour with a 10 s minimum: 3 s bills as 10 s, 20 s as 20 s.
        assert!(close(p.stt_usd(Some("groq"), "whisper-large-v3-turbo", 3_000), 10.0 / 3600.0 * 0.04));
        assert!(close(p.stt_usd(Some("groq"), "whisper-large-v3-turbo", 20_000), 20.0 / 3600.0 * 0.04));
        // Gemini: 60 s × 32 tokens/s at $0.30 per 1M.
        assert!(close(p.stt_usd(Some("gemini"), "gemini-3.5-flash-lite", 60_000), 1920.0 * 0.30 / 1e6));
        // Local is free, custom/unlisted is unknown, old rows match by model.
        assert!(close(p.stt_usd(Some("whisper_cpp"), "whisper-base-q5_1", 60_000), 0.0));
        assert_eq!(p.stt_usd(Some("custom"), "my-whisper", 60_000), None);
        assert!(close(p.stt_usd(None, "gpt-4o-transcribe", 60_000), 0.006));
        assert!(close(p.stt_usd(None, "whisper-base-q5_1", 60_000), 0.0), "old local rows are free");
    }

    #[test]
    fn cleanup_billing_rules() {
        let p = prices();
        assert!(close(p.cleanup_usd(Some("openai"), "gpt-6-luna", 1_000_000, 1_000_000), 0.60));
        assert!(close(p.cleanup_usd(Some("groq"), "openai/gpt-oss-20b", 2_000, 100), (2_000.0 * 0.075 + 100.0 * 0.30) / 1e6));
        assert!(close(p.cleanup_usd(Some(""), "", 0, 0), 0.0), "skipped cleanup costs nothing");
        assert!(close(p.cleanup_usd(Some("llama_cpp"), "qwen3.5-4b-q4_k_m", 500, 50), 0.0));
        assert_eq!(p.cleanup_usd(None, "", 0, 0), None, "pre-v5 rows: unknown");
        assert_eq!(p.cleanup_usd(Some("openrouter"), "some/other-model", 10, 10), None);
    }

    #[test]
    fn old_rows_estimate_stt_and_leave_cleanup_unknown() {
        let r = UsageRow {
            created_at: 1,
            duration_ms: 60_000,
            stt_provider: None,
            stt_model: "nova-3".into(),
            cleanup_provider: None,
            cleanup_model: String::new(),
            tokens_in: 0,
            tokens_out: 0,
        };
        let c = prices().estimate(&r);
        assert_eq!(c.stt_provider, "deepgram");
        assert!(close(c.stt_usd, 0.0043));
        assert_eq!(c.cleanup_usd, None);
    }

    #[test]
    fn token_estimates_cover_prompt_and_output() {
        let ctx = CleanupContext { tone: "neutral|full".into(), ..Default::default() };
        let (i, o) = clean_tokens("um so hello there", &ctx, "Hello there.");
        assert!(i > o && o > 0);
        let (i, o) = edit_tokens(Some("abc"), "make it formal", &ctx, "ABC");
        assert!(i > 0 && o > 0);
    }
}
