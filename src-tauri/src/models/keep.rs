//! Keeping local models loaded (phase-7.md item 13): a timed warm-up 10 s after start, and an
//! idle timer that frees them under "unload after 15 min idle". "After each use" unloading
//! happens in the providers themselves (whisper_cpp.rs, llama_cpp.rs).

use crate::providers::llama_cpp::{self, LlamaCppCleanup};
use crate::providers::{registry, whisper_cpp, AudioClip, SttOptions};
use crate::AppState;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

pub const IDLE: Duration = Duration::from_secs(15 * 60);

fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

/// When a local model was last used (now_ms); 0 = nothing used since the last unload.
static LAST_USE: AtomicU64 = AtomicU64::new(0);

/// Providers call this whenever they load or use a local model.
pub fn touch() {
    LAST_USE.store(now_ms(), Ordering::Relaxed);
}

/// Should the idle timer unload now?
pub fn idle_expired(mode: &str, last_use_ms: u64, now_ms: u64) -> bool {
    mode == "idle" && last_use_ms > 0 && now_ms.saturating_sub(last_use_ms) >= IDLE.as_millis() as u64
}

/// Background thread: warm-up, then the idle check every 30 s.
/// shortcut: a normal-priority thread that mostly sleeps; lower its OS priority if warm-up ever
/// competes with the first dictation.
pub fn spawn(state: Arc<AppState>) {
    let _ = std::thread::Builder::new().name("local-models".into()).spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        warm_up(&state);
        loop {
            std::thread::sleep(Duration::from_secs(30));
            let mode = state.settings.read().map(|s| s.keep_models_loaded.clone()).unwrap_or_default();
            // shortcut: a dictation that started > 15 min after the last one can race this; it
            // then just reloads the model (slower, nothing lost).
            if idle_expired(&mode, LAST_USE.load(Ordering::Relaxed), now_ms()) {
                log::info!("local models idle for 15 min, unloading");
                whisper_cpp::unload();
                llama_cpp::shutdown();
                LAST_USE.store(0, Ordering::Relaxed);
            }
        }
    });
}

/// Loads the selected local models and runs one 1 s silent inference so the GPU kernels are
/// initialised before the first dictation. Timings go to the log.
fn warm_up(state: &AppState) {
    let Ok(s) = state.settings.read().map(|s| s.clone()) else { return };
    if s.keep_models_loaded == "after_use" {
        return;
    }
    tauri::async_runtime::block_on(async {
        if s.stt_provider == "whisper_cpp" {
            let t0 = Instant::now();
            let silence =
                AudioClip { bytes: vec![], mime: "audio/wav", filename: "warmup.wav", pcm16k: vec![0; 16_000], duration_ms: 1000 };
            match registry::build_stt(&s, &state.http).transcribe(&silence, &SttOptions::default()).await {
                Ok(_) => log::info!("local speech warm-up took {} ms", t0.elapsed().as_millis()),
                Err(e) => log::warn!("local speech warm-up failed: {e:#}"),
            }
        }
        if s.cleanup_provider == "llama_cpp" {
            let t0 = Instant::now();
            match LlamaCppCleanup::from_settings(&s, Duration::from_secs(60)).wait_loaded(Duration::from_secs(60)).await {
                Ok(()) => log::info!("local cleanup warm-up took {} ms", t0.elapsed().as_millis()),
                Err(e) => log::warn!("local cleanup warm-up failed: {e:#}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_timer() {
        let m = IDLE.as_millis() as u64;
        assert!(idle_expired("idle", 1, m + 1));
        assert!(!idle_expired("idle", 1, m - 1));
        assert!(!idle_expired("idle", 0, m * 2), "nothing loaded");
        assert!(!idle_expired("always", 1, m * 2));
        assert!(!idle_expired("after_use", 1, m * 2));
    }
}
