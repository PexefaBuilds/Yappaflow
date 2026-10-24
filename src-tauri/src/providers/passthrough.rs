//! Phase 1 cleanup: returns the transcript unchanged. Phase 2 adds OpenAiCleanup.

use super::{CleanupContext, CleanupProvider};
use anyhow::Result;
use async_trait::async_trait;

pub struct PassthroughCleanup;

#[async_trait]
impl CleanupProvider for PassthroughCleanup {
    fn id(&self) -> &'static str {
        "none"
    }
    async fn clean(&self, text: &str, _ctx: &CleanupContext) -> Result<String> {
        Ok(text.to_string())
    }
}
